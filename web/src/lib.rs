//! Read-only browser client for local-tt.
//!
//! Pure formatting and URL builders compile natively so their tests run without
//! a browser. The Leptos app itself is gated to `wasm32`.

#![deny(missing_docs)]

pub mod format;
pub mod url;

#[cfg(target_arch = "wasm32")]
mod app;
#[cfg(target_arch = "wasm32")]
mod http;

#[cfg(target_arch = "wasm32")]
pub use app::start;
