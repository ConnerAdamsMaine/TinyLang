//! Checked scalar IR compiled directly to a leaf native x86-64 function.
//!
//! Supported hosts: x86-64 Windows and x86-64 Linux. The emitted image uses
//! only registers that are volatile under both the Windows x64 and SysV AMD64
//! ABIs, so the sole ABI difference is the argument register selected in
//! `x64::ENTRY_ARG_TO_R10`.
//!
//! No bytecode dispatcher, host arithmetic callback or interpreter fallback is
//! involved. Code can only originate from the verified `ScalarFunction` capability.

#[cfg(all(target_arch = "x86_64", any(target_os = "windows", target_os = "linux")))]
mod executable;
#[cfg(all(target_arch = "x86_64", any(target_os = "windows", target_os = "linux")))]
mod x64;

#[cfg(all(target_arch = "x86_64", any(target_os = "windows", target_os = "linux")))]
use super::TrapKind;
use super::{EngineError, ScalarFunction};

/// Owns immutable RX machine code and the bounded scalar frame specification.
pub struct NativeFunction {
    #[cfg(all(target_arch = "x86_64", any(target_os = "windows", target_os = "linux")))]
    function: ScalarFunction,
    #[cfg(all(target_arch = "x86_64", any(target_os = "windows", target_os = "linux")))]
    code: executable::ExecutableMemory,
}

// repr(C) field offsets are shared with the private emitter via offset_of!.
// Slots/stack point to separate host allocations; the leaf never uses the
// machine stack except for its caller-provided return address.
#[cfg(all(target_arch = "x86_64", any(target_os = "windows", target_os = "linux")))]
#[repr(C)]
struct CallFrame {
    slots: *mut i64,
    stack: *mut i64,
    fuel: u64,
    result: i64,
    trap: u64,
    instruction: u64,
}

#[cfg(all(target_arch = "x86_64", any(target_os = "windows", target_os = "linux")))]
const OVERFLOW: u32 = 1;
#[cfg(all(target_arch = "x86_64", any(target_os = "windows", target_os = "linux")))]
const DIVISION_BY_ZERO: u32 = 2;
#[cfg(all(target_arch = "x86_64", any(target_os = "windows", target_os = "linux")))]
const FUEL_EXHAUSTED: u32 = 3;
#[cfg(all(target_arch = "x86_64", any(target_os = "windows", target_os = "linux")))]
const CALL_DEPTH: u32 = 4;

impl NativeFunction {
    pub fn compile(function: &ScalarFunction) -> Result<Self, EngineError> {
        #[cfg(all(target_arch = "x86_64", any(target_os = "windows", target_os = "linux")))]
        {
            let bytes = x64::emit(function)?;
            let code = executable::ExecutableMemory::from_emitted(&bytes)?;
            Ok(Self {
                function: function.clone(),
                code,
            })
        }
        #[cfg(not(all(target_arch = "x86_64", any(target_os = "windows", target_os = "linux"))))]
        {
            let _ = function;
            Err(unavailable())
        }
    }

    pub fn invoke(&self, args: &[i64], fuel: u64) -> Result<i64, EngineError> {
        #[cfg(all(target_arch = "x86_64", any(target_os = "windows", target_os = "linux")))]
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
            // Its entry follows a one-pointer leaf ABI (argument in RCX on
            // Windows, RDI on Linux), preserves every register that is
            // nonvolatile under the host ABI, leaves RSP unchanged, and
            // accesses only these live, bounded frame buffers at offsets
            // proven by scalar lowering. `extern "system"` denotes exactly the
            // host ABI here (Windows x64, SysV AMD64 on Linux).
            // No executable pointer or arbitrary-code constructor is public.
            let entry: unsafe extern "system" fn(*mut CallFrame) = unsafe { std::mem::transmute(self.code.as_ptr()) };
            // SAFETY: The mapping and frame buffers outlive this synchronous
            // call. The emitted instruction fuel bounds every control-flow edge.
            unsafe { entry(&mut frame) };
            let kind = match frame.trap {
                0 => return Ok(frame.result),
                value if value == u64::from(OVERFLOW) => TrapKind::Overflow,
                value if value == u64::from(DIVISION_BY_ZERO) => TrapKind::DivisionByZero,
                value if value == u64::from(FUEL_EXHAUSTED) => TrapKind::FuelExhausted,
                value => {
                    return Err(EngineError::Codegen(format!("invalid native trap tag {value}")));
                }
            };
            Err(EngineError::Trap {
                kind,
                instruction: frame.instruction as usize,
            })
        }
        #[cfg(not(all(target_arch = "x86_64", any(target_os = "windows", target_os = "linux"))))]
        {
            let _ = (args, fuel);
            Err(unavailable())
        }
    }

    /// Inspect immutable emitted bytes; there is no arbitrary-code invocation API.
    pub fn code_bytes(&self) -> &[u8] {
        #[cfg(all(target_arch = "x86_64", any(target_os = "windows", target_os = "linux")))]
        {
            self.code.bytes()
        }
        #[cfg(not(all(target_arch = "x86_64", any(target_os = "windows", target_os = "linux"))))]
        {
            &[]
        }
    }
}

#[cfg(not(all(target_arch = "x86_64", any(target_os = "windows", target_os = "linux"))))]
fn unavailable() -> EngineError {
    EngineError::BackendUnavailable {
        backend: "x86-64 ASM",
        reason: "this backend requires x86-64 Windows or Linux".to_owned(),
    }
}

/// Per-invocation machine state shared by every function of one program
/// execution. Layout is fixed by `x64::emit_program` (u64 offsets).
#[cfg(all(target_arch = "x86_64", any(target_os = "windows", target_os = "linux")))]
#[repr(C)]
struct ProgramContext {
    depth: u64,
    trap: u32,
    pad: u32,
    fuel: u64,
    bank: *mut i64,
    pc: u64,
}

/// Owns the machine-code image of a whole scalar call closure plus its
/// runtime frame-bank specification.
pub struct NativeProgram {
    #[cfg(all(target_arch = "x86_64", any(target_os = "windows", target_os = "linux")))]
    parameters: usize,
    #[cfg(all(target_arch = "x86_64", any(target_os = "windows", target_os = "linux")))]
    depth_limit: usize,
    #[cfg(all(target_arch = "x86_64", any(target_os = "windows", target_os = "linux")))]
    stride_values: usize,
    #[cfg(all(target_arch = "x86_64", any(target_os = "windows", target_os = "linux")))]
    code: executable::ExecutableMemory,
}

#[cfg(all(target_arch = "x86_64", any(target_os = "windows", target_os = "linux")))]
impl NativeProgram {
    /// Compiles one whole call closure into a single image. Frame-bank and
    /// context conventions here must stay in lockstep with `x64::emit_program`.
    pub fn compile(program: &super::ScalarProgram) -> Result<Self, EngineError> {
        let entry = program.entry();
        let entry_function = program
            .function(entry)
            .ok_or_else(|| EngineError::UnknownFunction(format!("#{entry}")))?;
        let parameters = entry_function.parameter_count();
        let mut slot_capacity = 0usize;
        let mut stack_capacity = 0usize;
        for index in 0..program.function_count() {
            if let Some(function) = program.function(index) {
                slot_capacity = slot_capacity.max(function.slot_count());
                stack_capacity = stack_capacity.max(function.max_stack());
            }
        }
        // Collect references in program order for the emitter. Every lowered
        // function goes into its image slot; holes stay empty and can never be
        // called (whole-closure lowering guarantees that).
        let mut functions: Vec<Option<&ScalarFunction>> = Vec::with_capacity(program.function_count());
        for index in 0..program.function_count() {
            functions.push(program.function(index));
        }
        let bytes = x64::emit_program(&functions, entry, slot_capacity, stack_capacity, program.call_depth_limit())?;
        let code = executable::ExecutableMemory::from_emitted(&bytes)?;
        Ok(Self {
            parameters,
            depth_limit: program.call_depth_limit(),
            stride_values: slot_capacity + stack_capacity,
            code,
        })
    }
}

impl NativeProgram {
    /// Runs the entry function with `args` and one shared fuel budget, mirroring
    /// `ScalarProgram::interpret` including trap categories and failing program
    /// counters.
    pub fn invoke(&self, args: &[i64], fuel: u64) -> Result<i64, EngineError> {
        #[cfg(all(target_arch = "x86_64", any(target_os = "windows", target_os = "linux")))]
        {
            if args.len() != self.parameters {
                return Err(EngineError::InvalidArguments {
                    expected: self.parameters,
                    actual: args.len(),
                });
            }
            // Frame bank: the entry frame occupies segment 0 (its prologue reads
            // depth 0); nested calls go up to depth `depth_limit`, so the host
            // preallocates exactly `depth_limit + 1` segments of `stride_values`
            // values. Arguments are the entry frame's initial slots.
            let count = self
                .depth_limit
                .checked_add(1)
                .and_then(|segments| segments.checked_mul(self.stride_values))
                .ok_or_else(|| EngineError::Codegen("frame bank size exceeds usize".to_owned()))?;
            let mut bank = Vec::new();
            bank.try_reserve_exact(count)
                .map_err(|error| EngineError::Allocation(error.to_string()))?;
            bank.resize(count, 0i64);
            bank[..args.len()].copy_from_slice(args);
            let mut context = ProgramContext {
                depth: 0,
                trap: 0,
                pad: 0,
                fuel,
                bank: bank.as_mut_ptr(),
                pc: 0,
            };
            // SAFETY: Only our emitter can create this private RX allocation. Its
            // entry follows a one-pointer leaf ABI (RCX on Windows, RDI on
            // Linux) for the program profile: it touches only registers that
            // are volatile under the host ABI (the invocation context lives in
            // R10, volatile on both), preserves all nonvolatile registers and
            // RSP, uses the hardware stack only for return addresses bounded by
            // the emitted depth limit, and accesses only this live bank through
            // the context.
            // No executable pointer or arbitrary-code constructor is public.
            let entry: unsafe extern "system" fn(*mut ProgramContext) -> u64 =
                unsafe { std::mem::transmute(self.code.as_ptr()) };
            // SAFETY: The mapping and the bank outlive this synchronous call.
            // The emitted instruction fuel bounds every control-flow edge.
            let result = unsafe { entry(&mut context) };
            let kind = match context.trap {
                0 => return Ok(result as i64),
                value if value == OVERFLOW => TrapKind::Overflow,
                value if value == DIVISION_BY_ZERO => TrapKind::DivisionByZero,
                value if value == FUEL_EXHAUSTED => TrapKind::FuelExhausted,
                value if value == CALL_DEPTH => TrapKind::CallStackOverflow(self.depth_limit),
                value => {
                    return Err(EngineError::Codegen(format!("invalid native trap tag {value}")));
                }
            };
            Err(EngineError::Trap {
                kind,
                instruction: context.pc as usize,
            })
        }
        #[cfg(not(all(target_arch = "x86_64", any(target_os = "windows", target_os = "linux"))))]
        {
            let _ = (args, fuel);
            Err(unavailable())
        }
    }

    /// Inspect immutable emitted bytes; there is no arbitrary-code invocation API.
    pub fn code_bytes(&self) -> &[u8] {
        #[cfg(all(target_arch = "x86_64", any(target_os = "windows", target_os = "linux")))]
        {
            self.code.bytes()
        }
        #[cfg(not(all(target_arch = "x86_64", any(target_os = "windows", target_os = "linux"))))]
        {
            &[]
        }
    }
}
