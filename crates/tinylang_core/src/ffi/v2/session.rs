use super::schema::VmCreate;
use super::status::{TINY_STATUS_INVALID_ARGUMENT, TINY_STATUS_OUT_OF_MEMORY, TinyStatus};
use crate::{VM, VerifiedProgram, VmExecutionLimits};

// These mirror the current v1 runtime safety caps. They are deliberately
// enforced at the v2 boundary so a session can never promise limits the
// underlying VM cannot honor.
pub(crate) const MAX_VM_HEAP_BYTES: u64 = 4 * 1024 * 1024;
pub(crate) const MAX_VM_HEAP_OBJECTS: u32 = 1_000_000;
pub(crate) const MAX_VM_CALL_DEPTH: u32 = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct VmLimits {
    pub max_heap_bytes:   u64,
    pub max_heap_objects: u32,
    pub max_call_depth:   u32,
    pub allow_jit:        bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ExportedFunction {
    pub function_index:  usize,
    pub parameter_count: usize,
}

#[allow(dead_code)]
pub(crate) struct VmSession {
    pub execution:      VM,
    pub limits:         VmLimits,
    exported_functions: Vec<Option<ExportedFunction>>,
}

impl VmSession {
    pub fn new(program: VerifiedProgram, request: VmCreate) -> Result<Self, TinyStatus> {
        if request.max_heap_bytes > MAX_VM_HEAP_BYTES
            || request.max_heap_objects > MAX_VM_HEAP_OBJECTS
            || request.max_call_depth > MAX_VM_CALL_DEPTH
        {
            return Err(TINY_STATUS_INVALID_ARGUMENT);
        }
        let limits = VmExecutionLimits::new(
            usize::try_from(request.max_heap_bytes).map_err(|_| TINY_STATUS_INVALID_ARGUMENT)?,
            request.max_heap_objects as usize,
            request.max_call_depth as usize,
        )
        .map_err(|_| TINY_STATUS_INVALID_ARGUMENT)?;
        let execution =
            VM::from_verified_with_limits(&program, vec![], limits).map_err(|_| TINY_STATUS_OUT_OF_MEMORY)?;
        let exported_functions = program
            .program()
            .functions()
            .iter()
            .enumerate()
            .map(|(function_index, function)| {
                is_host_exported(&program, function.name()).then_some(ExportedFunction {
                    function_index,
                    parameter_count: function.param_count(),
                })
            })
            .collect();
        Ok(Self {
            execution,
            limits: VmLimits {
                max_heap_bytes:   request.max_heap_bytes,
                max_heap_objects: request.max_heap_objects,
                max_call_depth:   request.max_call_depth,
                allow_jit:        request.vm_flags & super::schema::VM_FLAG_ALLOW_JIT != 0,
            },
            exported_functions,
        })
    }

    pub fn exported_function(&self, export_id: u32) -> Option<ExportedFunction> {
        self.exported_functions
            .get(usize::try_from(export_id).ok()?)
            .copied()
            .flatten()
    }
}

fn is_host_exported(program: &VerifiedProgram, function_name: &str) -> bool {
    let Some((module_name, local_name)) = function_name.split_once('.') else {
        return true;
    };
    if local_name.contains('.') {
        return false;
    }
    program
        .program()
        .modules()
        .iter()
        .find(|module| module.name() == module_name)
        .is_some_and(|module| module.exported_functions().iter().any(|export| export == local_name))
}
