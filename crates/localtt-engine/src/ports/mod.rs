//! The engine's ports: interfaces the engine consumes, implemented by driven
//! adapters (docs/decisions.md ENG-1).
//!
//! Ports are object-safe, `Send`, and I/O boundaries only (see `docs/engine.md` section 1);
//! pure logic never takes a port.

pub mod rng;
pub mod store;
