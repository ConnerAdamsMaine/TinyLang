//! Cold source origins owned alongside bytecode, never inside instructions.

use std::collections::HashSet;
use std::sync::Arc;

use super::SourceMap;
use crate::{Program, TinyLangError};

/// Kept only while an error unwinds. The first failing frame owns the site,
/// even when that frame came from an artifact with no source metadata.
#[derive(Debug, Clone, Copy)]
pub(crate) enum BytecodeSite {
    Main { instruction: usize },
    Function { index: usize, instruction: usize },
}

impl BytecodeSite {
    pub(crate) fn new(function: Option<usize>, instruction: usize) -> Self {
        match function {
            Some(index) => Self::Function { index, instruction },
            None => Self::Main { instruction },
        }
    }

    pub(crate) fn annotate(self, program: &Program, error: TinyLangError) -> TinyLangError {
        let TinyLangError::Runtime(message) = error else {
            return error;
        };
        let (source, instruction) = match self {
            Self::Main { instruction } => (program.source.as_deref(), instruction),
            Self::Function { index, instruction } => {
                (
                    program
                        .functions
                        .get(index)
                        .and_then(|function| function.source.as_deref()),
                    instruction,
                )
            }
        };
        match source.and_then(|source| source.format(instruction, &message)) {
            Some(diagnostic) => TinyLangError::Runtime(diagnostic),
            None => TinyLangError::Runtime(message),
        }
    }
}

/// A byte offset, not a character index. Keep the host width: the public
/// string compiler does not impose the file loader's source-size limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SourceOffset(pub(crate) usize);

#[derive(Debug, Clone)]
pub(crate) struct BytecodeSource {
    source:  Arc<SourceMap>,
    offsets: Box<[SourceOffset]>,
}

impl BytecodeSource {
    pub(crate) fn new(source: Arc<SourceMap>, offsets: Vec<SourceOffset>) -> Arc<Self> {
        Arc::new(Self {
            source,
            offsets: offsets.into_boxed_slice(),
        })
    }

    pub(crate) fn format(&self, instruction: usize, message: &str) -> Option<String> {
        let SourceOffset(offset) = *self.offsets.get(instruction)?;
        Some(self.source.format(message, offset, offset.saturating_add(1)))
    }

    pub(crate) fn instruction_count(&self) -> usize {
        self.offsets.len()
    }

    pub(crate) fn offsets(&self) -> &[SourceOffset] {
        &self.offsets
    }

    /// The peephole pass supplies a total, forward-compacting old-to-new map.
    /// The final member of a folded range is its operator, so it owns the
    /// replacement's origin. Later instructions keep their own source sites.
    pub(crate) fn remap(&mut self, old_to_new: &[usize]) {
        for (old, &new) in old_to_new.iter().enumerate() {
            self.offsets[new] = self.offsets[old];
        }
    }

    pub(crate) fn truncate(&mut self, length: usize) {
        if length == self.offsets.len() {
            return;
        }
        let mut offsets = std::mem::take(&mut self.offsets).into_vec();
        offsets.truncate(length);
        self.offsets = offsets.into_boxed_slice();
    }

    pub(crate) fn allocation_bytes(&self) -> usize {
        size_of::<Self>().saturating_add(size_of_val(&*self.offsets))
    }

    pub(crate) fn source_owner(&self) -> &Arc<SourceMap> {
        &self.source
    }
}

impl SourceMap {
    pub(crate) fn allocation_bytes(&self) -> usize {
        size_of::<Self>()
            .saturating_add(self.filename.capacity())
            .saturating_add(self.source.capacity())
    }
}

impl crate::Program {
    /// Count each diagnostic allocation once within this program. Separate
    /// verified owners still receive inclusive charges, as with bytecode.
    pub(crate) fn source_allocation_bytes(&self) -> usize {
        let mut chunks = HashSet::new();
        let mut sources = HashSet::new();
        let mut bytes = 0usize;
        for chunk in self
            .source
            .iter()
            .chain(self.functions.iter().filter_map(|function| function.source.as_ref()))
        {
            if chunks.insert(Arc::as_ptr(chunk)) {
                bytes = bytes.saturating_add(chunk.allocation_bytes());
            }
            if sources.insert(Arc::as_ptr(chunk.source_owner())) {
                bytes = bytes.saturating_add(chunk.source_owner().allocation_bytes());
            }
        }
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Op,
        PeepholeOptimizer,
        VerifiedProgram,
        compile_source_unoptimized_verified_with_filename,
        compile_source_verified_with_filename,
        run_verified_program,
    };

    #[test]
    fn origins_share_one_file_owner_and_match_each_chunk_extent() {
        let verified = compile_source_verified_with_filename(
            "fn first(value) { return value / 0 } fn second(value) { return value + 1 } print first(2)",
            "origins.to",
        )
        .expect("fixture");
        let program = verified.program();
        let main = program.source.as_ref().expect("root source");
        assert_eq!(main.instruction_count(), program.code.len());
        let mut expected_bytes = main.allocation_bytes() + main.source_owner().allocation_bytes();
        for function in &program.functions {
            let source = function.source.as_ref().expect("function source");
            assert_eq!(source.instruction_count(), function.code.len());
            assert!(Arc::ptr_eq(source.source_owner(), main.source_owner()));
            expected_bytes += source.allocation_bytes();
        }
        assert_eq!(program.source_allocation_bytes(), expected_bytes);

        let mut cloned = program.clone();
        cloned.functions.push(cloned.functions[0].clone());
        assert_eq!(cloned.source_allocation_bytes(), expected_bytes, "shared chunk metadata is charged once");
    }

    #[test]
    fn optimizing_a_clone_preserves_original_offsets_and_unchanged_chunk_owners() {
        let text = "fn idle() { return 1 } let value = 0 print (1 + 2 * 3) / value";
        let original = compile_source_unoptimized_verified_with_filename(text, "fold.to").expect("fixture");
        let root = original.program().source.as_ref().expect("source");
        let before = root.offsets.clone();
        let optimized = PeepholeOptimizer::optimize(original.program().clone());
        let compiled = optimized.source.as_ref().expect("optimized source");
        assert!(optimized.code.len() < original.program().code.len());
        assert_eq!(compiled.instruction_count(), optimized.code.len());
        assert_eq!(root.offsets, before, "copy-on-write must not corrupt another verified owner");
        assert!(
            Arc::ptr_eq(
                original.program().functions[0]
                    .source
                    .as_ref()
                    .expect("original function source"),
                optimized.functions[0]
                    .source
                    .as_ref()
                    .expect("optimized function source"),
            ),
            "an unchanged chunk should not clone its origin table"
        );
        let division = optimized
            .code
            .iter()
            .position(|instruction| instruction.op == Op::Div)
            .expect("division");
        let offset = text.find('/').expect("division token");
        assert_eq!(compiled.offsets[division], SourceOffset(offset));
        let verified = VerifiedProgram::verify(optimized).expect("optimized verification");
        let error = run_verified_program(&verified, "vm", &mut Vec::new(), Vec::new()).expect_err("division by zero");
        assert!(
            error
                .to_string()
                .starts_with(&format!("fold.to:1:{}: Division by zero", offset + 1))
        );
    }

    #[test]
    fn source_context_does_not_change_content_identity_or_artifact_bytes() {
        let first = compile_source_verified_with_filename("fn f() { return 1 / 0 } print f()", "first.to")
            .expect("first fixture");
        let second = compile_source_verified_with_filename("\nfn f() { return 1 / 0 }\nprint f()", "second.to")
            .expect("second fixture");
        assert_eq!(first.program(), second.program());
        assert_eq!(first.fingerprint(), second.fingerprint());
        assert_eq!(first.program().to_artifact(), second.program().to_artifact());
        let bytes = first.program().to_binary_artifact().expect("binary artifact");
        assert_eq!(bytes, second.program().to_binary_artifact().expect("second binary artifact"));
        for mut decoded in [
            Program::from_artifact(first.program().to_artifact()).expect("JSON decoding"),
            Program::from_binary_artifact(&bytes).expect("binary decoding"),
        ] {
            assert!(decoded.source.is_none());
            assert!(decoded.functions.iter().all(|function| function.source.is_none()));
            assert_eq!(decoded.fingerprint(), first.fingerprint());
            assert_eq!(decoded.source_allocation_bytes(), 0);
            assert!(!decoded.policy_trusted, "debug metadata must not lend authority");
            decoded.policy_trusted = first.program().policy_trusted;
            assert_eq!(decoded, *first.program(), "equality still represents bytecode and policy");
        }
    }

    #[test]
    fn source_offsets_keep_host_width_and_invalid_instruction_sites_do_not_fabricate_locations() {
        let chunk = BytecodeSource::new(Arc::new(SourceMap::new("", "empty.to")), vec![SourceOffset(usize::MAX)]);
        assert_eq!(chunk.offsets[0], SourceOffset(usize::MAX));
        assert_eq!(size_of::<SourceOffset>(), size_of::<usize>());
        assert!(chunk.format(1, "failure").is_none());
        assert!(
            chunk
                .format(0, "failure")
                .expect("clamped diagnostic")
                .starts_with("empty.to:1:1:")
        );
    }

    #[test]
    fn a_sourceless_callee_is_not_relabelled_as_its_sourceful_caller() {
        let mut program = compile_source_verified_with_filename("fn f() { return 1 / 0 } print f()", "caller.to")
            .expect("fixture")
            .into_program();
        program.functions[0].source = None;
        let verified = VerifiedProgram::verify(program).expect("verification");
        for mode in ["vm", "jit"] {
            let error =
                run_verified_program(&verified, mode, &mut Vec::new(), Vec::new()).expect_err("division by zero");
            assert_eq!(error.to_string(), "Division by zero", "{mode}");
        }
    }

    #[test]
    #[ignore = "source-retention diagnostic; run explicitly with --ignored --nocapture"]
    fn report_bytecode_source_storage() {
        for (name, source) in [
            ("tiny", "print 42".to_owned()),
            ("functions", "fn a() { return 1 } fn b() { return 2 } print a() + b()".to_owned()),
            ("line_heavy", format!("{}print 42", "# source header\n".repeat(4096))),
        ] {
            let verified = compile_source_verified_with_filename(&source, "storage.to").expect("storage fixture");
            let program = verified.program();
            let instruction_count = program.code.len()
                + program
                    .functions
                    .iter()
                    .map(|function| function.code.len())
                    .sum::<usize>();
            eprintln!(
                "source {name}: source_bytes={} chunks={} origins={} offset_bytes={} diagnostic_retained_bytes={} verified_retained_bytes={}",
                source.len(),
                program.functions.len() + 1,
                instruction_count,
                instruction_count * size_of::<SourceOffset>(),
                program.source_allocation_bytes(),
                verified.retained_bytes(),
            );
        }
    }
}
