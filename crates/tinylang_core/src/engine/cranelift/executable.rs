use cranelift_codegen::ir::types;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{Linkage, Module, ModuleError, default_libcall_names};

use super::lowering;
use super::memory::MappedMemory;
use crate::engine::{EngineError, ScalarFunction, TrapKind};

type Entry = unsafe extern "C" fn(*mut i64, *mut i64, *mut i64, *mut u64, *mut u64) -> u32;

// The guard owns code even during failed compilation. No entry pointer escapes it.
struct ModuleMemory(Option<JITModule>);

impl Drop for ModuleMemory {
    fn drop(&mut self) {
        if let Some(module) = self.0.take() {
            // SAFETY: Only CraneliftFunction::invoke can call this module's code,
            // borrowing its owner. Exclusive destruction cannot overlap such a call.
            unsafe { module.free_memory() };
        }
    }
}

/// An executable function and its retained, immutable code mapping.
///
/// Invocation allocates a fresh zeroed frame, checks argument arity, and runs
/// generated machine code. It never calls the scalar interpreter or the VM.
pub struct CraneliftFunction {
    function: ScalarFunction,
    entry:    Entry,
    _memory:  ModuleMemory,
}

impl CraneliftFunction {
    pub fn compile(function: &ScalarFunction) -> Result<Self, EngineError> {
        if !cfg!(target_arch = "x86_64") {
            return Err(EngineError::BackendUnavailable {
                backend: "cranelift",
                reason:  "owned executable mappings currently support only x86-64".to_owned(),
            });
        }
        let mut flags = settings::builder();
        for (flag, value) in [
            ("is_pic", "false"),
            ("use_colocated_libcalls", "false"),
            ("enable_probestack", "false"),
        ] {
            flags
                .set(flag, value)
                .map_err(|error| EngineError::Codegen(error.to_string()))?;
        }
        let isa = cranelift_native::builder()
            .map_err(|reason| {
                EngineError::BackendUnavailable {
                    backend: "cranelift",
                    reason:  reason.to_owned(),
                }
            })?
            .finish(settings::Flags::new(flags))
            .map_err(|error| {
                EngineError::BackendUnavailable {
                    backend: "cranelift",
                    reason:  error.to_string(),
                }
            })?;
        if isa.pointer_type() != types::I64 {
            return Err(EngineError::BackendUnavailable {
                backend: "cranelift",
                reason:  "the generated frame ABI requires 64-bit pointers".to_owned(),
            });
        }
        let mut builder = JITBuilder::with_isa(isa, default_libcall_names());
        builder.memory_provider(Box::<MappedMemory>::default());
        let mut memory = ModuleMemory(Some(JITModule::new(builder)));
        let module = memory
            .0
            .as_mut()
            .ok_or_else(|| EngineError::Codegen("missing live JIT module".to_owned()))?;
        let mut context = module.make_context();
        lowering::signature(&mut context.func, module);
        let id = module
            .declare_function("tinylang_scalar", Linkage::Local, &context.func.signature)
            .map_err(module_error)?;
        lowering::emit(function, &mut context.func)?;
        module.define_function(id, &mut context).map_err(module_error)?;
        module.finalize_definitions().map_err(module_error)?;
        let code = module.get_finalized_function(id);
        // SAFETY: The definition has the host C ABI and exactly Entry's signature.
        // Finalization made its pages RX. `memory` retains them until self drops.
        let entry = unsafe { std::mem::transmute::<*const u8, Entry>(code) };
        Ok(Self {
            function: function.clone(),
            entry,
            _memory: memory,
        })
    }

    pub fn invoke(&self, args: &[i64], fuel: u64) -> Result<i64, EngineError> {
        let (mut slots, mut stack) = self.function.frame(args)?;
        let mut output = 0i64;
        let mut remaining = fuel;
        let mut instruction = u64::MAX;
        // SAFETY: The lowering capability guarantees all accessed frame offsets
        // fit these initialized buffers. Output/fuel/location are distinct live
        // values. Code is retained by self, and no generated pointers escape.
        let status = unsafe {
            (self.entry)(slots.as_mut_ptr(), stack.as_mut_ptr(), &mut output, &mut remaining, &mut instruction)
        };
        let kind = match status {
            0 => return Ok(output),
            1 => TrapKind::Overflow,
            2 => TrapKind::DivisionByZero,
            3 => TrapKind::FuelExhausted,
            other => {
                return Err(EngineError::Codegen(format!("invalid JIT trap status {other}")));
            }
        };
        let instruction =
            usize::try_from(instruction).map_err(|_| EngineError::Codegen("invalid JIT trap location".to_owned()))?;
        Err(EngineError::Trap { kind, instruction })
    }
}

fn module_error(error: ModuleError) -> EngineError {
    match error {
        ModuleError::Allocation { message, err } => EngineError::Allocation(format!("{message}: {err}")),
        error => EngineError::Codegen(error.to_string()),
    }
}

#[cfg(all(test, windows, target_arch = "x86_64"))]
mod tests {
    use std::ffi::c_void;

    use super::CraneliftFunction;

    #[repr(C)]
    #[derive(Default)]
    struct MemoryInformation {
        base:               *mut c_void,
        allocation_base:    *mut c_void,
        allocation_protect: u32,
        partition_id:       u16,
        region_size:        usize,
        state:              u32,
        protect:            u32,
        kind:               u32,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn VirtualQuery(address: *const c_void, information: *mut MemoryInformation, length: usize) -> usize;
    }

    fn query(address: *const c_void) -> MemoryInformation {
        let mut information = MemoryInformation::default();
        // SAFETY: VirtualQuery queries an address without dereferencing it; the
        // correctly laid-out output buffer is live and large enough for the API.
        assert_eq!(
            unsafe { VirtualQuery(address, &mut information, std::mem::size_of::<MemoryInformation>(),) },
            std::mem::size_of::<MemoryInformation>()
        );
        information
    }

    #[test]
    fn actual_cranelift_code_is_rw_then_rx_and_is_unmapped_on_drop() {
        let scalar = crate::engine::compile_scalar_source("fn test() { return 42 }", "test").unwrap();
        let function = CraneliftFunction::compile(&scalar).unwrap();
        let address = function.entry as *const c_void;
        let mapped = query(address);
        assert_eq!(mapped.state, 0x1000); // MEM_COMMIT
        assert_eq!(mapped.allocation_protect, 0x04); // Original PAGE_READWRITE
        assert_eq!(mapped.protect, 0x20); // Current PAGE_EXECUTE_READ
        assert_eq!(function.invoke(&[], 100).unwrap(), 42);
        drop(function);
        assert_eq!(query(address).state, 0x10000); // MEM_FREE, not leaked or reserved
    }
}
