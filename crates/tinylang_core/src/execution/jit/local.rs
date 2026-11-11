//! Ordered, block-local optimization over symbolic definitions and actions.
//! Definitions retain source order even when their physical emission is deferred.

use std::num::NonZeroU32;

use super::origins::OpOrigin;
#[cfg(feature = "testing-hooks")]
use super::preparation_profile::{PreparationPhase, PreparationSpan};
use super::scalar::{ScalarOp, SlotId};
use crate::language::bytecode::control_flow::BytecodeOffset;
use crate::{JitOp, Result, TinyLangError, floor_div};

#[derive(Debug, Clone, Copy)]
pub(super) struct TempId(NonZeroU32);

impl TempId {
    fn parse(index: usize) -> Result<Self> {
        index
            .checked_add(1)
            .and_then(|value| u32::try_from(value).ok())
            .and_then(NonZeroU32::new)
            .map(Self)
            .ok_or_else(|| TinyLangError::compile("JIT temporary index exceeds u32"))
    }

    pub(super) fn index(self) -> usize {
        (self.0.get() - 1) as usize
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum Literal {
    Int(i64),
    Bool(bool),
    Float(u64),
    Null,
}

impl Literal {
    pub(super) fn push(self) -> JitOp {
        match self {
            Self::Int(value) => JitOp::PushInt(value),
            Self::Bool(value) => JitOp::PushBool(value),
            Self::Float(bits) => JitOp::PushFloat(bits),
            Self::Null => JitOp::PushNull,
        }
    }

    fn is_false(self) -> bool {
        match self {
            Self::Int(value) => value == 0,
            Self::Bool(value) => !value,
            Self::Float(bits) => f64::from_bits(bits) == 0.0,
            Self::Null => true,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct SlotVersion(u32);

#[derive(Debug, Clone, Copy)]
pub(super) struct SlotRead {
    pub(super) slot: SlotId,
    version: SlotVersion,
}

impl SlotRead {
    pub(super) fn new(slot: SlotId) -> Self {
        Self {
            slot,
            version: SlotVersion::default(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum Expression {
    Literal(Literal),
    Slot(SlotRead),
    Binary { mode: ScalarOp, lhs: TempId, rhs: TempId },
    // A previous store computed this value. Folding may discover a constant;
    // otherwise read its saved slot, never re-evaluate the old expression.
    Stored { source: TempId, fallback: SlotRead },
    // Copy propagation proposes an alias. The version pass either proves the
    // original read still current or restores a read of the destination slot.
    Copy { source: TempId, fallback: SlotId },
}

#[derive(Debug, Clone, Copy)]
pub(super) struct EffectId(u32);

impl EffectId {
    pub(super) fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum Action {
    // All definitions after the previous run, through this ID, occur before
    // the next action. No store, observation or effect is hidden in a run.
    DefineThrough(TempId),
    Materialize(TempId),
    Discard(TempId),
    Store { destination: SlotId, value: TempId },
    Branch { target: usize, value: TempId },
    Jump(usize),
    Effect(EffectId),
    Eliminated,
}

/// Growable only during lowering. All scratch allocations die at installation;
/// large opaque JitOps stay out of the common definition/action records.
#[derive(Default)]
pub(super) struct LocalBlock {
    pub(super) expressions: Vec<Expression>,
    pub(super) effects: Vec<JitOp>,
    actions: Vec<Action>,
    pending_definition: Option<TempId>,
    expression_origins: Vec<BytecodeOffset>,
    action_origins: Vec<OpOrigin>,
    origin: OpOrigin,
}

impl LocalBlock {
    pub(super) fn clear(&mut self) {
        self.expressions.clear();
        self.actions.clear();
        self.effects.clear();
        self.expression_origins.clear();
        self.action_origins.clear();
        self.pending_definition = None;
    }

    pub(super) fn define(&mut self, expression: Expression) -> Result<TempId> {
        let value = TempId::parse(self.expressions.len())?;
        self.expressions.push(expression);
        self.expression_origins.push(self.origin.operation());
        self.pending_definition = Some(value);
        Ok(value)
    }

    fn finish_definitions(&mut self) {
        if let Some(last) = self.pending_definition.take() {
            self.actions.push(Action::DefineThrough(last));
            self.action_origins.push(self.origin);
        }
    }

    pub(super) fn push_action(&mut self, action: Action) {
        self.finish_definitions();
        self.actions.push(action);
        self.action_origins.push(self.origin);
    }

    pub(super) fn actions(&self) -> &[Action] {
        &self.actions
    }

    pub(super) fn set_origin(&mut self, origin: OpOrigin) {
        self.origin = origin;
    }

    pub(super) fn action_origins(&self) -> &[OpOrigin] {
        &self.action_origins
    }

    pub(super) fn expression_origin(&self, value: TempId) -> OpOrigin {
        OpOrigin::instruction(self.expression_origins[value.index()])
    }

    pub(super) fn effect(&mut self, op: JitOp) -> Result<()> {
        let index =
            u32::try_from(self.effects.len()).map_err(|_| TinyLangError::compile("JIT effect index exceeds u32"))?;
        self.effects.push(op);
        self.push_action(Action::Effect(EffectId(index)));
        Ok(())
    }

    pub(super) fn expression(&self, value: TempId) -> Expression {
        self.expressions[value.index()]
    }

    #[cfg(test)]
    pub(super) fn reserved_action_bytes(&self) -> usize {
        self.actions.capacity() * size_of::<Action>()
    }

    #[cfg(test)]
    pub(super) fn reserved_origin_bytes(&self) -> usize {
        self.expression_origins.capacity() * size_of::<BytecodeOffset>()
            + self.action_origins.capacity() * size_of::<OpOrigin>()
    }

    #[cfg(test)]
    pub(super) fn reserved_origin_extents(&self) -> [usize; 2] {
        [
            self.expression_origins.capacity() * size_of::<BytecodeOffset>(),
            self.action_origins.capacity() * size_of::<OpOrigin>(),
        ]
    }
}

#[derive(Default, Clone, Copy)]
struct SlotFact {
    version: SlotVersion,
    value: Option<TempId>,
}

/// Generation-stamped dense facts avoid clearing all 65,536 possible slots
/// at every effect/block/pass. No fact or version escapes this chunk builder.
#[derive(Default)]
pub(super) struct LocalOptimizer {
    slots: Vec<SlotFact>,
    clock: SlotVersion,
    epoch: SlotVersion,
}

impl LocalOptimizer {
    pub(super) fn optimize(&mut self, block: &mut LocalBlock) -> Result<()> {
        #[cfg(feature = "testing-hooks")]
        let mut phase = PreparationSpan::start(PreparationPhase::DefinitionFinalization);
        block.finish_definitions();
        #[cfg(feature = "testing-hooks")]
        phase.next(PreparationPhase::ConstantPropagation);
        self.propagate_constants(block)?;
        #[cfg(feature = "testing-hooks")]
        phase.next(PreparationPhase::CopyPropagation);
        match propagate_copies(block) {
            CopyProposals::NoAliases => {}
            CopyProposals::NeedsVersions => {
                #[cfg(feature = "testing-hooks")]
                phase.next(PreparationPhase::SlotVersionPropagation);
                self.propagate_slot_versions(block)?;
            }
        }
        #[cfg(feature = "testing-hooks")]
        phase.next(PreparationPhase::ConstantFolding);
        fold_constants(block);
        #[cfg(feature = "testing-hooks")]
        phase.next(PreparationPhase::BranchFolding);
        fold_branches(block);
        #[cfg(feature = "testing-hooks")]
        phase.next(PreparationPhase::DeadTemporaries);
        eliminate_dead_temporaries(block);
        #[cfg(feature = "testing-hooks")]
        phase.next(PreparationPhase::DeadStores);
        eliminate_dead_stores(block);
        // Final fusion belongs to LocalEncoder, after these ordered passes.
        Ok(())
    }

    fn tick(&mut self) -> Result<SlotVersion> {
        self.clock.0 = self
            .clock
            .0
            .checked_add(1)
            .ok_or_else(|| TinyLangError::compile("JIT slot version exceeds u32"))?;
        Ok(self.clock)
    }

    fn invalidate(&mut self) -> Result<()> {
        self.epoch = self.tick()?;
        Ok(())
    }

    fn assignment(&self, slot: SlotId) -> Option<TempId> {
        self.slots
            .get(slot.index())
            .filter(|fact| fact.version >= self.epoch)
            .and_then(|fact| fact.value)
    }

    fn read(&self, slot: SlotId) -> SlotRead {
        let version = self
            .slots
            .get(slot.index())
            .map_or(self.epoch, |fact| fact.version.max(self.epoch));
        SlotRead { slot, version }
    }

    fn assign(&mut self, slot: SlotId, value: TempId) -> Result<()> {
        let version = self.tick()?;
        // SlotId was parsed from verifier-bounded input. Reads of untouched
        // high slots do not allocate this table; only assignments grow it.
        if self.slots.len() <= slot.index() {
            self.slots.resize(slot.index() + 1, SlotFact::default());
        }
        self.slots[slot.index()] = SlotFact {
            version,
            value: Some(value),
        };
        Ok(())
    }

    fn propagate_constants(&mut self, block: &mut LocalBlock) -> Result<()> {
        self.invalidate()?;
        let mut next_definition = 0;
        for action in &block.actions {
            match *action {
                Action::DefineThrough(last) => {
                    let end = last.index() + 1;
                    for index in next_definition..end {
                        if let Expression::Slot(read) = block.expressions[index]
                            && let Some(source) = self.assignment(read.slot)
                        {
                            block.expressions[index] = match block.expression(source) {
                                Expression::Literal(literal) => Expression::Literal(literal),
                                _ => Expression::Stored { source, fallback: read },
                            };
                        }
                    }
                    next_definition = end;
                }
                Action::Store { destination, value } => self.assign(destination, value)?,
                Action::Effect(_) => self.invalidate()?,
                _ => {}
            }
        }
        Ok(())
    }

    fn propagate_slot_versions(&mut self, block: &mut LocalBlock) -> Result<()> {
        self.invalidate()?;
        let mut next_definition = 0;
        for action in &block.actions {
            match *action {
                Action::DefineThrough(last) => {
                    let end = last.index() + 1;
                    for index in next_definition..end {
                        let expression = match block.expressions[index] {
                            Expression::Slot(read) => Expression::Slot(self.read(read.slot)),
                            Expression::Stored { source, fallback } => Expression::Stored {
                                source,
                                fallback: self.read(fallback.slot),
                            },
                            Expression::Copy { source, fallback } => {
                                let read = match block.expression(source) {
                                    Expression::Slot(read) if read.version == self.read(read.slot).version => read,
                                    _ => self.read(fallback),
                                };
                                Expression::Slot(read)
                            }
                            expression => expression,
                        };
                        block.expressions[index] = expression;
                    }
                    next_definition = end;
                }
                Action::Store { destination, value } => self.assign(destination, value)?,
                Action::Effect(_) => self.invalidate()?,
                _ => {}
            }
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn reserved_bytes(&self) -> usize {
        self.slots.capacity() * size_of::<SlotFact>()
    }
}

enum CopyProposals {
    NoAliases,
    NeedsVersions,
}

fn propagate_copies(block: &mut LocalBlock) -> CopyProposals {
    // Constant propagation already records which stored definition each read
    // observes. Reuse that fact instead of rebuilding a second assignment map.
    let mut proposals = CopyProposals::NoAliases;
    for index in 0..block.expressions.len() {
        if let Expression::Stored { source, fallback } = block.expressions[index]
            && matches!(block.expression(source), Expression::Slot(_) | Expression::Copy { .. })
        {
            block.expressions[index] = Expression::Copy {
                source,
                fallback: fallback.slot,
            };
            proposals = CopyProposals::NeedsVersions;
        }
    }
    // Only Copy consumes revision metadata, and no later pass creates Copy.
    // Without proposals, versioning cannot change any emitted slot or value.
    proposals
}

fn fold_constants(block: &mut LocalBlock) {
    // Definition IDs are topological: children precede parents. Folding a
    // deep tree needs neither recursion nor another traversal allocation.
    for index in 0..block.expressions.len() {
        if let Expression::Stored { source, fallback } = block.expressions[index] {
            block.expressions[index] = match block.expression(source) {
                Expression::Literal(literal) => Expression::Literal(literal),
                _ => Expression::Slot(fallback),
            };
            continue;
        }
        let Expression::Binary { mode, lhs, rhs } = block.expressions[index] else {
            continue;
        };
        let (Expression::Literal(Literal::Int(lhs)), Expression::Literal(Literal::Int(rhs))) =
            (block.expression(lhs), block.expression(rhs))
        else {
            continue;
        };
        if let Some(literal) = fold_i64(mode, lhs, rhs) {
            block.expressions[index] = Expression::Literal(literal);
        }
    }
}

fn fold_i64(mode: ScalarOp, lhs: i64, rhs: i64) -> Option<Literal> {
    // Fold successes only. Overflow/division errors stay at their original
    // runtime action; compilation never constructs or swallows those errors.
    Some(match mode {
        ScalarOp::Add => Literal::Int(lhs.checked_add(rhs)?),
        ScalarOp::Sub => Literal::Int(lhs.checked_sub(rhs)?),
        ScalarOp::Mul => Literal::Int(lhs.checked_mul(rhs)?),
        ScalarOp::Div => Literal::Int(floor_div(lhs, rhs)?),
        ScalarOp::Lt => Literal::Bool(lhs < rhs),
        ScalarOp::Lte => Literal::Bool(lhs <= rhs),
        ScalarOp::Gt => Literal::Bool(lhs > rhs),
        ScalarOp::Gte => Literal::Bool(lhs >= rhs),
        ScalarOp::Eq => Literal::Bool(lhs == rhs),
        ScalarOp::Ne => Literal::Bool(lhs != rhs),
    })
}

fn fold_branches(block: &mut LocalBlock) {
    for action in &mut block.actions {
        if let Action::Branch { target, value } = *action
            && let Expression::Literal(literal) = block.expressions[value.index()]
        {
            *action = if literal.is_false() {
                Action::Jump(target)
            } else {
                Action::Eliminated
            };
        }
    }
}

fn eliminate_dead_temporaries(block: &mut LocalBlock) {
    for action in &mut block.actions {
        if let Action::Discard(value) = *action
            && matches!(block.expressions[value.index()], Expression::Literal(_) | Expression::Slot(_))
        {
            *action = Action::Eliminated;
        }
    }
}

fn eliminate_dead_stores(block: &mut LocalBlock) {
    let mut previous: Option<(usize, SlotId)> = None;
    let mut next_definition = 0;
    for index in 0..block.actions.len() {
        match block.actions[index] {
            Action::Store { destination, value } if matches!(block.expression(value), Expression::Literal(_)) => {
                if let Some((prior, slot)) = previous
                    && slot == destination
                {
                    block.actions[prior] = Action::Eliminated;
                }
                previous = Some((index, destination));
            }
            Action::DefineThrough(last) => {
                let end = last.index() + 1;
                if previous.is_some()
                    && block.expressions[next_definition..end]
                        .iter()
                        .any(|expression| !matches!(expression, Expression::Literal(_)))
                {
                    previous = None;
                }
                next_definition = end;
            }
            Action::Eliminated => {}
            // A read may observe the store; arithmetic, encoding arbitrary
            // Values, or an effect may fail before its replacement executes.
            // Only adjacent, proven non-failing literal writes are removable.
            _ => previous = None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clearing_a_block_discards_unfinished_definition_runs() {
        let mut block = LocalBlock::default();
        block
            .define(Expression::Literal(Literal::Int(42)))
            .expect("pending definition");
        block.clear();
        block.effect(JitOp::Halt).expect("next block effect");
        assert_eq!(block.actions.len(), 1);
        assert!(matches!(block.actions[0], Action::Effect(_)));
        LocalOptimizer::default()
            .optimize(&mut block)
            .expect("empty definition domain");
    }

    #[test]
    fn copy_proposals_select_version_analysis_only_when_needed() {
        let source_slot = SlotId::parse(0).expect("source slot");
        let destination = SlotId::parse(1).expect("copy slot");
        let mut optimizer = LocalOptimizer::default();
        let mut block = LocalBlock::default();
        let value = block
            .define(Expression::Literal(Literal::Int(42)))
            .expect("known value");
        block.push_action(Action::Store {
            destination: source_slot,
            value,
        });
        let read = block
            .define(Expression::Slot(SlotRead::new(source_slot)))
            .expect("known read");
        block.push_action(Action::Materialize(read));
        optimizer.propagate_constants(&mut block).expect("known assignment");
        assert!(matches!(propagate_copies(&mut block), CopyProposals::NoAliases));

        block.clear();
        let value = block
            .define(Expression::Slot(SlotRead::new(source_slot)))
            .expect("unknown input");
        block.push_action(Action::Store { destination, value });
        let read = block
            .define(Expression::Slot(SlotRead::new(destination)))
            .expect("copied read");
        block.push_action(Action::Materialize(read));
        optimizer
            .propagate_constants(&mut block)
            .expect("fresh block assignment");
        assert!(matches!(propagate_copies(&mut block), CopyProposals::NeedsVersions));
        optimizer.propagate_slot_versions(&mut block).expect("resolve alias");
        assert!(matches!(block.expression(read), Expression::Slot(read) if read.slot == source_slot));
    }

    #[test]
    fn adjacent_definitions_share_one_record_but_actions_split_runs() {
        let mut block = LocalBlock::default();
        let first = block
            .define(Expression::Literal(Literal::Int(0)))
            .expect("first definition");
        let capacity = block.actions.capacity();
        let mut last = first;
        for value in 1..65_536 {
            last = block
                .define(Expression::Literal(Literal::Int(value)))
                .expect("definition");
        }
        assert!(block.actions.is_empty());
        assert_eq!(block.actions.capacity(), capacity);
        assert!(matches!(block.pending_definition, Some(end) if end.index() == last.index()));

        let slot = SlotId::parse(0).expect("slot");
        block.push_action(Action::Store {
            destination: slot,
            value: last,
        });
        let before = block
            .define(Expression::Slot(SlotRead::new(slot)))
            .expect("read before write");
        let replacement = block.define(Expression::Literal(Literal::Int(7))).expect("new value");
        block.push_action(Action::Materialize(before));
        block.push_action(Action::Store {
            destination: slot,
            value: replacement,
        });
        let after = block
            .define(Expression::Slot(SlotRead::new(slot)))
            .expect("read after write");
        block.push_action(Action::Materialize(after));
        assert_eq!(block.actions.len(), 7);
        assert!(matches!(block.actions[2], Action::DefineThrough(end) if end.index() == replacement.index()));
        assert!(matches!(block.actions[5], Action::DefineThrough(end) if end.index() == after.index()));

        LocalOptimizer::default().optimize(&mut block).expect("optimize runs");
        assert!(matches!(block.expression(before), Expression::Literal(Literal::Int(65_535))));
        assert!(matches!(block.expression(after), Expression::Literal(Literal::Int(7))));
    }

    #[test]
    fn local_indexes_versions_and_records_remain_bounded() {
        assert_eq!(size_of::<TempId>(), 4);
        assert_eq!(size_of::<Option<TempId>>(), 4);
        assert!(size_of::<Expression>() <= 16);
        assert!(size_of::<Action>() <= 16);
        assert_eq!(size_of::<SlotFact>(), 8);
        assert_eq!(TempId::parse(u32::MAX as usize - 1).expect("last ID").index(), u32::MAX as usize - 1);
        assert!(TempId::parse(u32::MAX as usize).is_err());
        let mut optimizer = LocalOptimizer::default();
        optimizer.invalidate().expect("first epoch");
        let slot = SlotId::parse(65_535).expect("highest verified slot");
        let initial = optimizer.read(slot);
        assert!(optimizer.slots.is_empty(), "unknown high reads need no dense allocation");
        optimizer
            .assign(slot, TempId::parse(0).expect("value"))
            .expect("assignment");
        assert_eq!(optimizer.slots.len(), 65_536);
        assert_ne!(optimizer.read(slot).version, initial.version);
        optimizer.clock = SlotVersion(u32::MAX);
        assert!(optimizer.invalidate().is_err(), "versions must not silently wrap");
    }

    #[test]
    fn reusable_pass_storage_does_not_carry_facts_into_the_next_block() {
        let slot = SlotId::parse(0).expect("slot");
        let mut optimizer = LocalOptimizer::default();
        let mut block = LocalBlock::default();
        let value = block.define(Expression::Literal(Literal::Int(42))).expect("literal");
        block.push_action(Action::Store {
            destination: slot,
            value,
        });
        optimizer.optimize(&mut block).expect("first block");

        block.clear();
        let value = block.define(Expression::Slot(SlotRead::new(slot))).expect("read");
        block.push_action(Action::Materialize(value));
        optimizer.optimize(&mut block).expect("next block");
        assert!(matches!(block.expression(value), Expression::Slot(_)));
    }
}
