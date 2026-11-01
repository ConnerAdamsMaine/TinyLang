//! Native owned allocation handles for Rust callers.

use core::cell::Cell;
use core::ffi::c_void;
use core::marker::PhantomData;
use core::mem::{self, ManuallyDrop};
use core::ops::{Deref, DerefMut};
use core::ptr::{self, NonNull};
use core::slice;

use crate::ralloc::{allocate_aligned, is_supported_native_alignment, ralloc_free, reallocate_aligned};

/// Allocation failure or raw-handle validation error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RallocError {
    /// The allocator could not satisfy the request.
    OutOfMemory,
    /// The requested allocation alignment is not supported.
    InvalidAlignment,
    /// Raw ownership recovery received a null or otherwise invalid pointer.
    InvalidRawParts,
}

/// Owned byte allocation backed by Ralloc.
///
/// `RallocBuffer` gives Rust callers an ownership-based API over the allocator:
/// freeing happens on drop, resizing requires `&mut self`, and byte access is
/// exposed through Rust slices instead of requiring callers to manage raw
/// pointer lifetimes. Newly allocated bytes, including the tail added by
/// growth, are zero-initialized before safe byte access is exposed.
pub struct RallocBuffer {
    ptr:       NonNull<u8>,
    len:       usize,
    align:     usize,
    _not_sync: PhantomData<Cell<()>>,
}

// SAFETY: `RallocBuffer` owns its allocation, and moving ownership to another
// thread transfers the only safe mutable access path with it. Shared cross-thread
// access is intentionally not provided because `Cell` makes the type `!Sync`.
unsafe impl Send for RallocBuffer {}

impl RallocBuffer {
    /// Allocates a zero-initialized byte buffer of `len` bytes.
    ///
    /// A zero-length buffer does not allocate and always succeeds.
    #[must_use]
    pub fn new(len: usize) -> Option<Self> {
        Self::try_new(len).ok()
    }

    /// Allocates a zero-initialized byte buffer of `len` bytes, returning an explicit error on
    /// allocation failure.
    ///
    /// # Errors
    ///
    /// Returns [`RallocError::OutOfMemory`] if the allocator cannot satisfy the
    /// request, or [`RallocError::InvalidAlignment`] if Ralloc's default
    /// alignment is unsupported by the native allocator. A zero-length buffer
    /// never allocates and always succeeds.
    pub fn try_new(len: usize) -> Result<Self, RallocError> {
        Self::try_new_aligned(len, crate::block::ALIGNMENT)
    }

    /// Allocates a zero-initialized byte buffer of `len` bytes with at least `align` byte
    /// alignment.
    ///
    /// # Errors
    ///
    /// Returns [`RallocError::InvalidAlignment`] if `align` is not a supported
    /// native alignment, or [`RallocError::OutOfMemory`] if the allocator
    /// cannot satisfy a non-zero `len` request. A zero-length buffer never
    /// allocates and always succeeds.
    pub fn try_new_aligned(len: usize, align: usize) -> Result<Self, RallocError> {
        if !is_supported_native_alignment(align) {
            return Err(RallocError::InvalidAlignment);
        }

        if len == 0 {
            return Ok(Self::empty_with_alignment(align));
        }

        let ptr = allocate_aligned(len, align).cast::<u8>();
        let Some(ptr) = NonNull::new(ptr) else {
            return Err(RallocError::OutOfMemory);
        };

        // SAFETY: The allocator returned uniquely owned storage writable for
        // `len` bytes. A previous typed allocation may have left uninitialized
        // padding here, so initialize every byte before exposing safe slices.
        unsafe { ptr.as_ptr().write_bytes(0, len) };

        Ok(Self {
            ptr,
            len,
            align,
            _not_sync: PhantomData,
        })
    }

    /// Returns the number of bytes in the buffer.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Returns whether the buffer has length zero.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns the buffer as an immutable byte slice.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: `ptr` is either a live allocation of `len` initialized bytes
        // or a dangling non-null pointer with `len == 0`. Raw ownership recovery
        // requires the caller to preserve this initialization invariant.
        unsafe { slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }

    /// Returns the buffer as a mutable byte slice.
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: `&mut self` proves unique access to the owned allocation, and
        // `ptr` is either live for `len` initialized bytes or dangling with
        // `len == 0`. Raw ownership recovery preserves this invariant.
        unsafe { slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
    }

    /// Returns the allocation pointer for identity checks and FFI handoff.
    #[must_use]
    pub const fn as_ptr(&self) -> *const u8 {
        self.ptr.as_ptr().cast_const()
    }

    /// Resizes the buffer, preserving the first `min(old_len, new_len)` bytes.
    /// Newly added bytes are zero-initialized.
    ///
    /// Returns `false` if allocation fails; on failure the original buffer is
    /// left unchanged.
    pub fn resize(&mut self, new_len: usize) -> bool {
        self.try_resize(new_len).is_ok()
    }

    /// Resizes the buffer, preserving the first `min(old_len, new_len)` bytes.
    /// Newly added bytes are zero-initialized.
    ///
    /// On error the original buffer remains unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`RallocError::OutOfMemory`] if the allocator cannot reallocate
    /// the buffer to `new_len` bytes. The original contents and length are
    /// preserved on failure.
    pub fn try_resize(&mut self, new_len: usize) -> Result<(), RallocError> {
        if new_len == self.len {
            return Ok(());
        }

        #[cfg(any(test, feature = "testing-hooks"))]
        let grew = new_len > self.len;

        if new_len == 0 {
            self.deallocate();
            self.ptr = NonNull::dangling();
            self.len = 0;
            return Ok(());
        }

        if self.len == 0 {
            let buffer = Self::try_new_aligned(new_len, self.align)?;
            self.ptr = buffer.ptr;
            self.len = buffer.len;
            self.align = buffer.align;
            core::mem::forget(buffer);
            #[cfg(any(test, feature = "testing-hooks"))]
            if grew {
                crate::instrumentation::record_growth();
            }
            return Ok(());
        }

        let resized = reallocate_aligned(self.ptr.as_ptr().cast::<c_void>(), new_len, self.align).cast::<u8>();
        let Some(ptr) = NonNull::new(resized) else {
            return Err(RallocError::OutOfMemory);
        };

        if new_len > self.len {
            // SAFETY: Successful reallocation owns `new_len` writable bytes.
            // The old logical prefix is initialized; the new tail may include
            // uninitialized allocator capacity and must be initialized here.
            unsafe { ptr.as_ptr().add(self.len).write_bytes(0, new_len - self.len) };
        }

        self.ptr = ptr;
        self.len = new_len;
        #[cfg(any(test, feature = "testing-hooks"))]
        if grew {
            crate::instrumentation::record_growth();
        }
        Ok(())
    }

    /// Consumes the buffer and returns its raw allocation pointer and length.
    ///
    /// The caller becomes responsible for passing the returned parts back to
    /// `RallocBuffer::from_raw_parts` exactly once, or otherwise freeing them
    /// according to Ralloc's allocator contract.
    #[must_use]
    pub fn into_raw_parts(self) -> (*mut u8, usize) {
        let this = ManuallyDrop::new(self);
        (this.ptr.as_ptr(), this.len)
    }

    /// Reclaims ownership of raw parts produced by `into_raw_parts`.
    ///
    /// # Safety
    ///
    /// `ptr` and `len` must come from a previous `RallocBuffer::into_raw_parts`
    /// call, must not have been freed, and must not already be owned by another
    /// handle. All `len` bytes must remain initialized while ownership is raw.
    ///
    /// # Errors
    ///
    /// Returns [`RallocError::InvalidRawParts`] if `ptr` is null while `len` is
    /// non-zero. A zero-length (`len == 0`) reclamation always succeeds.
    pub unsafe fn from_raw_parts(ptr: *mut u8, len: usize) -> Result<Self, RallocError> {
        if len == 0 {
            return Ok(Self::empty());
        }

        let Some(ptr) = NonNull::new(ptr) else {
            return Err(RallocError::InvalidRawParts);
        };

        Ok(Self {
            ptr,
            len,
            align: crate::block::ALIGNMENT,
            _not_sync: PhantomData,
        })
    }

    fn empty() -> Self {
        Self::empty_with_alignment(crate::block::ALIGNMENT)
    }

    fn empty_with_alignment(align: usize) -> Self {
        Self {
            ptr: NonNull::dangling(),
            len: 0,
            align,
            _not_sync: PhantomData,
        }
    }

    fn deallocate(&mut self) {
        if self.len == 0 {
            return;
        }

        // SAFETY: `ptr` is a live allocation owned by this buffer.
        unsafe { ralloc_free(self.ptr.as_ptr().cast::<c_void>()) };
    }
}

impl Drop for RallocBuffer {
    fn drop(&mut self) {
        self.deallocate();
    }
}

/// Owned typed allocation backed by Ralloc.
///
/// `RallocBox<T>` owns exactly one initialized `T`. Dropping the handle drops
/// the value first and then releases the allocator storage, including when
/// the value's destructor unwinds. Zero-sized values retain the same ownership
/// and drop behavior without allocating storage.
pub struct RallocBox<T> {
    ptr:       NonNull<T>,
    allocated: bool,
    _not_sync: PhantomData<Cell<T>>,
}

// SAFETY: Moving `RallocBox<T>` to another thread transfers ownership of the
// contained value. This is sound when `T` itself may be sent across threads.
unsafe impl<T: Send> Send for RallocBox<T> {}

impl<T> RallocBox<T> {
    /// Allocates storage and moves `value` into it.
    pub fn new(value: T) -> Option<Self> {
        Self::try_new(value).ok()
    }

    /// Allocates storage and moves `value` into it, returning an explicit error
    /// if Ralloc cannot safely store this type.
    ///
    /// # Errors
    ///
    /// Returns [`RallocError::OutOfMemory`] if the allocator cannot provide
    /// `size_of::<T>()` bytes of storage for a non-zero-sized `T`. Zero-sized
    /// types are never allocated and always succeed.
    pub fn try_new(value: T) -> Result<Self, RallocError> {
        if mem::size_of::<T>() == 0 {
            // Transfer the zero-sized value's destructor obligation to the
            // returned handle; returning must not drop the incoming value.
            mem::forget(value);
            return Ok(Self {
                ptr:       NonNull::dangling(),
                allocated: false,
                _not_sync: PhantomData,
            });
        }

        let ptr = allocate_aligned(mem::size_of::<T>(), mem::align_of::<T>()).cast::<T>();
        let Some(ptr) = NonNull::new(ptr) else {
            return Err(RallocError::OutOfMemory);
        };

        // SAFETY: `ptr` points to allocator-owned storage of at least
        // `size_of::<T>()` bytes, and this handle becomes its owner.
        unsafe { ptr.as_ptr().write(value) };

        Ok(Self {
            ptr,
            allocated: true,
            _not_sync: PhantomData,
        })
    }

    /// Returns an immutable reference to the contained value.
    #[must_use]
    pub fn get(&self) -> &T {
        // SAFETY: `ptr` points to an initialized `T` owned by this handle.
        unsafe { self.ptr.as_ref() }
    }

    /// Returns a mutable reference to the contained value.
    pub fn get_mut(&mut self) -> &mut T {
        // SAFETY: `&mut self` proves unique access to the initialized `T`.
        unsafe { self.ptr.as_mut() }
    }

    /// Returns the owned value pointer.
    #[must_use]
    pub const fn as_ptr(&self) -> *const T {
        self.ptr.as_ptr().cast_const()
    }

    /// Moves the contained value out and releases the allocation.
    #[must_use]
    pub fn into_inner(self) -> T {
        let this = ManuallyDrop::new(self);
        // SAFETY: `ptr` points to an initialized `T`, and `ManuallyDrop`
        // prevents `Drop` from dropping it a second time.
        let value = unsafe { ptr::read(this.ptr.as_ptr()) };

        if this.allocated {
            // SAFETY: The value has been moved out, but the allocation is still
            // owned by this handle and must be released exactly once.
            unsafe { ralloc_free(this.ptr.as_ptr().cast::<c_void>()) };
        }

        value
    }

    /// Consumes the box and returns the owned raw pointer.
    ///
    /// The caller becomes responsible for passing the pointer back to
    /// `RallocBox::from_raw` exactly once, or otherwise dropping the value and
    /// freeing the allocation according to Ralloc's allocator contract.
    #[must_use]
    pub fn into_raw(self) -> *mut T {
        let this = ManuallyDrop::new(self);
        this.ptr.as_ptr()
    }

    /// Reclaims ownership of a pointer produced by `into_raw`.
    ///
    /// # Safety
    ///
    /// `ptr` must come from a previous `RallocBox<T>::into_raw` call, must point
    /// to an initialized `T`, must not have been freed, and must not already be
    /// owned by another handle.
    ///
    /// # Errors
    ///
    /// Returns [`RallocError::InvalidRawParts`] if `ptr` is null.
    pub unsafe fn from_raw(ptr: *mut T) -> Result<Self, RallocError> {
        let Some(ptr) = NonNull::new(ptr) else {
            return Err(RallocError::InvalidRawParts);
        };

        Ok(Self {
            ptr,
            allocated: mem::size_of::<T>() != 0,
            _not_sync: PhantomData,
        })
    }
}

impl<T> Drop for RallocBox<T> {
    fn drop(&mut self) {
        // Install the storage owner before invoking user-provided drop code.
        // Its cleanup runs after T's field drops, on success or unwinding.
        let _storage = if self.allocated {
            Some(BoxStorageRelease(self.ptr.cast()))
        } else {
            None
        };
        // SAFETY: `ptr` points to the initialized value owned by this handle.
        unsafe { ptr::drop_in_place(self.ptr.as_ptr()) };
    }
}

struct BoxStorageRelease(NonNull<c_void>);

impl Drop for BoxStorageRelease {
    fn drop(&mut self) {
        // SAFETY: RallocBox::drop transfers its non-zero allocation to this
        // sole storage owner. The value's drop attempt has finished, and no
        // other owner may free the allocation or access its value again.
        unsafe { ralloc_free(self.0.as_ptr()) };
    }
}

impl<T> Deref for RallocBox<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.get()
    }
}

impl<T> DerefMut for RallocBox<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.get_mut()
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use core::sync::atomic::{AtomicUsize, Ordering};
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use super::{RallocBox, RallocBuffer};
    use crate::{instrumentation, ralloc, region};

    struct PanickingValue<'a> {
        drops: &'a AtomicUsize,
        child: RallocBuffer,
    }

    impl Drop for PanickingValue<'_> {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::SeqCst);
            // The box and its field must stay live until the value's
            // destructor has finished, including the unwinding field drops.
            self.child.as_mut_slice()[0] = 91;
            assert_eq!(self.child.as_slice()[0], 91);
            std::panic!("intentional value-drop failure");
        }
    }

    #[repr(align(4096))]
    struct AlignedValue<'a>(PanickingValue<'a>);

    fn assert_nested_box_unwind_cleanup<T>(
        value: RallocBox<T>,
        drops: &AtomicUsize,
        before: instrumentation::InstrumentationSnapshot,
    ) {
        // The closure consumes the only box owner. The test observes only
        // independent counters after catching the intentional panic.
        let outcome = catch_unwind(AssertUnwindSafe(|| drop(value)));
        assert_eq!(
            outcome
                .expect_err("value destructor must unwind")
                .downcast_ref::<&str>(),
            Some(&"intentional value-drop failure"),
        );
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        let after = instrumentation::snapshot();
        assert_eq!(after.allocations - before.allocations, 2, "box and nested byte buffer");
        assert_eq!(after.deallocations - before.deallocations, 2, "both owners release storage");
        assert_eq!(after.live_payload_bytes, before.live_payload_bytes, "no leaked box payload");
    }

    #[test]
    fn box_unwinding_value_drop_releases_its_allocation() {
        let _exclusive = region::TEST_LOCK.lock();
        ralloc::reset_for_tests();
        let before = instrumentation::snapshot();
        let drops = AtomicUsize::new(0);
        let value = RallocBox::try_new(PanickingValue {
            drops: &drops,
            child: RallocBuffer::try_new(32).expect("nested byte buffer"),
        })
        .expect("box allocation");

        assert_nested_box_unwind_cleanup(value, &drops, before);
    }

    #[test]
    fn box_unwinding_field_drop_releases_raw_recovered_aligned_allocation() {
        let _exclusive = region::TEST_LOCK.lock();
        ralloc::reset_for_tests();
        let before = instrumentation::snapshot();
        let drops = AtomicUsize::new(0);
        let value = RallocBox::try_new(AlignedValue(PanickingValue {
            drops: &drops,
            child: RallocBuffer::try_new(32).expect("nested byte buffer"),
        }))
        .expect("aligned box allocation");
        assert_eq!(value.as_ptr().addr() % 4096, 0);
        assert_eq!(value.get().0.child.len(), 32);
        let raw = value.into_raw();
        // SAFETY: This recovers the sole, still-live owner just relinquished
        // by into_raw, with the original value type and no intervening access.
        let recovered = unsafe { RallocBox::from_raw(raw) }.expect("raw ownership recovery");

        assert_nested_box_unwind_cleanup(recovered, &drops, before);
    }

    #[test]
    fn box_unwinding_zero_sized_drop_does_not_free_storage() {
        #[repr(align(4096))]
        struct PanickingZeroSized;

        impl Drop for PanickingZeroSized {
            fn drop(&mut self) {
                std::panic!("intentional zero-sized drop failure");
            }
        }

        let _exclusive = region::TEST_LOCK.lock();
        ralloc::reset_for_tests();
        let before = instrumentation::snapshot();
        let value = RallocBox::try_new(PanickingZeroSized).expect("zero-sized owner");
        assert_eq!(value.as_ptr().addr() % 4096, 0);
        let outcome = catch_unwind(AssertUnwindSafe(|| drop(value)));
        assert_eq!(
            outcome
                .expect_err("zero-sized destructor must unwind")
                .downcast_ref::<&str>(),
            Some(&"intentional zero-sized drop failure"),
        );
        assert_eq!(instrumentation::snapshot(), before, "zero-sized values do not reach the allocator");
    }

    #[test]
    fn box_zero_sized_ownership_transfer_defers_drop_until_the_final_owner() {
        static DROPS: AtomicUsize = AtomicUsize::new(0);
        struct ZeroSized;

        impl Drop for ZeroSized {
            fn drop(&mut self) {
                DROPS.fetch_add(1, Ordering::SeqCst);
            }
        }

        let _exclusive = region::TEST_LOCK.lock();
        ralloc::reset_for_tests();
        let before = instrumentation::snapshot();
        DROPS.store(0, Ordering::SeqCst);

        let owned = RallocBox::try_new(ZeroSized).expect("zero-sized allocation");
        assert_eq!(DROPS.load(Ordering::SeqCst), 0, "construction transfers, rather than drops, the value");
        drop(owned);
        assert_eq!(DROPS.load(Ordering::SeqCst), 1);

        let owned = RallocBox::try_new(ZeroSized).expect("zero-sized allocation");
        let inner = owned.into_inner();
        assert_eq!(DROPS.load(Ordering::SeqCst), 1, "moving out preserves ownership");
        drop(inner);
        assert_eq!(DROPS.load(Ordering::SeqCst), 2);

        let owned = RallocBox::try_new(ZeroSized).expect("zero-sized allocation");
        let raw = owned.into_raw();
        assert_eq!(DROPS.load(Ordering::SeqCst), 2, "raw handoff does not drop the value");
        // SAFETY: The pointer retains sole ownership of the initialized
        // zero-sized value and has not been reclaimed or freed elsewhere.
        let recovered = unsafe { RallocBox::from_raw(raw) }.expect("zero-sized ownership recovery");
        drop(recovered);
        assert_eq!(DROPS.load(Ordering::SeqCst), 3);
        assert_eq!(instrumentation::snapshot(), before, "zero-sized ownership does not allocate");
    }
}
