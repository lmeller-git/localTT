//! Content sources behind the engine's `ReferenceSource` port
//! (`docs/rfcs/content.md` section 6.1).
//!
//! Layout: `index` composes loaded snapshots, `snapshot` owns the on-disk
//! format and its writer, `source` defines the reader seam and the directory
//! reader, `fixture` is the in-memory test source, `aliases` owns one-hop
//! resolution, and `import` holds the one-time converters.

pub mod import;

mod aliases;
mod fixture;
mod index;
mod snapshot;
mod source;

pub use fixture::FixtureSource;
pub use index::IndexedSources;
pub use snapshot::{CURRENT_SNAPSHOT_VERSION, SourceSnapshot, write_source};
pub use source::SnapshotReader;

/// The committed data root in this checkout: `<crate>/data`.
///
/// The converted `srd2014` namespace lives under it; this is the source the
/// tests and the workspace default use. It is a build-time path, not an
/// installed-layout promise: U7 resolves the runtime default root and the
/// `--data` flags, and a packaging story installs the directory next to the
/// binary (CONTENT-9, S6).
pub const SHIPPED_DATA_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data");
