use super::super::status::{TINY_STATUS_INVALID_HANDLE, TINY_STATUS_STALE_HANDLE, TinyStatus};

/// Exhausted slots are never reused: wrapping would resurrect old handles.
#[derive(Clone, Copy)]
pub(super) enum SlotLifetime {
    Vacant(u32),
    Live(u32),
    Retired,
}

impl SlotLifetime {
    pub const EMPTY: Self = Self::Vacant(1);

    pub fn is_available(self) -> bool {
        matches!(self, Self::Vacant(_))
    }

    pub fn is_live(self) -> bool {
        matches!(self, Self::Live(_))
    }

    pub fn activate(&mut self) -> Option<u32> {
        match *self {
            Self::Vacant(generation) => {
                *self = Self::Live(generation);
                Some(generation)
            }
            Self::Live(_) | Self::Retired => None,
        }
    }

    pub fn validate(self, expected: u32) -> Result<(), TinyStatus> {
        match self {
            Self::Live(generation) if generation == expected => Ok(()),
            Self::Vacant(generation) if generation == expected => Err(TINY_STATUS_INVALID_HANDLE),
            _ => Err(TINY_STATUS_STALE_HANDLE),
        }
    }

    pub fn release(&mut self) {
        if let Self::Live(generation) = *self {
            *self = match generation.checked_add(1) {
                Some(next) => Self::Vacant(next),
                None => Self::Retired,
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_exhaustion_retires_instead_of_reusing_an_old_identity() {
        let mut lifetime = SlotLifetime::Vacant(u32::MAX);
        assert_eq!(lifetime.activate(), Some(u32::MAX));
        assert_eq!(lifetime.validate(u32::MAX), Ok(()));
        lifetime.release();
        assert!(!lifetime.is_available());
        assert!(!lifetime.is_live());
        assert_eq!(lifetime.activate(), None);
        assert_eq!(lifetime.validate(1), Err(TINY_STATUS_STALE_HANDLE));
        assert_eq!(lifetime.validate(u32::MAX), Err(TINY_STATUS_STALE_HANDLE));
    }

    #[test]
    fn release_invalidates_the_previous_generation() {
        let mut lifetime = SlotLifetime::EMPTY;
        assert_eq!(lifetime.activate(), Some(1));
        assert_eq!(lifetime.activate(), None);
        lifetime.release();
        assert_eq!(lifetime.validate(1), Err(TINY_STATUS_STALE_HANDLE));
        assert_eq!(lifetime.activate(), Some(2));
        assert_eq!(lifetime.validate(2), Ok(()));
    }
}
