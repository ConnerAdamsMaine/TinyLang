use std::collections::HashMap;
use std::io::Write;

use super::call_feedback::CallSite;
use super::feedback::ObservedTypes;
use super::frame::{CallFrame, CallRequest, FrameCursor, FrameExit, FrameMemoryPool};
use super::op::SlotIntMode;
use super::origins::FailureStage;
use crate::source::bytecode::BytecodeSite;
use crate::{
    HeapData, JitBuiltin, JitOp, JitProgram, Result, TinyLangError, TinyMemory, TinyRunReport, TinyRuntimeContext,
    TypeKind, Value, VerifiedProgram, checked_div, checked_div_int, pop_args, require_builtin_capability, runtime_add,
    runtime_add_int, runtime_array_pop, runtime_array_push, runtime_call_builtin, runtime_compare, runtime_compare_int,
    runtime_get_field, runtime_index, runtime_is_false, runtime_make_array, runtime_make_enum, runtime_make_struct,
    runtime_mul, runtime_mul_int, runtime_neg, runtime_null, runtime_print, runtime_set_field, runtime_set_index,
    runtime_sub, runtime_sub_int,
};

fn jit_pop(stack: &mut Vec<Value>) -> Result<Value> {
    stack.pop().ok_or_else(|| TinyLangError::runtime("Stack underflow"))
}

fn jit_pop_pair(stack: &mut Vec<Value>) -> Result<(Value, Value)> {
    let rhs = jit_pop(stack)?;
    let lhs = jit_pop(stack)?;
    Ok((lhs, rhs))
}

fn run_direct_builtin(
    context: &mut TinyRuntimeContext,
    stack: &mut Vec<Value>,
    builtin: JitBuiltin,
    permissions: &crate::ModulePermissions,
) -> Result<()> {
    let result = match builtin {
        JitBuiltin::Len => {
            let target = jit_pop(stack)?;
            crate::execution::runtime::builtins::runtime_len(context, &target)?
        }
        JitBuiltin::ArrayPush => {
            let value = jit_pop(stack)?;
            let target = jit_pop(stack)?;
            runtime_array_push(context, &target, value)?
        }
        JitBuiltin::ArrayPop => {
            let target = jit_pop(stack)?;
            runtime_array_pop(context, &target)?
        }
        JitBuiltin::VecNew => crate::execution::runtime::stdlib::b_vec_new(context)?,
        JitBuiltin::VecClear => {
            let target = jit_pop(stack)?;
            crate::execution::runtime::stdlib::b_vec_clear(context, &target)?
        }
        JitBuiltin::MapNew => crate::execution::runtime::stdlib::b_map_new(context)?,
        JitBuiltin::MapSet => {
            let value = jit_pop(stack)?;
            let key = jit_pop(stack)?;
            let target = jit_pop(stack)?;
            match key {
                Value::I64(key) => crate::execution::runtime::stdlib::b_map_set_i64(context, &target, key, value)?,
                key => crate::execution::runtime::stdlib::b_map_set(context, &target, key, value)?,
            }
        }
        JitBuiltin::MapGet => {
            let key = jit_pop(stack)?;
            let target = jit_pop(stack)?;
            match key {
                Value::I64(key) => crate::execution::runtime::stdlib::b_map_get_i64(context, &target, key)?,
                key => crate::execution::runtime::stdlib::b_map_get(context, &target, &key)?,
            }
        }
        JitBuiltin::MapHas => {
            let key = jit_pop(stack)?;
            let target = jit_pop(stack)?;
            crate::execution::runtime::stdlib::b_map_has(context, &target, &key)?
        }
        JitBuiltin::MapDel => {
            let key = jit_pop(stack)?;
            let target = jit_pop(stack)?;
            crate::execution::runtime::stdlib::b_map_del(context, &target, &key)?
        }
        JitBuiltin::Alloc => {
            let value = jit_pop(stack)?;
            Value::Heap(context.heap().alloc_cell(value)?)
        }
        JitBuiltin::Load => {
            let target = jit_pop(stack)?;
            let heap = context.heap();
            let object = heap.get(&target)?;
            let HeapData::Cell(bytes) = &object.data else {
                return Err(TinyLangError::runtime("load() expects a pointer cell"));
            };
            crate::execution::runtime::value_codec::decode_value(bytes.as_slice().try_into().unwrap())
        }
        JitBuiltin::Store => {
            let value = jit_pop(stack)?;
            let target = jit_pop(stack)?;
            let mut heap = context.heap();
            let object = heap.get_mut(&target)?;
            let HeapData::Cell(bytes) = &mut object.data else {
                return Err(TinyLangError::runtime("store() expects a pointer cell"));
            };
            let encoded = crate::execution::runtime::value_codec::encode_value(&value)?;
            bytes.as_mut_slice().copy_from_slice(&encoded);
            value
        }
        JitBuiltin::Free => {
            require_builtin_capability("free", true, permissions)?;
            let target = jit_pop(stack)?;
            context.heap().free(&target)?;
            Value::I64(0)
        }
    };
    stack.push(result);
    Ok(())
}

pub(crate) struct JitVm<'a> {
    program: &'a mut JitProgram,
    context: TinyRuntimeContext,
    verified_program: VerifiedProgram,
    frame_memory: FrameMemoryPool,
}

impl<'a> JitVm<'a> {
    pub(crate) fn new(program: &'a mut JitProgram, inputs: Vec<String>) -> Self {
        // The JIT retains the verification token for its entire lifetime;
        // obtain the execution metadata from that token rather than from an
        // independently mutable program handle.
        let source_program = program.verified_program.program_arc();
        let mut context = TinyRuntimeContext::new(inputs);
        context.program_arc = Some(source_program);
        context.verified_program = Some(program.verified_program.clone());
        let verified_program = program.verified_program.clone();
        Self {
            program,
            context,
            verified_program,
            frame_memory: FrameMemoryPool::default(),
        }
    }

    pub(crate) fn set_sys_args(&mut self, args: Vec<String>) {
        self.context.set_sys_args(args);
    }

    pub(crate) fn set_sys_env(&mut self, env: HashMap<String, String>) {
        self.context.set_sys_env(env);
    }

    pub(crate) fn run(self, stdout: &mut dyn Write) -> Result<TinyMemory> {
        Ok(self.run_report(stdout)?.memory)
    }

    pub(crate) fn run_report(mut self, stdout: &mut dyn Write) -> Result<TinyRunReport> {
        let slot_count = self
            .program
            .chunks
            .first()
            .and_then(Option::as_ref)
            .ok_or_else(|| TinyLangError::runtime("JIT program has no main chunk"))?
            .slot_count;
        let mut memory = TinyMemory::try_new(slot_count)?;
        self.run_frames(&mut memory, stdout)?;
        let heap_before_shutdown = self.context.heap_stats();
        let heap_after_shutdown = self.context.shutdown();
        Ok(TinyRunReport {
            memory,
            heap_before_shutdown,
            heap_after_shutdown,
        })
    }

    fn new_cursor(&mut self, chunk_index: usize) -> Result<FrameCursor> {
        self.program.ensure_chunk(chunk_index)?;
        let capacity = self.verified_program.chunk_certificate(chunk_index)?.max_stack_depth;
        Ok(FrameCursor {
            chunk_index,
            pc: 0,
            stack: self.program.take_operand_stack(capacity),
        })
    }

    fn run_frames(&mut self, memory: &mut TinyMemory, stdout: &mut dyn Write) -> Result<()> {
        let mut root = self.new_cursor(0)?;
        // start_call enforces the verified logical depth bound before push.
        // A Vec keeps no-call runs allocation-free and owns only live frames;
        // a fixed boxed array would reserve the maximum and track empty slots.
        let mut calls = Vec::new();
        let result = self.dispatch_frames(memory, stdout, &mut root, &mut calls);
        self.program.recycle_operand_stack(root.stack);
        for call in calls {
            self.program.recycle_operand_stack(call.cursor.stack);
        }
        self.frame_memory.clear();
        result
    }

    fn dispatch_frames(
        &mut self,
        globals: &mut TinyMemory,
        stdout: &mut dyn Write,
        root: &mut FrameCursor,
        calls: &mut Vec<CallFrame>,
    ) -> Result<()> {
        loop {
            let exit = if let Some(call) = calls.last_mut() {
                self.run_frame(&mut call.cursor, &mut call.memory, stdout, Some(globals))?
            } else {
                self.run_frame(root, globals, stdout, None)?
            };
            match exit {
                FrameExit::Call(request) => {
                    let call_depth = calls.len();
                    let caller = calls.last_mut().map_or(&mut *root, |call| &mut call.cursor);
                    let call = self.start_call(request, caller, call_depth).map_err(|error| {
                        self.annotate_error(
                            caller.chunk_index,
                            caller.pc.saturating_sub(1),
                            FailureStage::Operation,
                            error,
                        )
                    })?;
                    calls.push(call);
                    self.program.stats.peak_call_depth = self.program.stats.peak_call_depth.max(calls.len());
                }
                FrameExit::Return(value) => {
                    let Some(call) = calls.pop() else {
                        return Ok(());
                    };
                    let value = value.ok_or_else(|| {
                        let caller = calls.last().map_or(&*root, |call| &call.cursor);
                        let error = TinyLangError::runtime(format!(
                            "Function {:?} returned no value",
                            self.function_name(call.cursor.chunk_index - 1),
                        ));
                        self.annotate_error(
                            caller.chunk_index,
                            caller.pc.saturating_sub(1),
                            FailureStage::Operation,
                            error,
                        )
                    })?;
                    self.program.recycle_operand_stack(call.cursor.stack);
                    self.frame_memory
                        .recycle(call.memory, call.slot_count, &mut self.program.stats);
                    let caller = calls.last_mut().map_or(&mut *root, |call| &mut call.cursor);
                    caller.stack.push(value);
                }
            }
        }
    }

    fn run_frame(
        &mut self,
        cursor: &mut FrameCursor,
        memory: &mut TinyMemory,
        stdout: &mut dyn Write,
        global_memory: Option<&TinyMemory>,
    ) -> Result<FrameExit> {
        let chunk_index = cursor.chunk_index;
        // Immutable authority is borrowed independently of mutable lowering
        // and heap state. Suspending/resuming a frame never clones allowlists.
        let permissions = self.verified_program.permissions_for_chunk(chunk_index)?;
        let stack = &mut cursor.stack;
        let mut pc = cursor.pc;
        let mut instruction = pc;
        let mut failure_stage = FailureStage::Operation;
        let result = {
            let mut execution = self.program.executing_chunk(chunk_index)?;
            (|| {
                loop {
                    instruction = pc;
                    let instr = execution.instruction(pc)?;
                    let op_pc = pc;
                    pc += 1;
                    execution.record_execution_op(instr, stack.len());
                    match instr {
                        JitOp::PushInt(value) => stack.push(Value::I64(value)),
                        JitOp::PushNull => stack.push(runtime_null()),
                        JitOp::PushBool(value) => stack.push(Value::Bool(value)),
                        JitOp::PushFloat(bits) => {
                            stack.push(Value::Float {
                                kind: TypeKind::Fp64,
                                bits: f64::from_bits(bits),
                            });
                        }
                        JitOp::PushFunction(function_index) => {
                            if function_index >= self.verified_program.program().functions.len() {
                                return Err(TinyLangError::runtime(format!("Invalid function index {function_index}")));
                            }
                            stack.push(Value::Function(function_index as u32));
                        }
                        JitOp::Pop => {
                            jit_pop(stack)?;
                        }
                        JitOp::PushString(index) => {
                            self.frame_memory.clear();
                            let text = self
                                .verified_program
                                .program()
                                .strings
                                .get(index)
                                .ok_or_else(|| TinyLangError::runtime(format!("Invalid string index {index}")))?
                                .clone();
                            stack.push(Value::Heap(self.context.heap().alloc_string(text)?));
                        }
                        JitOp::Load(slot) => stack.push(memory.load(slot)?),
                        JitOp::LoadGlobal(slot) => {
                            let globals = global_memory
                                .ok_or_else(|| TinyLangError::runtime("Global load outside a function frame"))?;
                            stack.push(globals.load(slot)?);
                        }
                        JitOp::Store(slot) => {
                            let value = jit_pop(stack)?;
                            memory.store(slot, value)?;
                        }
                        JitOp::MoveSlot(source, destination) => {
                            memory.store(destination, memory.load(source)?)?;
                        }
                        JitOp::StoreInt(slot, value) => {
                            memory.store_int(slot, value)?;
                        }
                        JitOp::BinarySlotsStore(mode, lhs, rhs, destination) => {
                            let lhs = memory.load(lhs.index())?;
                            let rhs = memory.load(rhs.index())?;
                            let result = mode.execute(lhs, rhs)?;
                            memory
                                .store(destination.index(), result)
                                .inspect_err(|_| failure_stage = FailureStage::Store)?;
                        }
                        JitOp::BinarySlotIntStore(mode, source, value, destination) => {
                            let source = memory.load(source.index())?;
                            let result = mode.execute(source, Value::I64(value))?;
                            memory
                                .store(destination.index(), result)
                                .inspect_err(|_| failure_stage = FailureStage::Store)?;
                        }
                        JitOp::BinarySlotFloatStore(mode, source, bits, destination) => {
                            let source = memory.load(source.index())?;
                            let literal = Value::Float {
                                kind: TypeKind::Fp64,
                                bits: f64::from_bits(bits),
                            };
                            let result = mode.execute(source, literal)?;
                            memory
                                .store(destination.index(), result)
                                .inspect_err(|_| failure_stage = FailureStage::Store)?;
                        }
                        JitOp::SampleBinary(mode) => {
                            let (lhs, rhs) = jit_pop_pair(stack)?;
                            execution.observe_types(op_pc, ObservedTypes::binary(&lhs, &rhs));
                            stack.push(mode.execute(lhs, rhs)?);
                        }
                        JitOp::SampleSlotInt(mode, slot, value) => {
                            let source = memory.load(slot.index())?;
                            execution.observe_types(op_pc, ObservedTypes::unary(&source));
                            let result = mode.scalar().execute(source, Value::I64(value))?;
                            match mode {
                                SlotIntMode::MulAssign | SlotIntMode::DivAssign => {
                                    memory
                                        .store(slot.index(), result)
                                        .inspect_err(|_| failure_stage = FailureStage::Store)?
                                }
                                SlotIntMode::MulPush | SlotIntMode::DivPush => stack.push(result),
                            }
                        }
                        JitOp::AddSlotInt(slot, value) => {
                            memory.add_int(slot, value)?;
                        }
                        JitOp::SubSlotInt(slot, value) => {
                            memory.sub_int(slot, value)?;
                        }
                        JitOp::MulSlotInt(slot, value) => {
                            let result = runtime_mul(memory.load(slot)?, Value::I64(value))?;
                            memory
                                .store(slot, result)
                                .inspect_err(|_| failure_stage = FailureStage::Store)?;
                        }
                        JitOp::MulSlotIntHot(slot, value) => {
                            if memory.try_mul_int_assign(slot, value)?.is_some() {
                                execution.record_i64_hit(op_pc);
                            } else {
                                let source = memory.load(slot)?;
                                execution.record_i64_miss(op_pc, ObservedTypes::unary(&source));
                                let result = runtime_mul(source, Value::I64(value))?;
                                memory
                                    .store(slot, result)
                                    .inspect_err(|_| failure_stage = FailureStage::Store)?;
                            }
                        }
                        JitOp::DivSlotInt(slot, value) => {
                            let result = checked_div(memory.load(slot)?, Value::I64(value))?;
                            memory
                                .store(slot, result)
                                .inspect_err(|_| failure_stage = FailureStage::Store)?;
                        }
                        JitOp::DivSlotIntHot(slot, value) => {
                            if memory.try_div_int_assign(slot, value)?.is_some() {
                                execution.record_i64_hit(op_pc);
                            } else {
                                let source = memory.load(slot)?;
                                execution.record_i64_miss(op_pc, ObservedTypes::unary(&source));
                                let result = checked_div(source, Value::I64(value))?;
                                memory
                                    .store(slot, result)
                                    .inspect_err(|_| failure_stage = FailureStage::Store)?;
                            }
                        }
                        JitOp::CompareSlotIntJumpIfZero(operands) => {
                            let slot = operands.slot();
                            let value = operands.value();
                            let op = operands.comparison();
                            let target = operands.target();
                            let condition = runtime_compare(op, memory.load(slot)?, Value::I64(value))?;
                            if runtime_is_false(&condition) {
                                if target < op_pc {
                                    execution.record_back_edge(op_pc, target);
                                }
                                pc = target;
                            }
                        }
                        JitOp::CompareSlotIntJumpIfZeroHot(operands) => {
                            let slot = operands.slot();
                            let value = operands.value();
                            let op = operands.comparison();
                            let target = operands.target();
                            let is_false = match memory.compare_int(slot, value, op)? {
                                Some(condition) => {
                                    execution.record_i64_hit(op_pc);
                                    !condition
                                }
                                None => {
                                    let source = memory.load(slot)?;
                                    execution.record_i64_miss(op_pc, ObservedTypes::unary(&source));
                                    runtime_is_false(&runtime_compare(op, source, Value::I64(value))?)
                                }
                            };
                            if is_false {
                                pc = target;
                            }
                        }
                        JitOp::SampleCompareSlotIntJumpIfZero(operands)
                        | JitOp::GenericCompareSlotIntJumpIfZero(operands) => {
                            let source = memory.load(operands.slot())?;
                            if matches!(instr, JitOp::SampleCompareSlotIntJumpIfZero(_)) {
                                execution.observe_types(op_pc, ObservedTypes::unary(&source));
                            }
                            let condition =
                                runtime_compare(operands.comparison(), source, Value::I64(operands.value()))?;
                            if runtime_is_false(&condition) {
                                pc = operands.target();
                            }
                        }
                        JitOp::JumpIfZeroSlot(slot, target) => {
                            if runtime_is_false(&memory.load(slot)?) {
                                if target < op_pc {
                                    execution.record_back_edge(op_pc, target);
                                }
                                pc = target;
                            }
                        }
                        JitOp::JumpIfZeroSlotHot(slot, target) => {
                            let is_false = match memory.is_int_zero(slot)? {
                                Some(is_zero) => {
                                    execution.record_i64_hit(op_pc);
                                    is_zero
                                }
                                None => {
                                    let source = memory.load(slot)?;
                                    execution.record_i64_miss(op_pc, ObservedTypes::unary(&source));
                                    runtime_is_false(&source)
                                }
                            };
                            if is_false {
                                pc = target;
                            }
                        }
                        JitOp::SampleJumpIfZeroSlot(slot, target) | JitOp::GenericJumpIfZeroSlot(slot, target) => {
                            let source = memory.load(slot)?;
                            if matches!(instr, JitOp::SampleJumpIfZeroSlot(_, _)) {
                                execution.observe_types(op_pc, ObservedTypes::unary(&source));
                            }
                            if runtime_is_false(&source) {
                                pc = target;
                            }
                        }
                        JitOp::ArrayLenSlotJumpIfZero(slot, target) => {
                            let target_value = memory.load(slot)?;
                            let length =
                                crate::execution::runtime::builtins::runtime_len(&self.context, &target_value)?;
                            if matches!(length, Value::I64(0)) {
                                if target < op_pc {
                                    execution.record_back_edge(op_pc, target);
                                }
                                pc = target;
                            }
                        }
                        JitOp::MapGetAddSlots(slots) => {
                            let total = memory.load(slots.total_slot())?;
                            let target = memory.load(slots.map_slot())?;
                            let key = memory.load(slots.key_slot())?;
                            let value = match key {
                                Value::I64(key) => {
                                    crate::execution::runtime::stdlib::b_map_get_i64(&mut self.context, &target, key)?
                                }
                                key => crate::execution::runtime::stdlib::b_map_get(&mut self.context, &target, &key)?,
                            };
                            let result =
                                runtime_add(total, value).inspect_err(|_| failure_stage = FailureStage::Secondary)?;
                            memory
                                .store(slots.total_slot(), result)
                                .inspect_err(|_| failure_stage = FailureStage::Store)?;
                        }
                        JitOp::MapSetMulSlots(slots) => {
                            self.frame_memory.clear();
                            let target = memory.load(slots.map_slot())?;
                            let key = memory.load(slots.key_slot())?;
                            let value = match memory.mul_int(slots.value_slot(), slots.multiplier())? {
                                Some(value) => Value::I64(value),
                                None => {
                                    runtime_mul_int(memory.load(slots.value_slot())?, Value::I64(slots.multiplier()))?
                                }
                            };
                            let result = match key {
                                Value::I64(key) => crate::execution::runtime::stdlib::b_map_set_i64(
                                    &mut self.context,
                                    &target,
                                    key,
                                    value,
                                ),
                                key => {
                                    crate::execution::runtime::stdlib::b_map_set(&mut self.context, &target, key, value)
                                }
                            }
                            .inspect_err(|_| failure_stage = FailureStage::Secondary)?;
                            memory
                                .store(slots.destination_slot(), result)
                                .inspect_err(|_| failure_stage = FailureStage::Store)?;
                        }
                        JitOp::PushMulSlotInt(slot, value) => {
                            stack.push(runtime_mul(memory.load(slot)?, Value::I64(value))?);
                        }
                        JitOp::PushMulSlotIntHot(slot, value) => {
                            let result = match memory.mul_int(slot, value)? {
                                Some(result) => {
                                    execution.record_i64_hit(op_pc);
                                    Value::I64(result)
                                }
                                None => {
                                    let source = memory.load(slot)?;
                                    execution.record_i64_miss(op_pc, ObservedTypes::unary(&source));
                                    runtime_mul(source, Value::I64(value))?
                                }
                            };
                            stack.push(result);
                        }
                        JitOp::PushDivSlotInt(slot, value) => {
                            stack.push(checked_div(memory.load(slot)?, Value::I64(value))?);
                        }
                        JitOp::PushDivSlotIntHot(slot, value) => {
                            let result = match memory.div_int(slot, value)? {
                                Some(result) => {
                                    execution.record_i64_hit(op_pc);
                                    Value::I64(result)
                                }
                                None => {
                                    let source = memory.load(slot)?;
                                    execution.record_i64_miss(op_pc, ObservedTypes::unary(&source));
                                    checked_div(source, Value::I64(value))?
                                }
                            };
                            stack.push(result);
                        }
                        JitOp::Add => {
                            let (lhs, rhs) = jit_pop_pair(stack)?;
                            stack.push(runtime_add(lhs, rhs)?);
                        }
                        JitOp::AddInt => {
                            let (lhs, rhs) = jit_pop_pair(stack)?;
                            execution.record_binary_guard(op_pc, &lhs, &rhs);
                            stack.push(runtime_add_int(lhs, rhs)?);
                        }
                        JitOp::Sub => {
                            let (lhs, rhs) = jit_pop_pair(stack)?;
                            stack.push(runtime_sub(lhs, rhs)?);
                        }
                        JitOp::SubInt => {
                            let (lhs, rhs) = jit_pop_pair(stack)?;
                            execution.record_binary_guard(op_pc, &lhs, &rhs);
                            stack.push(runtime_sub_int(lhs, rhs)?);
                        }
                        JitOp::Mul => {
                            let (lhs, rhs) = jit_pop_pair(stack)?;
                            stack.push(runtime_mul(lhs, rhs)?);
                        }
                        JitOp::MulInt => {
                            let (lhs, rhs) = jit_pop_pair(stack)?;
                            execution.record_binary_guard(op_pc, &lhs, &rhs);
                            stack.push(runtime_mul_int(lhs, rhs)?);
                        }
                        JitOp::Div => {
                            let (lhs, rhs) = jit_pop_pair(stack)?;
                            stack.push(checked_div(lhs, rhs)?);
                        }
                        JitOp::DivInt => {
                            let (lhs, rhs) = jit_pop_pair(stack)?;
                            execution.record_binary_guard(op_pc, &lhs, &rhs);
                            stack.push(checked_div_int(lhs, rhs)?);
                        }
                        JitOp::Neg => {
                            let value = jit_pop(stack)?;
                            stack.push(runtime_neg(value)?);
                        }
                        JitOp::Compare(op) => {
                            let (lhs, rhs) = jit_pop_pair(stack)?;
                            stack.push(runtime_compare(op, lhs, rhs)?);
                        }
                        JitOp::CompareInt(op) => {
                            let (lhs, rhs) = jit_pop_pair(stack)?;
                            execution.record_binary_guard(op_pc, &lhs, &rhs);
                            stack.push(runtime_compare_int(op, lhs, rhs)?);
                        }
                        JitOp::Jump(target) => {
                            if target < op_pc {
                                execution.record_back_edge(op_pc, target);
                            }
                            pc = target;
                        }
                        JitOp::JumpHot(target) => {
                            pc = target;
                        }
                        JitOp::JumpIfZero(target) => {
                            let value = jit_pop(stack)?;
                            if runtime_is_false(&value) {
                                if target < op_pc {
                                    execution.record_back_edge(op_pc, target);
                                }
                                pc = target;
                            }
                        }
                        JitOp::JumpIfZeroHot(target) => {
                            let value = jit_pop(stack)?;
                            if runtime_is_false(&value) {
                                pc = target;
                            }
                        }
                        JitOp::Call(function_index, arg_count) => {
                            cursor.pc = pc;
                            return Ok(FrameExit::Call(CallRequest::Function {
                                index: function_index,
                                arg_count,
                            }));
                        }
                        JitOp::CallValue(arg_count) => {
                            cursor.pc = pc;
                            return Ok(FrameExit::Call(CallRequest::Operand { arg_count }));
                        }
                        JitOp::SampleCallValue(arg_count, sample) => {
                            cursor.pc = pc;
                            return Ok(FrameExit::Call(CallRequest::Sampled { arg_count, sample }));
                        }
                        JitOp::GuardedCallValue(guard) => {
                            cursor.pc = pc;
                            return Ok(FrameExit::Call(CallRequest::Guarded(guard)));
                        }
                        JitOp::UnseenCallValue(arg_count) => {
                            cursor.pc = pc;
                            return Ok(FrameExit::Call(CallRequest::UnseenOperand { arg_count }));
                        }
                        JitOp::MakeArray(count) => {
                            self.frame_memory.clear();
                            let values = pop_args(stack, count)?;
                            stack.push(runtime_make_array(&mut self.context, values)?);
                        }
                        JitOp::MakeClosure(function_index, count) => {
                            self.frame_memory.clear();
                            if function_index >= self.verified_program.program().functions.len() {
                                return Err(TinyLangError::runtime(format!("Invalid function index {function_index}")));
                            }
                            let captures = pop_args(stack, count)?;
                            let closure = self.context.heap().alloc_closure(function_index as u32, captures)?;
                            stack.push(Value::Heap(closure));
                        }
                        JitOp::MakeCell => {
                            self.frame_memory.clear();
                            let value = jit_pop(stack)?;
                            stack.push(Value::Heap(self.context.heap().alloc_cell(value)?));
                        }
                        JitOp::LoadCell => {
                            let cell = jit_pop(stack)?;
                            stack.push(self.context.heap().cell_load(&cell)?);
                        }
                        JitOp::StoreCell => {
                            let cell = jit_pop(stack)?;
                            let value = jit_pop(stack)?;
                            self.context.heap().cell_store(&cell, &value)?;
                        }
                        JitOp::Index => {
                            self.frame_memory.clear();
                            let index = jit_pop(stack)?;
                            let container = jit_pop(stack)?;
                            stack.push(runtime_index(&mut self.context, container, index)?);
                        }
                        JitOp::SetIndex => {
                            self.frame_memory.clear();
                            let value = jit_pop(stack)?;
                            let index = jit_pop(stack)?;
                            let container = jit_pop(stack)?;
                            runtime_set_index(&mut self.context, container, index, value)?;
                        }
                        JitOp::MakeStruct(struct_index, field_count) => {
                            self.frame_memory.clear();
                            let values = pop_args(stack, field_count)?;
                            let struct_def =
                                self.verified_program
                                    .program()
                                    .structs
                                    .get(struct_index)
                                    .ok_or_else(|| {
                                        TinyLangError::runtime(format!("Invalid struct index {struct_index}"))
                                    })?;
                            stack.push(runtime_make_struct(
                                &mut self.context,
                                &struct_def.name,
                                &struct_def.fields,
                                values,
                            )?);
                        }
                        JitOp::GetField(field_index) => {
                            self.frame_memory.clear();
                            let target = jit_pop(stack)?;
                            let field =
                                self.verified_program.program().fields.get(field_index).ok_or_else(|| {
                                    TinyLangError::runtime(format!("Invalid field index {field_index}"))
                                })?;
                            stack.push(runtime_get_field(&self.context, target, field)?);
                        }
                        JitOp::SetField(field_index) => {
                            self.frame_memory.clear();
                            let value = jit_pop(stack)?;
                            let target = jit_pop(stack)?;
                            let field =
                                self.verified_program.program().fields.get(field_index).ok_or_else(|| {
                                    TinyLangError::runtime(format!("Invalid field index {field_index}"))
                                })?;
                            runtime_set_field(&mut self.context, target, field, value)?;
                        }
                        JitOp::MakeEnum(variant_id, field_count) => {
                            self.frame_memory.clear();
                            let values = pop_args(stack, field_count)?;
                            let variant_def = self
                                .verified_program
                                .program()
                                .enum_variants
                                .get(variant_id)
                                .ok_or_else(|| {
                                    TinyLangError::runtime(format!("Invalid enum variant index {variant_id}"))
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
                        JitOp::BuiltinDirect(builtin) => {
                            self.frame_memory.clear();
                            run_direct_builtin(&mut self.context, stack, builtin, permissions)?;
                        }
                        JitOp::Builtin(builtin_index, arg_count) => {
                            self.frame_memory.clear();
                            let args_start = stack
                                .len()
                                .checked_sub(arg_count)
                                .ok_or_else(|| TinyLangError::runtime("Stack underflow"))?;
                            let globals = global_memory.unwrap_or(&*memory);
                            let caller_function = chunk_index.checked_sub(1);
                            let result = runtime_call_builtin(
                                &mut self.context,
                                globals,
                                builtin_index,
                                caller_function,
                                permissions,
                                &stack[args_start..],
                            )?;
                            stack.truncate(args_start);
                            stack.push(result);
                        }
                        JitOp::Return => return Ok(FrameExit::Return(Some(jit_pop(stack)?))),
                        JitOp::Print => {
                            if !self.context.queued_stdout.is_empty() {
                                stdout
                                    .write_all(&self.context.queued_stdout)
                                    .map_err(|e| TinyLangError::runtime(format!("stdout flush error: {e}")))?;
                                self.context.queued_stdout.clear();
                            }
                            let value = jit_pop(stack)?;
                            runtime_print(&self.context, stdout, &value)?;
                        }
                        JitOp::Halt => {
                            if !self.context.queued_stdout.is_empty() {
                                stdout
                                    .write_all(&self.context.queued_stdout)
                                    .map_err(|e| TinyLangError::runtime(format!("stdout flush error: {e}")))?;
                                self.context.queued_stdout.clear();
                            }
                            if !stack.is_empty() {
                                let chunk_name = execution.name();
                                return Err(TinyLangError::runtime(format!(
                                    "Internal stack imbalance at halt in {chunk_name}"
                                )));
                            }
                            return Ok(FrameExit::Return(None));
                        }
                    }
                }
            })()
        };
        result.map_err(|error| self.annotate_error(chunk_index, instruction, failure_stage, error))
    }

    fn annotate_error(
        &self,
        chunk: usize,
        instruction: usize,
        stage: FailureStage,
        error: TinyLangError,
    ) -> TinyLangError {
        let original = self
            .program
            .chunks
            .get(chunk)
            .and_then(Option::as_ref)
            .and_then(|chunk| chunk.origins.instruction(instruction, stage));
        match original {
            Some(instruction) => {
                BytecodeSite::new(chunk.checked_sub(1), instruction).annotate(self.verified_program.program(), error)
            }
            None => error,
        }
    }

    fn start_call(&mut self, request: CallRequest, caller: &mut FrameCursor, call_depth: usize) -> Result<CallFrame> {
        let arg_count = request.arg_count();
        let argument_start = caller
            .stack
            .len()
            .checked_sub(arg_count)
            .ok_or_else(|| TinyLangError::runtime("Stack underflow"))?;
        let call_start = match request {
            CallRequest::Function { .. } => argument_start,
            CallRequest::Operand { .. }
            | CallRequest::Sampled { .. }
            | CallRequest::Guarded(_)
            | CallRequest::UnseenOperand { .. } => argument_start
                .checked_sub(1)
                .ok_or_else(|| TinyLangError::runtime("Stack underflow"))?,
        };
        // A call exit already advanced the suspended cursor past its opcode.
        // Its origin is recoverable here without copying it in every exit.
        let site = CallSite {
            chunk_index: caller.chunk_index,
            op_pc: caller.pc - 1,
        };
        // Draining the suffix preserves the caller's prefix and capacity.
        // Early errors drop every unconsumed argument without cloning values.
        let mut args = caller.stack.drain(call_start..);
        let (function_index, memory, slot_count, cached) = {
            let heap;
            let (function_index, captures) = match request {
                CallRequest::Function { index, .. } => (index, None),
                CallRequest::Operand { .. }
                | CallRequest::Sampled { .. }
                | CallRequest::Guarded(_)
                | CallRequest::UnseenOperand { .. } => {
                    match args.next().ok_or_else(|| TinyLangError::runtime("Stack underflow"))? {
                        Value::Function(index) => (index as usize, None),
                        Value::Heap(reference) => {
                            heap = self.context.heap();
                            let object = heap.get(&Value::Heap(reference)).inspect_err(|_| {
                                self.program.reject_call_target(site, request);
                            })?;
                            let HeapData::Closure { function_id, captures } = &object.data else {
                                self.program.reject_call_target(site, request);
                                return Err(TinyLangError::runtime("CallValue expects a function or Closure"));
                            };
                            (*function_id as usize, Some(captures))
                        }
                        _ => {
                            self.program.reject_call_target(site, request);
                            return Err(TinyLangError::runtime("CallValue expects a function or Closure"));
                        }
                    }
                }
            };
            let capture_count = captures.map_or(0, |values| values.len());
            // Both counts are bounded by verified stack/heap length limits.
            let total_args = capture_count + args.len();
            let cached = if let CallRequest::Guarded(guard) = request {
                let matched = guard.matches(function_index, total_args);
                self.program.record_call_guard(site, guard, matched);
                matched.then_some(guard)
            } else {
                None
            };
            let slot_count = if let Some(guard) = cached {
                guard.slot_count()
            } else {
                let function = self
                    .verified_program
                    .program()
                    .functions
                    .get(function_index)
                    .ok_or_else(|| TinyLangError::runtime(format!("Invalid function index {function_index}")))?;
                if total_args != function.param_count {
                    return Err(TinyLangError::runtime(format!(
                        "Function {:?} expects {} argument(s), got {}",
                        function.name, function.param_count, total_args,
                    )));
                }
                function.slot_count
            };
            let max_call_depth = self.verified_program.program().max_call_depth();
            if call_depth >= max_call_depth {
                return Err(TinyLangError::runtime(format!(
                    "Call stack overflow after {max_call_depth} nested call(s)"
                )));
            }
            let mut memory = self.frame_memory.take(slot_count, &mut self.program.stats)?;
            if let Some(captures) = captures {
                for (slot, value) in crate::execution::runtime::heap::iter_array_values(captures).enumerate() {
                    memory.store(slot, value)?;
                }
            }
            for (slot, value) in args.enumerate() {
                memory.store(capture_count + slot, value)?;
            }
            (function_index, memory, slot_count, cached)
            // Release any borrowed closure/heap lock before lazy lowering or
            // callee execution. No closure instance is retained in call state.
        };
        let cursor = if let Some(guard) = cached {
            FrameCursor {
                chunk_index: function_index + 1,
                pc: 0,
                stack: self.program.take_operand_stack(guard.stack_capacity()),
            }
        } else {
            self.new_cursor(function_index + 1)?
        };
        if matches!(request, CallRequest::UnseenOperand { .. } | CallRequest::Sampled { .. }) {
            self.program.observe_call_entry(site, request, function_index);
        }
        Ok(CallFrame {
            cursor,
            memory,
            slot_count,
        })
    }

    fn function_name(&self, function_index: usize) -> &str {
        self.program
            .verified_program
            .program()
            .functions
            .get(function_index)
            .map_or("<invalid>", |function| function.name.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::{CallRequest, FrameCursor, JitVm, jit_pop_pair};
    use crate::execution::runtime::value::{CastKind, PointerKind};
    use crate::{
        JitOptions, JitProgram, RawPointer, Value, VerifiedProgram, VmSettings, compile_source_verified,
        run_verified_program,
    };

    #[test]
    fn operand_pair_moves_owned_values_in_order_without_changing_the_prefix_or_capacity() {
        let left = RawPointer::new(0, PointerKind::Field, 0, "left".repeat(128), 1, CastKind::None);
        let right = RawPointer::new(1, PointerKind::Field, 0, "right".repeat(128), 1, CastKind::None);
        let left_text = left.field.as_ptr();
        let right_text = right.field.as_ptr();
        let mut stack = vec![Value::I64(99), Value::Pointer(left), Value::Reference(right)];
        let storage = stack.as_ptr();
        let capacity = stack.capacity();

        let (Value::Pointer(left), Value::Reference(right)) = jit_pop_pair(&mut stack).expect("two operands") else {
            panic!("operand extraction must preserve kind and order");
        };
        assert_eq!(left.field.as_ptr(), left_text, "left text ownership moves without cloning");
        assert_eq!(right.field.as_ptr(), right_text, "right text ownership moves without cloning");
        assert_eq!(stack, [Value::I64(99)]);
        assert_eq!(stack.as_ptr(), storage);
        assert_eq!(stack.capacity(), capacity);

        // Preserve the existing pop order even on an internal underflow:
        // a lone right operand is consumed before the missing left is reported.
        assert_eq!(jit_pop_pair(&mut stack).expect_err("one operand").to_string(), "Stack underflow");
        assert!(stack.is_empty());
        assert_eq!(jit_pop_pair(&mut stack).expect_err("empty stack").to_string(), "Stack underflow");
        assert_eq!(stack.capacity(), capacity);
    }

    #[test]
    fn call_transfer_drains_only_the_suffix_without_growing_the_caller() {
        let verified = compile_source_verified("fn pair(a, b) { return a + b }").expect("function declaration");
        for request in [
            CallRequest::Function { index: 0, arg_count: 2 },
            CallRequest::Operand { arg_count: 2 },
        ] {
            let mut compiled = JitProgram::compile_verified(&verified).expect("lower main");
            let mut vm = JitVm::new(&mut compiled, Vec::new());
            let mut stack = Vec::with_capacity(8);
            stack.push(Value::I64(99));
            if matches!(request, CallRequest::Operand { .. }) {
                stack.push(Value::Function(0));
            }
            stack.extend([Value::I64(7), Value::I64(8)]);
            let pointer = stack.as_ptr();
            let capacity = stack.capacity();
            let mut caller = FrameCursor {
                chunk_index: 0,
                pc: 1,
                stack,
            };
            let call = vm.start_call(request, &mut caller, 0).expect("argument transfer");
            assert_eq!(caller.stack, [Value::I64(99)]);
            assert_eq!(caller.stack.as_ptr(), pointer);
            assert_eq!(caller.stack.capacity(), capacity);
            assert_eq!(call.memory.snapshot(), [Value::I64(7), Value::I64(8)]);
        }
    }

    #[test]
    fn failed_slot_encoding_drops_the_remaining_arguments_and_keeps_the_prefix() {
        let verified = compile_source_verified("fn pair(a, b) { return a }").expect("function declaration");
        let mut compiled = JitProgram::compile_verified(&verified).expect("lower main");
        let mut vm = JitVm::new(&mut compiled, Vec::new());
        let mut caller = FrameCursor {
            chunk_index: 0,
            pc: 1,
            stack: vec![
                Value::I64(99),
                Value::Pointer(RawPointer::new(0, PointerKind::Field, 0, "x".repeat(28), 1, CastKind::None)),
                Value::Pointer(RawPointer::new(0, PointerKind::Field, 0, "y".repeat(28), 1, CastKind::None)),
            ],
        };
        let request = CallRequest::Function { index: 0, arg_count: 2 };
        let result = vm.start_call(request, &mut caller, 0);
        assert!(result.is_err());
        assert_eq!(caller.stack, [Value::I64(99)]);
        assert_eq!(vm.program.stats().compiled_chunks, 1, "failure must not lower the callee");
        caller.stack.extend([Value::I64(7), Value::I64(8)]);
        let call = vm
            .start_call(request, &mut caller, 0)
            .expect("fresh frame after failed transfer");
        assert_eq!(call.memory.snapshot(), [Value::I64(7), Value::I64(8)]);
    }

    #[test]
    fn explicit_frames_preserve_each_supported_logical_depth_boundary() {
        for limit in [1, 2, 16] {
            for argument in [limit - 1, limit] {
                let source = format!(
                    "fn recurse(n) {{ if n > 0 {{ return recurse(n - 1) }} return 7 }} print recurse({argument})"
                );
                let mut program = compile_source_verified(&source)
                    .expect("recursive source")
                    .into_program();
                program.vm_settings = VmSettings::with_max_call_depth(limit).expect("supported limit");
                let verified = VerifiedProgram::verify(program).expect("configured recursive program");
                let mut expected_output = Vec::new();
                let expected = run_verified_program(&verified, "vm", &mut expected_output, Vec::new());
                for threshold in [0, 1, 8] {
                    let mut compiled = JitProgram::compile_verified_with_options(
                        &verified,
                        JitOptions::new().with_hot_back_edge_threshold(threshold),
                    )
                    .expect("compile recursive program");
                    for _ in 0..2 {
                        let mut output = Vec::new();
                        let result = compiled.run(&mut output, Vec::new());
                        assert_eq!(output, expected_output);
                        assert_eq!(
                            result.as_ref().err().map(ToString::to_string),
                            expected.as_ref().err().map(ToString::to_string)
                        );
                        assert_eq!(compiled.stats().peak_call_depth, limit);
                    }
                }
            }
        }
    }
}
