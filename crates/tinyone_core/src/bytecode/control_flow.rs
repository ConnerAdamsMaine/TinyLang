//! Compact block boundaries and canonical stack states retained by verification.

use std::fmt;
use std::num::NonZeroU32;
use std::ops::Range;

use crate::{Instr, Op, Result, TinyOneError};

/// Bytecode offsets include the exclusive chunk end. A u16 cannot represent
/// the verifier's full 262,144-instruction budget.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct BytecodeOffset(u32);

impl BytecodeOffset {
    pub(crate) fn parse(index: usize) -> Result<Self> {
        u32::try_from(index)
            .map(Self)
            .map_err(|_| TinyOneError::compile("Verifier: bytecode offset exceeds u32"))
    }

    pub(crate) fn index(self) -> usize {
        self.0 as usize
    }
}

/// Encoding depth + 1 reserves zero for unreachable instructions while still
/// representing the maximum valid depth of 65,536 in four bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StackDepth(pub(super) NonZeroU32);

impl StackDepth {
    pub(super) const ZERO: Self = Self(NonZeroU32::MIN);

    pub(super) fn value(self) -> i64 {
        i64::from(self.0.get() - 1)
    }

    pub(crate) fn count(self) -> usize {
        (self.0.get() - 1) as usize
    }
}

impl fmt::Display for StackDepth {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.value().fmt(formatter)
    }
}

#[derive(Debug)]
pub(crate) struct BasicBlock {
    start:                  BytecodeOffset,
    end:                    BytecodeOffset,
    /// None means unreachable, not an assumed empty incoming stack.
    pub(crate) entry_depth: Option<StackDepth>,
}

impl BasicBlock {
    pub(crate) fn range(&self) -> Range<usize> {
        self.start.index()..self.end.index()
    }
}

/// Only the distinction needed to identify block starts. Both conditional
/// and unconditional branches end a block and contribute a target.
pub(super) enum InstructionFlow {
    Fallthrough,
    Branch(BytecodeOffset),
    Terminal,
}

impl InstructionFlow {
    pub(super) fn parse(instr: Instr, pc: usize, code_len: usize, chunk_name: &str) -> Result<Self> {
        match instr.op {
            Op::Jump | Op::JumpIfZero => {
                let target = usize::try_from(instr.arg)
                    .ok()
                    .filter(|&target| target < code_len)
                    .ok_or_else(|| {
                        TinyOneError::compile(format!(
                            "Verifier: instruction {pc} in {chunk_name} targets {}",
                            instr.arg
                        ))
                    })?;
                Ok(Self::Branch(BytecodeOffset::parse(target)?))
            }
            Op::Return | Op::Halt => Ok(Self::Terminal),
            _ => Ok(Self::Fallthrough),
        }
    }
}

pub(super) struct BlockStarts {
    starts: Vec<BytecodeOffset>,
    end:    BytecodeOffset,
}

impl BlockStarts {
    pub(super) fn new(code_len: usize) -> Result<Self> {
        Ok(Self {
            // Entry zero is implicit. Straight-line chunks need no temporary
            // leader allocation before their single boxed block is installed.
            starts: Vec::new(),
            end:    BytecodeOffset::parse(code_len)?,
        })
    }

    pub(super) fn record(&mut self, pc: usize, flow: InstructionFlow) -> Result<()> {
        match flow {
            InstructionFlow::Fallthrough => return Ok(()),
            InstructionFlow::Branch(target) => {
                if target.index() != 0 {
                    self.starts.push(target);
                }
            }
            InstructionFlow::Terminal => {}
        }
        if pc + 1 < self.end.index() {
            self.starts.push(BytecodeOffset::parse(pc + 1)?);
        }
        Ok(())
    }

    pub(super) fn finish(mut self, depths: &[Option<StackDepth>]) -> Box<[BasicBlock]> {
        if self.starts.is_empty() {
            return Box::new([BasicBlock {
                start:       BytecodeOffset(0),
                end:         self.end,
                entry_depth: depths[0],
            }]);
        }
        self.starts.push(BytecodeOffset(0));
        self.starts.sort_unstable();
        self.starts.dedup();
        self.starts
            .iter()
            .copied()
            .enumerate()
            .map(|(index, start)| {
                BasicBlock {
                    start,
                    end: self.starts.get(index + 1).copied().unwrap_or(self.end),
                    entry_depth: depths[start.index()],
                }
            })
            .collect()
    }
}

/// Isolated encoding fixtures intentionally omit stack verification. They can
/// exercise block partitioning but cannot create an executable verified token.
#[cfg(test)]
pub(crate) fn blocks_for_encoding_test(code: &[Instr]) -> Result<Box<[BasicBlock]>> {
    let mut starts = BlockStarts::new(code.len())?;
    for (pc, instr) in code.iter().copied().enumerate() {
        starts.record(pc, InstructionFlow::parse(instr, pc, code.len(), "encoding fixture")?)?;
    }
    // These small encoding-only fixtures sometimes begin mid-expression.
    // Synthetic incoming values let the lowerer account for their pops; this
    // deliberately does not claim the fixture is an executable program.
    let encoded = NonZeroU32::new(u32::try_from(code.len() + 1).expect("small encoding fixture"))
        .expect("nonzero synthetic depth");
    Ok(starts.finish(&vec![Some(StackDepth(encoded)); code.len()]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Function, Program, VerifiedProgram};

    #[test]
    fn certificate_retains_nonempty_merges_and_unreachable_block_states() {
        let program = Program::new(
            vec![
                Instr::new(Op::PushInt, 42, 0),
                Instr::new(Op::PushInt, 1, 0),
                Instr::new(Op::JumpIfZero, 6, 0),
                Instr::new(Op::PushInt, 3, 0),
                Instr::new(Op::Jump, 7, 0),
                Instr::new(Op::Halt, 0, 0),
                Instr::new(Op::PushInt, 5, 0),
                Instr::new(Op::Add, 0, 0),
                Instr::new(Op::Print, 0, 0),
                Instr::new(Op::Halt, 0, 0),
            ],
            0,
        )
        .with_functions(vec![Function::new(
            "early_return",
            0,
            vec![
                Instr::new(Op::PushInt, 1, 0),
                Instr::new(Op::Return, 0, 0),
                Instr::new(Op::PushInt, 2, 0),
                Instr::new(Op::Return, 0, 0),
            ],
            0,
        )]);
        let verified = VerifiedProgram::verify(program).expect("valid joins and early return");
        let main = verified.chunk_certificate(0).expect("main certificate");
        let blocks: Vec<_> = main
            .blocks
            .iter()
            .map(|block| (block.range(), block.entry_depth.map(StackDepth::count)))
            .collect();
        assert_eq!(
            blocks,
            [
                (0..3, Some(0)),
                (3..5, Some(1)),
                (5..6, None),
                (6..7, Some(1)),
                (7..10, Some(2))
            ]
        );
        let function = verified.chunk_certificate(1).expect("function certificate");
        assert_eq!(function.blocks.len(), 2);
        assert_eq!(function.blocks[0].range(), 0..2);
        assert_eq!(function.blocks[0].entry_depth.map(StackDepth::count), Some(0));
        assert_eq!(function.blocks[1].range(), 2..4);
        assert_eq!(function.blocks[1].entry_depth, None);
    }

    #[test]
    fn block_offsets_cover_the_full_verifier_budget_without_narrowing() {
        const LENGTH: usize = 262_144;
        let mut code = vec![Instr::new(Op::Halt, 0, 0); LENGTH];
        code[0] = Instr::new(Op::Jump, (LENGTH - 1) as i64, 0);
        let verified = VerifiedProgram::verify(Program::new(code, 0)).expect("maximum instruction budget");
        let certificate = verified.chunk_certificate(0).expect("certificate");
        assert_eq!(certificate.blocks.len(), LENGTH);
        let last = certificate.blocks.last().expect("last block");
        assert_eq!(last.range(), LENGTH - 1..LENGTH);
        assert_eq!(last.entry_depth.map(StackDepth::count), Some(0));
        assert_eq!(size_of::<BasicBlock>(), 12);
        assert_eq!(size_of::<Option<StackDepth>>(), 4);
    }
}
