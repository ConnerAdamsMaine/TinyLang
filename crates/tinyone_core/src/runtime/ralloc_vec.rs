//! A growable, fixed-stride vector backed by [`RallocBytes`] — the
//! Ralloc-arena equivalent of a `Vec<[u8; N]>`.
//!
//! Used to store `HeapData::Array`'s elements (`stride =
//! value_codec::ENCODED_VALUE_BYTES`) and `HeapData::Map`'s `(key, value)`
//! pairs (`stride = 2 * ENCODED_VALUE_BYTES`, contiguous per entry) as real
//! Ralloc-owned memory instead of a Rust-native `Vec<Value>`/
//! `Vec<(Value,Value)>`. Deliberately decoupled from `Value`/encoding
//! concerns — this only ever moves opaque `stride`-sized byte slices;
//! callers (`runtime::heap`) do the `value_codec::encode_value`/
//! `decode_value` translation.
//!
//! Capacity grows fourfold — exact-size-per-push would turn every
//! push-in-a-loop `TinyLang` program into O(n^2) real Ralloc reallocations.
//! Callers that need to charge a heap
//! byte budget by *logical* length (as `TinyHeap` already does via
//! `VALUE_BYTES`-per-element accounting) should keep doing so independently
//! of this type's physical capacity. Tests inspect physical reservation
//! separately through `byte_capacity()`.

use crate::tiny_allocator::RallocBytes;
use crate::{Result, TinyOneError};

#[derive(Debug)]
pub(crate) struct RallocVec {
    bytes:  RallocBytes,
    len:    usize,
    stride: usize,
}

impl RallocVec {
    /// Creates a vector with room for `capacity` elements of `stride` bytes
    /// each, initially empty.
    pub(crate) fn with_capacity(stride: usize, capacity: usize) -> Result<Self> {
        let byte_len = stride
            .checked_mul(capacity)
            .ok_or_else(|| TinyOneError::runtime("container capacity overflow"))?;
        let bytes = RallocBytes::zeroed(byte_len)
            .map_err(|e| TinyOneError::runtime(format!("failed to allocate container: {e}")))?;
        Ok(Self { bytes, len: 0, stride })
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    fn capacity(&self) -> usize {
        self.bytes.len() / self.stride
    }

    /// Physical bytes reserved (>= `len() * stride`), for capacity tests;
    /// this is not the logical payload charged to the heap-byte budget.
    #[cfg(test)]
    pub(crate) fn byte_capacity(&self) -> usize {
        self.bytes.len()
    }

    pub(crate) fn get(&self, index: usize) -> Option<&[u8]> {
        if index >= self.len {
            return None;
        }
        let start = index * self.stride;
        Some(&self.bytes.as_slice()[start..start + self.stride])
    }

    pub(crate) fn get_mut(&mut self, index: usize) -> Option<&mut [u8]> {
        if index >= self.len {
            return None;
        }
        let start = index * self.stride;
        Some(&mut self.bytes.as_mut_slice()[start..start + self.stride])
    }

    /// Returns a checked byte range within one logical element.
    pub(crate) fn get_part(&self, index: usize, offset: usize, len: usize) -> Option<&[u8]> {
        let element = self.get(index)?;
        let end = offset.checked_add(len)?;
        element.get(offset..end)
    }

    /// Appends one `stride`-byte element, growing capacity fourfold when full
    /// (or reserving four elements when initially empty).
    ///
    /// # Panics
    /// If `elem.len() != stride` — a caller bug, not a runtime condition.
    pub(crate) fn push(&mut self, elem: &[u8]) -> Result<()> {
        assert_eq!(elem.len(), self.stride, "RallocVec::push: element size mismatch");
        if self.len == self.capacity() {
            let capacity = self.capacity();
            let new_capacity = if capacity == 0 {
                4
            } else {
                capacity
                    // Fewer Ralloc reallocations are materially cheaper for
                    // collection-heavy programs than holding the bounded extra
                    // slack between growths. Exact logical length and heap-byte
                    // accounting remain unchanged.
                    .checked_mul(4)
                    .ok_or_else(|| TinyOneError::runtime("container capacity overflow"))?
            };
            let new_byte_len = new_capacity
                .checked_mul(self.stride)
                .ok_or_else(|| TinyOneError::runtime("container capacity overflow"))?;
            self.bytes
                .realloc(new_byte_len)
                .map_err(|e| TinyOneError::runtime(format!("failed to grow container: {e}")))?;
        }
        let start = self.len * self.stride;
        self.bytes.as_mut_slice()[start..start + self.stride].copy_from_slice(elem);
        self.len += 1;
        Ok(())
    }

    /// Removes the last element and maps its borrowed bytes before returning.
    /// The callback keeps fixed-width runtime slots off the host allocator.
    pub(crate) fn pop_with<T>(&mut self, map: impl FnOnce(&[u8]) -> T) -> Option<T> {
        if self.len == 0 {
            return None;
        }
        self.len -= 1;
        let start = self.len * self.stride;
        Some(map(&self.bytes.as_slice()[start..start + self.stride]))
    }

    /// Removes the element at `index`, shifting later elements down to keep
    /// the remaining elements in order (matching `Vec::remove`).
    pub(crate) fn remove(&mut self, index: usize) -> Option<Vec<u8>> {
        if index >= self.len {
            return None;
        }
        let start = index * self.stride;
        let removed = self.bytes.as_slice()[start..start + self.stride].to_vec();
        let tail_start = start + self.stride;
        let tail_len = (self.len - index - 1) * self.stride;
        if tail_len > 0 {
            self.bytes
                .as_mut_slice()
                .copy_within(tail_start..tail_start + tail_len, start);
        }
        self.len -= 1;
        Some(removed)
    }

    /// Logically empties the vector without shrinking its physical
    /// capacity (matching `Vec::clear`).
    pub(crate) fn clear(&mut self) {
        self.len = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn elem(byte: u8, stride: usize) -> Vec<u8> {
        vec![byte; stride]
    }

    #[test]
    fn push_get_and_len_round_trip() {
        let mut v = RallocVec::with_capacity(4, 0).unwrap();
        assert_eq!(v.len(), 0);
        v.push(&elem(1, 4)).unwrap();
        v.push(&elem(2, 4)).unwrap();
        v.push(&elem(3, 4)).unwrap();
        assert_eq!(v.len(), 3);
        assert_eq!(v.get(0), Some(&elem(1, 4)[..]));
        assert_eq!(v.get(1), Some(&elem(2, 4)[..]));
        assert_eq!(v.get(2), Some(&elem(3, 4)[..]));
        assert_eq!(v.get(3), None);
    }

    #[test]
    fn push_grows_past_initial_capacity() {
        let mut v = RallocVec::with_capacity(8, 1).unwrap();
        for i in 0..100u8 {
            v.push(&elem(i, 8)).unwrap();
        }
        assert_eq!(v.len(), 100);
        for i in 0..100u8 {
            assert_eq!(v.get(i as usize), Some(&elem(i, 8)[..]));
        }
    }

    #[test]
    fn pop_returns_elements_in_lifo_order_and_shrinks_len() {
        let mut v = RallocVec::with_capacity(4, 4).unwrap();
        v.push(&elem(1, 4)).unwrap();
        v.push(&elem(2, 4)).unwrap();
        assert_eq!(v.pop_with(<[u8]>::to_vec), Some(elem(2, 4)));
        assert_eq!(v.pop_with(<[u8]>::to_vec), Some(elem(1, 4)));
        assert_eq!(v.pop_with(<[u8]>::to_vec), None);
        assert_eq!(v.len(), 0);
    }

    #[test]
    fn get_mut_writes_are_visible_via_get() {
        let mut v = RallocVec::with_capacity(4, 4).unwrap();
        v.push(&elem(1, 4)).unwrap();
        v.get_mut(0).unwrap().copy_from_slice(&elem(9, 4));
        assert_eq!(v.get(0), Some(&elem(9, 4)[..]));
    }

    #[test]
    fn get_part_checks_element_and_subrange_boundaries() {
        let mut v = RallocVec::with_capacity(8, 1).unwrap();
        v.push(&[0, 1, 2, 3, 4, 5, 6, 7]).unwrap();
        assert_eq!(v.get_part(0, 2, 3), Some(&[2, 3, 4][..]));
        assert_eq!(v.get_part(1, 0, 1), None);
        assert_eq!(v.get_part(0, 7, 2), None);
        assert_eq!(v.get_part(0, usize::MAX, 2), None);
    }

    #[test]
    fn remove_shifts_tail_down_and_preserves_order() {
        let mut v = RallocVec::with_capacity(4, 8).unwrap();
        for i in 0..5u8 {
            v.push(&elem(i, 4)).unwrap();
        }
        assert_eq!(v.remove(1), Some(elem(1, 4)));
        assert_eq!(v.len(), 4);
        assert_eq!(
            (0..4).map(|i| v.get(i).unwrap().to_vec()).collect::<Vec<_>>(),
            vec![elem(0, 4), elem(2, 4), elem(3, 4), elem(4, 4)]
        );
    }

    #[test]
    fn remove_out_of_bounds_leaves_length_contents_and_capacity_unchanged() {
        let mut v = RallocVec::with_capacity(4, 4).unwrap();
        assert_eq!(v.remove(0), None);
        v.push(&elem(1, 4)).unwrap();
        let capacity_before = v.byte_capacity();
        assert_eq!(v.remove(1), None);
        assert_eq!(v.remove(5), None);
        assert_eq!(v.remove(usize::MAX), None);
        assert_eq!(v.len(), 1);
        assert_eq!(v.get(0), Some(&[1; 4][..]));
        assert_eq!(v.byte_capacity(), capacity_before);
    }

    #[test]
    fn remove_first_last_and_only_element_preserves_capacity() {
        let mut v = RallocVec::with_capacity(128, 4).unwrap();
        for byte in 1..=3 {
            v.push(&[byte; 128]).unwrap();
        }
        let capacity_before = v.byte_capacity();
        assert_eq!(v.remove(0), Some(vec![1; 128]));
        assert_eq!(v.len(), 2);
        assert_eq!(v.get(0), Some(&[2; 128][..]));
        assert_eq!(v.get(1), Some(&[3; 128][..]));
        assert_eq!(v.remove(1), Some(vec![3; 128]));
        assert_eq!(v.len(), 1);
        assert_eq!(v.get(0), Some(&[2; 128][..]));
        assert_eq!(v.get(1), None);
        assert_eq!(v.remove(0), Some(vec![2; 128]));
        assert_eq!(v.len(), 0);
        assert_eq!(v.get(0), None);
        assert_eq!(v.remove(0), None);
        assert_eq!(v.byte_capacity(), capacity_before);
    }

    #[test]
    fn removed_storage_is_hidden_and_fully_overwritten_on_push() {
        let mut v = RallocVec::with_capacity(128, 1).unwrap();
        v.push(&[0xff; 128]).unwrap();
        let capacity_before = v.byte_capacity();
        assert_eq!(v.remove(0), Some(vec![0xff; 128]));
        assert_eq!(v.get(0), None);
        assert_eq!(v.get_mut(0), None);
        assert_eq!(v.get_part(0, 0, 128), None);
        let replacement = std::array::from_fn::<_, 128, _>(|index| u8::try_from(index).unwrap());
        v.push(&replacement).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v.get(0), Some(&replacement[..]));
        assert_eq!(v.byte_capacity(), capacity_before);
    }

    #[test]
    fn clear_resets_len_but_keeps_capacity() {
        let mut v = RallocVec::with_capacity(4, 4).unwrap();
        v.push(&elem(1, 4)).unwrap();
        v.push(&elem(2, 4)).unwrap();
        let capacity_before = v.byte_capacity();
        v.clear();
        assert_eq!(v.len(), 0);
        assert_eq!(v.byte_capacity(), capacity_before);
        // Pushing again reuses the same physical storage.
        v.push(&elem(3, 4)).unwrap();
        assert_eq!(v.get(0), Some(&elem(3, 4)[..]));
    }

    #[test]
    #[should_panic(expected = "element size mismatch")]
    fn push_wrong_stride_panics() {
        let mut v = RallocVec::with_capacity(4, 4).unwrap();
        v.push(&elem(1, 8)).unwrap();
    }
}
