use super::{CAPACITY, Registry};

enum Serial {
    Available(std::num::NonZeroU64),
    Exhausted,
}

impl Serial {
    fn new() -> Self {
        Self::Available(std::num::NonZeroU64::MIN)
    }

    fn take(&mut self) -> Option<u64> {
        let Self::Available(value) = self else {
            return None;
        };
        let value = value.get();
        *self = match value.checked_add(1).and_then(std::num::NonZeroU64::new) {
            Some(next) => Self::Available(next),
            None => Self::Exhausted,
        };
        Some(value)
    }
}

#[cfg(feature = "b")]
pub struct Linear {
    slots: [Option<std::num::NonZeroU64>; CAPACITY],
    serial: Serial,
}

#[cfg(feature = "b")]
impl Registry for Linear {
    fn new() -> Self {
        Self {
            slots: [None; CAPACITY],
            serial: Serial::new(),
        }
    }
    fn insert(&mut self) -> Option<u64> {
        let slot = self.slots.iter_mut().find(|slot| slot.is_none())?;
        let handle = self.serial.take()?;
        *slot = std::num::NonZeroU64::new(handle);
        Some(handle)
    }
    fn contains(&self, handle: u64) -> bool {
        let Some(handle) = std::num::NonZeroU64::new(handle) else {
            return false;
        };
        self.slots.contains(&Some(handle))
    }
    fn remove(&mut self, handle: u64) -> bool {
        let Some(handle) = std::num::NonZeroU64::new(handle) else {
            return false;
        };
        let Some(slot) = self.slots.iter_mut().find(|slot| **slot == Some(handle)) else {
            return false;
        };
        *slot = None;
        true
    }
}

#[cfg(feature = "c")]
pub struct Hashed {
    handles: std::collections::HashSet<u64>,
    serial: Serial,
}

#[cfg(feature = "c")]
impl Registry for Hashed {
    fn new() -> Self {
        Self {
            handles: std::collections::HashSet::new(),
            serial: Serial::new(),
        }
    }
    fn insert(&mut self) -> Option<u64> {
        if self.handles.len() == CAPACITY || self.handles.try_reserve(1).is_err() {
            return None;
        }
        let handle = self.serial.take()?;
        self.handles.insert(handle);
        Some(handle)
    }
    fn contains(&self, handle: u64) -> bool {
        self.handles.contains(&handle)
    }
    fn remove(&mut self, handle: u64) -> bool {
        self.handles.remove(&handle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_exhaustion_cannot_resurrect_a_stale_handle() {
        let mut registry = super::super::Selected::new();
        let stale = registry.insert().unwrap();
        assert!(registry.remove(stale));
        registry.serial = Serial::Available(std::num::NonZeroU64::new(u64::MAX).unwrap());
        let last = registry.insert().unwrap();
        assert_eq!(last, u64::MAX);
        assert!(registry.remove(last));
        assert!(registry.insert().is_none());
        assert!(!registry.contains(stale));
        assert!(!registry.contains(last));
    }
}
