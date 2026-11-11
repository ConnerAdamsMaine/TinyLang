//! Bounded, payload-free observations for adaptive scalar instruction sites.

use crate::{TypeKind, Value};

pub(super) const MAX_FEEDBACK_BYTES: usize = 64 * 1024;
const STABLE_SAMPLES: u8 = 8;
const MAX_SAMPLES: u8 = 32;
const MISSES_BEFORE_RESAMPLING: u8 = 4;
const HITS_BEFORE_FORGETTING_MISSES: u8 = 32;
const MAX_PROMOTIONS: u8 = 2;

/// These are storage tags, not coercible language types. No value, pointer,
/// closure capture or heap identity can survive an execution in this table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Representation {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    Float(TypeKind),
    Bool,
    Unit,
    Null,
    Function,
    Reference,
    Phantom,
    Zst(TypeKind),
    Unsafe,
    Heap,
    Pointer,
}

impl Representation {
    fn of(value: &Value) -> Self {
        match value {
            Value::I8(_) => Self::I8,
            Value::I16(_) => Self::I16,
            Value::I32(_) => Self::I32,
            Value::I64(_) => Self::I64,
            Value::U8(_) => Self::U8,
            Value::U16(_) => Self::U16,
            Value::U32(_) => Self::U32,
            Value::U64(_) => Self::U64,
            Value::Float { kind, .. } => Self::Float(*kind),
            Value::Bool(_) => Self::Bool,
            Value::Unit => Self::Unit,
            Value::Null => Self::Null,
            Value::Function(_) => Self::Function,
            Value::Reference(_) => Self::Reference,
            Value::Phantom => Self::Phantom,
            Value::Zst(kind) => Self::Zst(*kind),
            Value::Unsafe => Self::Unsafe,
            Value::Heap(_) => Self::Heap,
            Value::Pointer(_) => Self::Pointer,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ObservedTypes {
    Unary(Representation),
    Binary(Representation, Representation),
}

impl ObservedTypes {
    pub(super) fn unary(value: &Value) -> Self {
        Self::Unary(Representation::of(value))
    }

    pub(super) fn binary(lhs: &Value, rhs: &Value) -> Self {
        Self::Binary(Representation::of(lhs), Representation::of(rhs))
    }

    fn is_i64(self) -> bool {
        matches!(self, Self::Unary(Representation::I64) | Self::Binary(Representation::I64, Representation::I64))
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum FeedbackState {
    #[default]
    Unseen,
    Sampling,
    Monomorphic {
        observed: ObservedTypes,
        streak: u8,
    },
    SmallPolymorphic {
        first: ObservedTypes,
        second: ObservedTypes,
        samples: u8,
        i64_streak: u8,
    },
    Specialized {
        hits: u8,
        misses: u8,
    },
    Generic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FeedbackChange {
    Keep,
    Specialize,
    Resample,
    Generic,
}

#[derive(Debug, Default, Clone, Copy)]
pub(super) struct SiteFeedback {
    state: FeedbackState,
    promotions: u8,
}

impl SiteFeedback {
    pub(super) fn arm(&mut self) -> bool {
        if self.state != FeedbackState::Unseen {
            return false;
        }
        self.state = FeedbackState::Sampling;
        true
    }

    pub(super) fn observe(&mut self, observed: ObservedTypes) -> FeedbackChange {
        match self.state {
            FeedbackState::Sampling => {
                self.state = FeedbackState::Monomorphic { observed, streak: 1 };
            }
            FeedbackState::Monomorphic {
                observed: first,
                streak,
            } => {
                if observed != first {
                    self.state = FeedbackState::SmallPolymorphic {
                        first,
                        second: observed,
                        samples: streak.saturating_add(1),
                        i64_streak: u8::from(observed.is_i64()),
                    };
                } else if streak.saturating_add(1) >= STABLE_SAMPLES {
                    return self.finish_sampling(observed.is_i64());
                } else {
                    self.state = FeedbackState::Monomorphic {
                        observed,
                        streak: streak.saturating_add(1),
                    };
                }
            }
            FeedbackState::SmallPolymorphic {
                first,
                second,
                samples,
                i64_streak,
            } => {
                if observed != first && observed != second {
                    return self.finish_sampling(false);
                }
                let i64_streak = if observed.is_i64() {
                    i64_streak.saturating_add(1)
                } else {
                    0
                };
                let samples = samples.saturating_add(1);
                if i64_streak >= STABLE_SAMPLES {
                    return self.finish_sampling(true);
                }
                if samples >= MAX_SAMPLES {
                    return self.finish_sampling(false);
                }
                self.state = FeedbackState::SmallPolymorphic {
                    first,
                    second,
                    samples,
                    i64_streak,
                };
            }
            FeedbackState::Unseen | FeedbackState::Specialized { .. } | FeedbackState::Generic => {}
        }
        FeedbackChange::Keep
    }

    fn finish_sampling(&mut self, i64: bool) -> FeedbackChange {
        if i64 && self.promotions < MAX_PROMOTIONS {
            self.promotions = self.promotions.saturating_add(1);
            self.state = FeedbackState::Specialized { hits: 0, misses: 0 };
            FeedbackChange::Specialize
        } else {
            self.state = FeedbackState::Generic;
            FeedbackChange::Generic
        }
    }

    pub(super) fn hit(&mut self) {
        if let FeedbackState::Specialized { hits, misses } = &mut self.state
            && *hits < HITS_BEFORE_FORGETTING_MISSES
        {
            *hits = hits.saturating_add(1);
            if *hits == HITS_BEFORE_FORGETTING_MISSES {
                *misses = 0;
            }
        }
    }

    pub(super) fn miss(&mut self, observed: ObservedTypes) -> FeedbackChange {
        let FeedbackState::Specialized { misses, .. } = self.state else {
            return FeedbackChange::Keep;
        };
        let misses = misses.saturating_add(1);
        if misses < MISSES_BEFORE_RESAMPLING {
            self.state = FeedbackState::Specialized { hits: 0, misses };
            return FeedbackChange::Keep;
        }
        if self.promotions >= MAX_PROMOTIONS {
            return self.finish_sampling(false);
        }
        // The miss already observed this representation; don't discard it or
        // evaluate the failing operation again when restarting the sampler.
        self.state = FeedbackState::Monomorphic { observed, streak: 1 };
        FeedbackChange::Resample
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn i64_types() -> ObservedTypes {
        ObservedTypes::binary(&Value::I64(1), &Value::I64(2))
    }

    fn float_types() -> ObservedTypes {
        ObservedTypes::binary(
            &Value::Float {
                kind: TypeKind::Fp64,
                bits: 1.0,
            },
            &Value::I64(2),
        )
    }

    fn specialize(site: &mut SiteFeedback) {
        for _ in 0..STABLE_SAMPLES - 1 {
            assert_eq!(site.observe(i64_types()), FeedbackChange::Keep);
        }
        assert_eq!(site.observe(i64_types()), FeedbackChange::Specialize);
    }

    #[test]
    fn feedback_requires_observed_i64_tags_and_has_no_owned_payloads() {
        assert!(size_of::<SiteFeedback>() <= 16);
        assert!(!std::mem::needs_drop::<SiteFeedback>());
        let mut site = SiteFeedback::default();
        assert_eq!(site.observe(i64_types()), FeedbackChange::Keep);
        assert_eq!(site.state, FeedbackState::Unseen);
        assert!(site.arm());
        assert!(!site.arm());
        specialize(&mut site);
        assert_eq!(site.promotions, 1);
    }

    #[test]
    fn stable_unsupported_tags_and_unstable_pairs_stop_sampling() {
        for observed in [
            float_types(),
            ObservedTypes::unary(&Value::U8(1)),
            ObservedTypes::unary(&Value::Bool(true)),
        ] {
            let mut site = SiteFeedback::default();
            site.arm();
            for _ in 0..STABLE_SAMPLES - 1 {
                assert_eq!(site.observe(observed), FeedbackChange::Keep);
            }
            assert_eq!(site.observe(observed), FeedbackChange::Generic);
            assert!(!site.arm());
            assert_eq!(site.observe(i64_types()), FeedbackChange::Keep);
        }
        let mut site = SiteFeedback::default();
        site.arm();
        for sample in 0..MAX_SAMPLES {
            let observed = if sample % 2 == 0 { i64_types() } else { float_types() };
            let expected = if sample == MAX_SAMPLES - 1 {
                FeedbackChange::Generic
            } else {
                FeedbackChange::Keep
            };
            assert_eq!(site.observe(observed), expected);
        }
        assert_eq!(site.state, FeedbackState::Generic);
        assert_eq!(site.promotions, 0);

        let mut third_shape = SiteFeedback::default();
        third_shape.arm();
        assert_eq!(third_shape.observe(i64_types()), FeedbackChange::Keep);
        assert_eq!(third_shape.observe(float_types()), FeedbackChange::Keep);
        assert_eq!(
            third_shape.observe(ObservedTypes::binary(&Value::Bool(true), &Value::I64(2))),
            FeedbackChange::Generic,
        );
        assert_eq!(third_shape.promotions, 0);
    }

    #[test]
    fn repeated_instability_cannot_rewrite_a_site_indefinitely() {
        let mut site = SiteFeedback::default();
        site.arm();
        specialize(&mut site);
        for promotion in 1..=MAX_PROMOTIONS {
            for _ in 0..MISSES_BEFORE_RESAMPLING - 1 {
                assert_eq!(site.miss(float_types()), FeedbackChange::Keep);
            }
            if promotion == MAX_PROMOTIONS {
                assert_eq!(site.miss(float_types()), FeedbackChange::Generic);
            } else {
                assert_eq!(site.miss(float_types()), FeedbackChange::Resample);
                specialize(&mut site);
            }
        }
        assert_eq!(site.state, FeedbackState::Generic);
        assert_eq!(site.promotions, MAX_PROMOTIONS);
        assert!(!site.arm());
    }

    #[test]
    fn sparse_misses_do_not_demote_a_stable_site_and_hit_counts_saturate() {
        let mut site = SiteFeedback::default();
        site.arm();
        specialize(&mut site);
        for _ in 0..512 {
            assert_eq!(site.miss(float_types()), FeedbackChange::Keep);
            for _ in 0..100 {
                site.hit();
            }
        }
        assert_eq!(
            site.state,
            FeedbackState::Specialized {
                hits: HITS_BEFORE_FORGETTING_MISSES,
                misses: 0,
            }
        );
        assert_eq!(site.promotions, 1);
    }

    #[test]
    fn observations_distinguish_width_arity_and_float_representation() {
        assert_ne!(ObservedTypes::unary(&Value::I64(1)), ObservedTypes::unary(&Value::U64(1)));
        assert_ne!(ObservedTypes::unary(&Value::I64(1)), i64_types());
        assert_ne!(
            ObservedTypes::unary(&Value::Float {
                kind: TypeKind::Fp32,
                bits: 1.0,
            }),
            ObservedTypes::unary(&Value::Float {
                kind: TypeKind::Fp64,
                bits: 1.0,
            }),
        );
    }
}
