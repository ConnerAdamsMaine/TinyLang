//! Static declaration metadata and compile-time extension definitions.

mod metadata;
pub(crate) mod registry;
mod stdx;

pub use metadata::{Decorator, DecoratorArgument};

pub(crate) const MAX_DECORATORS: usize = 16;
pub(crate) const MAX_PATH_SEGMENTS: usize = 8;
pub(crate) const MAX_ARGUMENTS: usize = 16;
pub(crate) const MAX_DECORATOR_TEXT_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeclarationTarget {
    Function,
}
