use std::collections::VecDeque;

use super::{BinaryOp, EngineError, Instruction};

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScalarType {
    I64,
    Bool,
    I64OrBool,
}

#[derive(Clone)]
struct FrameTypes {
    slots: Vec<ScalarType>,
    stack: Vec<ScalarType>,
}

pub(super) fn check(name: &str, slots: usize, max_stack: usize, code: &[Instruction]) -> Result<(), EngineError> {
    let invalid = |instruction, message: &str| {
        EngineError::InvalidIr {
            function: name.to_owned(),
            instruction,
            message: message.to_owned(),
        }
    };
    if code.len().saturating_mul(slots + max_stack) > 4_194_304 {
        return Err(invalid(0, "scalar type-flow state exceeds 4194304 cells"));
    }
    let mut entries: Vec<Option<FrameTypes>> = vec![None; code.len()];
    entries[0] = Some(FrameTypes {
        slots: vec![ScalarType::I64; slots],
        stack: Vec::new(),
    });
    let mut pending = VecDeque::from([0]);
    while let Some(pc) = pending.pop_front() {
        let mut state = entries[pc]
            .as_ref()
            .ok_or_else(|| invalid(pc, "missing type-flow entry"))?
            .clone();
        let require_integer = |value| {
            if value != ScalarType::I64 {
                Err(invalid(pc, "operation requires i64; bool is not implicitly converted"))
            } else {
                Ok(())
            }
        };
        let pop = |stack: &mut Vec<ScalarType>| {
            stack
                .pop()
                .ok_or_else(|| invalid(pc, "type-flow operand stack underflow"))
        };
        match code[pc] {
            Instruction::Constant(_) => state.stack.push(ScalarType::I64),
            Instruction::Load(slot) => state.stack.push(state.slots[slot]),
            Instruction::Store(slot) => state.slots[slot] = pop(&mut state.stack)?,
            Instruction::Binary(op) => {
                require_integer(pop(&mut state.stack)?)?;
                require_integer(pop(&mut state.stack)?)?;
                state.stack.push(match op {
                    BinaryOp::Add | BinaryOp::Subtract | BinaryOp::Multiply | BinaryOp::Divide => ScalarType::I64,
                    _ => ScalarType::Bool,
                });
            }
            Instruction::Negate => {
                require_integer(pop(&mut state.stack)?)?;
                state.stack.push(ScalarType::I64);
            }
            Instruction::Pop | Instruction::JumpIfZero(_) => {
                pop(&mut state.stack)?;
            }
            Instruction::Jump(_) => {}
            Instruction::Return => {
                require_integer(pop(&mut state.stack)?)?;
                continue;
            }
        }
        let mut successor = |target: usize| -> Result<(), EngineError> {
            let entry = entries
                .get_mut(target)
                .ok_or_else(|| invalid(pc, "type-flow successor leaves function"))?;
            match entry {
                None => {
                    *entry = Some(state.clone());
                    pending.push_back(target);
                }
                Some(previous) => {
                    if previous.stack.len() != state.stack.len() {
                        return Err(invalid(target, "inconsistent type-flow stack heights"));
                    }
                    let mut changed = false;
                    for (old, new) in previous
                        .slots
                        .iter_mut()
                        .zip(&state.slots)
                        .chain(previous.stack.iter_mut().zip(&state.stack))
                    {
                        if *old != *new && *old != ScalarType::I64OrBool {
                            *old = ScalarType::I64OrBool;
                            changed = true;
                        }
                    }
                    if changed {
                        pending.push_back(target);
                    }
                }
            }
            Ok(())
        };
        match code[pc] {
            Instruction::Jump(target) => successor(target)?,
            Instruction::JumpIfZero(target) => {
                successor(target)?;
                successor(pc + 1)?;
            }
            _ => successor(pc + 1)?,
        }
    }
    Ok(())
}
