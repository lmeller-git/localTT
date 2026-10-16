//! Driven adapters: persistence and external data sources (RFC-0001 section 3.4).
//!
//! Depends on [`localtt_core`] only, never on the engine (RFC-0001 section 3.1).

#![deny(missing_docs)]
#![deny(clippy::missing_safety_doc, clippy::undocumented_unsafe_blocks)]
#![warn(unsafe_op_in_unsafe_fn)]

pub mod load;
