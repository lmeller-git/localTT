//! Rest bookkeeping (RFC `engine-executor.md` 5.3): short and long rests are
//! pure effects over a statblock plus the optional clock move.
//!
//! The effect functions land with the implement phase; the module keeps the
//! rest internals out of the command surface in `mod.rs`.
