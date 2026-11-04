use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{self, AbiParam, Block, InstBuilder, MemFlags, Value, types};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::Module;

use crate::engine::{BinaryOp, EngineError, Instruction, ScalarFunction};

pub(super) fn signature(function: &mut ir::Function, module: &impl Module) {
    function.signature = module.make_signature();
    for _ in 0..5 {
        function.signature.params.push(AbiParam::new(types::I64));
    }
    function.signature.returns.push(AbiParam::new(types::I32));
}

pub(super) fn emit(scalar: &ScalarFunction, function: &mut ir::Function) -> Result<(), EngineError> {
    let mut context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(function, &mut context);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    let params = builder.block_params(entry);
    let frame = Frame {
        slots:  params[0],
        stack:  params[1],
        output: params[2],
        fuel:   params[3],
        pc:     params[4],
    };
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
        let remaining = builder.ins().load(types::I64, MemFlags::new(), frame.fuel, 0);
        let exhausted = builder.ins().icmp_imm(IntCC::Equal, remaining, 0);
        guard(&mut builder, frame, exhausted, 3, pc);
        let remaining = builder.ins().iadd_imm(remaining, -1);
        builder.ins().store(MemFlags::new(), remaining, frame.fuel, 0);
        match operation {
            Instruction::Constant(value) => {
                let value = builder.ins().iconst(types::I64, value);
                store(&mut builder, frame.stack, height, value);
            }
            Instruction::Load(slot) => {
                let value = load(&mut builder, frame.slots, slot);
                store(&mut builder, frame.stack, height, value);
            }
            Instruction::Store(slot) => {
                let value = load(&mut builder, frame.stack, height - 1);
                store(&mut builder, frame.slots, slot, value);
            }
            Instruction::Binary(op) => {
                let left = load(&mut builder, frame.stack, height - 2);
                let right = load(&mut builder, frame.stack, height - 1);
                let value = binary(&mut builder, frame, op, left, right, pc);
                store(&mut builder, frame.stack, height - 2, value);
            }
            Instruction::Negate => {
                let value = load(&mut builder, frame.stack, height - 1);
                let zero = builder.ins().iconst(types::I64, 0);
                let (value, overflow) = builder.ins().ssub_overflow(zero, value);
                guard(&mut builder, frame, overflow, 1, pc);
                store(&mut builder, frame.stack, height - 1, value);
            }
            Instruction::Jump(pc) => {
                builder.ins().jump(target(&blocks, pc)?, &[]);
                continue;
            }
            Instruction::JumpIfZero(destination) => {
                let value = load(&mut builder, frame.stack, height - 1);
                let zero = builder.ins().icmp_imm(IntCC::Equal, value, 0);
                builder
                    .ins()
                    .brif(zero, target(&blocks, destination)?, &[], target(&blocks, pc + 1)?, &[]);
                continue;
            }
            Instruction::Pop => {}
            Instruction::Return => {
                let value = load(&mut builder, frame.stack, 0);
                builder.ins().store(MemFlags::new(), value, frame.output, 0);
                let status = builder.ins().iconst(types::I32, 0);
                builder.ins().return_(&[status]);
                continue;
            }
        }
        builder.ins().jump(target(&blocks, pc + 1)?, &[]);
    }
    builder.seal_all_blocks();
    builder.finalize();
    Ok(())
}

#[derive(Clone, Copy)]
struct Frame {
    slots:  Value,
    stack:  Value,
    output: Value,
    fuel:   Value,
    pc:     Value,
}

fn target(blocks: &[Option<Block>], pc: usize) -> Result<Block, EngineError> {
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

fn guard(builder: &mut FunctionBuilder<'_>, frame: Frame, condition: Value, status: i64, pc: usize) {
    let trap = builder.create_block();
    let next = builder.create_block();
    builder.ins().brif(condition, trap, &[], next, &[]);
    builder.switch_to_block(trap);
    let pc = builder.ins().iconst(types::I64, pc as i64);
    builder.ins().store(MemFlags::new(), pc, frame.pc, 0);
    let status = builder.ins().iconst(types::I32, status);
    builder.ins().return_(&[status]);
    builder.switch_to_block(next);
}

fn binary(
    builder: &mut FunctionBuilder<'_>,
    frame: Frame,
    op: BinaryOp,
    left: Value,
    right: Value,
    pc: usize,
) -> Value {
    let (value, overflow) = match op {
        BinaryOp::Add => builder.ins().sadd_overflow(left, right),
        BinaryOp::Subtract => builder.ins().ssub_overflow(left, right),
        BinaryOp::Multiply => builder.ins().smul_overflow(left, right),
        BinaryOp::Divide => {
            let zero = builder.ins().icmp_imm(IntCC::Equal, right, 0);
            guard(builder, frame, zero, 2, pc);
            let minimum = builder.ins().icmp_imm(IntCC::Equal, left, i64::MIN);
            let negative_one = builder.ins().icmp_imm(IntCC::Equal, right, -1);
            let overflow = builder.ins().band(minimum, negative_one);
            guard(builder, frame, overflow, 1, pc);
            let quotient = builder.ins().sdiv(left, right);
            let remainder = builder.ins().srem(left, right);
            let inexact = builder.ins().icmp_imm(IntCC::NotEqual, remainder, 0);
            let signs = builder.ins().bxor(left, right);
            let opposite = builder.ins().icmp_imm(IntCC::SignedLessThan, signs, 0);
            let adjustment = builder.ins().band(inexact, opposite);
            let adjustment = builder.ins().uextend(types::I64, adjustment);
            // Match VM floor division. An inexact quotient cannot be i64::MIN,
            // so subtracting the 0/1 adjustment is representable.
            return builder.ins().isub(quotient, adjustment);
        }
        BinaryOp::Less => return comparison(builder, IntCC::SignedLessThan, left, right),
        BinaryOp::LessEqual => {
            return comparison(builder, IntCC::SignedLessThanOrEqual, left, right);
        }
        BinaryOp::Greater => return comparison(builder, IntCC::SignedGreaterThan, left, right),
        BinaryOp::GreaterEqual => {
            return comparison(builder, IntCC::SignedGreaterThanOrEqual, left, right);
        }
        BinaryOp::Equal => return comparison(builder, IntCC::Equal, left, right),
        BinaryOp::NotEqual => return comparison(builder, IntCC::NotEqual, left, right),
    };
    guard(builder, frame, overflow, 1, pc);
    value
}

fn comparison(builder: &mut FunctionBuilder<'_>, condition: IntCC, left: Value, right: Value) -> Value {
    let value = builder.ins().icmp(condition, left, right);
    builder.ins().uextend(types::I64, value)
}
