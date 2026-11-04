use core::ptr;

pub type TinyRuntimeHandle = u64;
pub type TinyHandle = u64;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TinyBuffer {
    pub data:   *const u8,
    pub length: u64,
}

impl Default for TinyBuffer {
    fn default() -> Self {
        Self {
            data:   ptr::null(),
            length: 0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TinyMutableBuffer {
    pub data:     *mut u8,
    pub capacity: u64,
}

impl Default for TinyMutableBuffer {
    fn default() -> Self {
        Self {
            data:     ptr::null_mut(),
            capacity: 0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TinyOwnedBuffer {
    pub data:        *mut u8,
    pub length:      u64,
    pub capacity:    u64,
    pub owner_token: u64,
}

impl Default for TinyOwnedBuffer {
    fn default() -> Self {
        Self {
            data:        ptr::null_mut(),
            length:      0,
            capacity:    0,
            owner_token: 0,
        }
    }
}
