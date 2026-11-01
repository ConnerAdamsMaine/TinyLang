//! Typed operands shared by symbolic lowering and direct scalar execution.

use crate::{
    JitOp,
    Op,
    Result,
    TinyOneError,
    Value,
    checked_div_int,
    runtime_add_int,
    runtime_compare_int,
    runtime_mul_int,
    runtime_sub_int,
};

/// An index, not a slot count: index 65,535 is valid in a 65,536-slot frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SlotId(u16);

impl SlotId {
    /// Adaptive encoding is optional; decline an index that cannot fit rather
    /// than narrowing it or changing the existing executable operation.
    pub(super) fn from_index(index: usize) -> Option<Self> {
        u16::try_from(index).ok().map(Self)
    }

    pub(super) fn parse(raw: i64) -> Result<Self> {
        u16::try_from(raw)
            .map(Self)
            .map_err(|_| TinyOneError::compile(format!("JIT invalid slot index {raw}")))
    }

    pub(crate) fn index(self) -> usize {
        usize::from(self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScalarOp {
    Add,
    Sub,
    Mul,
    Div,
    Lt,
    Lte,
    Gt,
    Gte,
    Eq,
    Ne,
}

impl ScalarOp {
    pub(super) fn from_opcode(op: Op) -> Option<Self> {
        Some(match op {
            Op::Add => Self::Add,
            Op::Sub => Self::Sub,
            Op::Mul => Self::Mul,
            Op::Div => Self::Div,
            Op::Lt => Self::Lt,
            Op::Lte => Self::Lte,
            Op::Gt => Self::Gt,
            Op::Gte => Self::Gte,
            Op::Eq => Self::Eq,
            Op::Ne => Self::Ne,
            _ => return None,
        })
    }

    pub(super) fn opcode(self) -> Op {
        match self {
            Self::Add => Op::Add,
            Self::Sub => Op::Sub,
            Self::Mul => Op::Mul,
            Self::Div => Op::Div,
            Self::Lt => Op::Lt,
            Self::Lte => Op::Lte,
            Self::Gt => Op::Gt,
            Self::Gte => Op::Gte,
            Self::Eq => Op::Eq,
            Self::Ne => Op::Ne,
        }
    }

    pub(super) fn stack_op(self) -> JitOp {
        match self {
            Self::Add => JitOp::Add,
            Self::Sub => JitOp::Sub,
            Self::Mul => JitOp::Mul,
            Self::Div => JitOp::Div,
            _ => JitOp::Compare(self.opcode()),
        }
    }

    pub(super) fn execute(self, lhs: Value, rhs: Value) -> Result<Value> {
        // These shared handlers guard the actual I64 tags on every execution
        // and fall back to generic semantics for all other representations.
        // Fusion must not lose the checked I64 path formerly reached by the
        // separate quickened arithmetic instruction.
        match self {
            Self::Add => runtime_add_int(lhs, rhs),
            Self::Sub => runtime_sub_int(lhs, rhs),
            Self::Mul => runtime_mul_int(lhs, rhs),
            Self::Div => checked_div_int(lhs, rhs),
            _ => runtime_compare_int(self.opcode(), lhs, rhs),
        }
    }

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Sub => "sub",
            Self::Mul => "mul",
            Self::Div => "div",
            Self::Lt => "lt",
            Self::Lte => "lte",
            Self::Gt => "gt",
            Self::Gte => "gte",
            Self::Eq => "eq",
            Self::Ne => "ne",
        }
    }
}
