use ralloc::{RallocBox, RallocBuffer};

#[repr(C)]
struct Padded {
    tag:   u8,
    value: u64,
}

fn release_typed_storage() {
    let value = RallocBox::try_new(Padded { tag: 7, value: 42 }).expect("typed allocation");
    drop(value);
}

fn read_every_byte(buffer: &RallocBuffer) {
    // Reading bytes through the safe API must never read uninitialized
    // padding left behind by an earlier typed allocation. Miri checks that.
    std::hint::black_box(buffer.as_slice().iter().copied().fold(0u8, u8::wrapping_add));
}

#[test]
fn byte_allocation_initializes_reused_typed_padding() {
    release_typed_storage();
    let buffer = RallocBuffer::try_new(core::mem::size_of::<Padded>()).expect("byte allocation");
    read_every_byte(&buffer);
    assert!(buffer.as_slice().iter().all(|byte| *byte == 0));
}

#[test]
fn byte_growth_initializes_new_bytes_in_place_and_after_a_move() {
    for size in [8, 128] {
        release_typed_storage();
        let mut buffer = RallocBuffer::try_new(1).expect("small byte allocation");
        buffer.as_mut_slice()[0] = 91;
        buffer.try_resize(size).expect("byte growth");
        assert_eq!(buffer.as_slice()[0], 91);
        read_every_byte(&buffer);
        assert!(buffer.as_slice()[1..].iter().all(|byte| *byte == 0));
    }
}

#[test]
fn shrinking_then_growing_does_not_expose_the_discarded_tail() {
    let mut buffer = RallocBuffer::try_new_aligned(128, 64).expect("aligned buffer");
    buffer.as_mut_slice().fill(73);
    buffer.try_resize(1).expect("shrink");
    buffer.try_resize(128).expect("regrow");
    assert_eq!(buffer.as_ptr().addr() % 64, 0);
    assert_eq!(buffer.as_slice()[0], 73);
    assert!(buffer.as_slice()[1..].iter().all(|byte| *byte == 0));
    buffer.try_resize(0).expect("empty");
    buffer.try_resize(64).expect("grow empty");
    assert_eq!(buffer.as_ptr().addr() % 64, 0);
    assert!(buffer.as_slice().iter().all(|byte| *byte == 0));
}
