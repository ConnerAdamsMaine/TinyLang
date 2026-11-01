use crate::{Result, TinyOneError};

/// Statically selected at the execution boundary, including nested calls.
pub(super) trait InstructionBudget {
    fn consume(&mut self) -> Result<()>;
}

pub(super) struct Unlimited;

impl InstructionBudget for Unlimited {
    #[inline]
    fn consume(&mut self) -> Result<()> {
        Ok(())
    }
}

pub(super) struct Fuel(u64);

impl Fuel {
    pub fn new(instructions: u64) -> Self {
        Self(instructions)
    }
}

impl InstructionBudget for Fuel {
    #[inline]
    fn consume(&mut self) -> Result<()> {
        self.0 = self
            .0
            .checked_sub(1)
            .ok_or_else(|| TinyOneError::runtime("Instruction fuel exhausted"))?;
        Ok(())
    }
}
