use core::ptr;

pub type TynraRuntimeHandle = u64;
pub type TynraHandle = u64;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TynraBuffer {
    pub data: *const u8,
    pub length: u64,
}

impl Default for TynraBuffer {
    fn default() -> Self {
        Self {
            data: ptr::null(),
            length: 0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TynraMutableBuffer {
    pub data: *mut u8,
    pub capacity: u64,
}

impl Default for TynraMutableBuffer {
    fn default() -> Self {
        Self {
            data: ptr::null_mut(),
            capacity: 0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TynraOwnedBuffer {
    pub data: *mut u8,
    pub length: u64,
    pub capacity: u64,
    pub owner_token: u64,
}

impl Default for TynraOwnedBuffer {
    fn default() -> Self {
        Self {
            data: ptr::null_mut(),
            length: 0,
            capacity: 0,
            owner_token: 0,
        }
    }
}
