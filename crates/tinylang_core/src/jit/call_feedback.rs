//! Inline dynamic-callee observations and verified call-entry metadata.
//!
//! These states contain integer identities and immutable layout facts only.
//! They occupy existing JitOp storage, never retain captures, and never grow a
//! side table. Unstable sites have a finite sampling/rewrite budget.

use super::frame::CallRequest;
use crate::{JitOp, JitProgram};

const STABLE_CALLS: u8 = 8;
const MAX_SAMPLES: u8 = 32;
const MAX_MISSES: u8 = 4;
const HITS_TO_CLEAR_MISSES: u8 = 32;

#[derive(Clone, Copy)]
pub(super) struct CallSite {
    pub(super) chunk_index: usize,
    pub(super) op_pc:       usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FunctionId(u16);

impl FunctionId {
    fn from_index(index: usize) -> Option<Self> {
        u16::try_from(index).ok().map(Self)
    }

    fn index(self) -> usize {
        usize::from(self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CallSample {
    candidate: FunctionId,
    streak:    u8,
    samples:   u8,
}

enum SampleChange {
    Continue(CallSample),
    Specialize,
    Generic,
}

impl CallSample {
    fn first(candidate: FunctionId) -> Self {
        Self {
            candidate,
            streak: 1,
            samples: 1,
        }
    }

    fn observe(mut self, observed: FunctionId) -> SampleChange {
        self.samples = self.samples.saturating_add(1);
        if self.candidate == observed {
            self.streak = self.streak.saturating_add(1);
        } else {
            self.candidate = observed;
            self.streak = 1;
        }
        if self.streak >= STABLE_CALLS {
            SampleChange::Specialize
        } else if self.samples >= MAX_SAMPLES {
            SampleChange::Generic
        } else {
            SampleChange::Continue(self)
        }
    }
}

/// Constructed only after successful entry into an installed verified callee.
/// Counts use u32: 65,536 slots/stack entries must not truncate to zero.
/// Reachable dynamic argument counts also need room for the callable; wider
/// cold/unreachable encodings remain full-width generic operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GuardedCall {
    function:       FunctionId,
    arg_count:      u16,
    param_count:    u32,
    slot_count:     u32,
    stack_capacity: u32,
    hits:           u8,
    misses:         u8,
}

enum GuardChange {
    Keep,
    Rewrite(GuardedCall),
    Generic,
}

impl GuardedCall {
    /// Declining optional caching is deliberate: it must never add an error
    /// to a call which already completed normal validation and frame entry.
    fn from_entered(program: &JitProgram, function_index: usize, arg_count: usize) -> Option<Self> {
        let function = program.verified_program.program().functions.get(function_index)?;
        let chunk_index = function_index.checked_add(1)?;
        program.chunks.get(chunk_index)?.as_ref()?;
        let certificate = program.verified_program.chunk_certificate(chunk_index).ok()?;
        Some(Self {
            function:       FunctionId::from_index(function_index)?,
            arg_count:      u16::try_from(arg_count).ok()?,
            param_count:    u32::try_from(function.param_count).ok()?,
            slot_count:     u32::try_from(function.slot_count).ok()?,
            stack_capacity: u32::try_from(certificate.max_stack_depth).ok()?,
            hits:           0,
            misses:         0,
        })
    }

    pub(super) fn matches(self, function_index: usize, total_args: usize) -> bool {
        self.function.index() == function_index && self.param_count as usize == total_args
    }

    pub(super) fn function_index(self) -> usize {
        self.function.index()
    }

    pub(super) fn arg_count(self) -> usize {
        usize::from(self.arg_count)
    }

    pub(super) fn slot_count(self) -> usize {
        self.slot_count as usize
    }

    pub(super) fn stack_capacity(self) -> usize {
        self.stack_capacity as usize
    }

    fn record(mut self, matched: bool) -> GuardChange {
        if matched {
            if self.hits >= HITS_TO_CLEAR_MISSES {
                return GuardChange::Keep;
            }
            self.hits = self.hits.saturating_add(1);
            if self.hits == HITS_TO_CLEAR_MISSES {
                self.misses = 0;
            }
        } else {
            self.hits = 0;
            self.misses = self.misses.saturating_add(1);
            if self.misses >= MAX_MISSES {
                return GuardChange::Generic;
            }
        }
        GuardChange::Rewrite(self)
    }
}

impl JitProgram {
    pub(super) fn observe_call_entry(&mut self, site: CallSite, request: CallRequest, function_index: usize) {
        // Unseen/sample opcodes exist only in chunks built with feedback
        // enabled. Ordinary CallValue is also the permanent generic fallback.
        if !matches!(request, CallRequest::UnseenOperand { .. } | CallRequest::Sampled { .. }) {
            return;
        }
        let Some(observed) = FunctionId::from_index(function_index) else {
            return;
        };
        let (arg_count, change) = match request {
            CallRequest::UnseenOperand { arg_count } => {
                (arg_count, SampleChange::Continue(CallSample::first(observed)))
            }
            CallRequest::Sampled { arg_count, sample } => (arg_count, sample.observe(observed)),
            _ => return,
        };
        if let Some(profile) = &mut self.execution_profile {
            profile.call_feedback_samples = profile.call_feedback_samples.saturating_add(1);
        }
        let replacement = match change {
            SampleChange::Continue(sample) => JitOp::SampleCallValue(arg_count, sample),
            SampleChange::Specialize => {
                if let Some(guard) = GuardedCall::from_entered(self, function_index, arg_count) {
                    self.stats.quickened_ops += 1;
                    if let Some(profile) = &mut self.execution_profile {
                        profile.call_specializations = profile.call_specializations.saturating_add(1);
                    }
                    JitOp::GuardedCallValue(guard)
                } else {
                    JitOp::CallValue(arg_count)
                }
            }
            SampleChange::Generic => JitOp::CallValue(arg_count),
        };
        self.replace_call_site(site, replacement);
    }

    pub(super) fn record_call_guard(&mut self, site: CallSite, guard: GuardedCall, matched: bool) {
        if let Some(profile) = &mut self.execution_profile {
            if matched {
                profile.call_guard_hits = profile.call_guard_hits.saturating_add(1);
            } else {
                profile.call_guard_misses = profile.call_guard_misses.saturating_add(1);
            }
        }
        match guard.record(matched) {
            GuardChange::Keep => {}
            GuardChange::Rewrite(guard) => self.replace_call_site(site, JitOp::GuardedCallValue(guard)),
            GuardChange::Generic => {
                if let Some(profile) = &mut self.execution_profile {
                    profile.call_deoptimizations = profile.call_deoptimizations.saturating_add(1);
                }
                self.replace_call_site(site, JitOp::CallValue(guard.arg_count()));
            }
        }
    }

    pub(super) fn reject_call_target(&mut self, site: CallSite, request: CallRequest) {
        if let CallRequest::Guarded(guard) = request {
            self.record_call_guard(site, guard, false);
        }
    }

    fn replace_call_site(&mut self, site: CallSite, replacement: JitOp) {
        if let Some(op) = self
            .chunks
            .get_mut(site.chunk_index)
            .and_then(Option::as_mut)
            .and_then(|chunk| chunk.ops.get_mut(site.op_pc))
            && matches!(*op, JitOp::UnseenCallValue(_) | JitOp::SampleCallValue(..) | JitOp::GuardedCallValue(_))
        {
            *op = replacement;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_guard_hits_do_not_change_the_first_miss_state() {
        let verified = crate::compile_source_verified("fn target(value) { return value }").expect("source");
        let mut program = JitProgram::compile_verified(&verified).expect("main lowering");
        program.ensure_chunk(1).expect("callee installation");
        let guard = GuardedCall::from_entered(&program, 0, 1).expect("installed metadata");
        let GuardChange::Rewrite(first_miss) = guard.record(false) else {
            panic!("first miss must retain the guard");
        };
        assert_eq!(first_miss.hits, 0);
        assert_eq!(first_miss.misses, 1);
        // All initial hit counts converge on the same state at the first
        // miss, so settling a clean guard cannot change its miss budget.
        for hits in 0..=HITS_TO_CLEAR_MISSES {
            let initial = GuardedCall { hits, ..guard };
            let GuardChange::Rewrite(after_miss) = initial.record(false) else {
                panic!("initial hit count must not exhaust the miss budget");
            };
            assert_eq!(after_miss, first_miss);
        }
    }

    #[test]
    fn guard_counters_saturate_for_stable_calls_and_stop_after_four_misses() {
        let mut guard = GuardedCall {
            function:       FunctionId(4095),
            arg_count:      1,
            param_count:    65_536,
            slot_count:     65_536,
            stack_capacity: 65_536,
            hits:           0,
            misses:         1,
        };
        assert!(guard.matches(4095, 65_536));
        assert!(!guard.matches(4094, 65_536));
        assert!(!guard.matches(4095, 0));
        assert_eq!(guard.slot_count(), 65_536);
        assert_eq!(guard.stack_capacity(), 65_536);
        for _ in 0..HITS_TO_CLEAR_MISSES {
            let GuardChange::Rewrite(next) = guard.record(true) else {
                panic!("hit counter must advance")
            };
            guard = next;
        }
        assert_eq!(guard.misses, 0);
        for _ in 0..1000 {
            assert!(matches!(guard.record(true), GuardChange::Keep));
        }
        for _ in 0..MAX_MISSES - 1 {
            let GuardChange::Rewrite(next) = guard.record(false) else {
                panic!("isolated miss retains guard")
            };
            guard = next;
        }
        assert!(matches!(guard.record(false), GuardChange::Generic));
    }

    #[test]
    fn interrupted_guard_recovery_preserves_the_miss_budget() {
        for hit_run in [0, 1, HITS_TO_CLEAR_MISSES - 1] {
            let mut guard = GuardedCall {
                function:       FunctionId(0),
                arg_count:      1,
                param_count:    1,
                slot_count:     1,
                stack_capacity: 1,
                hits:           HITS_TO_CLEAR_MISSES,
                misses:         0,
            };
            for misses in 1..=MAX_MISSES {
                let change = guard.record(false);
                if misses == MAX_MISSES {
                    assert!(matches!(change, GuardChange::Generic));
                    break;
                }
                let GuardChange::Rewrite(next) = change else {
                    panic!("unexhausted miss budget must retain the guard");
                };
                guard = next;
                assert_eq!(guard.hits, 0);
                assert_eq!(guard.misses, misses);
                for hits in 1..=hit_run {
                    let GuardChange::Rewrite(next) = guard.record(true) else {
                        panic!("incomplete recovery must advance the streak");
                    };
                    guard = next;
                    assert_eq!(guard.hits, hits);
                    assert_eq!(guard.misses, misses);
                }
            }
        }
    }

    #[test]
    fn only_installed_metadata_is_cached_and_wide_arities_decline_narrowing() {
        let verified = crate::compile_source_verified("fn target(value) { return value }").expect("source");
        let mut program = JitProgram::compile_verified(&verified).expect("main lowering");
        assert!(GuardedCall::from_entered(&program, 0, 1).is_none());
        program.ensure_chunk(1).expect("private callee installation");
        let guard = GuardedCall::from_entered(&program, 0, 1).expect("installed metadata");
        assert_eq!(guard.function_index(), 0);
        assert!(guard.matches(0, 1));
        assert!(GuardedCall::from_entered(&program, 0, 65_536).is_none());
        assert!(GuardedCall::from_entered(&program, usize::MAX, 1).is_none());
        assert!(FunctionId::from_index(usize::MAX).is_none());
    }

    #[test]
    fn sampling_is_bounded_and_requires_consecutive_identities() {
        assert_eq!(size_of::<CallSample>(), 4);
        assert!(size_of::<GuardedCall>() <= 20);
        assert_eq!(size_of::<JitOp>(), 24);
        assert!(!std::mem::needs_drop::<GuardedCall>());
        let mut sample = CallSample::first(FunctionId(0));
        for count in 2..=STABLE_CALLS {
            match sample.observe(FunctionId(0)) {
                SampleChange::Continue(next) => {
                    assert!(count < STABLE_CALLS);
                    sample = next;
                }
                SampleChange::Specialize => assert_eq!(count, STABLE_CALLS),
                SampleChange::Generic => panic!("stable calls must specialize"),
            }
        }
        let mut sample = CallSample::first(FunctionId(0));
        for count in 2..=MAX_SAMPLES {
            match sample.observe(FunctionId(u16::from(count % 3))) {
                SampleChange::Continue(next) => {
                    assert!(count < MAX_SAMPLES);
                    sample = next;
                }
                SampleChange::Specialize => panic!("unstable calls cannot specialize"),
                SampleChange::Generic => assert_eq!(count, MAX_SAMPLES),
            }
        }
    }
}
