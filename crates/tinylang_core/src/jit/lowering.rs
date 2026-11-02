//! Build block-local symbolic values and an ordered action stream. Optimization
//! and final encoding run only after the block's canonical stack is established.

use super::encoding::LocalEncoder;
use super::local::{Action, Expression, Literal, LocalBlock, LocalOptimizer, SlotRead, TempId};
use super::origins::{LoweredChunk, OpOrigin};
#[cfg(feature = "testing-hooks")]
use super::preparation_profile::{PreparationPhase, PreparationSpan};
use super::scalar::{ScalarOp, SlotId};
use crate::bytecode::control_flow::StackDepth;
use crate::{Instr, JitOp, Op, Result, TinyLangError, checked_non_negative_usize};

#[cfg(test)]
mod storage_tests;

/// All scratch buffers are reused between blocks and die at installation.
/// Incoming values stay physical; roots are the deferred values above them.
#[derive(Default)]
pub(super) struct SymbolicLowerer {
    block:          LocalBlock,
    roots:          Vec<TempId>,
    optimizer:      LocalOptimizer,
    encoder:        LocalEncoder,
    physical_depth: usize,
}

impl SymbolicLowerer {
    pub(super) fn set_origin(&mut self, origin: OpOrigin) {
        self.block.set_origin(origin);
    }

    pub(super) fn begin_block(&mut self, entry_depth: StackDepth) {
        self.block.clear();
        self.roots.clear();
        self.physical_depth = entry_depth.count();
    }

    fn push(&mut self, expression: Expression) -> Result<()> {
        self.roots.push(self.block.define(expression)?);
        Ok(())
    }

    pub(super) fn instruction(&mut self, instr: Instr) -> Result<()> {
        if let Some(mode) = ScalarOp::from_opcode(instr.op) {
            return self.binary(mode);
        }
        match instr.op {
            Op::PushInt => self.push(Expression::Literal(Literal::Int(instr.arg))),
            Op::PushBool => self.push(Expression::Literal(Literal::Bool(instr.arg != 0))),
            Op::PushFloat => self.push(Expression::Literal(Literal::Float(instr.arg as u64))),
            Op::PushNull => self.push(Expression::Literal(Literal::Null)),
            Op::Load => self.push(Expression::Slot(SlotRead::new(SlotId::parse(instr.arg)?))),
            Op::Store => {
                let destination = SlotId::parse(instr.arg)?;
                let Some(value) = self.roots.pop() else {
                    return self.effect(JitOp::Store(destination.index()));
                };
                // Snapshot older reads and execute older possible failures
                // before this write. Facts may still refer to their nodes.
                self.flush();
                self.block.push_action(Action::Store { destination, value });
                Ok(())
            }
            Op::Pop => {
                let Some(value) = self.roots.pop() else {
                    return self.effect(JitOp::Pop);
                };
                if matches!(self.block.expression(value), Expression::Binary { .. }) {
                    self.flush();
                }
                self.block.push_action(Action::Discard(value));
                Ok(())
            }
            Op::JumpIfZero => {
                let target = checked_non_negative_usize(instr.arg, "JIT branch target")?;
                let Some(value) = self.roots.pop() else {
                    return self.effect(JitOp::JumpIfZero(target));
                };
                self.flush();
                self.block.push_action(Action::Branch { target, value });
                Ok(())
            }
            // Calls, globals, heap/IO and unsupported scalar forms are opaque
            // effects: neither pending evaluations nor slot facts cross them.
            _ => self.effect(JitOp::from_instr(instr)?),
        }
    }

    fn binary(&mut self, mode: ScalarOp) -> Result<()> {
        if self.roots.len() < 2 {
            return self.effect(mode.stack_op());
        }
        // The length branch proves these indexed roots exist; no references
        // escape. Truncating consumes them without a per-operation allocation.
        let end = self.roots.len();
        let lhs = self.roots[end - 2];
        let rhs = self.roots[end - 1];
        self.roots.truncate(end - 2);
        self.push(Expression::Binary { mode, lhs, rhs })
    }

    fn flush(&mut self) {
        for value in &self.roots {
            self.block.push_action(Action::Materialize(*value));
        }
        self.physical_depth += self.roots.len();
        self.roots.clear();
    }

    pub(super) fn effect(&mut self, op: JitOp) -> Result<()> {
        self.flush();
        let (pushes, pops) = op.operand_stack_traffic();
        self.physical_depth = self
            .physical_depth
            .checked_sub(pops)
            .ok_or_else(|| TinyLangError::compile("JIT symbolic lowering disagrees with verified stack depth"))?
            + pushes;
        self.block.effect(op)
    }

    pub(super) fn finish(&mut self, ops: &mut LoweredChunk) -> Result<()> {
        #[cfg(feature = "testing-hooks")]
        let finalization = PreparationSpan::start(PreparationPhase::StackFinalization);
        self.flush();
        #[cfg(feature = "testing-hooks")]
        drop(finalization);
        self.optimizer.optimize(&mut self.block)?;
        #[cfg(feature = "testing-hooks")]
        let _encoding = PreparationSpan::start(PreparationPhase::Encoding);
        self.encoder.encode(&self.block, ops)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::control_flow::BytecodeOffset;
    use crate::{Program, VerifiedProgram};

    #[test]
    #[ignore = "symbolic-builder storage diagnostic; run explicitly with --ignored --nocapture"]
    fn report_symbolic_builder_layout() {
        eprintln!(
            "symbolic layout: Expression={} TempId={} Action={} Builder={}",
            size_of::<Expression>(),
            size_of::<TempId>(),
            size_of::<Action>(),
            size_of::<SymbolicLowerer>()
        );
        for count in [4, 1_024, 65_536] {
            let mut code = Vec::with_capacity(2 * count + 3);
            code.push(Instr::new(Op::PushInt, 0, 0));
            for _ in 0..count {
                code.push(Instr::new(Op::Load, 0, 0));
                code.push(Instr::new(Op::Add, 0, 0));
            }
            code.push(Instr::new(Op::Print, 0, 0));
            code.push(Instr::new(Op::Halt, 0, 0));
            let verified = VerifiedProgram::verify(Program::new(code, 1)).expect("deep fixture");
            let certificate = verified.chunk_certificate(0).expect("certificate");
            let mut lowerer = SymbolicLowerer::default();
            lowerer.begin_block(certificate.blocks[0].entry_depth.expect("reachable entry"));
            let mut ops = LoweredChunk::default();
            for (pc, instr) in verified.program().code.iter().enumerate() {
                lowerer.set_origin(OpOrigin::instruction(BytecodeOffset::parse(pc).expect("fixture origin")));
                lowerer.instruction(*instr).expect("lowering");
            }
            lowerer.finish(&mut ops).expect("optimization and encoding");
            eprintln!(
                "depth={count}: nodes={} expressions_reserved={} actions_reserved={} effects_reserved={} origins_reserved={} roots_reserved={} traversal_reserved={} facts_reserved={} ops={}",
                lowerer.block.expressions.len(),
                lowerer.block.expressions.capacity() * size_of::<Expression>(),
                lowerer.block.reserved_action_bytes(),
                lowerer.block.effects.capacity() * size_of::<JitOp>(),
                lowerer.block.reserved_origin_bytes(),
                lowerer.roots.capacity() * size_of::<TempId>(),
                lowerer.encoder.reserved_bytes(),
                lowerer.optimizer.reserved_bytes(),
                ops.len()
            );
        }
    }
}
