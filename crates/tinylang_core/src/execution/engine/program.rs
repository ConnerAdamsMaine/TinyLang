//! Program-level checked scalar capability: direct function calls.
//!
//! [`ScalarProgram::lower`] lowers one entry function together with its entire
//! static call closure. Every function in that closure must be a checked-i64
//! scalar function; a single unsupported instruction anywhere in the closure
//! rejects the whole entry, because a native call chain cannot execute a
//! callee the checked profile refused to certify. This mirrors the
//! [`ScalarFunction`] contract, which only lowers leaf functions.
//!
//! [`ScalarProgram::interpret`] is the dynamic reference implementation used
//! for differential testing against the authoritative VM and against machine
//! backends. Semantics come from the VM:
//! * one fuel unit per executed IR instruction, shared across the entire call
//!   chain (the VM budget behaves the same way for direct calls),
//! * nested calls are rejected when they would exceed the program's declared
//!   call depth, exactly like the VM's `call_depth >= limit` check,
//! * arguments leave the caller in operand-stack order (the deepest value is
//!   argument zero), matching `pop_args` in the VM's `Op::Call`,
//! * a scalar callee always returns one value, so the result replaces the
//!   deepest popped argument on the caller's operand stack.
//!
//! The checked profile only passes integer values between functions: argument
//! values must be `I64` in the type-flow analysis, so Bool comparison results
//! cannot become call arguments even though their machine representation
//! (0/1) would allow silent passage.

use super::{EngineError, ScalarFunction, TrapKind};

#[derive(Clone, Debug)]
pub struct ScalarProgram {
    entry: usize,
    /// All functions reachable from `entry`, indexed by program function index.
    functions: Vec<Option<ScalarFunction>>,
    call_depth_limit: usize,
}

impl ScalarProgram {
    pub fn lower(program: &crate::VerifiedProgram, entry: usize) -> Result<Self, EngineError> {
        let count = program.program().functions().len();
        if entry >= count {
            return Err(EngineError::UnknownFunction(format!("#{entry}")));
        }
        let mut functions: Vec<Option<ScalarFunction>> = vec![None; count];
        let mut pending = vec![entry];
        while let Some(index) = pending.pop() {
            if functions[index].is_some() {
                continue;
            }
            let function = ScalarFunction::lower_with_calls(program, index)?;
            let targets: Vec<usize> = function
                .instructions()
                .iter()
                .filter_map(|instruction| match instruction {
                    &super::Instruction::Call(callee, _) => Some(callee),
                    _ => None,
                })
                .collect();
            // Callees are lowered before the caller result is published: any
            // failing target aborts `lower` before a ScalarProgram exists.
            for callee in targets {
                if functions[callee].is_none() {
                    pending.push(callee);
                }
            }
            functions[index] = Some(function);
        }
        Ok(Self {
            entry,
            functions,
            call_depth_limit: program.program().max_call_depth(),
        })
    }

    pub fn entry(&self) -> usize {
        self.entry
    }

    /// Number of program function slots: lowered functions and holes.
    pub fn function_count(&self) -> usize {
        self.functions.len()
    }

    pub fn function(&self, index: usize) -> Option<&ScalarFunction> {
        self.functions.get(index).and_then(Option::as_ref)
    }

    pub fn call_depth_limit(&self) -> usize {
        self.call_depth_limit
    }

    /// Reference interpreter mirroring single-function `ScalarFunction::interpret`.
    pub fn interpret(&self, args: &[i64], fuel: u64) -> Result<i64, EngineError> {
        self.execute(self.entry, args, &mut fuel.clone(), 0)
    }

    fn execute(&self, index: usize, args: &[i64], fuel: &mut u64, depth: usize) -> Result<i64, EngineError> {
        let function = self
            .function(index)
            .ok_or_else(|| EngineError::UnknownFunction(format!("#{index}")))?;
        let (mut slots, mut stack) = function.frame(args)?;
        let mut pc = 0usize;
        loop {
            let instruction = pc;
            let trap = |kind| EngineError::Trap { kind, instruction };
            if *fuel == 0 {
                return Err(trap(TrapKind::FuelExhausted));
            }
            *fuel -= 1;
            // Lowering proves stack heights, operands and successor indices.
            let height = function.stack_heights()[instruction].ok_or_else(|| EngineError::InvalidIr {
                function: function.name().to_owned(),
                instruction,
                message: "execution reached an unreachable instruction".to_owned(),
            })?;
            pc += 1;
            match function.instructions()[instruction] {
                super::Instruction::Constant(value) => stack[height] = value,
                super::Instruction::Load(slot) => stack[height] = slots[slot],
                super::Instruction::Store(slot) => slots[slot] = stack[height - 1],
                super::Instruction::Pop => {}
                super::Instruction::Negate => {
                    stack[height - 1] = stack[height - 1].checked_neg().ok_or(trap(TrapKind::Overflow))?;
                }
                super::Instruction::Binary(op) => {
                    let lhs = stack[height - 2];
                    let rhs = stack[height - 1];
                    stack[height - 2] = super::interpreter::checked_binary(op, lhs, rhs).map_err(trap)?;
                }
                super::Instruction::Call(callee, argument_count) => {
                    if depth + 1 > self.call_depth_limit {
                        return Err(trap(TrapKind::CallStackOverflow(self.call_depth_limit)));
                    }
                    let arguments = &stack[height - argument_count..height];
                    stack[height - argument_count] = self.execute(callee, arguments, fuel, depth + 1)?;
                }
                super::Instruction::Jump(target) => pc = target,
                super::Instruction::JumpIfZero(target) => {
                    if stack[height - 1] == 0 {
                        pc = target;
                    }
                }
                super::Instruction::Return => return Ok(stack[height - 1]),
            }
        }
    }
}
