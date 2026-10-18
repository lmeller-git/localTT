//! The application layer of localtt: rules, commands, and the derived values
//! the DM and players see.
//!
//! Depends on [`localtt_core`] only (RFC-0001 section 3.1): the server drives it
//! and the adapter stores what it produces. Nothing here does I/O.

#![deny(missing_docs)]
#![deny(clippy::missing_safety_doc, clippy::undocumented_unsafe_blocks)]
#![warn(unsafe_op_in_unsafe_fn)]

pub mod clock;
pub mod commands;
pub mod content;
pub mod error;
pub mod executor;
pub mod export;
pub mod ports;
pub mod state;
