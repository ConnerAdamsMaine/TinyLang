use blake2::{Blake2b512, Digest};

/// An ordered, canonical decorator path and its static arguments.
///
/// Metadata carries no authority to execute on a device. The verifier checks
/// registrations and owning permissions independently of these values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decorator {
    pub(crate) path:      Vec<String>,
    pub(crate) arguments: Vec<DecoratorArgument>,
}

/// Static values accepted by the decorator grammar. Identifiers are symbols,
/// never references to runtime variables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecoratorArgument {
    String(String),
    Integer(i64),
    Bool(bool),
    Identifier(String),
}

impl Decorator {
    #[must_use]
    pub fn path(&self) -> &[String] {
        &self.path
    }

    #[must_use]
    pub fn arguments(&self) -> &[DecoratorArgument] {
        &self.arguments
    }

    pub(crate) fn coprocessor_target(&self) -> Option<&str> {
        if !super::stdx::is_coprocessor(self) {
            return None;
        }
        match self.arguments.as_slice() {
            [DecoratorArgument::String(target)] => Some(target),
            _ => None,
        }
    }

    pub(crate) fn hash(&self, hasher: &mut Blake2b512) {
        hasher.update(b"decorator");
        hasher.update((self.path.len() as u64).to_le_bytes());
        for segment in &self.path {
            hash_text(hasher, segment);
        }
        hasher.update((self.arguments.len() as u64).to_le_bytes());
        for argument in &self.arguments {
            match argument {
                DecoratorArgument::String(value) => {
                    hasher.update([0]);
                    hash_text(hasher, value);
                }
                DecoratorArgument::Integer(value) => {
                    hasher.update([1]);
                    hasher.update(value.to_le_bytes());
                }
                DecoratorArgument::Bool(value) => hasher.update([2, u8::from(*value)]),
                DecoratorArgument::Identifier(value) => {
                    hasher.update([3]);
                    hash_text(hasher, value);
                }
            }
        }
    }
}

fn hash_text(hasher: &mut Blake2b512, value: &str) {
    hasher.update((value.len() as u64).to_le_bytes());
    hasher.update(value.as_bytes());
}
