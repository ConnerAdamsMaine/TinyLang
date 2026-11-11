pub type TinyStatus = u32;

pub const TINY_STATUS_OK: TinyStatus = 0;
pub const TINY_STATUS_INVALID_ARGUMENT: TinyStatus = 1;
pub const TINY_STATUS_INVALID_FRAME: TinyStatus = 2;
pub const TINY_STATUS_UNSUPPORTED_VERSION: TinyStatus = 3;
pub const TINY_STATUS_UNKNOWN_OPCODE: TinyStatus = 4;
pub const TINY_STATUS_BUFFER_TOO_SMALL: TinyStatus = 5;
pub const TINY_STATUS_INVALID_HANDLE: TinyStatus = 6;
pub const TINY_STATUS_STALE_HANDLE: TinyStatus = 7;
pub const TINY_STATUS_OUT_OF_MEMORY: TinyStatus = 8;
pub const TINY_STATUS_VM_TRAP: TinyStatus = 9;
pub const TINY_STATUS_INTERNAL: TinyStatus = 10;
pub const TINY_STATUS_PANIC: TinyStatus = 11;
pub const TINY_STATUS_BUSY: TinyStatus = 12;
pub const TINY_STATUS_INVALID_ARTIFACT: TinyStatus = 13;
