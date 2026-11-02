//! Immutable facts established by the authoritative verifier.
//!
//! This is execution metadata, not artifact content. It is constructed only
//! after all chunks pass verification and shared by verification-token clones.

use super::control_flow::BasicBlock;
use crate::ModulePermissions;

#[derive(Debug, Clone, Copy)]
pub(crate) enum PermissionSetId {
    Root,
    Module(usize),
}

#[derive(Debug)]
pub(crate) struct ChunkCertificate {
    pub(crate) max_stack_depth: usize,
    pub(crate) permissions:     PermissionSetId,
    pub(crate) blocks:          Box<[BasicBlock]>,
}

#[derive(Debug)]
pub(crate) struct VerificationCertificate {
    /// Main first, followed by functions in their verified index order.
    pub(crate) chunks:             Box<[ChunkCertificate]>,
    pub(crate) denied_permissions: ModulePermissions,
}
