//! Shared support for the ignored browser integration tests.
//!
//! Cargo compiles this module into every integration test binary, so a helper
//! used by only one suite is dead code in the others.
#![allow(dead_code)]

use std::path::PathBuf;

pub(crate) mod browser;
pub(crate) mod flows;
pub(crate) mod journey;
pub(crate) mod scenario;
pub(crate) mod server;

/// Reads the test artifact root, honoring Cargo's target directory when set.
pub(crate) fn artifact_root() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/ui"),
        |dir| PathBuf::from(dir).join("ui"),
    )
}
