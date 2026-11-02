//! Final instruction selection from the optimized local representation.

use super::local::{Action, Expression, Literal, LocalBlock, TempId};
use super::origins::{LoweredChunk, OpOrigin};
use super::scalar::{ScalarOp, SlotId};
use crate::bytecode::control_flow::BytecodeOffset;
use crate::{JitOp, Result, TinyLangError};

enum Emit {
    Value(TempId),
    Binary(ScalarOp, BytecodeOffset),
}

#[derive(Default)]
pub(super) struct LocalEncoder {
    emit: Vec<Emit>,
}

impl LocalEncoder {
    pub(super) fn encode(&mut self, block: &LocalBlock, ops: &mut LoweredChunk) -> Result<()> {
        self.emit.clear();
        for (action, &origin) in block.actions().iter().zip(block.action_origins()) {
            match *action {
                Action::DefineThrough(_) | Action::Eliminated => {}
                Action::Materialize(value) => self.emit_value(block, value, ops)?,
                Action::Discard(value) => {
                    self.emit_value(block, value, ops)?;
                    ops.push(JitOp::Pop, origin);
                }
                Action::Store { destination, value } => self.store(block, destination, value, origin, ops)?,
                Action::Branch { target, value } => self.branch(block, target, value, origin, ops)?,
                Action::Jump(target) => ops.push(JitOp::Jump(target), origin),
                Action::Effect(effect) => ops.push(block.effects[effect.index()], origin),
            }
        }
        Ok(())
    }

    fn store(
        &mut self,
        block: &LocalBlock,
        destination: SlotId,
        value: TempId,
        origin: OpOrigin,
        ops: &mut LoweredChunk,
    ) -> Result<()> {
        match block.expression(value) {
            Expression::Literal(Literal::Int(value)) => ops.push(JitOp::StoreInt(destination.index(), value), origin),
            Expression::Slot(source) => ops.push(JitOp::MoveSlot(source.slot.index(), destination.index()), origin),
            Expression::Binary { mode, lhs, rhs } => {
                if let Some(op) = direct_store(block, mode, lhs, rhs, destination) {
                    ops.push(op, block.expression_origin(value).with_store(origin.operation()));
                } else {
                    self.emit_value(block, value, ops)?;
                    ops.push(JitOp::Store(destination.index()), origin);
                }
            }
            _ => {
                self.emit_value(block, value, ops)?;
                ops.push(JitOp::Store(destination.index()), origin);
            }
        }
        Ok(())
    }

    fn branch(
        &mut self,
        block: &LocalBlock,
        target: usize,
        value: TempId,
        origin: OpOrigin,
        ops: &mut LoweredChunk,
    ) -> Result<()> {
        match block.expression(value) {
            Expression::Slot(read) => {
                ops.push(JitOp::JumpIfZeroSlot(read.slot.index(), target), block.expression_origin(value));
                return Ok(());
            }
            Expression::Binary { mode, lhs, rhs } => {
                if let (Expression::Slot(read), Expression::Literal(Literal::Int(immediate))) =
                    (block.expression(lhs), block.expression(rhs))
                    && let Some(op) =
                        JitOp::compare_slot_int_jump_if_zero(read.slot.index(), immediate, mode.opcode(), target)
                {
                    ops.push(op, block.expression_origin(value));
                    return Ok(());
                }
            }
            _ => {}
        }
        self.emit_value(block, value, ops)?;
        ops.push(JitOp::JumpIfZero(target), origin);
        Ok(())
    }

    /// Iterative postorder preserves left-to-right failure order without host
    /// recursion. Leaf-only blocks never allocate traversal storage.
    fn emit_value(&mut self, block: &LocalBlock, value: TempId, ops: &mut LoweredChunk) -> Result<()> {
        let origin = block.expression_origin(value);
        match block.expression(value) {
            Expression::Literal(value) => {
                ops.push(value.push(), origin);
                return Ok(());
            }
            Expression::Slot(read) => {
                ops.push(JitOp::Load(read.slot.index()), origin);
                return Ok(());
            }
            _ => {}
        }
        self.emit.push(Emit::Value(value));
        while let Some(next) = self.emit.pop() {
            let mut value = match next {
                Emit::Value(value) => value,
                Emit::Binary(mode, origin) => {
                    ops.push(mode.stack_op(), OpOrigin::instruction(origin));
                    continue;
                }
            };
            loop {
                let origin = block.expression_origin(value);
                match block.expression(value) {
                    Expression::Literal(value) => {
                        ops.push(value.push(), origin);
                        break;
                    }
                    Expression::Slot(read) => {
                        ops.push(JitOp::Load(read.slot.index()), origin);
                        break;
                    }
                    Expression::Binary { mode, lhs, rhs } => {
                        if let (Expression::Slot(read), Expression::Literal(Literal::Int(value))) =
                            (block.expression(lhs), block.expression(rhs))
                        {
                            match mode {
                                ScalarOp::Mul => {
                                    ops.push(JitOp::PushMulSlotInt(read.slot.index(), value), origin);
                                    break;
                                }
                                ScalarOp::Div => {
                                    ops.push(JitOp::PushDivSlotInt(read.slot.index(), value), origin);
                                    break;
                                }
                                _ => {}
                            }
                        }
                        self.emit.push(Emit::Binary(mode, origin.operation()));
                        self.emit.push(Emit::Value(rhs));
                        // The left child is next in postorder. Descend directly
                        // instead of pushing it only to pop it immediately.
                        value = lhs;
                    }
                    Expression::Copy { .. } | Expression::Stored { .. } => {
                        return Err(TinyLangError::compile("JIT unresolved local value"));
                    }
                }
            }
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn reserved_bytes(&self) -> usize {
        self.emit.capacity() * size_of::<Emit>()
    }
}

fn direct_store(block: &LocalBlock, mode: ScalarOp, lhs: TempId, rhs: TempId, destination: SlotId) -> Option<JitOp> {
    match (block.expression(lhs), block.expression(rhs)) {
        (Expression::Slot(lhs), Expression::Slot(rhs)) => {
            Some(JitOp::BinarySlotsStore(mode, lhs.slot, rhs.slot, destination))
        }
        (Expression::Slot(source), Expression::Literal(Literal::Int(value))) => {
            // These are encodings of a generalized expression, not adjacent
            // source patterns. Preserve the existing checked in-place paths.
            if source.slot == destination {
                match mode {
                    ScalarOp::Add => return Some(JitOp::AddSlotInt(source.slot.index(), value)),
                    ScalarOp::Sub => return Some(JitOp::SubSlotInt(source.slot.index(), value)),
                    ScalarOp::Mul => return Some(JitOp::MulSlotInt(source.slot.index(), value)),
                    ScalarOp::Div => return Some(JitOp::DivSlotInt(source.slot.index(), value)),
                    _ => {}
                }
            }
            Some(JitOp::BinarySlotIntStore(mode, source.slot, value, destination))
        }
        (Expression::Slot(source), Expression::Literal(Literal::Float(bits))) => {
            Some(JitOp::BinarySlotFloatStore(mode, source.slot, bits, destination))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jit::local::{LocalOptimizer, SlotRead};
    use crate::jit::origins::FailureStage;
    use crate::{Instr, Op};

    #[test]
    fn postorder_preserves_both_subtrees_and_origins_across_reuse() {
        let mut encoder = LocalEncoder::default();
        let mut block = LocalBlock::default();
        let cases = [
            [Op::Load, Op::Load, Op::Add, Op::Load, Op::Div, Op::Load, Op::Sub],
            [Op::Load, Op::Load, Op::Load, Op::Load, Op::Sub, Op::Div, Op::Add],
            [Op::Load, Op::Load, Op::Add, Op::Load, Op::Load, Op::Sub, Op::Div],
        ];
        for (round, code) in cases.into_iter().enumerate() {
            block.clear();
            let mut roots = Vec::new();
            let mut expected = Vec::new();
            let base = 65_536 + round * 100;
            for (index, op) in code.into_iter().enumerate() {
                block.set_origin(OpOrigin::instruction(BytecodeOffset::parse(base + index).expect("source site")));
                let instruction = Instr::new(op, (index % 4) as i64, 0);
                let expression = if let Some(mode) = ScalarOp::from_opcode(op) {
                    let rhs = roots.pop().expect("right fixture operand");
                    let lhs = roots.pop().expect("left fixture operand");
                    Expression::Binary { mode, lhs, rhs }
                } else {
                    Expression::Slot(SlotRead::new(SlotId::parse(instruction.arg).expect("fixture slot")))
                };
                roots.push(block.define(expression).expect("fixture expression"));
                expected.push(JitOp::from_instr(instruction).expect("reference instruction"));
            }
            let value = roots.pop().expect("fixture root");
            assert!(roots.is_empty());
            block.push_action(Action::Materialize(value));
            let mut lowered = LoweredChunk::default();
            encoder.encode(&block, &mut lowered).expect("postorder encoding");
            assert!(encoder.emit.is_empty());
            let (ops, origins) = lowered.finish();
            assert_eq!(ops, expected, "shape {round}");
            for index in 0..ops.len() {
                for stage in [FailureStage::Operation, FailureStage::Secondary, FailureStage::Store] {
                    assert_eq!(origins.instruction(index, stage), Some(base + index), "shape {round}");
                }
            }
        }
    }

    #[test]
    fn fused_children_resume_pending_siblings_before_the_parent() {
        let mut block = LocalBlock::default();
        let mut children = Vec::new();
        for (slot, mode, immediate) in [(0, ScalarOp::Mul, 3), (1, ScalarOp::Div, 0)] {
            let lhs = block
                .define(Expression::Slot(SlotRead::new(SlotId::parse(slot).expect("fixture slot"))))
                .expect("slot read");
            let rhs = block
                .define(Expression::Literal(Literal::Int(immediate)))
                .expect("immediate");
            let site = 20 + slot as usize * 30;
            block.set_origin(OpOrigin::instruction(BytecodeOffset::parse(site).expect("child site")));
            children.push(block.define(Expression::Binary { mode, lhs, rhs }).expect("child"));
        }
        block.set_origin(OpOrigin::instruction(BytecodeOffset::parse(60).expect("parent site")));
        let value = block
            .define(Expression::Binary {
                mode: ScalarOp::Sub,
                lhs:  children[0],
                rhs:  children[1],
            })
            .expect("parent");
        block.push_action(Action::Materialize(value));
        let mut encoder = LocalEncoder::default();
        let mut lowered = LoweredChunk::default();
        encoder.encode(&block, &mut lowered).expect("fused postorder");
        assert!(encoder.emit.is_empty());
        let (ops, origins) = lowered.finish();
        assert_eq!(ops, [JitOp::PushMulSlotInt(0, 3), JitOp::PushDivSlotInt(1, 0), JitOp::Sub]);
        for (index, site) in [20, 50, 60].into_iter().enumerate() {
            assert_eq!(origins.instruction(index, FailureStage::Operation), Some(site));
        }
    }

    #[test]
    fn action_rewrites_and_block_reuse_keep_origins_attached() {
        let mut block = LocalBlock::default();
        let mut optimizer = LocalOptimizer::default();
        let mut encoder = LocalEncoder::default();
        let destination = SlotId::parse(0).expect("destination");
        for (round, (condition, target)) in [(false, 65_536), (true, 262_143), (false, 262_143)]
            .into_iter()
            .enumerate()
        {
            block.clear();
            let offset = |index| BytecodeOffset::parse(round * 100 + index).expect("source site");
            for (site, literal) in [(11, 1), (22, 2)] {
                block.set_origin(OpOrigin::instruction(offset(site)));
                let value = block
                    .define(Expression::Literal(Literal::Int(literal)))
                    .expect("literal");
                block.push_action(Action::Store { destination, value });
            }
            block.set_origin(OpOrigin::instruction(offset(30)));
            let value = block
                .define(Expression::Literal(Literal::Bool(condition)))
                .expect("condition");
            block.set_origin(OpOrigin::instruction(offset(31)));
            block.push_action(Action::Branch { target, value });
            block.set_origin(OpOrigin::fused(offset(40), offset(41), offset(42)));
            block.effect(JitOp::Halt).expect("opaque effect");
            optimizer.optimize(&mut block).expect("action rewrites");

            let mut lowered = LoweredChunk::default();
            encoder.encode(&block, &mut lowered).expect("encode optimized block");
            let (ops, origins) = lowered.finish();
            assert_eq!(ops[0], JitOp::StoreInt(0, 2), "the superseded store is eliminated");
            let effect = if condition {
                assert_eq!(ops.len(), 2, "the constant true branch is eliminated");
                1
            } else {
                assert_eq!(ops, [JitOp::StoreInt(0, 2), JitOp::Jump(target), JitOp::Halt]);
                assert_eq!(origins.instruction(1, FailureStage::Operation), Some(offset(31).index()));
                2
            };
            assert_eq!(ops[effect], JitOp::Halt);
            for stage in [FailureStage::Operation, FailureStage::Secondary, FailureStage::Store] {
                assert_eq!(origins.instruction(0, stage), Some(offset(22).index()));
            }
            for (stage, site) in [
                (FailureStage::Operation, 40),
                (FailureStage::Secondary, 41),
                (FailureStage::Store, 42),
            ] {
                assert_eq!(origins.instruction(effect, stage), Some(offset(site).index()));
            }
        }
    }

    #[test]
    fn float_store_encoding_preserves_literal_bits_and_has_no_stack_traffic() {
        let source = SlotId::parse(65_535).expect("maximum slot");
        let destination = SlotId::parse(0).expect("first slot");
        for bits in [0, 1, (-0.0f64).to_bits(), 0x7ff8_0000_0000_0042, 0xfff0_0000_0000_0084] {
            let mut block = LocalBlock::default();
            let lhs = block.define(Expression::Slot(SlotRead::new(source))).expect("source");
            let rhs = block
                .define(Expression::Literal(Literal::Float(bits)))
                .expect("literal");
            let op = direct_store(&block, ScalarOp::Add, lhs, rhs, destination).expect("direct float store");
            assert_eq!(op, JitOp::BinarySlotFloatStore(ScalarOp::Add, source, bits, destination));
            assert_eq!(op.operand_stack_traffic(), (0, 0));
            assert_eq!(op.profile_name(), "binary.slot.f.store");
            assert!(op.sampling().is_none(), "fusion must not imply adaptive operand types");
        }
    }
}
