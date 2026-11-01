use core::sync::atomic::{AtomicUsize, Ordering};

use ralloc::{RallocBox, RallocError};

struct DropCounter<'a>(u32, &'a AtomicUsize);

impl Drop for DropCounter<'_> {
    fn drop(&mut self) {
        self.1.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn box_owns_value_and_drops_it_once() {
    let drop_count = AtomicUsize::new(0);

    {
        let value = RallocBox::new(DropCounter(17, &drop_count)).expect("box allocation should succeed");
        assert_eq!(value.get().0, 17);
        assert_eq!(drop_count.load(Ordering::SeqCst), 0);
    }

    assert_eq!(drop_count.load(Ordering::SeqCst), 1);
}

#[test]
fn box_mutation_requires_unique_handle() {
    let mut value = RallocBox::new(41usize).expect("box allocation should succeed");

    *value += 1;

    assert_eq!(*value, 42);
}

#[test]
fn box_into_inner_moves_value_out_and_frees_storage() {
    let drop_count = AtomicUsize::new(0);

    let value = RallocBox::new(DropCounter(99, &drop_count)).expect("box allocation should succeed");
    let inner = value.into_inner();

    assert_eq!(inner.0, 99);
    assert_eq!(drop_count.load(Ordering::SeqCst), 0);

    drop(inner);
    assert_eq!(drop_count.load(Ordering::SeqCst), 1);
}

#[test]
fn box_can_move_across_threads_when_value_is_send() {
    let value = RallocBox::new(123usize).expect("box allocation should succeed");

    let worker = std::thread::spawn(move || *value.get());

    assert_eq!(worker.join().expect("worker should not panic"), 123);
}

#[test]
fn box_type_is_send_when_value_is_send() {
    fn assert_send<T: Send>() {}

    assert_send::<RallocBox<usize>>();
}

#[test]
fn box_supports_zero_sized_values_without_allocation() {
    let value = RallocBox::new(()).expect("zero-sized box should not allocate");

    assert_eq!(*value.get(), ());
}

#[repr(align(64))]
struct OverAligned<'a>(u8, &'a AtomicUsize);

impl Drop for OverAligned<'_> {
    fn drop(&mut self) {
        self.1.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn box_supports_over_aligned_values() {
    let drop_count = AtomicUsize::new(0);

    {
        let value = RallocBox::try_new(OverAligned(7, &drop_count)).expect("over-aligned box should allocate");
        assert_eq!(value.as_ptr().addr() % 64, 0);
        assert_eq!(value.0, 7);
        assert_eq!(drop_count.load(Ordering::SeqCst), 0);
    }

    assert_eq!(drop_count.load(Ordering::SeqCst), 1);
}

#[test]
fn box_into_inner_works_for_over_aligned_values() {
    let drop_count = AtomicUsize::new(0);

    let value = RallocBox::try_new(OverAligned(9, &drop_count)).expect("over-aligned box should allocate");
    assert_eq!(value.as_ptr().addr() % 64, 0);

    let inner = value.into_inner();
    assert_eq!(inner.0, 9);
    assert_eq!(drop_count.load(Ordering::SeqCst), 0);

    drop(inner);
    assert_eq!(drop_count.load(Ordering::SeqCst), 1);
}

#[test]
fn box_raw_pointer_round_trip_transfers_ownership_explicitly() {
    let value = RallocBox::new(55usize).expect("box allocation should succeed");
    let ptr = value.into_raw();

    // SAFETY: `ptr` came from `RallocBox::into_raw` and has not been freed or
    // rewrapped.
    let recovered = unsafe { RallocBox::from_raw(ptr) }.expect("raw pointer should recover");

    assert_eq!(*recovered, 55);
}

#[test]
fn box_rejects_null_raw_pointer_recovery() {
    let recovered = unsafe { RallocBox::<usize>::from_raw(core::ptr::null_mut()) };

    assert_eq!(recovered.err(), Some(RallocError::InvalidRawParts));
}
