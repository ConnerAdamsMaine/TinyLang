use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use super::feedback::{FeedbackChange, ObservedTypes, SiteFeedback};
use super::lowering::SymbolicLowerer;
use super::origins::{ChunkOrigins, LoweredChunk, OpOrigin};
#[cfg(feature = "testing-hooks")]
use super::preparation_profile::{PreparationPhase, PreparationSpan};
use crate::language::bytecode::certificate::ChunkCertificate;
use crate::language::bytecode::control_flow::{BasicBlock, BytecodeOffset};
use crate::{Instr, JitOp, Op, Result, TinyLangError, builtin_index, checked_non_negative_usize};

const MIN_SPARE_OP_BYTES_TO_RECLAIM: usize = 4 * 1024;

/// Installed operations have a fixed length even while quickening mutates
/// elements. Expose only slice operations, not the builder Vec's growth API.
#[derive(Debug, Clone)]
pub(crate) struct JitOps(Vec<JitOp>);

impl JitOps {
    fn freeze(mut ops: Vec<JitOp>) -> Self {
        // Retain the bounded installation policy; the current Vec/Box timing
        // recheck was inconclusive, not evidence that Vec is generally faster.
        // Reclaim large spare extents and expose no growth after installation.
        let spare_bytes = (ops.capacity() - ops.len()) * size_of::<JitOp>();
        if spare_bytes >= MIN_SPARE_OP_BYTES_TO_RECLAIM {
            ops = ops.into_boxed_slice().into_vec();
        }
        Self(ops)
    }

    pub(super) fn reserved_bytes(&self) -> usize {
        self.0.capacity() * size_of::<JitOp>()
    }
}

impl Deref for JitOps {
    type Target = [JitOp];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for JitOps {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(Debug, Clone)]
pub(crate) struct JitChunk {
    pub(crate) name: String,
    pub(crate) slot_count: usize,
    pub(crate) ops: JitOps,
    pub(super) origins: Arc<ChunkOrigins>,
    pub(crate) edge_counts: Box<[u16]>,
    // Allocated only after hotness, never resized, and charged to the owning
    // program's byte budget. Sampling opcodes prove their entry exists.
    feedback: Box<[SiteFeedback]>,
}

#[derive(Default)]
pub(super) struct ActivatedRange {
    pub(super) allocated_bytes: usize,
    pub(super) sampling_sites: usize,
    pub(super) branches: usize,
}

impl JitChunk {
    /// Installed code/counter/name allocations; the owner charges its boxed
    /// chunk slots and separately budgeted feedback tables.
    pub(super) fn fixed_storage_bytes(&self) -> usize {
        self.name
            .capacity()
            .saturating_add(self.ops.reserved_bytes())
            .saturating_add(self.origins.allocation_bytes())
            .saturating_add(size_of_val(&*self.edge_counts))
    }

    pub(crate) fn compile(
        name: impl Into<String>,
        slot_count: usize,
        code: &[Instr],
        certificate: &ChunkCertificate,
        call_feedback: bool,
    ) -> Result<Self> {
        let (mut ops, origins) = lower_blocks(code, &certificate.blocks)?.finish();
        #[cfg(feature = "testing-hooks")]
        let _installation = PreparationSpan::start(PreparationPhase::ChunkInstallation);
        // Only fresh, privately built chunks can arm call sites. Generic
        // fallback operations can therefore never be rearmed by a back edge.
        if call_feedback {
            for op in &mut ops {
                if let JitOp::CallValue(arg_count) = *op {
                    *op = JitOp::UnseenCallValue(arg_count);
                }
            }
        }
        Ok(Self {
            name: name.into(),
            slot_count,
            edge_counts: vec![0; ops.len()].into_boxed_slice(),
            ops: JitOps::freeze(ops),
            origins,
            feedback: Box::default(),
        })
    }

    pub(super) fn activate_range(&mut self, start: usize, end: usize, available_bytes: usize) -> ActivatedRange {
        let start = start.min(self.ops.len());
        let end = end.min(self.ops.len());
        let mut activated = ActivatedRange::default();
        if self.feedback.is_empty()
            && self.ops[start..end].iter().any(|op| op.sampling().is_some())
            && let Some(bytes) = self
                .ops
                .len()
                .checked_mul(size_of::<SiteFeedback>())
                .filter(|bytes| *bytes <= available_bytes)
        {
            self.feedback = vec![SiteFeedback::default(); self.ops.len()].into_boxed_slice();
            activated.allocated_bytes = bytes;
        }
        for index in start..end {
            let op = &mut self.ops[index];
            let quickened = op.quickened_branch();
            if quickened != *op {
                *op = quickened;
                activated.branches += 1;
            }
            if let Some(sampled) = op.sampling()
                && self.feedback.get_mut(index).is_some_and(SiteFeedback::arm)
            {
                *op = sampled;
                activated.sampling_sites += 1;
            }
        }
        activated
    }

    pub(super) fn observe(&mut self, index: usize, types: ObservedTypes) -> FeedbackChange {
        let change = self.feedback[index].observe(types);
        self.apply_feedback(index, change);
        change
    }

    pub(super) fn record_hit(&mut self, index: usize) {
        self.feedback[index].hit();
    }

    pub(super) fn record_miss(&mut self, index: usize, types: ObservedTypes) -> FeedbackChange {
        let change = self.feedback[index].miss(types);
        self.apply_feedback(index, change);
        change
    }

    fn apply_feedback(&mut self, index: usize, change: FeedbackChange) {
        let op = &mut self.ops[index];
        *op = match change {
            FeedbackChange::Keep => return,
            FeedbackChange::Specialize => op.specialized(),
            FeedbackChange::Resample => op.sampling().expect("specialized opcode has a sampling form"),
            FeedbackChange::Generic => op.generic(),
        };
    }
}

fn lower_blocks(code: &[Instr], blocks: &[BasicBlock]) -> Result<LoweredChunk> {
    #[cfg(feature = "testing-hooks")]
    let setup = PreparationSpan::start(PreparationPhase::ChunkSetup);
    let mut original_to_compiled = vec![0usize; code.len() + 1];
    let mut ops = LoweredChunk::with_capacity(code.len());
    let mut lowerer = SymbolicLowerer::default();
    #[cfg(feature = "testing-hooks")]
    drop(setup);

    for block in blocks {
        #[cfg(feature = "testing-hooks")]
        let mut construction = PreparationSpan::start(PreparationPhase::BranchMap);
        let range = block.range();
        let block_code = &code[range.clone()];
        // Every branch target is a block start. Interior offsets are not
        // executable entry points, even when symbolic emission is deferred.
        original_to_compiled[range.clone()].fill(ops.len());
        #[cfg(feature = "testing-hooks")]
        construction.next(PreparationPhase::SymbolicConstruction);
        // Unreachable instructions passed static checks, but have no proven
        // stack state. Keep their ordinary encoding without symbolic claims.
        let Some(entry_depth) = block.entry_depth else {
            #[cfg(feature = "testing-hooks")]
            construction.next(PreparationPhase::UnreachableEncoding);
            for (pc, instr) in block_code.iter().copied().enumerate() {
                ops.push(JitOp::from_instr(instr)?, OpOrigin::instruction(BytecodeOffset::parse(range.start + pc)?));
            }
            continue;
        };
        lowerer.begin_block(entry_depth);
        let mut pc = 0;
        while pc < block_code.len() {
            if let Some((op, width)) = collection_superinstruction(block_code, pc) {
                let start = range.start + pc;
                let (operation, secondary, store) = match op {
                    JitOp::ArrayLenSlotJumpIfZero(..) => (start + 1, start + 1, start + 1),
                    JitOp::MapGetAddSlots(..) => (start + 3, start + 4, start + 5),
                    JitOp::MapSetMulSlots(..) => (start + 4, start + 5, start + 6),
                    _ => return Err(TinyLangError::compile("JIT collection fusion has no source-origin rule")),
                };
                lowerer.set_origin(OpOrigin::fused(
                    BytecodeOffset::parse(operation)?,
                    BytecodeOffset::parse(secondary)?,
                    BytecodeOffset::parse(store)?,
                ));
                lowerer.effect(op)?;
                pc += width;
                continue;
            }
            lowerer.set_origin(OpOrigin::instruction(BytecodeOffset::parse(range.start + pc)?));
            lowerer.instruction(block_code[pc])?;
            pc += 1;
        }
        #[cfg(feature = "testing-hooks")]
        drop(construction);
        lowerer.finish(&mut ops)?;
    }
    #[cfg(feature = "testing-hooks")]
    let _remapping = PreparationSpan::start(PreparationPhase::TargetRemapping);
    original_to_compiled[code.len()] = ops.len();

    for op in ops.ops_mut() {
        op.remap_targets(&original_to_compiled);
    }
    Ok(ops)
}

// Collection-specific idioms remain explicit effect barriers during migration
// to local IR. Scalar lowering no longer depends on adjacent source patterns.
// This slice is one block, so no branch can enter any candidate's interior.
fn collection_superinstruction(code: &[Instr], pc: usize) -> Option<(JitOp, usize)> {
    if let Some(op) = array_len_slot_jump(code, pc) {
        return Some((op, 5));
    }
    if let Some(op) = map_get_add_slots(code, pc) {
        return Some((op, 6));
    }
    if let Some(op) = map_set_mul_slots(code, pc) {
        return Some((op, 7));
    }
    None
}

/// Fuses the canonical lowering of `len(array_slot) > 0` used by pop loops.
/// Block-local input prevents entry into the fused range's stack effects.
fn array_len_slot_jump(code: &[Instr], pc: usize) -> Option<JitOp> {
    let [load, builtin, zero, compare, jump] = code.get(pc..pc + 5)? else {
        return None;
    };
    if !matches!(load.op, Op::Load)
        || !matches!(builtin.op, Op::Builtin)
        || builtin.arg != builtin_index("len")? as i64
        || builtin.arg2 != 1
        || !matches!(zero.op, Op::PushInt)
        || zero.arg != 0
        || !matches!(compare.op, Op::Gt)
        || !matches!(jump.op, Op::JumpIfZero)
    {
        return None;
    }
    Some(JitOp::ArrayLenSlotJumpIfZero(jit_operand(load.arg).ok()?, jit_operand(jump.arg).ok()?))
}

/// Fuses `total = total + map_get(map, key)` while retaining the direct JIT
/// builtin's numeric-key and generic fallback behavior in the VM executor.
fn map_get_add_slots(code: &[Instr], pc: usize) -> Option<JitOp> {
    let [total, map, key, builtin, add, store] = code.get(pc..pc + 6)? else {
        return None;
    };
    if !matches!(total.op, Op::Load)
        || !matches!(map.op, Op::Load)
        || !matches!(key.op, Op::Load)
        || !matches!(builtin.op, Op::Builtin)
        || builtin.arg != builtin_index("map_get")? as i64
        || builtin.arg2 != 2
        || !matches!(add.op, Op::Add)
        || !matches!(store.op, Op::Store)
        || store.arg != total.arg
    {
        return None;
    }
    JitOp::map_get_add_slots(jit_operand(total.arg).ok()?, jit_operand(map.arg).ok()?, jit_operand(key.arg).ok()?)
}

/// Fuses `ignored = map_set(map, key, value * K)` into one lowered operation.
/// The executor uses the I64 fast path only when the dynamic slots actually
/// hold I64 values and preserves the checked generic arithmetic otherwise.
fn map_set_mul_slots(code: &[Instr], pc: usize) -> Option<JitOp> {
    let [map, key, value, multiplier, multiply, builtin, store] = code.get(pc..pc + 7)? else {
        return None;
    };
    if !matches!(map.op, Op::Load)
        || !matches!(key.op, Op::Load)
        || !matches!(value.op, Op::Load)
        || !matches!(multiplier.op, Op::PushInt)
        || !matches!(multiply.op, Op::Mul)
        || !matches!(builtin.op, Op::Builtin)
        || builtin.arg != builtin_index("map_set")? as i64
        || builtin.arg2 != 3
        || !matches!(store.op, Op::Store)
    {
        return None;
    }
    JitOp::map_set_mul_slots(
        jit_operand(map.arg).ok()?,
        jit_operand(key.arg).ok()?,
        jit_operand(value.arg).ok()?,
        jit_operand(store.arg).ok()?,
        multiplier.arg,
    )
}

fn jit_operand(value: i64) -> Result<usize> {
    checked_non_negative_usize(value, "JIT operand")
        .map_err(|error| TinyLangError::compile(format!("JIT invalid operand: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compile_ops(code: &[Instr]) -> Result<Vec<JitOp>> {
        let blocks = crate::language::bytecode::control_flow::blocks_for_encoding_test(code)?;
        Ok(lower_blocks(code, &blocks)?.finish().0)
    }

    #[test]
    fn collection_matchers_require_load_at_every_window_offset() {
        let patterns = [
            vec![
                Instr::new(Op::Load, 0, 0),
                Instr::new(Op::Builtin, builtin_index("len").expect("len") as i64, 1),
                Instr::new(Op::PushInt, 0, 0),
                Instr::new(Op::Gt, 0, 0),
                Instr::new(Op::JumpIfZero, 7, 0),
            ],
            vec![
                Instr::new(Op::Load, 0, 0),
                Instr::new(Op::Load, 1, 0),
                Instr::new(Op::Load, 2, 0),
                Instr::new(Op::Builtin, builtin_index("map_get").expect("map_get") as i64, 2),
                Instr::new(Op::Add, 0, 0),
                Instr::new(Op::Store, 0, 0),
            ],
            vec![
                Instr::new(Op::Load, 0, 0),
                Instr::new(Op::Load, 1, 0),
                Instr::new(Op::Load, 2, 0),
                Instr::new(Op::PushInt, 3, 0),
                Instr::new(Op::Mul, 0, 0),
                Instr::new(Op::Builtin, builtin_index("map_set").expect("map_set") as i64, 3),
                Instr::new(Op::Store, 3, 0),
            ],
        ];
        for pattern in patterns {
            let expected = collection_superinstruction(&pattern, 0).expect("collection fixture");
            assert_eq!(expected.1, pattern.len());
            for offset in [0, 1, 4] {
                let mut code = vec![Instr::new(Op::PushNull, 0, 0); offset];
                code.extend_from_slice(&pattern);
                // All 36 currently defined portable opcode ordinals, including
                // Load itself, exercise the common-prefix precondition.
                for ordinal in 1..=36 {
                    let leading = Op::from_ordinal(ordinal).expect("defined opcode");
                    code[offset].op = leading;
                    assert_eq!(
                        collection_superinstruction(&code, offset),
                        (leading == Op::Load).then_some(expected),
                        "width={}, offset={offset}, leading={leading:?}",
                        pattern.len(),
                    );
                    for end in offset..code.len() {
                        assert_eq!(collection_superinstruction(&code[..end], offset), None, "truncated window");
                    }
                }
            }
        }
    }

    #[test]
    fn single_block_lowering_preserves_execution_and_failure_origins() {
        for (operation, rhs, expected_output) in [(Op::Add, 35, &b"42\n"[..]), (Op::Div, 0, &b""[..])] {
            let code = vec![
                Instr::new(Op::PushInt, 7, 0),
                Instr::new(Op::PushInt, rhs, 0),
                Instr::new(operation, 0, 0),
                Instr::new(Op::Print, 0, 0),
                Instr::new(Op::Halt, 0, 0),
            ];
            let verified = crate::VerifiedProgram::verify(crate::Program::new(code, 0)).expect("verified block");
            assert_eq!(verified.chunk_certificate(0).expect("certificate").blocks.len(), 1);
            let mut reference = Vec::new();
            let reference_error = crate::run_verified_program(&verified, "vm", &mut reference, Vec::new())
                .err()
                .map(|error| error.to_string());
            assert_eq!(reference, expected_output);
            assert_eq!(reference_error.is_some(), operation == Op::Div);
            for threshold in [0, 1, 8] {
                let mut compiled = crate::JitProgram::compile_verified_with_options(
                    &verified,
                    crate::JitOptions::new().with_hot_back_edge_threshold(threshold),
                )
                .expect("lowering");
                for _ in 0..2 {
                    let mut output = Vec::new();
                    let error = compiled
                        .run(&mut output, Vec::new())
                        .err()
                        .map(|error| error.to_string());
                    assert_eq!(output, reference);
                    assert_eq!(error, reference_error);
                }
            }
        }
    }

    #[test]
    fn single_block_encoding_keeps_the_only_possible_branch_target_at_zero() {
        // Encoding-only fixtures may omit the verifier-required terminal op.
        // Even this weaker boundary has no nonzero target with just one block.
        let code = [
            Instr::new(Op::PushInt, 1, 0),
            Instr::new(Op::PushInt, 2, 0),
            Instr::new(Op::Add, 0, 0),
            Instr::new(Op::Print, 0, 0),
            Instr::new(Op::Jump, 0, 0),
        ];
        let blocks = crate::language::bytecode::control_flow::blocks_for_encoding_test(&code).expect("encoding blocks");
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            lower_blocks(&code, &blocks).expect("lowering").finish().0,
            [JitOp::PushInt(3), JitOp::Print, JitOp::Jump(0)]
        );

        let conditional = [
            Instr::new(Op::Load, 0, 0),
            Instr::new(Op::PushInt, 5, 0),
            Instr::new(Op::Gt, 0, 0),
            Instr::new(Op::JumpIfZero, 0, 0),
        ];
        let blocks =
            crate::language::bytecode::control_flow::blocks_for_encoding_test(&conditional).expect("encoding blocks");
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            lower_blocks(&conditional, &blocks).expect("lowering").finish().0,
            [JitOp::compare_slot_int_jump_if_zero(0, 5, Op::Gt, 0).expect("zero target")]
        );
    }

    #[test]
    fn nested_hot_loop_retains_the_existing_executable_sequence() {
        let verified = crate::compile_source_verified(
            "let i = 0 let total = 0 while i < 4096 { total = total + (i * 3) i = i + 1 } print total",
        )
        .expect("hot-loop fixture");
        let compiled = crate::JitProgram::compile_verified(&verified).expect("lowering");
        let chunk = compiled.chunks[0].as_ref().expect("main");
        assert_eq!(
            &*chunk.ops,
            &[
                JitOp::StoreInt(0, 0),
                JitOp::StoreInt(1, 0),
                JitOp::compare_slot_int_jump_if_zero(0, 4096, Op::Lt, 9).expect("bounded comparison"),
                JitOp::Load(1),
                JitOp::PushMulSlotInt(0, 3),
                JitOp::Add,
                JitOp::Store(1),
                JitOp::AddSlotInt(0, 1),
                JitOp::Jump(2),
                JitOp::Load(1),
                JitOp::Print,
                JitOp::Halt,
            ]
        );
    }

    #[test]
    fn installed_chunk_keeps_its_extent_across_quickening() {
        let code = [
            Instr::new(Op::Load, 0, 0),
            Instr::new(Op::PushInt, 1, 0),
            Instr::new(Op::Mul, 0, 0),
            Instr::new(Op::Store, 0, 0),
            Instr::new(Op::Halt, 0, 0),
        ];
        let verified = crate::VerifiedProgram::verify(crate::Program::new(code.to_vec(), 1)).expect("fixture");
        let mut chunk =
            JitChunk::compile("fixed extent", 1, &code, verified.chunk_certificate(0).expect("certificate"), false)
                .expect("lowering");
        assert_eq!(chunk.ops.len(), 2);
        assert_eq!(chunk.edge_counts.len(), chunk.ops.len());
        let origins = Arc::clone(&chunk.origins);
        let origin_bytes = origins.allocation_bytes();
        let activated = chunk.activate_range(0, chunk.ops.len(), super::super::feedback::MAX_FEEDBACK_BYTES);
        assert_eq!(activated.sampling_sites, 1);
        assert!(activated.allocated_bytes > 0);
        for _ in 0..8 {
            chunk.observe(0, ObservedTypes::unary(&crate::Value::I64(3)));
        }
        assert!(matches!(chunk.ops[0], JitOp::MulSlotIntHot(0, 1)));
        assert_eq!(chunk.ops.len(), 2);
        assert!(Arc::ptr_eq(&chunk.origins, &origins));
        assert_eq!(chunk.origins.allocation_bytes(), origin_bytes);
        assert_eq!(origins.instruction(0, super::super::origins::FailureStage::Operation), Some(2));
        assert_eq!(origins.instruction(0, super::super::origins::FailureStage::Store), Some(3));
        let cloned = chunk.clone();
        assert!(Arc::ptr_eq(&cloned.origins, &origins));
        assert_ne!(cloned.ops.as_ptr(), chunk.ops.as_ptr(), "only immutable metadata is shared");
    }

    #[test]
    fn operation_storage_reclaims_large_slack_without_resizing_tiny_builders() {
        let mut small = Vec::with_capacity(4);
        small.push(JitOp::Halt);
        let small_capacity = small.capacity();
        let small_pointer = small.as_ptr();
        let small = JitOps::freeze(small);
        assert_eq!(small.0.capacity(), small_capacity);
        assert_eq!(small.as_ptr(), small_pointer);

        let mut large = Vec::with_capacity(1024);
        large.push(JitOp::Halt);
        let large = JitOps::freeze(large);
        assert_eq!(large.0.capacity(), large.len());
        assert_eq!(large[0], JitOp::Halt);
    }

    #[test]
    fn operation_storage_enforces_the_slack_bound_at_installation() {
        let first_reclaimed = MIN_SPARE_OP_BYTES_TO_RECLAIM.div_ceil(size_of::<JitOp>());
        for length in [0, 1, 5, 256] {
            for spare in [0, first_reclaimed - 1, first_reclaimed, first_reclaimed + 1, 1024] {
                let mut ops = Vec::with_capacity(length + spare);
                ops.resize(length, JitOp::Halt);
                let capacity = ops.capacity();
                let pointer = ops.as_ptr();
                let spare_bytes = (capacity - length) * size_of::<JitOp>();

                let frozen = JitOps::freeze(ops);
                assert_eq!(frozen.len(), length);
                assert!(frozen.iter().all(|op| *op == JitOp::Halt));
                if spare_bytes < MIN_SPARE_OP_BYTES_TO_RECLAIM {
                    assert_eq!(frozen.0.capacity(), capacity);
                    assert_eq!(frozen.as_ptr(), pointer);
                } else {
                    assert_eq!(frozen.0.capacity(), length);
                }
                assert!(frozen.reserved_bytes() - size_of_val(&*frozen) < MIN_SPARE_OP_BYTES_TO_RECLAIM);
            }
        }
    }

    #[test]
    fn fuses_slot_immediate_compare_and_conditional_jump() {
        for compare in [Op::Lt, Op::Lte, Op::Gt, Op::Gte, Op::Eq, Op::Ne] {
            let code = [
                Instr::new(Op::Load, 0, 0),
                Instr::new(Op::PushInt, 10, 0),
                Instr::new(compare, 0, 0),
                Instr::new(Op::JumpIfZero, 5, 0),
                Instr::new(Op::PushInt, 1, 0),
                Instr::new(Op::Halt, 0, 0),
            ];

            let ops = compile_ops(&code).unwrap();
            assert_eq!(ops.len(), 3);
            assert_eq!(ops[0].listing(), format!("slot.cmp.{}.i.jz 0 10 2", compare.name().to_ascii_lowercase()));
        }
    }

    #[test]
    fn branch_target_inside_compare_sequence_prevents_fusion() {
        let code = [
            Instr::new(Op::Jump, 2, 0),
            Instr::new(Op::Load, 0, 0),
            Instr::new(Op::PushInt, 10, 0),
            Instr::new(Op::Lt, 0, 0),
            Instr::new(Op::JumpIfZero, 5, 0),
            Instr::new(Op::Halt, 0, 0),
        ];

        let ops = compile_ops(&code).unwrap();
        assert!(matches!(ops[1], JitOp::Load(0)));
        assert!(matches!(ops[2], JitOp::PushInt(10)));
        assert!(matches!(ops[3], JitOp::Compare(Op::Lt)));
        assert!(matches!(ops[4], JitOp::JumpIfZero(5)));
    }

    #[test]
    fn fuses_slot_immediate_multiply_and_divide() {
        for (op, expected) in [
            (Op::Mul, JitOp::PushMulSlotInt(0, -3)),
            (Op::Div, JitOp::PushDivSlotInt(0, -3)),
        ] {
            let code = [
                Instr::new(Op::Load, 0, 0),
                Instr::new(Op::PushInt, -3, 0),
                Instr::new(op, 0, 0),
                Instr::new(Op::Print, 0, 0),
                Instr::new(Op::Halt, 0, 0),
            ];
            let ops = compile_ops(&code).unwrap();
            assert_eq!(ops.len(), 3);
            assert_eq!(ops[0], expected);
        }
    }

    #[test]
    fn fuses_slot_move_and_in_place_multiply_and_divide() {
        let move_code = [
            Instr::new(Op::Load, 0, 0),
            Instr::new(Op::Store, 1, 0),
            Instr::new(Op::Halt, 0, 0),
        ];
        assert_eq!(compile_ops(&move_code).unwrap()[0], JitOp::MoveSlot(0, 1));

        for (op, expected) in [(Op::Mul, JitOp::MulSlotInt(0, -3)), (Op::Div, JitOp::DivSlotInt(0, -3))] {
            let code = [
                Instr::new(Op::Load, 0, 0),
                Instr::new(Op::PushInt, -3, 0),
                Instr::new(op, 0, 0),
                Instr::new(Op::Store, 0, 0),
                Instr::new(Op::Halt, 0, 0),
            ];
            assert_eq!(compile_ops(&code).unwrap()[0], expected);
        }
    }

    #[test]
    fn fuses_slot_zero_jump_and_remaps_its_target() {
        let code = [
            Instr::new(Op::Load, 0, 0),
            Instr::new(Op::JumpIfZero, 3, 0),
            Instr::new(Op::PushInt, 1, 0),
            Instr::new(Op::Halt, 0, 0),
        ];
        let ops = compile_ops(&code).unwrap();
        assert_eq!(ops.len(), 3);
        assert_eq!(ops[0], JitOp::JumpIfZeroSlot(0, 2));
    }

    #[test]
    fn fuses_array_length_loop_guard_without_exposing_internal_targets() {
        let len = builtin_index("len").expect("known builtin") as i64;
        let code = [
            Instr::new(Op::Load, 0, 0),
            Instr::new(Op::Builtin, len, 1),
            Instr::new(Op::PushInt, 0, 0),
            Instr::new(Op::Gt, 0, 0),
            Instr::new(Op::JumpIfZero, 6, 0),
            Instr::new(Op::Halt, 0, 0),
            Instr::new(Op::Halt, 0, 0),
        ];
        let ops = compile_ops(&code).unwrap();
        assert_eq!(ops[0], JitOp::ArrayLenSlotJumpIfZero(0, 2));

        let guarded = [
            Instr::new(Op::Jump, 2, 0),
            Instr::new(Op::Load, 0, 0),
            Instr::new(Op::Builtin, len, 1),
            Instr::new(Op::PushInt, 0, 0),
            Instr::new(Op::Gt, 0, 0),
            Instr::new(Op::JumpIfZero, 6, 0),
            Instr::new(Op::Halt, 0, 0),
        ];
        let guarded_ops = compile_ops(&guarded).unwrap();
        assert!(matches!(guarded_ops[1], JitOp::Load(0)));
    }

    #[test]
    fn fuses_map_get_add_store_for_local_i64_loop_state() {
        let map_get = builtin_index("map_get").expect("known builtin") as i64;
        let code = [
            Instr::new(Op::Load, 0, 0),
            Instr::new(Op::Load, 1, 0),
            Instr::new(Op::Load, 2, 0),
            Instr::new(Op::Builtin, map_get, 2),
            Instr::new(Op::Add, 0, 0),
            Instr::new(Op::Store, 0, 0),
            Instr::new(Op::Halt, 0, 0),
        ];
        let ops = compile_ops(&code).unwrap();
        assert_eq!(ops[0].listing(), "map.get.add.slots 0 1 2");

        let guarded = [
            Instr::new(Op::Jump, 2, 0),
            Instr::new(Op::Load, 0, 0),
            Instr::new(Op::Load, 1, 0),
            Instr::new(Op::Load, 2, 0),
            Instr::new(Op::Builtin, map_get, 2),
            Instr::new(Op::Add, 0, 0),
            Instr::new(Op::Store, 0, 0),
            Instr::new(Op::Halt, 0, 0),
        ];
        let guarded_ops = compile_ops(&guarded).unwrap();
        assert!(matches!(guarded_ops[1], JitOp::Load(0)));
    }

    #[test]
    fn fuses_map_set_with_slot_immediate_product() {
        let map_set = builtin_index("map_set").expect("known builtin") as i64;
        let code = [
            Instr::new(Op::Load, 0, 0),
            Instr::new(Op::Load, 1, 0),
            Instr::new(Op::Load, 1, 0),
            Instr::new(Op::PushInt, 3, 0),
            Instr::new(Op::Mul, 0, 0),
            Instr::new(Op::Builtin, map_set, 3),
            Instr::new(Op::Store, 2, 0),
            Instr::new(Op::Halt, 0, 0),
        ];
        let ops = compile_ops(&code).unwrap();
        assert_eq!(ops[0].listing(), "map.set.mul.slots 0 1 1 2 3");
    }

    #[test]
    fn branch_target_inside_slot_move_or_update_prevents_fusion() {
        let move_code = [
            Instr::new(Op::Jump, 2, 0),
            Instr::new(Op::Load, 0, 0),
            Instr::new(Op::Store, 1, 0),
            Instr::new(Op::Halt, 0, 0),
        ];
        let move_ops = compile_ops(&move_code).unwrap();
        assert!(matches!(move_ops[1], JitOp::Load(0)));
        assert!(matches!(move_ops[2], JitOp::Store(1)));

        let update_code = [
            Instr::new(Op::Jump, 2, 0),
            Instr::new(Op::Load, 0, 0),
            Instr::new(Op::PushInt, 3, 0),
            Instr::new(Op::Mul, 0, 0),
            Instr::new(Op::Store, 0, 0),
            Instr::new(Op::Halt, 0, 0),
        ];
        let update_ops = compile_ops(&update_code).unwrap();
        assert!(matches!(update_ops[1], JitOp::Load(0)));
        assert!(matches!(update_ops[2], JitOp::PushInt(3)));
        assert!(matches!(update_ops[3], JitOp::Mul));
        assert!(matches!(update_ops[4], JitOp::Store(0)));

        let zero_jump_code = [
            Instr::new(Op::Jump, 2, 0),
            Instr::new(Op::Load, 0, 0),
            Instr::new(Op::JumpIfZero, 3, 0),
            Instr::new(Op::Halt, 0, 0),
        ];
        let zero_jump_ops = compile_ops(&zero_jump_code).unwrap();
        assert!(matches!(zero_jump_ops[1], JitOp::Load(0)));
        assert!(matches!(zero_jump_ops[2], JitOp::JumpIfZero(3)));
    }

    #[test]
    fn branch_target_inside_slot_arithmetic_prevents_fusion() {
        let code = [
            Instr::new(Op::Jump, 2, 0),
            Instr::new(Op::Load, 0, 0),
            Instr::new(Op::PushInt, 3, 0),
            Instr::new(Op::Mul, 0, 0),
            Instr::new(Op::Halt, 0, 0),
        ];
        let ops = compile_ops(&code).unwrap();
        assert!(matches!(ops[1], JitOp::Load(0)));
        assert!(matches!(ops[2], JitOp::PushInt(3)));
        assert!(matches!(ops[3], JitOp::Mul));
    }
}
