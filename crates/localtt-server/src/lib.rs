//! The driving layer: async façade, actor, startup and the TUI/HTTP adapters.
//!
//! The engine and the stores stay synchronous. [`app::AppHandle`] serializes
//! every call on one actor thread, and [`app::EventStream`] projects committed
//! changes per viewer. The TUI and the REST adapter in [`http`] are both clients
//! of this crate; [`protocol`] holds their shared wire contract. The WebSocket
//! fan-out and browser client are the next layer.

#![deny(missing_docs)]

pub mod app;
pub mod config;
pub mod content;
pub mod env;
pub mod error;
pub mod http;
pub mod protocol;
pub mod session;
#[cfg(feature = "tui")]
pub mod tui;
