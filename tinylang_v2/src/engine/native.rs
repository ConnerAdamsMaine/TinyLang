//! Checked scalar IR compiled directly to a leaf Windows x86-64 function.
//!
//! No bytecode dispatcher, host arithmetic callback or interpreter fallback is
//! involved. Code can only originate from the verified `ScalarFunction` capability.

#[cfg(all(target_arch = "x86_64", target_os = "windows"))]
mod executable;
#[cfg(all(target_arch = "x86_64", target_os = "windows"))]
mod x64;

#[cfg(all(target_arch = "x86_64", target_os = "windows"))]
use super::TrapKind;
use super::{EngineError, ScalarFunction};

/// Owns immutable RX machine code and the bounded scalar frame specification.
pub struct NativeFunction {
    #[cfg(all(target_arch = "x86_64", target_os = "windows"))]
    function: ScalarFunction,
    #[cfg(all(target_arch = "x86_64", target_os = "windows"))]
    code: executable::ExecutableMemory,
}

// repr(C) field offsets are shared with the private emitter via offset_of!.
// Slots/stack point to separate host allocations; the leaf never uses the
// machine stack except for its caller-provided return address.
#[cfg(all(target_arch = "x86_64", target_os = "windows"))]
#[repr(C)]
struct CallFrame {
    slots: *mut i64,
    stack: *mut i64,
    fuel: u64,
    result: i64,
    trap: u64,
    instruction: u64,
}

#[cfg(all(target_arch = "x86_64", target_os = "windows"))]
const OVERFLOW: u32 = 1;
#[cfg(all(target_arch = "x86_64", target_os = "windows"))]
const DIVISION_BY_ZERO: u32 = 2;
#[cfg(all(target_arch = "x86_64", target_os = "windows"))]
const FUEL_EXHAUSTED: u32 = 3;

impl NativeFunction {
    pub fn compile(function: &ScalarFunction) -> Result<Self, EngineError> {
        #[cfg(all(target_arch = "x86_64", target_os = "windows"))]
        {
            let bytes = x64::emit(function)?;
            let code = executable::ExecutableMemory::from_emitted(&bytes)?;
            Ok(Self {
                function: function.clone(),
                code,
            })
        }
        #[cfg(not(all(target_arch = "x86_64", target_os = "windows")))]
        {
            let _ = function;
            Err(unavailable())
        }
    }

    pub fn invoke(&self, args: &[i64], fuel: u64) -> Result<i64, EngineError> {
        #[cfg(all(target_arch = "x86_64", target_os = "windows"))]
        {
            let (mut slots, mut stack) = self.function.frame(args)?;
            let mut frame = CallFrame {
                slots: slots.as_mut_ptr(),
                stack: stack.as_mut_ptr(),
                fuel,
                result: 0,
                trap: 0,
                instruction: 0,
            };
            // SAFETY: Only our emitter can create this private RX allocation.
            // Its entry follows the Windows x64 one-pointer leaf ABI, preserves
            // all nonvolatile registers and RSP, and accesses only these live,
            // bounded frame buffers at offsets proven by scalar lowering.
            // No executable pointer or arbitrary-code constructor is public.
            let entry: unsafe extern "system" fn(*mut CallFrame) =
                unsafe { std::mem::transmute(self.code.as_ptr()) };
            // SAFETY: The mapping and frame buffers outlive this synchronous
            // call. The emitted instruction fuel bounds every control-flow edge.
            unsafe { entry(&mut frame) };
            let kind = match frame.trap {
                0 => return Ok(frame.result),
                value if value == u64::from(OVERFLOW) => TrapKind::Overflow,
                value if value == u64::from(DIVISION_BY_ZERO) => TrapKind::DivisionByZero,
                value if value == u64::from(FUEL_EXHAUSTED) => TrapKind::FuelExhausted,
                value => {
                    return Err(EngineError::Codegen(format!(
                        "invalid native trap tag {value}"
                    )));
                }
            };
            Err(EngineError::Trap {
                kind,
                instruction: frame.instruction as usize,
            })
        }
        #[cfg(not(all(target_arch = "x86_64", target_os = "windows")))]
        {
            let _ = (args, fuel);
            Err(unavailable())
        }
    }

    /// Inspect immutable emitted bytes; there is no arbitrary-code invocation API.
    pub fn code_bytes(&self) -> &[u8] {
        #[cfg(all(target_arch = "x86_64", target_os = "windows"))]
        {
            self.code.bytes()
        }
        #[cfg(not(all(target_arch = "x86_64", target_os = "windows")))]
        {
            &[]
        }
    }
}

#[cfg(not(all(target_arch = "x86_64", target_os = "windows")))]
fn unavailable() -> EngineError {
    EngineError::BackendUnavailable {
        backend: "x86-64 ASM",
        reason: "this backend requires Windows x86-64".to_owned(),
    }
}
