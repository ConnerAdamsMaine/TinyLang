//! A frame-resume borrow of installed code and its adaptive accounting.

use super::feedback::{FeedbackChange, MAX_FEEDBACK_BYTES, ObservedTypes};
use crate::{JitChunk, JitExecutionProfile, JitOp, JitStats, Result, TinyOneError, Value};

/// Resolving an installed chunk once keeps table lookup out of instruction
/// fetch and feedback recording. This borrow ends on call, return or error;
/// lazy installation and recursive resumption therefore see current code.
pub(super) struct ChunkExecution<'a> {
    chunk:          &'a mut JitChunk,
    threshold:      u16,
    stats:          &'a mut JitStats,
    profile:        Option<&'a mut JitExecutionProfile>,
    feedback_bytes: &'a mut usize,
}

impl<'a> ChunkExecution<'a> {
    pub(super) fn new(
        chunk: &'a mut JitChunk,
        threshold: u16,
        stats: &'a mut JitStats,
        profile: Option<&'a mut JitExecutionProfile>,
        feedback_bytes: &'a mut usize,
    ) -> Self {
        Self {
            chunk,
            threshold,
            stats,
            profile,
            feedback_bytes,
        }
    }

    pub(super) fn name(&self) -> &str {
        &self.chunk.name
    }

    pub(super) fn instruction(&self, pc: usize) -> Result<JitOp> {
        self.chunk
            .ops
            .get(pc)
            .copied()
            .ok_or_else(|| TinyOneError::runtime(format!("Invalid program counter in {}", self.chunk.name)))
    }

    pub(super) fn record_back_edge(&mut self, op_pc: usize, target: usize) {
        if self.threshold == 0 || target >= op_pc {
            return;
        }
        self.stats.hot_back_edges += 1;
        let Some(counter) = self.chunk.edge_counts.get_mut(op_pc) else {
            return;
        };
        // Even threshold u16::MAX activates a range only once. Saturation
        // must not repeatedly allocate/scan a range whose budget was denied.
        if *counter >= self.threshold {
            return;
        }
        *counter = counter.saturating_add(1);
        if *counter != self.threshold {
            return;
        }
        let available_bytes = MAX_FEEDBACK_BYTES.saturating_sub(*self.feedback_bytes);
        let activated = self.chunk.activate_range(target, op_pc + 1, available_bytes);
        *self.feedback_bytes += activated.allocated_bytes;
        if activated.sampling_sites + activated.branches > 0 {
            self.stats.hot_ranges += 1;
            self.stats.quickened_ops += activated.branches;
        }
    }

    pub(super) fn observe_types(&mut self, op_pc: usize, types: ObservedTypes) {
        let change = self.chunk.observe(op_pc, types);
        if change == FeedbackChange::Specialize {
            self.stats.quickened_ops += 1;
        }
        if let Some(profile) = &mut self.profile {
            profile.feedback_samples = profile.feedback_samples.saturating_add(1);
            if change == FeedbackChange::Specialize {
                profile.specialization_promotions = profile.specialization_promotions.saturating_add(1);
            }
        }
    }

    pub(super) fn record_i64_hit(&mut self, op_pc: usize) {
        self.chunk.record_hit(op_pc);
        if let Some(profile) = &mut self.profile {
            profile.specialization_hits = profile.specialization_hits.saturating_add(1);
        }
    }

    pub(super) fn record_binary_guard(&mut self, op_pc: usize, lhs: &Value, rhs: &Value) {
        if matches!((lhs, rhs), (Value::I64(_), Value::I64(_))) {
            self.record_i64_hit(op_pc);
        } else {
            self.record_i64_miss(op_pc, ObservedTypes::binary(lhs, rhs));
        }
    }

    pub(super) fn record_i64_miss(&mut self, op_pc: usize, types: ObservedTypes) {
        let change = self.chunk.record_miss(op_pc, types);
        if let Some(profile) = &mut self.profile {
            profile.specialization_misses = profile.specialization_misses.saturating_add(1);
            if matches!(change, FeedbackChange::Resample | FeedbackChange::Generic) {
                profile.specialization_demotions = profile.specialization_demotions.saturating_add(1);
            }
        }
    }

    pub(super) fn record_execution_op(&mut self, op: JitOp, stack_depth: usize) {
        if let Some(profile) = &mut self.profile {
            profile.record(op, stack_depth);
        }
    }
}
