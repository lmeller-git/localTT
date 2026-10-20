//! The driving layer: async façade, actor, startup and (later) the TUI/HTTP
//! adapters.
//!
//! The engine and the stores stay synchronous. [`app::AppHandle`] serializes
//! every call on one actor thread, and [`app::EventStream`] projects committed
//! changes per viewer. The TUI and the future HTTP/WebSocket adapter are both
//! clients of this crate.

#![deny(missing_docs)]

pub mod app;
pub mod env;
pub mod error;
pub mod session;
