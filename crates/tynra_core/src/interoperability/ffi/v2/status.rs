pub type TynraStatus = u32;

pub const TYNRA_STATUS_OK: TynraStatus = 0;
pub const TYNRA_STATUS_INVALID_ARGUMENT: TynraStatus = 1;
pub const TYNRA_STATUS_INVALID_FRAME: TynraStatus = 2;
pub const TYNRA_STATUS_UNSUPPORTED_VERSION: TynraStatus = 3;
pub const TYNRA_STATUS_UNKNOWN_OPCODE: TynraStatus = 4;
pub const TYNRA_STATUS_BUFFER_TOO_SMALL: TynraStatus = 5;
pub const TYNRA_STATUS_INVALID_HANDLE: TynraStatus = 6;
pub const TYNRA_STATUS_STALE_HANDLE: TynraStatus = 7;
pub const TYNRA_STATUS_OUT_OF_MEMORY: TynraStatus = 8;
pub const TYNRA_STATUS_VM_TRAP: TynraStatus = 9;
pub const TYNRA_STATUS_INTERNAL: TynraStatus = 10;
pub const TYNRA_STATUS_PANIC: TynraStatus = 11;
pub const TYNRA_STATUS_BUSY: TynraStatus = 12;
pub const TYNRA_STATUS_INVALID_ARTIFACT: TynraStatus = 13;
