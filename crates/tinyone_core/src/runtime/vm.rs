use std::collections::HashMap;
use std::io::Write;
use std::sync::Arc;

mod budget;
mod interchange;
pub use interchange::VmHeapValue;
use budget::{Fuel, InstructionBudget, Unlimited};

use crate::source::bytecode::BytecodeSite;
use crate::{
    HeapData,
    Instr,
    Op,
    Program,
    Result,
    TinyHeapStats,
    TinyMemory,
    TinyOneError,
    TinyRuntimeContext,
    TypeKind,
    Value,
    VerifiedProgram,
    checked_div,
    checked_non_negative_usize,
    pop_args,
    runtime_add,
    runtime_call_builtin,
    runtime_compare,
    runtime_get_field,
    runtime_index,
    runtime_is_false,
    runtime_make_array,
    runtime_make_enum,
    runtime_make_struct,
    runtime_mul,
    runtime_neg,
    runtime_null,
    runtime_print,
    runtime_set_field,
    runtime_set_index,
    runtime_sub,
};

#[derive(Debug, Clone, PartialEq)]
pub struct TinyRunReport {
    pub memory:               TinyMemory,
    pub heap_before_shutdown: TinyHeapStats,
    pub heap_after_shutdown:  TinyHeapStats,
}

pub struct VM {
    pub(crate) program: Arc<Program>,
    memory:             TinyMemory,
    pub(crate) context: TinyRuntimeContext,
    call_depth:         usize,
    call_depth_limit:   usize,
    failure_site:       Option<BytecodeSite>,
}

enum ChunkIdentity<'a> {
    Main,
    Function { name: &'a str, index: usize },
}

fn vm_pop(stack: &mut Vec<Value>) -> Result<Value> {
    stack.pop().ok_or_else(|| TinyOneError::runtime("Stack underflow"))
}

fn vm_pop_pair(stack: &mut Vec<Value>) -> Result<(Value, Value)> {
    let rhs = vm_pop(stack)?;
    let lhs = vm_pop(stack)?;
    Ok((lhs, rhs))
}

fn lookup_field(fields: &[String], index: usize) -> Result<&str> {
    fields
        .get(index)
        .map(String::as_str)
        .ok_or_else(|| TinyOneError::runtime(format!("Invalid field index {index}")))
}

impl VM {
    /// Creates a new VM instance with the given program, memory, and inputs. The program is verified before execution.
    ///
    /// # Errors
    /// Returns a `TinyOneError` if the program fails verification or if there is an error initializing the VM.
    pub fn new(program: Arc<Program>, memory: TinyMemory, inputs: Vec<String>) -> Result<Self> {
        let verified = VerifiedProgram::verify_arc(program)?;
        Ok(Self::new_unchecked(&verified, memory, inputs))
    }

    /// Create a VM with limits that tighten the program and runtime policies.
    ///
    /// # Errors
    /// Returns verification, initialization, or limit errors.
    pub fn new_with_limits(
        program: Arc<Program>, memory: TinyMemory, inputs: Vec<String>, limits: crate::VmExecutionLimits,
    ) -> Result<Self> {
        let mut vm = Self::new(program, memory, inputs)?;
        vm.context.heap().set_execution_limits(limits)?;
        vm.call_depth_limit = vm.call_depth_limit.min(limits.call_depth);
        Ok(vm)
    }

    /// Create a persistent VM from an already verified program without
    /// repeating verification. Allocates global memory, but executes no code.
    ///
    /// # Errors
    /// Returns allocation or execution-policy initialization errors.
    pub fn from_verified_with_limits(
        program: &VerifiedProgram, inputs: Vec<String>, limits: crate::VmExecutionLimits,
    ) -> Result<Self> {
        let memory = TinyMemory::try_new(program.program().slot_count())?;
        let mut vm = Self::new_unchecked(program, memory, inputs);
        vm.context.heap().set_execution_limits(limits)?;
        vm.call_depth_limit = vm.call_depth_limit.min(limits.call_depth);
        Ok(vm)
    }

    /// Construct a VM without re-verifying from a verification token.
    pub(crate) fn new_unchecked(verified: &VerifiedProgram, memory: TinyMemory, inputs: Vec<String>) -> Self {
        let mut context = TinyRuntimeContext::new(inputs);
        context.program_arc = Some(verified.program_arc());
        context.verified_program = Some(verified.clone());
        Self {
            program: verified.program_arc(),
            memory,
            context,
            call_depth: 0,
            call_depth_limit: verified.program().max_call_depth(),
            failure_site: None,
        }
    }

    pub(crate) fn new_unchecked_with_context(
        verified: &VerifiedProgram,
        memory: TinyMemory,
        context: TinyRuntimeContext,
    ) -> Self {
        let mut context = context;
        context.program_arc = Some(verified.program_arc());
        context.verified_program = Some(verified.clone());
        Self {
            program: verified.program_arc(),
            memory,
            context,
            call_depth: 0,
            call_depth_limit: verified.program().max_call_depth(),
            failure_site: None,
        }
    }

    pub fn set_sys_args(&mut self, args: Vec<String>) {
        self.context.set_sys_args(args);
    }

    pub fn set_sys_env(&mut self, env: HashMap<String, String>) {
        self.context.set_sys_env(env);
    }

    /// Runs the program and returns the final memory state after execution.
    ///
    /// # Errors
    /// Returns a `TinyOneError` if the program encounters a runtime error during execution.
    pub fn run(self, stdout: &mut dyn Write) -> Result<TinyMemory> {
        Ok(self.run_report(stdout)?.memory)
    }

    /// Runs the program and returns a report containing the final memory state and heap statistics.
    ///
    /// # Errors
    /// Returns a `TinyOneError` if the program encounters a runtime error during execution.
    pub fn run_report(self, stdout: &mut dyn Write) -> Result<TinyRunReport> {
        self.run_report_with_budget(stdout, Unlimited)
    }

    /// Runs with one unit of fuel consumed before each bytecode instruction,
    /// including instructions in nested calls and the final halt/return.
    /// Zero fuel stops before executing any instruction. This bounds bytecode
    /// dispatch, not time or work performed inside a native builtin.
    ///
    /// # Errors
    /// Returns a runtime error on fuel exhaustion or any execution failure.
    pub fn run_report_with_fuel(self, stdout: &mut dyn Write, fuel: u64) -> Result<TinyRunReport> {
        self.run_report_with_budget(stdout, Fuel::new(fuel))
    }

    /// Inspect the session's global memory without transferring ownership.
    pub fn memory(&self) -> &TinyMemory {
        &self.memory
    }

    /// Execute main from its first instruction while retaining globals and heap.
    /// Completed side effects remain after a runtime error; the next call starts
    /// a new execution with a fresh fuel budget. This does not resume a trapped PC.
    ///
    /// # Errors
    /// Returns runtime errors, including fuel exhaustion. After a host panic,
    /// discard the VM rather than invoking it again.
    pub fn execute_main_with_fuel(&mut self, stdout: &mut dyn Write, fuel: u64) -> Result<()> {
        self.failure_site = None;
        let mut memory = std::mem::take(&mut self.memory);
        let code = self.program.code.clone();
        let result = self.run_chunk(&code, &mut memory, stdout, ChunkIdentity::Main, None, &mut Fuel::new(fuel));
        self.memory = memory;
        result.map(|_| ()).map_err(|error| self.annotate_error(error))
    }

    /// Invoke a function while retaining this VM's globals and heap. Heap-valued
    /// arguments/results remain local to this VM and must not outlive it.
    /// Function indices are internal program indices, not foreign export IDs.
    ///
    /// # Errors
    /// Returns an error for invalid indices/arity, fuel exhaustion or runtime
    /// failure. Completed side effects remain; discard the VM after a host panic.
    pub fn invoke_function_with_fuel(
        &mut self,
        function_index: usize,
        args: Vec<Value>,
        stdout: &mut dyn Write,
        fuel: u64,
    ) -> Result<Value> {
        self.failure_site = None;
        let globals = std::mem::take(&mut self.memory);
        let result = self.call_function_with_args(function_index, args, stdout, &globals, &mut Fuel::new(fuel));
        self.memory = globals;
        result.map_err(|error| self.annotate_error(error))
    }

    fn run_report_with_budget<B: InstructionBudget>(mut self, stdout: &mut dyn Write, mut budget: B) -> Result<TinyRunReport> {
        let mut memory = std::mem::take(&mut self.memory);
        let code = self.program.code.clone();
        self.run_chunk(&code, &mut memory, stdout, ChunkIdentity::Main, None, &mut budget)
            .map_err(|error| self.annotate_error(error))?;
        let heap_before_shutdown = self.context.heap_stats();
        let heap_after_shutdown = self.context.shutdown();
        Ok(TinyRunReport {
            memory,
            heap_before_shutdown,
            heap_after_shutdown,
        })
    }

    /// Run a single function by index. Used by `thread_spawn`.
    ///
    /// # Errors
    /// Returns a `TinyOneError` if the function index is invalid or if the function encounters a runtime error during execution.
    pub(crate) fn run_function_by_index(
        mut self,
        fn_index: usize,
        args: Vec<Value>,
        stdout: &mut dyn Write,
    ) -> Result<Value> {
        let function = self
            .program
            .functions
            .get(fn_index)
            .ok_or_else(|| TinyOneError::runtime(format!("thread_spawn: invalid function index {fn_index}")))?;
        let slot_count = function.slot_count;
        let fn_name = function.name.clone();
        let code = function.code.clone();
        let mut memory = TinyMemory::try_new(slot_count)?;
        for (i, v) in args.into_iter().enumerate() {
            memory.store(i, v)?;
        }
        let global_memory = std::mem::take(&mut self.memory);
        let result = self
            .run_chunk(&code, &mut memory, stdout, ChunkIdentity::Function { name: &fn_name, index: fn_index }, Some(&global_memory), &mut Unlimited)
            .map_err(|error| self.annotate_error(error))?;
        result.ok_or_else(|| TinyOneError::runtime("thread function returned no value"))
    }

    fn annotate_error(&self, error: TinyOneError) -> TinyOneError {
        match self.failure_site {
            Some(site) => site.annotate(&self.program, error),
            None => error,
        }
    }

    fn run_chunk<B: InstructionBudget>(
        &mut self,
        code: &[Instr],
        memory: &mut TinyMemory,
        stdout: &mut dyn Write,
        identity: ChunkIdentity<'_>,
        global_memory: Option<&TinyMemory>,
        budget: &mut B,
    ) -> Result<Option<Value>> {
        let (chunk_name, function_index) = match identity {
            ChunkIdentity::Main => ("main", None),
            ChunkIdentity::Function { name, index } => (name, Some(index)),
        };
        let permissions = self.program.capabilities_for_function(function_index);
        let mut stack: Vec<Value> = Vec::with_capacity(code.len().min(32));
        let mut pc = 0usize;
        let mut instruction = 0usize;
        let result = (|| {
            loop {
                instruction = pc;
                budget.consume()?;
                let instr = code
                    .get(pc)
                    .copied()
                    .ok_or_else(|| TinyOneError::runtime(format!("Invalid program counter in {chunk_name}")))?;
                pc += 1;
                match instr.op {
                    Op::PushInt => stack.push(Value::I64(instr.arg)),
                    Op::PushNull => stack.push(runtime_null()),
                    Op::PushBool => stack.push(Value::Bool(instr.arg != 0)),
                    Op::PushFloat => {
                        stack.push(Value::Float {
                            kind: TypeKind::Fp64,
                            bits: f64::from_bits(instr.arg as u64),
                        });
                    }
                    Op::PushFunction => {
                        let function_index = checked_non_negative_usize(instr.arg, "function index")?;
                        if function_index >= self.program.functions.len() {
                            return Err(TinyOneError::runtime(format!("Invalid function index {function_index}")));
                        }
                        stack.push(Value::Function(function_index as u32));
                    }
                    Op::Pop => {
                        vm_pop(&mut stack)?;
                    }
                    Op::PushString => {
                        let string_index = checked_non_negative_usize(instr.arg, "string index")?;
                        let text = self
                            .program
                            .strings
                            .get(string_index)
                            .ok_or_else(|| TinyOneError::runtime(format!("Invalid string index {string_index}")))?
                            .clone();
                        stack.push(Value::Heap(self.context.heap().alloc_string(text)?));
                    }
                    Op::Load => {
                        let slot = checked_non_negative_usize(instr.arg, "memory slot")?;
                        stack.push(memory.load(slot)?);
                    }
                    Op::LoadGlobal => {
                        let slot = checked_non_negative_usize(instr.arg, "global memory slot")?;
                        let globals = global_memory
                            .ok_or_else(|| TinyOneError::runtime("Global load outside a function frame"))?;
                        stack.push(globals.load(slot)?);
                    }
                    Op::Store => {
                        let slot = checked_non_negative_usize(instr.arg, "memory slot")?;
                        memory.store(slot, vm_pop(&mut stack)?)?;
                    }
                    Op::Add => {
                        let (lhs, rhs) = vm_pop_pair(&mut stack)?;
                        stack.push(runtime_add(lhs, rhs)?);
                    }
                    Op::Sub => {
                        let (lhs, rhs) = vm_pop_pair(&mut stack)?;
                        stack.push(runtime_sub(lhs, rhs)?);
                    }
                    Op::Mul => {
                        let (lhs, rhs) = vm_pop_pair(&mut stack)?;
                        stack.push(runtime_mul(lhs, rhs)?);
                    }
                    Op::Div => {
                        let (lhs, rhs) = vm_pop_pair(&mut stack)?;
                        stack.push(checked_div(lhs, rhs)?);
                    }
                    Op::Neg => {
                        let value = vm_pop(&mut stack)?;
                        stack.push(runtime_neg(value)?);
                    }
                    Op::Lt | Op::Lte | Op::Gt | Op::Gte | Op::Eq | Op::Ne => {
                        let (lhs, rhs) = vm_pop_pair(&mut stack)?;
                        stack.push(runtime_compare(instr.op, lhs, rhs)?);
                    }
                    Op::Jump => pc = checked_non_negative_usize(instr.arg, "jump target")?,
                    Op::JumpIfZero => {
                        if runtime_is_false(&vm_pop(&mut stack)?) {
                            pc = checked_non_negative_usize(instr.arg, "jump target")?;
                        }
                    }
                    Op::Call => {
                        let function_index = checked_non_negative_usize(instr.arg, "function index")?;
                        let arg_count = checked_non_negative_usize(instr.arg2, "function arity")?;
                        let globals = global_memory.unwrap_or(&*memory);
                        let result = self.call_function(function_index, &mut stack, arg_count, stdout, globals, budget)?;
                        stack.push(result);
                    }
                    Op::CallValue => {
                        let arg_count = checked_non_negative_usize(instr.arg, "call arity")?;
                        let args = pop_args(&mut stack, arg_count)?;
                        let callable = vm_pop(&mut stack)?;
                        let (function_index, call_args) = match callable {
                            Value::Function(index) => (index as usize, args),
                            Value::Heap(reference) => {
                                let captures = {
                                    let heap = self.context.heap();
                                    let object = heap.get(&Value::Heap(reference))?;
                                    let HeapData::Closure { function_id, captures } = &object.data else {
                                        return Err(TinyOneError::runtime("CallValue expects a function or Closure"));
                                    };
                                    (*function_id as usize, crate::runtime::heap::decode_array_values(captures))
                                };
                                let (function_index, mut captures) = captures;
                                captures.extend(args);
                                (function_index, captures)
                            }
                            _ => {
                                return Err(TinyOneError::runtime("CallValue expects a function or Closure"));
                            }
                        };
                        let globals = global_memory.unwrap_or(&*memory);
                        stack.push(self.call_function_with_args(function_index, call_args, stdout, globals, budget)?);
                    }
                    Op::MakeArray => {
                        let count = checked_non_negative_usize(instr.arg, "array arity")?;
                        let values = pop_args(&mut stack, count)?;
                        stack.push(runtime_make_array(&mut self.context, values)?);
                    }
                    Op::Index => {
                        let (container, index) = vm_pop_pair(&mut stack)?;
                        stack.push(runtime_index(&mut self.context, container, index)?);
                    }
                    Op::SetIndex => {
                        let value = vm_pop(&mut stack)?;
                        let index = vm_pop(&mut stack)?;
                        let container = vm_pop(&mut stack)?;
                        runtime_set_index(&mut self.context, container, index, value)?;
                    }
                    Op::MakeStruct => {
                        let field_count = checked_non_negative_usize(instr.arg2, "struct arity")?;
                        let values = pop_args(&mut stack, field_count)?;
                        let struct_index = checked_non_negative_usize(instr.arg, "struct index")?;
                        let struct_def = self
                            .program
                            .structs
                            .get(struct_index)
                            .ok_or_else(|| TinyOneError::runtime(format!("Invalid struct index {struct_index}")))?;
                        stack.push(runtime_make_struct(
                            &mut self.context,
                            &struct_def.name,
                            &struct_def.fields,
                            values,
                        )?);
                    }
                    Op::GetField => {
                        let target = vm_pop(&mut stack)?;
                        let field_index = checked_non_negative_usize(instr.arg, "field index")?;
                        let field = lookup_field(&self.program.fields, field_index)?;
                        stack.push(runtime_get_field(&self.context, target, field)?);
                    }
                    Op::SetField => {
                        let value = vm_pop(&mut stack)?;
                        let target = vm_pop(&mut stack)?;
                        let field_index = checked_non_negative_usize(instr.arg, "field index")?;
                        let field = lookup_field(&self.program.fields, field_index)?;
                        runtime_set_field(&mut self.context, target, field, value)?;
                    }
                    Op::MakeEnum => {
                        let field_count = checked_non_negative_usize(instr.arg2, "enum variant arity")?;
                        let values = pop_args(&mut stack, field_count)?;
                        let variant_id = checked_non_negative_usize(instr.arg, "enum variant index")?;
                        let variant_def =
                            self.program.enum_variants.get(variant_id).ok_or_else(|| {
                                TinyOneError::runtime(format!("Invalid enum variant index {variant_id}"))
                            })?;
                        stack.push(runtime_make_enum(
                            &mut self.context,
                            &variant_def.enum_name,
                            &variant_def.variant_name,
                            variant_def.tag,
                            &variant_def.fields,
                            values,
                        )?);
                    }
                    Op::Builtin => {
                        let builtin_index = checked_non_negative_usize(instr.arg, "builtin index")?;
                        let arg_count = checked_non_negative_usize(instr.arg2, "builtin arity")?;
                        let args_start = stack
                            .len()
                            .checked_sub(arg_count)
                            .ok_or_else(|| TinyOneError::runtime("Stack underflow"))?;
                        let globals = global_memory.unwrap_or(&*memory);
                        let result = runtime_call_builtin(
                            &mut self.context,
                            globals,
                            builtin_index,
                            function_index,
                            &permissions,
                            &stack[args_start..],
                        )?;
                        stack.truncate(args_start);
                        stack.push(result);
                    }
                    Op::Return => return Ok(Some(vm_pop(&mut stack)?)),
                    Op::Print => {
                        if !self.context.queued_stdout.is_empty() {
                            stdout
                                .write_all(&self.context.queued_stdout)
                                .map_err(|e| TinyOneError::runtime(format!("stdout flush error: {e}")))?;
                            self.context.queued_stdout.clear();
                        }
                        let value = vm_pop(&mut stack)?;
                        runtime_print(&self.context, stdout, &value)?;
                    }
                    Op::Halt => {
                        if !self.context.queued_stdout.is_empty() {
                            stdout
                                .write_all(&self.context.queued_stdout)
                                .map_err(|e| TinyOneError::runtime(format!("stdout flush error: {e}")))?;
                            self.context.queued_stdout.clear();
                        }
                        if !stack.is_empty() {
                            return Err(TinyOneError::runtime(format!(
                                "Internal stack imbalance at halt in {chunk_name}"
                            )));
                        }
                        return Ok(None);
                    }
                }
            }
        })();
        if result.is_err() && self.failure_site.is_none() {
            self.failure_site = Some(BytecodeSite::new(function_index, instruction));
        }
        result
    }

    fn call_function<B: InstructionBudget>(
        &mut self,
        function_index: usize,
        caller_stack: &mut Vec<Value>,
        arg_count: usize,
        stdout: &mut dyn Write,
        global_memory: &TinyMemory,
        budget: &mut B,
    ) -> Result<Value> {
        let args = pop_args(caller_stack, arg_count)?;
        self.call_function_with_args(function_index, args, stdout, global_memory, budget)
    }

    fn call_function_with_args<B: InstructionBudget>(
        &mut self,
        function_index: usize,
        args: Vec<Value>,
        stdout: &mut dyn Write,
        global_memory: &TinyMemory,
        budget: &mut B,
    ) -> Result<Value> {
        let (fn_name, fn_slot_count, fn_code, fn_param_count) = {
            let function = self
                .program
                .functions
                .get(function_index)
                .ok_or_else(|| TinyOneError::runtime(format!("Invalid function index {function_index}")))?;
            (function.name.clone(), function.slot_count, function.code.clone(), function.param_count)
        };
        if args.len() != fn_param_count {
            return Err(TinyOneError::runtime(format!(
                "Function {:?} expects {} argument(s), got {}",
                fn_name,
                fn_param_count,
                args.len()
            )));
        }
        let max_call_depth = self.call_depth_limit;
        if self.call_depth >= max_call_depth {
            return Err(TinyOneError::runtime(format!("Call stack overflow after {max_call_depth} nested call(s)")));
        }
        let mut memory = TinyMemory::try_new(fn_slot_count)?;
        for (slot, value) in args.into_iter().enumerate() {
            memory.store(slot, value)?;
        }
        self.call_depth += 1;
        let result = self.run_chunk(&fn_code, &mut memory, stdout, ChunkIdentity::Function { name: &fn_name, index: function_index }, Some(global_memory), budget);
        self.call_depth -= 1;
        result?.ok_or_else(|| TinyOneError::runtime(format!("Function {fn_name:?} returned no value")))
    }
}
