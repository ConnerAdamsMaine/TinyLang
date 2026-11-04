use std::collections::VecDeque;

use super::EngineError;
use crate::{Op, VerifiedProgram};

const MAX_INSTRUCTIONS: usize = 4096;
const MAX_FRAME_VALUES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Equal,
    NotEqual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Instruction {
    Constant(i64),
    Load(usize),
    Store(usize),
    Binary(BinaryOp),
    Negate,
    Jump(usize),
    JumpIfZero(usize),
    Pop,
    Return,
}

/// Only lowering verified bytecode can construct this native-code capability.
#[derive(Debug, Clone)]
pub struct ScalarFunction {
    name:            String,
    parameter_count: usize,
    slot_count:      usize,
    instructions:    Box<[Instruction]>,
    stack_heights:   Box<[Option<usize>]>,
    max_stack:       usize,
}

impl ScalarFunction {
    pub fn lower(program: &VerifiedProgram, index: usize) -> Result<Self, EngineError> {
        let function = program
            .program()
            .functions()
            .get(index)
            .ok_or_else(|| EngineError::UnknownFunction(format!("#{index}")))?;
        let invalid = |instruction, message: &str| {
            EngineError::InvalidIr {
                function: function.name().to_owned(),
                instruction,
                message: message.to_owned(),
            }
        };
        let code = function.code();
        if code.is_empty() || code.len() > MAX_INSTRUCTIONS {
            return Err(invalid(0, "function must contain 1..=4096 instructions"));
        }
        let mut slots = function.param_count();
        let mut instructions = Vec::with_capacity(code.len());
        for (pc, raw) in code.iter().enumerate() {
            let operand = || usize::try_from(raw.arg).map_err(|_| invalid(pc, "negative or oversized operand"));
            let op = match raw.op {
                Op::PushInt => Instruction::Constant(raw.arg),
                Op::Load | Op::Store => {
                    let slot = operand()?;
                    slots = slots.max(slot.checked_add(1).ok_or_else(|| invalid(pc, "slot count overflow"))?);
                    if raw.op == Op::Load {
                        Instruction::Load(slot)
                    } else {
                        Instruction::Store(slot)
                    }
                }
                Op::Add => Instruction::Binary(BinaryOp::Add),
                Op::Sub => Instruction::Binary(BinaryOp::Subtract),
                Op::Mul => Instruction::Binary(BinaryOp::Multiply),
                Op::Div => Instruction::Binary(BinaryOp::Divide),
                Op::Lt => Instruction::Binary(BinaryOp::Less),
                Op::Lte => Instruction::Binary(BinaryOp::LessEqual),
                Op::Gt => Instruction::Binary(BinaryOp::Greater),
                Op::Gte => Instruction::Binary(BinaryOp::GreaterEqual),
                Op::Eq => Instruction::Binary(BinaryOp::Equal),
                Op::Ne => Instruction::Binary(BinaryOp::NotEqual),
                Op::Neg => Instruction::Negate,
                Op::Jump | Op::JumpIfZero => {
                    let target = operand()?;
                    if target >= code.len() {
                        return Err(invalid(pc, "jump leaves function"));
                    }
                    if raw.op == Op::Jump {
                        Instruction::Jump(target)
                    } else {
                        Instruction::JumpIfZero(target)
                    }
                }
                Op::Pop => Instruction::Pop,
                Op::Return => Instruction::Return,
                _ => {
                    return Err(EngineError::UnsupportedInstruction {
                        function:    function.name().to_owned(),
                        instruction: pc,
                        opcode:      format!("{:?}", raw.op),
                    });
                }
            };
            instructions.push(op);
        }
        if slots > MAX_FRAME_VALUES {
            return Err(invalid(0, "scalar local frame exceeds 4096 values"));
        }
        let mut heights = vec![None; code.len()];
        heights[0] = Some(0);
        let mut pending = VecDeque::from([0]);
        let mut max_stack = 0;
        while let Some(pc) = pending.pop_front() {
            let height = heights[pc].ok_or_else(|| invalid(pc, "missing reachable stack height"))?;
            let (inputs, outputs) = match instructions[pc] {
                Instruction::Constant(_) | Instruction::Load(_) => (0, 1),
                Instruction::Store(_) | Instruction::Pop | Instruction::JumpIfZero(_) => (1, 0),
                Instruction::Binary(_) => (2, 1),
                Instruction::Negate => (1, 1),
                Instruction::Jump(_) => (0, 0),
                Instruction::Return => (1, 0),
            };
            if height < inputs {
                return Err(invalid(pc, "operand stack underflow"));
            }
            let next_height = height - inputs + outputs;
            max_stack = max_stack.max(height).max(next_height);
            if max_stack > MAX_FRAME_VALUES {
                return Err(invalid(pc, "scalar operand stack exceeds 4096 values"));
            }
            if instructions[pc] == Instruction::Return {
                if height != 1 {
                    return Err(invalid(pc, "scalar return requires exactly one stack value"));
                }
                continue;
            }
            let mut successor = |target: usize| -> Result<(), EngineError> {
                if target >= code.len() {
                    return Err(invalid(pc, "reachable path falls off function"));
                }
                match heights[target] {
                    Some(previous) if previous != next_height => {
                        Err(invalid(target, "inconsistent stack heights at control-flow join"))
                    }
                    Some(_) => Ok(()),
                    None => {
                        heights[target] = Some(next_height);
                        pending.push_back(target);
                        Ok(())
                    }
                }
            };
            match instructions[pc] {
                Instruction::Jump(target) => successor(target)?,
                Instruction::JumpIfZero(target) => {
                    successor(target)?;
                    successor(pc + 1)?;
                }
                _ => successor(pc + 1)?,
            }
        }
        super::typing::check(function.name(), slots, max_stack, &instructions)?;
        Ok(Self {
            name: function.name().to_owned(),
            parameter_count: function.param_count(),
            slot_count: slots,
            instructions: instructions.into_boxed_slice(),
            stack_heights: heights.into_boxed_slice(),
            max_stack,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn parameter_count(&self) -> usize {
        self.parameter_count
    }

    pub fn slot_count(&self) -> usize {
        self.slot_count
    }

    pub fn instructions(&self) -> &[Instruction] {
        &self.instructions
    }

    pub fn stack_heights(&self) -> &[Option<usize>] {
        &self.stack_heights
    }

    pub fn max_stack(&self) -> usize {
        self.max_stack
    }

    pub(crate) fn frame(&self, args: &[i64]) -> Result<(Vec<i64>, Vec<i64>), EngineError> {
        if args.len() != self.parameter_count {
            return Err(EngineError::InvalidArguments {
                expected: self.parameter_count,
                actual:   args.len(),
            });
        }
        let allocate = |size| {
            let mut values = Vec::new();
            values
                .try_reserve_exact(size)
                .map_err(|error| EngineError::Allocation(error.to_string()))?;
            values.resize(size, 0i64);
            Ok::<_, EngineError>(values)
        };
        let mut slots = allocate(self.slot_count)?;
        slots[..args.len()].copy_from_slice(args);
        Ok((slots, allocate(self.max_stack)?))
    }
}
