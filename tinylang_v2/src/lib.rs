#![deny(unsafe_op_in_unsafe_fn)]

//! Experimental TinyLang v2 binary ABI.
//!
//! This crate is intentionally standalone so the frozen v1 crate and ABI do
//! not need to be modified while the v2 protocol is developed.

pub mod engine;
pub mod ffi;

pub use ffi::v2::*;
