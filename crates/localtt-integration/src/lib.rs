//! The end-to-end scenario suite and API playground of RFC-0003.
//!
//! This crate consumes `localtt-core` (and, as they land, `localtt-engine` and
//! `localtt-adapter`) the way a real client would: it assembles campaigns and
//! characters, drives the paper-sheet lifecycle, and round-trips everything
//! through JSON. It is a leaf crate: nothing in production depends on it, and it
//! adds no rules of its own.

#![deny(missing_docs)]
#![deny(clippy::missing_safety_doc, clippy::undocumented_unsafe_blocks)]
#![warn(unsafe_op_in_unsafe_fn)]

pub mod fixtures;
pub mod roundtrip;

pub(crate) mod ids;
