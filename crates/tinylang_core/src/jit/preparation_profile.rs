//! Opt-in wall-clock attribution for JIT preparation in testing-hook builds.
//! Phase intervals are disjoint; caller/setup/drop and observer work remains
//! in the capture total, rather than being assigned to an optimizer pass.

use std::cell::RefCell;
use std::time::{Duration, Instant};

const PHASE_COUNT: usize = 17;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum PreparationPhase {
    ChunkSetup,
    BranchMap,
    SymbolicConstruction,
    UnreachableEncoding,
    StackFinalization,
    DefinitionFinalization,
    ConstantPropagation,
    CopyPropagation,
    SlotVersionPropagation,
    ConstantFolding,
    BranchFolding,
    DeadTemporaries,
    DeadStores,
    Encoding,
    TargetRemapping,
    OriginInstallation,
    ChunkInstallation,
}

impl PreparationPhase {
    pub const ALL: [Self; PHASE_COUNT] = [
        Self::ChunkSetup,
        Self::BranchMap,
        Self::SymbolicConstruction,
        Self::UnreachableEncoding,
        Self::StackFinalization,
        Self::DefinitionFinalization,
        Self::ConstantPropagation,
        Self::CopyPropagation,
        Self::SlotVersionPropagation,
        Self::ConstantFolding,
        Self::BranchFolding,
        Self::DeadTemporaries,
        Self::DeadStores,
        Self::Encoding,
        Self::TargetRemapping,
        Self::OriginInstallation,
        Self::ChunkInstallation,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::ChunkSetup => "chunk_setup",
            Self::BranchMap => "branch_map",
            Self::SymbolicConstruction => "symbolic_construction",
            Self::UnreachableEncoding => "unreachable_encoding",
            Self::StackFinalization => "stack_finalization",
            Self::DefinitionFinalization => "definition_finalization",
            Self::ConstantPropagation => "constant_propagation",
            Self::CopyPropagation => "copy_propagation",
            Self::SlotVersionPropagation => "slot_version_propagation",
            Self::ConstantFolding => "constant_folding",
            Self::BranchFolding => "branch_folding",
            Self::DeadTemporaries => "dead_temporaries",
            Self::DeadStores => "dead_stores",
            Self::Encoding => "encoding",
            Self::TargetRemapping => "target_remapping",
            Self::OriginInstallation => "origin_installation",
            Self::ChunkInstallation => "chunk_installation",
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PreparationMeasurement {
    calls:   u64,
    elapsed: Duration,
}

impl PreparationMeasurement {
    pub fn calls(self) -> u64 {
        self.calls
    }

    pub fn elapsed(self) -> Duration {
        self.elapsed
    }

    fn add(&mut self, elapsed: Duration) -> Result<(), PreparationProfileError> {
        let calls = self
            .calls
            .checked_add(1)
            .ok_or(PreparationProfileError::CounterOverflow)?;
        let elapsed = self
            .elapsed
            .checked_add(elapsed)
            .ok_or(PreparationProfileError::CounterOverflow)?;
        *self = Self { calls, elapsed };
        Ok(())
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct PreparationProfile {
    elapsed: Duration,
    phases:  [PreparationMeasurement; PHASE_COUNT],
}

impl PreparationProfile {
    /// Includes the supplied closure and observation overhead, not capture
    /// allocation or report extraction. This is wall time, not thread CPU time.
    pub fn elapsed(&self) -> Duration {
        self.elapsed
    }

    pub fn measurement(&self, phase: PreparationPhase) -> PreparationMeasurement {
        self.phases[phase as usize]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PreparationProfileError {
    #[error("a JIT preparation capture is already active on this thread")]
    AlreadyActive,
    #[error("the JIT preparation clock moved backwards")]
    ClockWentBackwards,
    #[error("JIT preparation counters exceeded their representable range")]
    CounterOverflow,
    #[error("the JIT preparation capture ended without its recording state")]
    MissingCapture,
}

#[derive(Default)]
enum CaptureState {
    #[default]
    Inactive,
    Recording(Box<Result<PreparationProfile, PreparationProfileError>>),
}

thread_local! {
    static CAPTURE: RefCell<CaptureState> = const { RefCell::new(CaptureState::Inactive) };
}

struct CaptureGuard;

impl CaptureGuard {
    fn start() -> Result<Self, PreparationProfileError> {
        CAPTURE.with_borrow_mut(|state| {
            match state {
                CaptureState::Inactive => {
                    *state = CaptureState::Recording(Box::new(Ok(PreparationProfile::default())));
                    Ok(Self)
                }
                CaptureState::Recording(_) => Err(PreparationProfileError::AlreadyActive),
            }
        })
    }

    fn finish(
        self,
        elapsed: Result<Duration, PreparationProfileError>,
    ) -> Result<PreparationProfile, PreparationProfileError> {
        let state = CAPTURE.replace(CaptureState::Inactive);
        let CaptureState::Recording(recording) = state else {
            return Err(PreparationProfileError::MissingCapture);
        };
        let mut profile = (*recording)?;
        profile.elapsed = elapsed?;
        Ok(profile)
    }
}

impl Drop for CaptureGuard {
    fn drop(&mut self) {
        // The guard never escapes capture_jit_preparation. In particular,
        // unwinding clears this thread's recording before any later capture.
        CAPTURE.set(CaptureState::Inactive);
    }
}

/// Capture preparation phases on this thread only. Nested captures are rejected
/// before invoking their closure. The operation runs outside all TLS borrows;
/// its returned value/error is unchanged, and panics propagate after cleanup.
///
/// Intended for isolated diagnostics, not ordinary-release acceptance timings.
/// Compile/verify inputs before entering when measuring lowering alone.
pub fn capture_jit_preparation<T>(
    operation: impl FnOnce() -> T,
) -> Result<(T, PreparationProfile), PreparationProfileError> {
    let guard = CaptureGuard::start()?;
    let started = Instant::now();
    let value = operation();
    let elapsed = elapsed_since(started);
    Ok((value, guard.finish(elapsed)?))
}

fn elapsed_since(started: Instant) -> Result<Duration, PreparationProfileError> {
    Instant::now()
        .checked_duration_since(started)
        .ok_or(PreparationProfileError::ClockWentBackwards)
}

/// Private synchronous scopes only. No timer is retained in installed code or
/// crosses a thread boundary. Disabled captures never read the clock.
pub(super) enum PreparationSpan {
    Inactive,
    Recording {
        phase:   PreparationPhase,
        started: Instant,
    },
}

impl PreparationSpan {
    pub(super) fn start(phase: PreparationPhase) -> Self {
        if CAPTURE.with_borrow(|state| matches!(state, CaptureState::Inactive)) {
            return Self::Inactive;
        }
        Self::Recording {
            phase,
            started: Instant::now(),
        }
    }

    pub(super) fn next(&mut self, phase: PreparationPhase) {
        let Self::Recording {
            phase: previous,
            started,
        } = self
        else {
            return;
        };
        record(*previous, elapsed_since(*started));
        // Exclude the preceding record update from the following phase.
        *previous = phase;
        *started = Instant::now();
    }
}

impl Drop for PreparationSpan {
    fn drop(&mut self) {
        if let Self::Recording { phase, started } = *self {
            record(phase, elapsed_since(started));
        }
    }
}

fn record(phase: PreparationPhase, elapsed: Result<Duration, PreparationProfileError>) {
    CAPTURE.with_borrow_mut(|state| {
        let CaptureState::Recording(recording) = state else {
            return;
        };
        let outcome = match recording.as_mut() {
            Ok(profile) => elapsed.and_then(|elapsed| profile.phases[phase as usize].add(elapsed)),
            Err(_) => return,
        };
        if let Err(error) = outcome {
            **recording = Err(error);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_keys_are_dense_and_disabled_scopes_do_not_leak() {
        for (index, phase) in PreparationPhase::ALL.into_iter().enumerate() {
            assert_eq!(phase as usize, index);
            assert!(!phase.name().is_empty());
        }
        drop(PreparationSpan::start(PreparationPhase::Encoding));
        let (value, profile) = capture_jit_preparation(|| {
            let mut span = PreparationSpan::start(PreparationPhase::ConstantPropagation);
            span.next(PreparationPhase::Encoding);
            42
        })
        .expect("capture");
        assert_eq!(value, 42);
        assert_eq!(profile.measurement(PreparationPhase::ConstantPropagation).calls(), 1);
        assert_eq!(profile.measurement(PreparationPhase::Encoding).calls(), 1);
        assert_eq!(profile.measurement(PreparationPhase::SlotVersionPropagation).calls(), 0);
        let (_, empty) = capture_jit_preparation(|| ()).expect("fresh capture");
        assert!(
            PreparationPhase::ALL
                .into_iter()
                .all(|phase| empty.measurement(phase).calls() == 0)
        );
    }

    #[test]
    fn nested_capture_is_rejected_without_invoking_or_resetting_it() {
        let (_, profile) = capture_jit_preparation(|| {
            let _span = PreparationSpan::start(PreparationPhase::ChunkSetup);
            let nested = capture_jit_preparation(|| panic!("nested operation must not run"));
            assert!(matches!(nested, Err(PreparationProfileError::AlreadyActive)));
        })
        .expect("outer capture");
        assert_eq!(profile.measurement(PreparationPhase::ChunkSetup).calls(), 1);
    }

    #[test]
    fn errors_and_panics_keep_the_operation_contract_and_release_capture() {
        let (result, _) = capture_jit_preparation(|| Err::<(), _>("operation failed")).expect("recording succeeds");
        assert_eq!(result, Err("operation failed"));
        let failure = std::panic::catch_unwind(|| {
            capture_jit_preparation(|| {
                let _span = PreparationSpan::start(PreparationPhase::Encoding);
                panic!("operation panic");
            })
        });
        assert!(failure.is_err());
        assert!(capture_jit_preparation(|| ()).is_ok(), "unwind releases the session");
    }

    #[test]
    fn captures_are_thread_local() {
        let (_, outer) = capture_jit_preparation(|| {
            let _span = PreparationSpan::start(PreparationPhase::ChunkSetup);
            std::thread::spawn(|| {
                let (_, inner) = capture_jit_preparation(|| {
                    drop(PreparationSpan::start(PreparationPhase::Encoding));
                })
                .expect("independent thread");
                assert_eq!(inner.measurement(PreparationPhase::Encoding).calls(), 1);
                assert_eq!(inner.measurement(PreparationPhase::ChunkSetup).calls(), 0);
            })
            .join()
            .expect("worker");
        })
        .expect("outer thread");
        assert_eq!(outer.measurement(PreparationPhase::ChunkSetup).calls(), 1);
        assert_eq!(outer.measurement(PreparationPhase::Encoding).calls(), 0);
    }

    #[test]
    fn invalid_clock_and_overflow_reject_the_profile_instead_of_saturating() {
        let invalid = capture_jit_preparation(|| {
            record(PreparationPhase::Encoding, Err(PreparationProfileError::ClockWentBackwards));
            record(PreparationPhase::Encoding, Ok(Duration::ZERO));
        });
        assert!(matches!(invalid, Err(PreparationProfileError::ClockWentBackwards)));
        let mut measurement = PreparationMeasurement {
            calls:   u64::MAX,
            elapsed: Duration::ZERO,
        };
        assert_eq!(measurement.add(Duration::ZERO), Err(PreparationProfileError::CounterOverflow));
        measurement = PreparationMeasurement {
            calls:   0,
            elapsed: Duration::MAX,
        };
        assert_eq!(measurement.add(Duration::from_nanos(1)), Err(PreparationProfileError::CounterOverflow));
        assert!(capture_jit_preparation(|| ()).is_ok());
    }

    #[test]
    #[ignore = "preparation observer/clock floor; run alone in release mode with --ignored --nocapture"]
    fn report_empty_phase_clock_floor() {
        const SPANS: u64 = 100_000;
        for sample in 1..=7 {
            let (_, profile) = capture_jit_preparation(|| {
                for _ in 0..SPANS {
                    drop(std::hint::black_box(PreparationSpan::start(PreparationPhase::ChunkSetup)));
                }
            })
            .expect("clock-floor capture");
            let measurement = profile.measurement(PreparationPhase::ChunkSetup);
            assert_eq!(measurement.calls(), SPANS);
            assert!(measurement.elapsed() <= profile.elapsed());
            eprintln!(
                "jit-phase-clock sample={sample} spans={SPANS} elapsed_ns={} recorded_ns={}",
                profile.elapsed().as_nanos(),
                measurement.elapsed().as_nanos()
            );
        }
    }
}
