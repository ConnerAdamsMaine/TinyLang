use super::{BinaryOp, EngineError, Instruction, ScalarFunction, TrapKind};

/// Checked binary arithmetic shared by every reference interpreter so the
/// single-function and program profiles cannot silently drift apart.
pub(super) fn checked_binary(op: BinaryOp, lhs: i64, rhs: i64) -> Result<i64, TrapKind> {
    match op {
        BinaryOp::Add => lhs.checked_add(rhs).ok_or(TrapKind::Overflow),
        BinaryOp::Subtract => lhs.checked_sub(rhs).ok_or(TrapKind::Overflow),
        BinaryOp::Multiply => lhs.checked_mul(rhs).ok_or(TrapKind::Overflow),
        BinaryOp::Divide => {
            if rhs == 0 {
                return Err(TrapKind::DivisionByZero);
            }
            let quotient = lhs.checked_div(rhs).ok_or(TrapKind::Overflow)?;
            let remainder = lhs.checked_rem(rhs).ok_or(TrapKind::Overflow)?;
            if remainder != 0 && ((remainder > 0) != (rhs > 0)) {
                quotient.checked_sub(1).ok_or(TrapKind::Overflow)
            } else {
                Ok(quotient)
            }
        }
        BinaryOp::Less => Ok(i64::from(lhs < rhs)),
        BinaryOp::LessEqual => Ok(i64::from(lhs <= rhs)),
        BinaryOp::Greater => Ok(i64::from(lhs > rhs)),
        BinaryOp::GreaterEqual => Ok(i64::from(lhs >= rhs)),
        BinaryOp::Equal => Ok(i64::from(lhs == rhs)),
        BinaryOp::NotEqual => Ok(i64::from(lhs != rhs)),
    }
}

impl ScalarFunction {
    pub fn interpret(&self, args: &[i64], mut fuel: u64) -> Result<i64, EngineError> {
        let (mut slots, mut stack) = self.frame(args)?;
        let mut pc = 0;
        loop {
            let instruction = pc;
            let trap = |kind| EngineError::Trap { kind, instruction };
            if fuel == 0 {
                return Err(trap(TrapKind::FuelExhausted));
            }
            fuel -= 1;
            // Lowering proves stack heights, operands and successor indices.
            let height = self.stack_heights()[pc].ok_or_else(|| EngineError::InvalidIr {
                function: self.name().to_owned(),
                instruction: pc,
                message: "execution reached an unreachable instruction".to_owned(),
            })?;
            pc += 1;
            match self.instructions()[instruction] {
                Instruction::Constant(value) => stack[height] = value,
                Instruction::Load(slot) => stack[height] = slots[slot],
                Instruction::Store(slot) => slots[slot] = stack[height - 1],
                Instruction::Pop => {}
                Instruction::Negate => {
                    stack[height - 1] = stack[height - 1]
                        .checked_neg()
                        .ok_or_else(|| trap(TrapKind::Overflow))?;
                }
                Instruction::Binary(op) => {
                    let lhs = stack[height - 2];
                    let rhs = stack[height - 1];
                    stack[height - 2] = checked_binary(op, lhs, rhs).map_err(trap)?;
                }
                Instruction::Jump(target) => pc = target,
                Instruction::JumpIfZero(target) => {
                    if stack[height - 1] == 0 {
                        pc = target;
                    }
                }
                Instruction::Call(..) => {
                    return Err(EngineError::InvalidIr {
                        function: self.name().to_owned(),
                        instruction: pc,
                        message: "call instructions require ScalarProgram execution".to_owned(),
                    });
                }
                Instruction::Return => return Ok(stack[height - 1]),
            }
        }
    }
}
