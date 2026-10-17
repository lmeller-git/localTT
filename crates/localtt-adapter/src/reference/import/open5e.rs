//! The pinned Open5e `wotc-srd` converter (`docs/rfcs/content.md` section 6.3).
//!
//! Reads the eleven `data/WOTC_5e_SRD_v5.1/*.json` datasets of
//! `open5e/open5e-api`, maps them to `ContentEntry`s and records the pinned
//! revision in the provenance and report. The tool does not read git: the
//! revision is given by the caller.

use std::path::Path;

use localtt_core::types::Namespace;

use crate::reference::{
    import::{ImportError, Importer},
    snapshot::SourceSnapshot,
};

/// The Open5e `wotc-srd` converter (SRD 5.1, 2014 rules).
#[derive(Clone, Debug)]
pub struct Open5eImporter {
    /// The namespace the snapshot is written under, e.g. `srd2014`.
    namespace: Namespace,
    /// The pinned upstream revision, recorded verbatim.
    source_revision: String,
}

impl Open5eImporter {
    /// Creates the converter for one namespace and revision.
    #[must_use]
    #[inline]
    pub fn new(namespace: Namespace, source_revision: String) -> Self {
        todo!()
    }
}

impl Importer for Open5eImporter {
    #[inline]
    fn format(&self) -> &'static str {
        todo!()
    }

    #[inline]
    fn import(&self, input: &Path) -> Result<SourceSnapshot, ImportError> {
        todo!()
    }
}
