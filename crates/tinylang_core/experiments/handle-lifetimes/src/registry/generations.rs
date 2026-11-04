use super::{CAPACITY, Registry};

#[derive(Clone, Copy)]
enum Slot {
    Vacant(u32),
    Live(u32),
    Retired,
}

pub struct Generations<const RETIRE: bool> {
    slots: [Slot; CAPACITY],
}

impl<const RETIRE: bool> Registry for Generations<RETIRE> {
    fn new() -> Self {
        Self {
            slots: [Slot::Vacant(1); CAPACITY],
        }
    }

    fn insert(&mut self) -> Option<u64> {
        for (index, slot) in self.slots.iter_mut().enumerate() {
            if let Slot::Vacant(generation) = *slot {
                *slot = Slot::Live(generation);
                return Some((u64::from(generation) << 32) | (index as u64 + 1));
            }
        }
        None
    }

    fn contains(&self, handle: u64) -> bool {
        let index = (handle as u32).wrapping_sub(1) as usize;
        matches!(self.slots.get(index), Some(Slot::Live(generation)) if *generation == (handle >> 32) as u32)
    }

    fn remove(&mut self, handle: u64) -> bool {
        if !self.contains(handle) {
            return false;
        }
        let index = (handle as u32 - 1) as usize;
        let generation = (handle >> 32) as u32;
        self.slots[index] = if RETIRE {
            match generation.checked_add(1) {
                Some(next) => Slot::Vacant(next),
                None => Slot::Retired,
            }
        } else {
            Slot::Vacant(generation.wrapping_add(1).max(1))
        };
        true
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
        // Equivalent to repeated reuse, without billions of test iterations.
        registry.slots[0] = Slot::Vacant(u32::MAX);
        let last = registry.insert().unwrap();
        assert!(registry.remove(last));
        let next = registry.insert().unwrap();
        if cfg!(feature = "baseline") {
            assert_eq!(next, stale, "baseline reproduces the ABA defect");
            assert!(registry.contains(stale));
        } else {
            assert_ne!(next, stale);
            assert!(!registry.contains(stale));
        }
    }
}
