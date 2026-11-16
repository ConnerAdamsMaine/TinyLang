//! Original bytecode sites survive instruction selection and in-place quickening.
//! The interpreter reads these cold tables only after a runtime failure.

use std::sync::Arc;

#[cfg(feature = "testing-hooks")]
use super::preparation_profile::{PreparationPhase, PreparationSpan};
use crate::JitOp;
use crate::language::bytecode::control_flow::BytecodeOffset;

#[derive(Debug, Default, Clone, Copy)]
pub(super) enum FailureStage {
    #[default]
    Operation,
    Secondary,
    Store,
}

/// A construction-time description, not an executable instruction payload.
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct OpOrigin {
    operation: BytecodeOffset,
    secondary: BytecodeOffset,
    store: BytecodeOffset,
}

impl OpOrigin {
    pub(super) fn instruction(offset: BytecodeOffset) -> Self {
        Self {
            operation: offset,
            secondary: offset,
            store: offset,
        }
    }

    pub(super) fn fused(operation: BytecodeOffset, secondary: BytecodeOffset, store: BytecodeOffset) -> Self {
        Self {
            operation,
            secondary,
            store,
        }
    }

    pub(super) fn operation(self) -> BytecodeOffset {
        self.operation
    }

    pub(super) fn with_store(self, store: BytecodeOffset) -> Self {
        Self { store, ..self }
    }
}

#[derive(Debug)]
struct FusedOrigin {
    instruction: usize,
    secondary: BytecodeOffset,
    store: BytecodeOffset,
}

#[derive(Debug)]
pub(super) struct ChunkOrigins {
    operations: Box<[BytecodeOffset]>,
    fused: Box<[FusedOrigin]>,
}

impl ChunkOrigins {
    pub(super) fn instruction(&self, instruction: usize, stage: FailureStage) -> Option<usize> {
        let original = *self.operations.get(instruction)?;
        if matches!(stage, FailureStage::Operation) {
            return Some(original.index());
        }
        let Ok(index) = self
            .fused
            .binary_search_by_key(&instruction, |origin| origin.instruction)
        else {
            return Some(original.index());
        };
        Some(
            match stage {
                FailureStage::Operation => original,
                FailureStage::Secondary => self.fused[index].secondary,
                FailureStage::Store => self.fused[index].store,
            }
            .index(),
        )
    }

    pub(super) fn allocation_bytes(&self) -> usize {
        size_of::<Self>()
            .saturating_add(size_of_val(&*self.operations))
            .saturating_add(size_of_val(&*self.fused))
    }
}

/// Instruction emission owns both streams so no push can omit its origin.
/// Only the instruction elements remain mutable after installation.
#[derive(Default)]
pub(super) struct LoweredChunk {
    ops: Vec<JitOp>,
    operations: Vec<BytecodeOffset>,
    fused: Vec<FusedOrigin>,
}

impl LoweredChunk {
    pub(super) fn with_capacity(capacity: usize) -> Self {
        Self {
            ops: Vec::with_capacity(capacity),
            operations: Vec::with_capacity(capacity),
            fused: Vec::new(),
        }
    }

    pub(super) fn push(&mut self, op: JitOp, origin: OpOrigin) {
        if origin.secondary != origin.operation || origin.store != origin.operation {
            self.fused.push(FusedOrigin {
                instruction: self.ops.len(),
                secondary: origin.secondary,
                store: origin.store,
            });
        }
        self.operations.push(origin.operation);
        self.ops.push(op);
    }

    pub(super) fn len(&self) -> usize {
        self.ops.len()
    }

    pub(super) fn ops_mut(&mut self) -> &mut [JitOp] {
        &mut self.ops
    }

    pub(super) fn finish(self) -> (Vec<JitOp>, Arc<ChunkOrigins>) {
        #[cfg(feature = "testing-hooks")]
        let _installation = PreparationSpan::start(PreparationPhase::OriginInstallation);
        (
            self.ops,
            Arc::new(ChunkOrigins {
                operations: self.operations.into_boxed_slice(),
                fused: self.fused.into_boxed_slice(),
            }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offset(index: usize) -> BytecodeOffset {
        BytecodeOffset::parse(index).expect("bytecode offset")
    }

    #[test]
    fn sparse_fusion_sites_keep_each_failure_stage_and_instruction_boundary() {
        let mut lowered = LoweredChunk::with_capacity(16);
        lowered.push(JitOp::PushInt(7), OpOrigin::instruction(offset(2)));
        lowered.push(JitOp::Add, OpOrigin::fused(offset(7), offset(8), offset(9)));
        lowered.push(JitOp::Pop, OpOrigin::instruction(offset(10)));
        lowered.push(JitOp::Store(0), OpOrigin::instruction(offset(13)).with_store(offset(14)));
        let (ops, origins) = lowered.finish();
        assert_eq!(origins.operations.len(), ops.len());
        assert_eq!(origins.fused.len(), 2, "ordinary operations need no auxiliary record");
        for (instruction, expected) in [(0, [2, 2, 2]), (1, [7, 8, 9]), (2, [10, 10, 10]), (3, [13, 13, 14])] {
            for (stage, expected) in [FailureStage::Operation, FailureStage::Secondary, FailureStage::Store]
                .into_iter()
                .zip(expected)
            {
                assert_eq!(origins.instruction(instruction, stage), Some(expected));
            }
        }
        for stage in [FailureStage::Operation, FailureStage::Secondary, FailureStage::Store] {
            assert_eq!(origins.instruction(ops.len(), stage), None);
            assert_eq!(origins.instruction(usize::MAX, stage), None);
        }
        assert_eq!(size_of::<BytecodeOffset>(), 4);
        assert_eq!(
            origins.allocation_bytes(),
            size_of::<ChunkOrigins>() + 4 * size_of::<BytecodeOffset>() + 2 * size_of::<FusedOrigin>(),
            "frozen metadata charges its exact extent, not the builder capacity"
        );
    }

    #[test]
    fn empty_origin_tables_have_no_backing_storage_or_invented_site() {
        let (ops, origins) = LoweredChunk::default().finish();
        assert!(ops.is_empty());
        assert_eq!(origins.allocation_bytes(), size_of::<ChunkOrigins>());
        assert_eq!(origins.instruction(0, FailureStage::Operation), None);
    }
}
