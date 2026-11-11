//! Cranelift execution of whole scalar call closures (program profile).
//!
//! Mirrors the emitted x86-64 image in `native::x64` and the authoritative
//! accounting in `program.rs::execute`: one shared fuel budget and one trap
//! channel live in the invocation context, every frame owns a bank segment
//! addressed by its call depth, and each call checks the depth limit before
//! touching the callee's segment. Results, shared fuel minima and trap pairs
//! (category and failing instruction) therefore match the reference
//! interpreter exactly.

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{self, AbiParam, FuncRef, InstBuilder, MemFlags, Value, types};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::JITModule;
use cranelift_module::{FuncId, Linkage, Module};

use super::executable::{self, ModuleMemory};
use super::lowering::{self, TrapEmitter};
use crate::execution::engine::{EngineError, Instruction, ScalarFunction, ScalarProgram, TrapKind};

/// Per-invocation machine state shared by every function of one program
/// execution. `repr(C)` fixes the offsets the lowering uses (u64 offsets).
#[repr(C)]
struct ProgramContext {
    depth: u64,
    trap: u32,
    pad: u32,
    fuel: u64,
    bank: *mut i64,
    pc: u64,
}

const OVERFLOW: u32 = 1;
const DIVISION_BY_ZERO: u32 = 2;
const FUEL_EXHAUSTED: u32 = 3;
const CALL_DEPTH: u32 = 4;

type Entry = unsafe extern "C" fn(*mut ProgramContext) -> i64;

/// Owns Cranelift-emitted machine code for one whole scalar call closure plus
/// its runtime frame-bank specification.
pub struct CraneliftProgram {
    parameters: usize,
    depth_limit: usize,
    stride_values: usize,
    entry: Entry,
    _memory: ModuleMemory,
}

impl CraneliftProgram {
    /// Compiles one whole call closure into one Cranelift module. Frame-bank
    /// and context conventions here must stay in lockstep with `emit`.
    pub fn compile(program: &ScalarProgram) -> Result<Self, EngineError> {
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
        let stride_values = slot_capacity + stack_capacity;
        // One shared uniform segment geometry: slots start the segment and the
        // operand stack follows, so every function finds its frame at
        // `bank + depth * stride`.
        let stride_bytes = (stride_values * 8) as i64;
        let depth_limit = program.call_depth_limit();

        let builder = executable::jit_builder()?;
        let mut memory = ModuleMemory(Some(JITModule::new(builder)));
        let module = memory
            .0
            .as_mut()
            .ok_or_else(|| EngineError::Codegen("missing live JIT module".to_owned()))?;

        // Declare every lowered function first so callees can be referenced
        // before their bodies are defined. Holes stay undeclared and are
        // unreachable from lowered code by construction.
        let mut ids: Vec<Option<FuncId>> = Vec::with_capacity(program.function_count());
        for index in 0..program.function_count() {
            if program.function(index).is_none() {
                ids.push(None);
                continue;
            }
            let signature = program_signature(module);
            let id = module
                .declare_function(&format!("tinylang_program_{index}"), Linkage::Local, &signature)
                .map_err(executable::module_error)?;
            ids.push(Some(id));
        }

        for (index, declared) in ids.iter().enumerate() {
            let Some(id) = *declared else { continue };
            let function = program
                .function(index)
                .ok_or_else(|| EngineError::UnknownFunction(format!("#{index}")))?;
            let mut context = module.make_context();
            context.func.signature = program_signature(module);
            // Cross-call references resolve per definition; a self reference
            // covers recursion expressible only in raw bytecode.
            let mut references: Vec<Option<FuncRef>> = vec![None; ids.len()];
            for (slot, target) in ids.iter().enumerate() {
                if let Some(target) = *target {
                    references[slot] = Some(module.declare_func_in_func(target, &mut context.func));
                }
            }
            emit(function, stride_bytes, depth_limit as i64, &references, &mut context.func)?;
            module
                .define_function(id, &mut context)
                .map_err(executable::module_error)?;
        }
        module.finalize_definitions().map_err(executable::module_error)?;
        let code = module.get_finalized_function(ids[entry].expect("entry is declared"));
        // SAFETY: The definition has the host C ABI and exactly Entry's
        // signature. Finalization made its pages RX. `memory` retains them
        // until self drops.
        let entry = unsafe { std::mem::transmute::<*const u8, Entry>(code) };
        Ok(Self {
            parameters,
            depth_limit,
            stride_values,
            entry,
            _memory: memory,
        })
    }

    /// Runs the entry function with `args` and one shared fuel budget, mirroring
    /// `ScalarProgram::interpret` including trap categories and failing program
    /// counters.
    pub fn invoke(&self, args: &[i64], fuel: u64) -> Result<i64, EngineError> {
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
        // SAFETY: Only our lowering can create this private RX allocation. Its
        // entry follows the C one-pointer ABI for the program profile and
        // accesses only this live bank through the context. Code is retained
        // by self, and no generated pointers escape.
        // The emitted instruction fuel bounds every control-flow edge.
        let result = unsafe { (self.entry)(&mut context) };
        let kind = match context.trap {
            0 => return Ok(result),
            value if value == OVERFLOW => TrapKind::Overflow,
            value if value == DIVISION_BY_ZERO => TrapKind::DivisionByZero,
            value if value == FUEL_EXHAUSTED => TrapKind::FuelExhausted,
            value if value == CALL_DEPTH => TrapKind::CallStackOverflow(self.depth_limit),
            value => {
                return Err(EngineError::Codegen(format!("invalid cranelift trap tag {value}")));
            }
        };
        Err(EngineError::Trap {
            kind,
            instruction: context.pc as usize,
        })
    }
}

fn program_signature(module: &JITModule) -> ir::Signature {
    let mut signature = module.make_signature();
    signature.params.push(AbiParam::new(types::I64));
    signature.returns.push(AbiParam::new(types::I64));
    signature
}

/// Emitted-function state defined in the entry block: the context pointer, the
/// context depth this frame runs at, the bank base and the uniform segment
/// stride. SSA values survive calls, so frames stay materialized across them.
#[derive(Clone, Copy)]
struct Machine {
    ctx: Value,
    depth: Value,
    bank: Value,
    stride_bytes: Value,
}

impl TrapEmitter for Machine {
    fn trap(&mut self, builder: &mut FunctionBuilder<'_>, condition: Value, tag: i64, _pc: usize) {
        let trap = builder.create_block();
        let next = builder.create_block();
        builder.ins().brif(condition, trap, &[], next, &[]);
        builder.switch_to_block(trap);
        let tag = builder.ins().iconst(types::I32, tag);
        builder.ins().store(MemFlags::new(), tag, self.ctx, 8);
        let zero = builder.ins().iconst(types::I64, 0);
        builder.ins().return_(&[zero]);
        builder.switch_to_block(next);
    }
}

/// Lowers one closure member into `function`. The prologue derives the frame
/// pointers from the context depth, records the failing instruction before
/// anything can trap, and shares one fuel budget through the context.
fn emit(
    scalar: &ScalarFunction,
    stride_bytes: i64,
    depth_limit: i64,
    references: &[Option<FuncRef>],
    function: &mut ir::Function,
) -> Result<(), EngineError> {
    let mut context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(function, &mut context);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    let ctx = builder.block_params(entry)[0];
    let depth = builder.ins().load(types::I64, MemFlags::new(), ctx, 0);
    let bank = builder.ins().load(types::I64, MemFlags::new(), ctx, 24);
    let stride_bytes = builder.ins().iconst(types::I64, stride_bytes);
    let mut machine = Machine {
        ctx,
        depth,
        bank,
        stride_bytes,
    };
    // One shared uniform segment geometry: slots start the segment and the
    // operand stack follows, so every function finds its frame at
    // `bank + depth * stride`.
    let offset = builder.ins().imul(depth, stride_bytes);
    let slots = builder.ins().iadd(bank, offset);
    let stack = builder.ins().iadd_imm(slots, (scalar.slot_count() * 8) as i64);
    // Body invariant: the context depth names the next callee's frame, which
    // is also this frame's segment index + 1.
    let nested = builder.ins().iadd_imm(depth, 1);
    builder.ins().store(MemFlags::new(), nested, ctx, 0);

    let mut blocks = Vec::new();
    blocks
        .try_reserve_exact(scalar.instructions().len())
        .map_err(|error| EngineError::Allocation(error.to_string()))?;
    for height in scalar.stack_heights() {
        blocks.push(height.map(|_| builder.create_block()));
    }
    builder
        .ins()
        .jump(blocks[0].ok_or_else(|| EngineError::Codegen("unreachable entry".to_owned()))?, &[]);
    for (pc, &operation) in scalar.instructions().iter().enumerate() {
        let Some(height) = scalar.stack_heights()[pc] else {
            continue;
        };
        builder.switch_to_block(target(&blocks, pc)?);
        // The failing instruction is recorded before anything can trap.
        let location = builder.ins().iconst(types::I64, pc as i64);
        builder.ins().store(MemFlags::new(), location, machine.ctx, 32);
        let remaining = builder.ins().load(types::I64, MemFlags::new(), machine.ctx, 16);
        let exhausted = builder.ins().icmp_imm(IntCC::Equal, remaining, 0);
        machine.trap(&mut builder, exhausted, i64::from(FUEL_EXHAUSTED), pc);
        let remaining = builder.ins().iadd_imm(remaining, -1);
        builder.ins().store(MemFlags::new(), remaining, machine.ctx, 16);
        match operation {
            Instruction::Constant(value) => {
                let value = builder.ins().iconst(types::I64, value);
                store(&mut builder, stack, height, value);
            }
            Instruction::Load(slot) => {
                let value = load(&mut builder, slots, slot);
                store(&mut builder, stack, height, value);
            }
            Instruction::Store(slot) => {
                let value = load(&mut builder, stack, height - 1);
                store(&mut builder, slots, slot, value);
            }
            Instruction::Binary(op) => {
                let left = load(&mut builder, stack, height - 2);
                let right = load(&mut builder, stack, height - 1);
                let value = lowering::binary(&mut builder, &mut machine, op, left, right, pc);
                store(&mut builder, stack, height - 2, value);
            }
            Instruction::Negate => {
                let value = load(&mut builder, stack, height - 1);
                let zero = builder.ins().iconst(types::I64, 0);
                let (value, overflow) = builder.ins().ssub_overflow(zero, value);
                machine.trap(&mut builder, overflow, i64::from(OVERFLOW), pc);
                store(&mut builder, stack, height - 1, value);
            }
            Instruction::Jump(destination) => {
                builder.ins().jump(target(&blocks, destination)?, &[]);
                continue;
            }
            Instruction::JumpIfZero(destination) => {
                let value = load(&mut builder, stack, height - 1);
                let zero = builder.ins().icmp_imm(IntCC::Equal, value, 0);
                builder
                    .ins()
                    .brif(zero, target(&blocks, destination)?, &[], target(&blocks, pc + 1)?, &[]);
                continue;
            }
            Instruction::Pop => {}
            Instruction::Call(callee, argument_count) => {
                let reference = references
                    .get(callee)
                    .copied()
                    .flatten()
                    .ok_or_else(|| EngineError::Codegen(format!("unregistered program callee {callee}")))?;
                // The reference interpreter traps when `depth + 1` exceeds the
                // limit; here the callee depth is already in the context.
                let overflow = builder.ins().icmp_imm(IntCC::SignedGreaterThan, nested, depth_limit);
                machine.trap(&mut builder, overflow, i64::from(CALL_DEPTH), pc);
                let offset = builder.ins().imul(nested, machine.stride_bytes);
                let callee_segment = builder.ins().iadd(machine.bank, offset);
                // Arguments leave in operand-stack order: the deepest value is
                // argument zero and lands in the callee's first slot.
                for index in 0..argument_count {
                    let value = load(&mut builder, stack, height - argument_count + index);
                    store(&mut builder, callee_segment, index, value);
                }
                let call = builder.ins().call(reference, &[machine.ctx]);
                let result = builder.inst_results(call)[0];
                // A callee trap propagates untouched: this frame returns
                // without touching the context's trap tag or location.
                let trapped = builder.ins().load(types::I32, MemFlags::new(), machine.ctx, 8);
                let trapped = builder.ins().icmp_imm(IntCC::NotEqual, trapped, 0);
                let propagate = builder.create_block();
                builder.append_block_param(propagate, types::I64);
                let next = builder.create_block();
                builder.ins().brif(trapped, propagate, &[result.into()], next, &[]);
                builder.switch_to_block(propagate);
                let passed = builder.block_params(propagate)[0];
                builder.ins().return_(&[passed]);
                builder.switch_to_block(next);
                // The result replaces the deepest popped argument.
                store(&mut builder, stack, height - argument_count, result);
            }
            Instruction::Return => {
                let value = load(&mut builder, stack, height - 1);
                // Hand the context depth back to the caller.
                builder.ins().store(MemFlags::new(), machine.depth, machine.ctx, 0);
                builder.ins().return_(&[value]);
                continue;
            }
        }
        builder.ins().jump(target(&blocks, pc + 1)?, &[]);
    }
    builder.seal_all_blocks();
    builder.finalize();
    Ok(())
}

fn target(
    blocks: &[Option<cranelift_codegen::ir::Block>],
    pc: usize,
) -> Result<cranelift_codegen::ir::Block, EngineError> {
    blocks
        .get(pc)
        .copied()
        .flatten()
        .ok_or_else(|| EngineError::Codegen(format!("unreachable CFG target {pc}")))
}

fn load(builder: &mut FunctionBuilder<'_>, base: Value, slot: usize) -> Value {
    // Verified scalar frames have at most 4096 i64 elements, so offsets fit i32.
    builder.ins().load(types::I64, MemFlags::new(), base, (slot * 8) as i32)
}

fn store(builder: &mut FunctionBuilder<'_>, base: Value, slot: usize, value: Value) {
    builder.ins().store(MemFlags::new(), value, base, (slot * 8) as i32);
}
