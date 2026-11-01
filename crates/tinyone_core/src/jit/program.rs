use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::Path;

use super::execution::ChunkExecution;
use crate::{
    JitChunk,
    JitExecutionProfile,
    JitOptions,
    JitStats,
    JitVm,
    Program,
    Result,
    TinyMemory,
    TinyOneError,
    TinyRunReport,
    Value,
    VerifiedProgram,
};

const MAX_REUSABLE_OPERAND_STACKS: usize = 8;

#[derive(Debug)]
pub struct JitProgram {
    pub(crate) verified_program:  VerifiedProgram,
    pub(crate) fingerprint:       String,
    // All verified function positions are reserved once. Lazy compilation
    // fills existing slots; it never grows the table or invalidates indexes.
    pub(crate) chunks:            Box<[Option<JitChunk>]>,
    pub(crate) options:           JitOptions,
    pub(crate) stats:             JitStats,
    // Detailed attribution is cold, opt-in storage, not an inline cost paid
    // by every normal compiled program when more counters are introduced.
    pub(crate) execution_profile: Option<Box<JitExecutionProfile>>,
    operand_stacks:               Vec<Vec<Value>>,
    feedback_bytes:               usize,
    installed_bytes:              usize,
}

impl Clone for JitProgram {
    /// Copy compiled/adaptive state and historical counters, but not scratch
    /// capacity. Cloning empty Vecs loses their reservations and would fill
    /// the bounded pool with unusable entries. Retained-storage gauges start
    /// at zero because the clone has no idle stacks yet.
    fn clone(&self) -> Self {
        let chunks = self.chunks.clone();
        // Vec/String cloning may discard spare capacity, so use the clone's
        // actual extents instead of copying the source's storage charge.
        let installed_bytes = chunks
            .iter()
            .flatten()
            .fold(0usize, |bytes, chunk| bytes.saturating_add(chunk.fixed_storage_bytes()));
        Self {
            verified_program: self.verified_program.clone(),
            fingerprint: self.fingerprint.clone(),
            chunks,
            options: self.options,
            stats: JitStats {
                cached_operand_stacks: 0,
                cached_operand_stack_capacity: 0,
                ..self.stats
            },
            execution_profile: self.execution_profile.clone(),
            operand_stacks: Vec::new(),
            feedback_bytes: self.feedback_bytes,
            installed_bytes,
        }
    }
}

impl JitProgram {
    /// Compile a [`Program`] into a JIT-compiled [`JitProgram`].
    ///
    /// # Errors
    ///
    /// Returns `Err` if bytecode verification of `program` fails (for example a
    /// slot/function/struct limit exceeded, an invalid opcode, a malformed
    /// branch target, or a stack under/overflow), or if a chunk cannot be
    /// lowered to JIT ops because an instruction operand is out of range.
    pub fn compile(program: &Program) -> crate::Result<Self> {
        let verified = VerifiedProgram::verify(program.clone())?;
        let fingerprint = verified.fingerprint().to_owned();
        Self::compile_with_fingerprint(&verified, fingerprint, JitOptions::default())
    }

    /// Compile a [`Program`] into a JIT-compiled [`JitProgram`] using the given
    /// [`JitOptions`].
    ///
    /// # Errors
    ///
    /// Returns `Err` if bytecode verification of `program` fails (for example a
    /// slot/function/struct limit exceeded, an invalid opcode, a malformed
    /// branch target, or a stack under/overflow), or if a chunk cannot be
    /// lowered to JIT ops because an instruction operand is out of range.
    pub fn compile_with_options(program: &Program, options: JitOptions) -> crate::Result<Self> {
        let verified = VerifiedProgram::verify(program.clone())?;
        let fingerprint = verified.fingerprint().to_owned();
        Self::compile_with_fingerprint(&verified, fingerprint, options)
    }

    /// Compile an already-verified [`VerifiedProgram`] into a JIT-compiled
    /// [`JitProgram`], skipping bytecode verification.
    ///
    /// # Errors
    ///
    /// Returns `Err` if a chunk cannot be lowered to JIT ops. The verified
    /// token shares immutable program content, so it cannot become stale
    /// through later mutation and does not need bytecode re-verification.
    pub fn compile_verified(verified: &VerifiedProgram) -> Result<Self> {
        Self::compile_verified_with_options(verified, JitOptions::default())
    }

    /// Compile an already-verified [`VerifiedProgram`] into a JIT-compiled
    /// [`JitProgram`] using the given [`JitOptions`], skipping bytecode
    /// verification.
    ///
    /// # Errors
    ///
    /// Returns `Err` if a chunk cannot be lowered to JIT ops because an
    /// instruction operand is out of range.
    pub fn compile_verified_with_options(verified: &VerifiedProgram, options: JitOptions) -> Result<Self> {
        Self::compile_with_fingerprint(verified, verified.fingerprint().to_owned(), options)
    }

    pub(crate) fn compile_with_fingerprint(
        verified: &VerifiedProgram,
        fingerprint: String,
        options: JitOptions,
    ) -> Result<Self> {
        let program = verified.program();
        let main = JitChunk::compile(
            "main",
            program.slot_count,
            &program.code,
            verified.chunk_certificate(0)?,
            options.call_feedback_enabled(),
        )?;
        let compiled_ops = main.ops.len();
        let installed_bytes = main.fixed_storage_bytes();
        let mut chunks = Vec::with_capacity(program.functions.len() + 1);
        chunks.push(Some(main));
        chunks.resize_with(program.functions.len() + 1, || None);
        Ok(Self {
            verified_program: verified.clone(),
            fingerprint,
            chunks: chunks.into_boxed_slice(),
            options,
            stats: JitStats {
                compiled_chunks: 1,
                compiled_ops,
                ..JitStats::default()
            },
            execution_profile: if options.execution_profile_enabled() {
                Some(Box::default())
            } else {
                None
            },
            operand_stacks: Vec::new(),
            feedback_bytes: 0,
            installed_bytes,
        })
    }

    #[must_use]
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    #[must_use]
    pub fn stats(&self) -> JitStats {
        self.stats
    }

    /// Bytes in installed per-site feedback tables, excluding owner headers
    /// and allocator overhead. Lazy compilation shares a 64-KiB program budget.
    #[must_use]
    pub fn feedback_bytes(&self) -> usize {
        self.feedback_bytes
    }

    /// Inclusive retained storage charge, including this header, verified
    /// bytecode/constants/metadata, lazy chunks, feedback and idle operand
    /// capacity. Shared verified data is charged in full, not apportioned by
    /// reference count. No Ralloc frame or heap payload survives in this owner.
    ///
    /// Excludes allocator/Arc overhead and opaque BTreeMap node slack in the
    /// opt-in execution profile. This is not an RSS measurement.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        size_of::<Self>()
            .saturating_add(self.verified_program.retained_bytes())
            .saturating_add(self.fingerprint.capacity())
            .saturating_add(size_of_val(&*self.chunks))
            .saturating_add(self.installed_bytes)
            .saturating_add(self.feedback_bytes)
            .saturating_add(self.operand_stacks.capacity().saturating_mul(size_of::<Vec<Value>>()))
            .saturating_add(
                self.stats
                    .cached_operand_stack_capacity
                    .saturating_mul(size_of::<Value>()),
            )
            .saturating_add(
                self.execution_profile
                    .as_deref()
                    .map_or(0, |profile| size_of::<JitExecutionProfile>().saturating_add(profile.allocation_bytes())),
            )
    }

    /// Returns optional dispatch and stack attribution collected when this
    /// program was compiled with `JitOptions::with_execution_profile(true)`.
    #[must_use]
    pub fn execution_profile(&self) -> Option<&JitExecutionProfile> {
        self.execution_profile.as_deref()
    }

    pub fn reset_execution_profile(&mut self) {
        if let Some(profile) = self.execution_profile.as_deref_mut() {
            *profile = JitExecutionProfile::default();
        }
    }

    #[must_use]
    pub fn listing(&self) -> String {
        use std::fmt::Write as _;

        let mut out = String::new();
        let _ = writeln!(&mut out, "; tinyone adaptive-jit {}", self.fingerprint);
        let _ = writeln!(
            &mut out,
            "; chunks={} ops={} hot_back_edges={} hot_ranges={} quickened_ops={} hot_threshold={}",
            self.stats.compiled_chunks,
            self.stats.compiled_ops,
            self.stats.hot_back_edges,
            self.stats.hot_ranges,
            self.stats.quickened_ops,
            self.options.hot_back_edge_threshold()
        );
        for (chunk_index, chunk) in self.chunks.iter().enumerate() {
            if let Some(chunk) = chunk {
                let _ = writeln!(
                    &mut out,
                    ".chunk {chunk_index} {} slots={} ops={}",
                    chunk.name,
                    chunk.slot_count,
                    chunk.ops.len()
                );
                for (pc, op) in chunk.ops.iter().enumerate() {
                    let _ = writeln!(&mut out, "  {pc:04} {}", op.listing());
                }
            } else {
                let function_index = chunk_index.saturating_sub(1);
                if let Some(function) = self.verified_program.program().functions.get(function_index) {
                    let _ = writeln!(
                        &mut out,
                        ".lazy {chunk_index} {} slots={} bytecode_ops={}",
                        function.name,
                        function.slot_count,
                        function.code.len()
                    );
                }
            }
        }
        out
    }

    pub(crate) fn ensure_chunk(&mut self, chunk_index: usize) -> Result<()> {
        let slot = self
            .chunks
            .get(chunk_index)
            .ok_or_else(|| TinyOneError::runtime(format!("Invalid JIT chunk {chunk_index}")))?;
        if slot.is_some() {
            return Ok(());
        }
        let function_index = chunk_index
            .checked_sub(1)
            .ok_or_else(|| TinyOneError::runtime(format!("Invalid lazy JIT chunk {chunk_index}")))?;
        let function = self
            .verified_program
            .program()
            .functions
            .get(function_index)
            .ok_or_else(|| TinyOneError::runtime(format!("Invalid function index {function_index}")))?;
        let chunk = JitChunk::compile(
            function.name.clone(),
            function.slot_count,
            &function.code,
            self.verified_program.chunk_certificate(chunk_index)?,
            self.options.call_feedback_enabled(),
        )?;
        self.stats.compiled_chunks += 1;
        self.stats.compiled_ops += chunk.ops.len();
        self.installed_bytes = self.installed_bytes.saturating_add(chunk.fixed_storage_bytes());
        self.chunks[chunk_index] = Some(chunk);
        Ok(())
    }

    fn compile_all(&mut self) -> Result<()> {
        for chunk_index in 1..self.chunks.len() {
            self.ensure_chunk(chunk_index)?;
        }
        Ok(())
    }

    /// Execute this JIT-compiled program, returning its final memory.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the program has no compiled main chunk, if the heap or
    /// operand stack cannot be allocated, or if runtime execution fails (for
    /// example a stack overflow, an illegal/unsupported operation, or another
    /// runtime fault raised by the VM).
    pub fn run(&mut self, stdout: &mut dyn Write, inputs: Vec<String>) -> Result<TinyMemory> {
        JitVm::new(self, inputs).run(stdout)
    }

    /// Execute this JIT-compiled program with explicit system arguments and
    /// environment, returning its final memory.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the program has no compiled main chunk, if the heap or
    /// operand stack cannot be allocated, or if runtime execution fails (for
    /// example a stack overflow, an illegal/unsupported operation, or another
    /// runtime fault raised by the VM).
    pub fn run_with_env(
        &mut self,
        stdout: &mut dyn Write,
        inputs: Vec<String>,
        sys_args: Vec<String>,
        sys_env: HashMap<String, String>,
    ) -> Result<TinyMemory> {
        let mut vm = JitVm::new(self, inputs);
        vm.set_sys_args(sys_args);
        vm.set_sys_env(sys_env);
        vm.run(stdout)
    }

    /// Execute this JIT-compiled program, collecting a [`TinyRunReport`] with
    /// heap statistics in addition to the final memory.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the program has no compiled main chunk, if the heap or
    /// operand stack cannot be allocated, or if runtime execution fails (for
    /// example a stack overflow, an illegal/unsupported operation, or another
    /// runtime fault raised by the VM).
    pub fn run_report(&mut self, stdout: &mut dyn Write, inputs: Vec<String>) -> Result<TinyRunReport> {
        JitVm::new(self, inputs).run_report(stdout)
    }

    pub(super) fn executing_chunk(&mut self, chunk_index: usize) -> Result<ChunkExecution<'_>> {
        let chunk = self
            .chunks
            .get_mut(chunk_index)
            .and_then(Option::as_mut)
            .ok_or_else(|| TinyOneError::runtime(format!("Invalid JIT chunk {chunk_index}")))?;
        Ok(ChunkExecution::new(
            chunk,
            self.options.hot_back_edge_threshold(),
            &mut self.stats,
            self.execution_profile.as_deref_mut(),
            &mut self.feedback_bytes,
        ))
    }

    pub(crate) fn take_operand_stack(&mut self, required_capacity: usize) -> Vec<Value> {
        if let Some(index) = self
            .operand_stacks
            .iter()
            .position(|stack| stack.capacity() >= required_capacity)
        {
            self.stats.operand_stack_reuses = self.stats.operand_stack_reuses.saturating_add(1);
            let stack = self.operand_stacks.swap_remove(index);
            self.stats.cached_operand_stacks = self.operand_stacks.len();
            self.stats.cached_operand_stack_capacity -= stack.capacity();
            return stack;
        }
        if required_capacity != 0 {
            self.stats.operand_stack_allocations = self.stats.operand_stack_allocations.saturating_add(1);
        }
        Vec::with_capacity(required_capacity)
    }

    pub(crate) fn recycle_operand_stack(&mut self, mut stack: Vec<Value>) {
        stack.clear();
        if stack.capacity() == 0 || self.operand_stacks.len() >= MAX_REUSABLE_OPERAND_STACKS {
            return;
        }
        self.stats.cached_operand_stack_capacity += stack.capacity();
        self.operand_stacks.push(stack);
        self.stats.cached_operand_stacks = self.operand_stacks.len();
    }
}

/// Write a human-readable JIT compilation listing for `program` to `path`.
///
/// # Errors
///
/// Returns `Err` if bytecode verification of `program` fails, if a chunk cannot
/// be lowered to JIT ops because an instruction operand is out of range, if all
/// chunks cannot be compiled, or if the listing file at `path` cannot be
/// written.
pub fn write_jit_listing(program: &Program, path: impl AsRef<Path>) -> Result<()> {
    let verified = VerifiedProgram::verify(program.clone())?;
    write_verified_jit_listing(&verified, path)
}

/// Write a human-readable JIT compilation listing for an already-verified
/// `verified` program to `path`.
///
/// # Errors
///
/// Returns `Err` if a chunk cannot be lowered to JIT ops because an instruction
/// operand is out of range, if all chunks cannot be compiled, or if the
/// listing file at `path` cannot be written.
pub fn write_verified_jit_listing(verified: &VerifiedProgram, path: impl AsRef<Path>) -> Result<()> {
    let mut compiled = JitProgram::compile_verified(verified)?;
    compiled.compile_all()?;
    fs::write(path, compiled.listing())
        .map_err(|error| TinyOneError::compile(format!("JIT listing write error: {error}")))
}

#[cfg(test)]
mod storage_tests {
    use super::*;
    use crate::jit::feedback::MAX_FEEDBACK_BYTES;
    use crate::{JitOp, compile_source_verified};

    #[test]
    fn execution_requires_an_installed_chunk_and_keeps_instruction_bounds() {
        let verified = compile_source_verified("fn answer() { return 42 } print answer()").expect("fixture");
        let mut compiled = JitProgram::compile_verified(&verified).expect("main lowering");
        assert!(compiled.executing_chunk(1).is_err());
        assert!(compiled.executing_chunk(usize::MAX).is_err());
        assert_eq!(compiled.stats().compiled_chunks, 1);
        assert!(compiled.chunks[1].is_none());

        compiled.ensure_chunk(1).expect("lazy lowering");
        let execution = compiled.executing_chunk(1).expect("installed function");
        assert_eq!(execution.name(), "answer");
        assert_eq!(execution.instruction(0).expect("function entry"), JitOp::PushInt(42));
        assert!(execution.instruction(usize::MAX).is_err());
    }

    #[test]
    fn retained_bytes_include_idle_capacity_and_profile_allocations() {
        let verified = compile_source_verified("let value = 42 print value").expect("fixture");
        let mut compiled =
            JitProgram::compile_verified_with_options(&verified, JitOptions::new().with_execution_profile(true))
                .expect("lowering");
        let cold = compiled.retained_bytes();
        let stack = compiled.take_operand_stack(256);
        let stack_bytes = stack.capacity() * size_of::<Value>();
        assert_eq!(compiled.retained_bytes(), cold, "checked-out storage is not retained");
        compiled.recycle_operand_stack(stack);
        let table_bytes = compiled.operand_stacks.capacity() * size_of::<Vec<Value>>();
        assert_eq!(compiled.retained_bytes(), cold + stack_bytes + table_bytes);
        compiled.run(&mut Vec::new(), Vec::new()).expect("profiled execution");
        let profile = compiled.execution_profile().expect("profile");
        assert!(!profile.opcodes.is_empty());
        let profile_bytes = profile.allocation_bytes();
        assert!(profile_bytes > 0);
        assert_eq!(compiled.retained_bytes(), cold + stack_bytes + table_bytes + profile_bytes);
        compiled.reset_execution_profile();
        assert_eq!(compiled.retained_bytes(), cold + stack_bytes + table_bytes);
        let cloned = compiled.clone();
        let expected_installed: usize = cloned.chunks.iter().flatten().map(JitChunk::fixed_storage_bytes).sum();
        assert_eq!(cloned.installed_bytes, expected_installed);
        assert!(cloned.retained_bytes() < cold + stack_bytes);
    }

    #[test]
    fn optional_profile_storage_is_charged_only_when_present_and_clones_independently() {
        let verified = compile_source_verified("let value = 42 print value").expect("fixture");
        let plain = JitProgram::compile_verified(&verified).expect("ordinary lowering");
        let mut profiled =
            JitProgram::compile_verified_with_options(&verified, JitOptions::new().with_execution_profile(true))
                .expect("profiled lowering");
        assert!(plain.execution_profile().is_none());
        assert_eq!(profiled.retained_bytes(), plain.retained_bytes() + size_of::<JitExecutionProfile>(),);
        profiled.run(&mut Vec::new(), Vec::new()).expect("profiled execution");
        let cloned = profiled.clone();
        let dispatches = cloned.execution_profile().expect("cloned profile").dispatches;
        assert!(dispatches > 0);
        profiled.reset_execution_profile();
        assert_eq!(profiled.execution_profile().expect("reset profile").dispatches, 0);
        assert_eq!(cloned.execution_profile().expect("independent profile").dispatches, dispatches);
    }

    #[test]
    fn saturated_back_edge_activates_feedback_only_once() {
        let verified = compile_source_verified("let i = 0 while i < 4 { i = i + 1 } print i").expect("fixture");
        let options = JitOptions::new().with_hot_back_edge_threshold(u16::MAX);
        let mut compiled = JitProgram::compile_verified_with_options(&verified, options).expect("lowering");
        let main = compiled.chunks[0].as_mut().expect("main");
        let (pc, target) = main
            .ops
            .iter()
            .enumerate()
            .find_map(|(pc, op)| {
                match *op {
                    JitOp::Jump(target) if target < pc => Some((pc, target)),
                    _ => None,
                }
            })
            .expect("back edge");
        main.edge_counts[pc] = u16::MAX - 1;
        compiled
            .executing_chunk(0)
            .expect("main execution")
            .record_back_edge(pc, target);
        assert_eq!(compiled.stats.hot_ranges, 1);
        let bytes = compiled.feedback_bytes();
        assert!(bytes > 0);
        for _ in 0..32 {
            compiled
                .executing_chunk(0)
                .expect("main execution")
                .record_back_edge(pc, target);
        }
        assert_eq!(compiled.stats.hot_ranges, 1);
        assert_eq!(compiled.feedback_bytes(), bytes);
    }

    #[test]
    fn lazy_chunks_share_one_feedback_budget_and_disabled_adaptation_allocates_none() {
        use super::super::feedback::SiteFeedback;
        use crate::{Instr, Op};

        let source = "fn first(v) { let i = 0 while i < 16 { v = v * 1 i = i + 1 } return v } \
                      fn second(v) { let i = 0 while i < 16 { v = v * 1 i = i + 1 } return v } \
                      print first(3) print second(7)";
        let mut program = compile_source_verified(source).expect("fixture").program().clone();
        let padding = MAX_FEEDBACK_BYTES / 2 / size_of::<SiteFeedback>() + 32;
        // Verifier-valid unreachable instructions keep the dense table large
        // without executing work or altering the observed values. Either
        // function fits alone; both cannot fit the program-wide byte budget.
        for function in &mut program.functions {
            function
                .code
                .extend(std::iter::repeat_n(Instr::new(Op::Return, 0, 0), padding));
        }
        let verified = VerifiedProgram::verify(program).expect("padded fixture");
        for threshold in [0, 1] {
            let options = JitOptions::new().with_hot_back_edge_threshold(threshold);
            let mut compiled = JitProgram::compile_verified_with_options(&verified, options).expect("lowering");
            assert_eq!(compiled.feedback_bytes(), 0);
            assert_eq!(compiled.stats.compiled_chunks, 1);
            for _ in 0..2 {
                let mut output = Vec::new();
                compiled.run(&mut output, Vec::new()).expect("execution");
                assert_eq!(output, b"3\n7\n");
                assert_eq!(compiled.stats.compiled_chunks, 3);
                if threshold == 0 {
                    assert_eq!(compiled.feedback_bytes(), 0);
                } else {
                    assert!(compiled.feedback_bytes() > MAX_FEEDBACK_BYTES / 2);
                    assert!(compiled.feedback_bytes() <= MAX_FEEDBACK_BYTES);
                    assert_eq!(compiled.listing().matches("slot.mul.i.hot").count(), 1);
                }
            }
        }
    }

    #[test]
    fn cloned_program_starts_with_an_empty_reusable_stack_pool() {
        let verified = compile_source_verified("print 42").expect("fixture");
        let mut original = JitProgram::compile_verified(&verified).expect("lowering");
        for capacity in 1..=MAX_REUSABLE_OPERAND_STACKS {
            original.recycle_operand_stack(Vec::with_capacity(capacity));
        }
        let original_capacity = original.stats.cached_operand_stack_capacity;
        assert_eq!(original.stats.cached_operand_stacks, MAX_REUSABLE_OPERAND_STACKS);
        assert!(original_capacity > 0);

        let mut cloned = original.clone();
        assert!(cloned.operand_stacks.is_empty());
        assert_eq!(cloned.stats.cached_operand_stacks, 0);
        assert_eq!(cloned.stats.cached_operand_stack_capacity, 0);
        let stack = cloned.take_operand_stack(32);
        cloned.recycle_operand_stack(stack);
        assert_eq!(cloned.stats.cached_operand_stacks, 1);
        assert!(cloned.stats.cached_operand_stack_capacity >= 32);
        let stack = cloned.take_operand_stack(32);
        assert_eq!(cloned.stats.operand_stack_allocations, 1);
        assert_eq!(cloned.stats.operand_stack_reuses, 1);
        cloned.recycle_operand_stack(stack);

        assert_eq!(original.stats.cached_operand_stack_capacity, original_capacity);
        assert_eq!(original.stats.cached_operand_stacks, MAX_REUSABLE_OPERAND_STACKS);
        let mut output = Vec::new();
        cloned.run(&mut output, Vec::new()).expect("cloned execution");
        assert_eq!(output, b"42\n");
    }

    #[test]
    fn reusable_stack_statistics_track_only_idle_storage() {
        let verified = compile_source_verified("print 42").expect("fixture");
        let mut program = JitProgram::compile_verified(&verified).expect("lowering");
        let stack = program.take_operand_stack(32);
        let capacity = stack.capacity();
        program.recycle_operand_stack(stack);
        assert_eq!(program.stats.cached_operand_stacks, 1);
        assert_eq!(program.stats.cached_operand_stack_capacity, capacity);

        let stack = program.take_operand_stack(16);
        assert_eq!(program.stats.cached_operand_stacks, 0);
        assert_eq!(program.stats.cached_operand_stack_capacity, 0);
        program.recycle_operand_stack(stack);
        assert_eq!(program.stats.cached_operand_stacks, 1);
        assert_eq!(program.stats.cached_operand_stack_capacity, capacity);
        program.recycle_operand_stack(Vec::new());
        assert_eq!(program.stats.cached_operand_stacks, 1);
        assert_eq!(program.stats.cached_operand_stack_capacity, capacity);
    }

    #[test]
    #[ignore = "retained-storage diagnostic; run explicitly with --ignored --nocapture"]
    fn report_sparse_feedback_storage() {
        for (prefix, suffix) in [(0, 0), (1024, 0), (0, 1024), (8192, 0), (0, 8192), (8192, 8192)] {
            let source = format!(
                "fn identity(value) {{ return value }} \
                 if 0 {{ {} }} let value = identity(7) let i = 0 \
                 while i < 32 {{ value = value * 1 i = i + 1 }} \
                 print value if 0 {{ {} }}",
                "print 0 ".repeat(prefix),
                "print 0 ".repeat(suffix),
            );
            let verified = compile_source_verified(&source).expect("sparse feedback fixture");
            for threshold in [0, 1, 8] {
                let options = JitOptions::new()
                    .with_hot_back_edge_threshold(threshold)
                    .with_execution_profile(true);
                let mut compiled = JitProgram::compile_verified_with_options(&verified, options).expect("lowering");
                let before_bytes = compiled.retained_bytes();
                let mut output = Vec::new();
                compiled
                    .run(&mut output, Vec::new())
                    .expect("sparse feedback execution");
                assert_eq!(output, b"7\n");
                let after_bytes = compiled.retained_bytes();
                let profile = compiled.execution_profile().expect("profile");
                eprintln!(
                    "sparse/{prefix}/{suffix}/{threshold}: chunk_header={} op_count={} feedback_bytes={} before_bytes={before_bytes} after_bytes={after_bytes} samples={} promotions={} hits={}",
                    size_of::<JitChunk>(),
                    compiled.stats.compiled_ops,
                    compiled.feedback_bytes(),
                    profile.feedback_samples,
                    profile.specialization_promotions,
                    profile.specialization_hits,
                );
                assert!(compiled.feedback_bytes() <= MAX_FEEDBACK_BYTES);
            }
        }
    }

    #[test]
    #[ignore = "retained-storage diagnostic; run explicitly with --ignored --nocapture"]
    fn report_lowered_storage_layout() {
        eprintln!(
            "layout: JitOp={} JitChunk={} Option<JitChunk>={} JitProgram={} JitExecutionProfile={} OptionalProfile={}",
            size_of::<JitOp>(),
            size_of::<JitChunk>(),
            size_of::<Option<JitChunk>>(),
            size_of::<JitProgram>(),
            size_of::<JitExecutionProfile>(),
            size_of::<Option<Box<JitExecutionProfile>>>(),
        );
        for (name, source) in [
            ("small", "let answer = 42 print answer"),
            ("loop", "let i = 0 let total = 0 while i < 4096 { total = total + i i = i + 1 } print total"),
            ("function", "fn add(a, b) { return a + b } print add(1, 2)"),
            (
                "map",
                "let values = map_new() let i = 0 while i < 128 { let ignored = map_set(values, i, i * 3) i = i + 1 } print map_get(values, 7)",
            ),
            ("many_stores", "let value = 0 "),
        ] {
            let source = if name == "many_stores" {
                format!("{source}{}print value", "value = 42 ".repeat(1024))
            } else {
                source.to_owned()
            };
            let verified = compile_source_verified(&source).expect("storage fixture");
            let mut compiled = JitProgram::compile_verified(&verified).expect("storage lowering");
            compiled.compile_all().expect("lazy lowering");
            let mut code_bytes = 0;
            let mut reserved_code_bytes = 0;
            let mut block_bytes = 0;
            let mut origin_bytes = 0;
            for index in 0..compiled.chunks.len() {
                block_bytes += size_of_val(&*verified.chunk_certificate(index).expect("certificate").blocks);
            }
            for chunk in compiled.chunks.iter().flatten() {
                code_bytes += chunk.ops.len() * size_of::<JitOp>() + chunk.edge_counts.len() * size_of::<u16>();
                reserved_code_bytes += chunk.ops.reserved_bytes() + size_of_val(&*chunk.edge_counts);
                origin_bytes += chunk.origins.allocation_bytes();
            }
            eprintln!(
                "{name}: chunks={} chunk_slots_bytes={} code_bytes={code_bytes} reserved_code_bytes={reserved_code_bytes} block_bytes={block_bytes} origin_bytes={origin_bytes}",
                compiled.chunks.len(),
                size_of_val(&*compiled.chunks),
            );
            assert!(reserved_code_bytes - code_bytes < compiled.chunks.len() * 4096);
            assert_eq!(compiled.feedback_bytes(), 0);
            compiled
                .run(&mut Vec::new(), Vec::new())
                .expect("feedback storage warm-up");
            eprintln!(
                "{name}: feedback_site_bytes={} feedback_bytes={}",
                size_of::<super::super::feedback::SiteFeedback>(),
                compiled.feedback_bytes(),
            );
            assert!(compiled.feedback_bytes() <= MAX_FEEDBACK_BYTES);
        }
    }
}
