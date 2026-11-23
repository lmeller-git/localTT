//! The document schema version.

use std::hash::Hash;

use serde::{Deserialize, Serialize};

/// Document schema version, `major.minor`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SchemaVersion {
    /// Breaking-change counter.
    pub major: u16,
    /// Additive-change counter.
    pub minor: u16,
}

impl SchemaVersion {
    /// The version this build writes.
    ///
    /// `1.1` adds the campaign's initiative queue. `1.2` adds the optional
    /// spell-entry level snapshot.
    pub const CURRENT: Self = Self { major: 1, minor: 2 };

    /// True when this build may load `file`: same or older major.
    #[must_use]
    #[inline]
    pub const fn accepts(self, file: Self) -> bool {
        file.major <= self.major
    }
}
