//! Code-page ownership: allocate RW, publish RX, never create a WX view.
//!
//! Each supported host provides one private implementation with the same
//! stricter RW -> RX publication invariant. `NativeProgram` and
//! `NativeFunction` see a single [`ExecutableMemory`] type regardless of the
//! operating system.

#[cfg(all(target_arch = "x86_64", target_os = "windows"))]
mod windows;
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
mod linux;

#[cfg(all(target_arch = "x86_64", target_os = "windows"))]
pub(super) use windows::ExecutableMemory;
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
pub(super) use linux::ExecutableMemory;
