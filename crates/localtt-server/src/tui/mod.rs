//! The terminal frontend: pure reducer, renderer and task.
//!
//! The reducer and renderer are testable without a terminal. The task owns the
//! terminal, the input thread and a `JoinSet` of effect futures; it feeds their
//! results back as messages.

#![cfg(feature = "tui")]

pub mod command;
pub mod render;
pub mod state;

mod task;

pub use task::{TuiError, install_panic_hook, run};
