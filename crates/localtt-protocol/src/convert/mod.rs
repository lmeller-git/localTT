//! Conversions from the engine's types into the transport DTOs.
//!
//! Everything that names an engine type lives here, behind the `engine`
//! feature, so clients that only consume the DTOs do not link the engine.

mod command;
mod content;
mod error;
mod event;
mod outcome;
mod sheet;
mod view;
