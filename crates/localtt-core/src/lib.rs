//! Core domain types for localTT (the data model of RFC-0002).
//!
//! This crate is I/O-free: it defines the serialized documents (campaigns,
//! entities, statblocks, items, rolls, feed entries) as plain data plus the
//! validation that keeps them well-formed. Storage, content packs, rules
//! evaluation and transport live in the other crates.

#![deny(missing_docs)]
#![deny(clippy::missing_safety_doc, clippy::undocumented_unsafe_blocks)]
#![warn(unsafe_op_in_unsafe_fn)]

pub mod types;
