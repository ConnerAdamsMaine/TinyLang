pub const CAPACITY: usize = 64;

pub trait Registry: Sized {
    fn new() -> Self;
    fn insert(&mut self) -> Option<u64>;
    fn contains(&self, handle: u64) -> bool;
    fn remove(&mut self, handle: u64) -> bool;
}

#[cfg(any(feature = "baseline", feature = "a"))]
mod generations;
#[cfg(any(feature = "b", feature = "c"))]
mod serial;

#[cfg(feature = "baseline")]
pub type Selected = generations::Generations<false>;
#[cfg(feature = "a")]
pub type Selected = generations::Generations<true>;
#[cfg(feature = "b")]
pub type Selected = serial::Linear;
#[cfg(feature = "c")]
pub type Selected = serial::Hashed;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_release_stale_and_invalid_handles() {
        let mut registry = Selected::new();
        assert!(!registry.contains(0));
        assert!(!registry.remove(0));
        let handles: Vec<_> = (0..CAPACITY).map(|_| registry.insert().unwrap()).collect();
        assert!(registry.insert().is_none());
        for &handle in &handles {
            assert!(registry.contains(handle));
        }
        assert!(registry.remove(handles[17]));
        assert!(!registry.remove(handles[17]));
        let replacement = registry.insert().unwrap();
        assert_ne!(handles[17], replacement);
        assert!(!registry.contains(handles[17]));
        assert!(registry.remove(replacement));
        for (index, handle) in handles.into_iter().enumerate() {
            if index != 17 {
                assert!(registry.remove(handle));
            }
        }
    }
}
