//! The end-to-end scenario suite and API playground of RFC-0003.
//!
//! This crate consumes `localtt-core` (and, later, `localtt-engine`) the way a
//! real client would: it assembles campaigns and characters, drives the
//! paper-sheet lifecycle with provisional command stand-ins, and round-trips
//! everything through JSON. It is a leaf crate: nothing in production depends
//! on it, and it adds no rules of its own.
//!
//! The [`builders`] module is a thin facade over [`localtt_core::builders`];
//! the definitions live in core so every consumer can share them.

#![deny(missing_docs)]
#![deny(clippy::missing_safety_doc, clippy::undocumented_unsafe_blocks)]
#![warn(unsafe_op_in_unsafe_fn)]

pub mod builders;
pub mod fixtures;
pub mod roundtrip;
pub mod transform;

pub(crate) mod ids;
