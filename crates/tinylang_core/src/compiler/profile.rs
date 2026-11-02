//! Explicit source-pipeline attribution for testing-hook builds.
//! The ordinary compiler has no observer calls. These intervals follow its
//! existing ownership boundaries; parsing and emission are not separate passes.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::{
    Compiler,
    CompilerSharedState,
    ModuleCapabilities,
    ModulePermissions,
    PeepholeOptimizer,
    Result,
    VerifiedProgram,
};

const PHASE_COUNT: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum FrontendPhase {
    /// Shared compiler state, source ownership, lexing, and compiler setup.
    ConstructionAndLexing,
    /// Parsing, semantic/name work, emission, and owned Program assembly.
    ParsingEmissionAndAssembly,
    Peephole,
    /// Authoritative verification, certificate creation, and verified owners.
    /// The lazy fingerprint and retained-byte scan are not requested here.
    Verification,
}

impl FrontendPhase {
    pub const ALL: [Self; PHASE_COUNT] = [
        Self::ConstructionAndLexing,
        Self::ParsingEmissionAndAssembly,
        Self::Peephole,
        Self::Verification,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::ConstructionAndLexing => "construction_and_lexing",
            Self::ParsingEmissionAndAssembly => "parsing_emission_and_assembly",
            Self::Peephole => "peephole",
            Self::Verification => "verification",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrontendOptimization {
    Enabled,
    Disabled,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum FrontendMeasurement {
    #[default]
    NotRun,
    Succeeded(Duration),
    Failed(Duration),
}

impl FrontendMeasurement {
    pub fn elapsed(self) -> Option<Duration> {
        match self {
            Self::NotRun => None,
            Self::Succeeded(elapsed) | Self::Failed(elapsed) => Some(elapsed),
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct FrontendProfile {
    elapsed: Duration,
    phases:  [FrontendMeasurement; PHASE_COUNT],
}

impl FrontendProfile {
    /// Wall time through the final attempted stage and intervening observer
    /// work. Final profile validation/extraction, returned-owner destruction,
    /// lazy fingerprinting and reporting are outside. This is not CPU time.
    pub fn elapsed(&self) -> Duration {
        self.elapsed
    }

    pub fn measurement(&self, phase: FrontendPhase) -> FrontendMeasurement {
        self.phases[phase as usize]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FrontendProfileError {
    #[error("the frontend profiling clock moved backwards")]
    ClockWentBackwards,
    #[error("frontend profiling intervals were repeated, overlapping, or exceeded their representable range")]
    InvalidIntervals,
}

struct Recorder {
    started: Instant,
    profile: std::result::Result<FrontendProfile, FrontendProfileError>,
}

impl Recorder {
    fn new() -> Self {
        Self {
            started: Instant::now(),
            profile: Ok(FrontendProfile::default()),
        }
    }

    fn measure<T>(&mut self, phase: FrontendPhase, operation: impl FnOnce() -> Result<T>) -> Result<T> {
        if self.profile.is_err() {
            // A broken observation must not change the compiler's operation.
            return operation();
        }
        let started = Instant::now();
        let result = operation();
        let measurement = elapsed_since(started).map(|elapsed| {
            if result.is_ok() {
                FrontendMeasurement::Succeeded(elapsed)
            } else {
                FrontendMeasurement::Failed(elapsed)
            }
        });
        self.record(phase, measurement);
        result
    }

    fn record(
        &mut self,
        phase: FrontendPhase,
        measurement: std::result::Result<FrontendMeasurement, FrontendProfileError>,
    ) {
        let Ok(profile) = &mut self.profile else {
            return;
        };
        match measurement {
            Ok(measurement) if profile.phases[phase as usize] == FrontendMeasurement::NotRun => {
                profile.phases[phase as usize] = measurement;
            }
            Ok(_) => self.profile = Err(FrontendProfileError::InvalidIntervals),
            Err(error) => self.profile = Err(error),
        }
    }

    fn finish(self) -> std::result::Result<FrontendProfile, FrontendProfileError> {
        let elapsed = elapsed_since(self.started)?;
        let mut profile = self.profile?;
        let accounted = profile
            .phases
            .iter()
            .try_fold(Duration::ZERO, |sum, phase| sum.checked_add(phase.elapsed().unwrap_or(Duration::ZERO)));
        if !accounted.is_some_and(|accounted| accounted <= elapsed) {
            return Err(FrontendProfileError::InvalidIntervals);
        }
        profile.elapsed = elapsed;
        Ok(profile)
    }
}

fn elapsed_since(started: Instant) -> std::result::Result<Duration, FrontendProfileError> {
    Instant::now()
        .checked_duration_since(started)
        .ok_or(FrontendProfileError::ClockWentBackwards)
}

/// Compile a source string using the same stages, policy and verification
/// boundary as the corresponding public source compiler. File/import resolution
/// and disk caching are not part of this source-only diagnostic.
///
/// The inner result preserves compile errors and source locations. Later phases
/// remain NotRun after an error; disabling optimization skips only Peephole.
/// Panics propagate normally. All observation state is owned by this call, with
/// no TLS, global counters, heap-allocated profile, or production-path hooks.
///
/// This is hooks-build wall-clock attribution, not ordinary-release acceptance
/// timing. Keep source generation, output checks and returned-owner destruction
/// outside the named phases, and report observer effects separately.
pub fn capture_source_compilation(
    source: &str,
    filename: &str,
    optimization: FrontendOptimization,
) -> std::result::Result<(Result<VerifiedProgram>, FrontendProfile), FrontendProfileError> {
    let mut recorder = Recorder::new();
    let result = (|| {
        let compiler = recorder.measure(FrontendPhase::ConstructionAndLexing, || {
            let shared = Rc::new(RefCell::new(CompilerSharedState::default()));
            Compiler::new(
                source,
                filename,
                None,
                false,
                "",
                ModulePermissions::from_capabilities(ModuleCapabilities::all()),
                shared,
            )
        })?;
        let program = recorder.measure(FrontendPhase::ParsingEmissionAndAssembly, || compiler.compile())?;
        let program = match optimization {
            FrontendOptimization::Enabled => {
                recorder.measure(FrontendPhase::Peephole, || Ok(PeepholeOptimizer::optimize(program)))?
            }
            FrontendOptimization::Disabled => program,
        };
        recorder.measure(FrontendPhase::Verification, || VerifiedProgram::verify(program))
    })();
    Ok((result, recorder.finish()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TinyLangError;

    #[test]
    fn phase_keys_are_dense_and_recorded_outcomes_are_explicit() {
        for (index, phase) in FrontendPhase::ALL.into_iter().enumerate() {
            assert_eq!(phase as usize, index);
            assert!(!phase.name().is_empty());
        }
        let mut recorder = Recorder::new();
        assert_eq!(
            recorder
                .measure(FrontendPhase::ConstructionAndLexing, || Ok(42))
                .expect("operation"),
            42
        );
        let error = recorder
            .measure(FrontendPhase::Verification, || Err::<(), _>(TinyLangError::compile("test failure")))
            .expect_err("operation failure");
        assert_eq!(error.to_string(), TinyLangError::compile("test failure").to_string());
        let profile = recorder.finish().expect("valid intervals");
        assert!(matches!(
            profile.measurement(FrontendPhase::ConstructionAndLexing),
            FrontendMeasurement::Succeeded(_)
        ));
        assert!(matches!(profile.measurement(FrontendPhase::Verification), FrontendMeasurement::Failed(_)));
        assert_eq!(profile.measurement(FrontendPhase::Peephole), FrontendMeasurement::NotRun);
    }

    #[test]
    fn clock_failure_rejects_the_profile_without_changing_later_operations() {
        let mut recorder = Recorder::new();
        recorder.record(FrontendPhase::ConstructionAndLexing, Err(FrontendProfileError::ClockWentBackwards));
        assert_eq!(
            recorder
                .measure(FrontendPhase::Verification, || Ok(7))
                .expect("operation still runs"),
            7
        );
        assert_eq!(recorder.finish(), Err(FrontendProfileError::ClockWentBackwards));
        assert!(capture_source_compilation("print 42", "fresh.to", FrontendOptimization::Enabled).is_ok());
    }

    #[test]
    fn repeated_and_impossible_intervals_are_rejected() {
        let mut repeated = Recorder::new();
        for _ in 0..2 {
            repeated.record(FrontendPhase::Peephole, Ok(FrontendMeasurement::Succeeded(Duration::ZERO)));
        }
        assert_eq!(repeated.finish(), Err(FrontendProfileError::InvalidIntervals));
        let mut overflowing = Recorder::new();
        overflowing.record(FrontendPhase::ConstructionAndLexing, Ok(FrontendMeasurement::Succeeded(Duration::MAX)));
        overflowing.record(FrontendPhase::Verification, Ok(FrontendMeasurement::Succeeded(Duration::from_nanos(1))));
        assert_eq!(overflowing.finish(), Err(FrontendProfileError::InvalidIntervals));
        let mut overlapping = Recorder::new();
        overlapping.record(FrontendPhase::Verification, Ok(FrontendMeasurement::Succeeded(Duration::MAX)));
        assert_eq!(overlapping.finish(), Err(FrontendProfileError::InvalidIntervals));
    }

    #[test]
    fn panic_unwinds_owned_observation_state() {
        let panic = std::panic::catch_unwind(|| {
            let mut recorder = Recorder::new();
            recorder.measure::<()>(FrontendPhase::Peephole, || panic!("operation panic"))
        });
        assert!(panic.is_err());
        assert!(capture_source_compilation("print 42", "after-panic.to", FrontendOptimization::Disabled).is_ok());
    }

    #[test]
    #[ignore = "frontend clock floor; run alone in release mode with --ignored --nocapture"]
    fn report_empty_frontend_clock_floor() {
        const ITERATIONS: u64 = 100_000;
        for sample in 1..=7 {
            let started = Instant::now();
            let mut captured = Duration::ZERO;
            let mut accounted = Duration::ZERO;
            for _ in 0..ITERATIONS {
                let mut recorder = Recorder::new();
                for phase in FrontendPhase::ALL {
                    recorder
                        .measure(phase, || {
                            std::hint::black_box(());
                            Ok(())
                        })
                        .expect("empty operation");
                }
                let profile = recorder.finish().expect("clock-floor intervals");
                captured += profile.elapsed();
                for phase in FrontendPhase::ALL {
                    accounted += profile.measurement(phase).elapsed().expect("phase ran");
                }
            }
            let elapsed = elapsed_since(started).expect("clock-floor batch");
            assert!(accounted <= captured && captured <= elapsed);
            println!(
                "frontend-clock sample={sample} iterations={ITERATIONS} phases=4 elapsed_ns={} captured_ns={} accounted_ns={}",
                elapsed.as_nanos(),
                captured.as_nanos(),
                accounted.as_nanos(),
            );
        }
    }
}
