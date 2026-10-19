//! Driven adapters: persistence and external data sources (RFC-0001 section 3.4).
//!
//! Depends on [`localtt_core`] (the wire contract) and [`localtt_engine`] (whose
//! ports the adapters implement; docs/decisions.md ARCH-3 and ENG-1). The engine
//! never depends on this crate.

#![deny(missing_docs)]
#![deny(clippy::missing_safety_doc, clippy::undocumented_unsafe_blocks)]
#![warn(unsafe_op_in_unsafe_fn)]

pub mod load;
pub mod reference;
pub mod rng;
pub mod store;
