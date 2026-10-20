//! Runtime content-source loading (`docs/rfcs/server.md` 5).
//!
//! The data root's `reference` directory wins when it contains namespaces. A
//! missing or empty directory falls back to the checkout's shipped `srd2014`
//! snapshot. A malformed runtime snapshot does not silently fall back: the
//! process starts with an empty source and a warning, so content-dependent
//! commands fail closed.

use std::{fs, path::Path, sync::Arc};

use localtt_adapter::reference::{IndexedSources, SHIPPED_DATA_ROOT};
use localtt_core::types::RefId;
use localtt_engine::content::{
    ContentEntry,
    ContentFault,
    ContentSummary,
    Provenance,
    ReferenceSource,
    SearchQuery,
};

/// The loaded content source plus an optional startup warning.
pub struct RuntimeContent {
    /// The shared source handed to [`crate::env::ServerEnv`].
    pub source: Arc<dyn ReferenceSource>,
    /// Present when the configured roots could not be loaded.
    pub warning: Option<String>,
}

/// Loads the runtime reference source.
#[must_use]
#[inline]
pub fn load(reference_dir: &Path) -> RuntimeContent {
    if has_namespaces(reference_dir) {
        return match IndexedSources::from_dirs(&[reference_dir.to_path_buf()]) {
            Ok(sources) => RuntimeContent {
                source: Arc::new(sources),
                warning: None,
            },
            Err(error) => RuntimeContent {
                source: Arc::new(EmptySource),
                warning: Some(format!("content unavailable: {error}")),
            },
        };
    }

    let shipped = Path::new(SHIPPED_DATA_ROOT);
    match IndexedSources::from_dirs(&[shipped.to_path_buf()]) {
        Ok(sources) => RuntimeContent {
            source: Arc::new(sources),
            warning: None,
        },
        Err(error) => RuntimeContent {
            source: Arc::new(EmptySource),
            warning: Some(format!(
                "content unavailable at {} and {}: {error}",
                reference_dir.display(),
                shipped.display()
            )),
        },
    }
}

fn has_namespaces(dir: &Path) -> bool {
    fs::read_dir(dir)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| entry.path().is_dir())
}

/// A source with no entries; content-dependent commands fail closed.
#[derive(Clone, Copy, Debug, Default)]
pub struct EmptySource;

impl ReferenceSource for EmptySource {
    #[inline]
    fn get(&self, _id: &RefId) -> Result<Option<ContentEntry>, ContentFault> {
        Ok(None)
    }

    #[inline]
    fn search(&self, _query: &SearchQuery<'_>) -> Result<Vec<ContentSummary>, ContentFault> {
        Ok(Vec::new())
    }

    #[inline]
    fn autocomplete(&self, _query: &SearchQuery<'_>) -> Result<Vec<ContentSummary>, ContentFault> {
        Ok(Vec::new())
    }

    #[inline]
    fn sources(&self) -> Vec<Provenance> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use localtt_engine::content::{ReferenceSource, SearchQuery};
    use tempfile::tempdir;

    use super::{EmptySource, load};

    #[test]
    fn an_empty_runtime_root_falls_back_to_the_shipped_snapshot() {
        let dir = tempdir().expect("a temporary directory");
        let content = load(dir.path());
        assert!(content.warning.is_none());
        assert!(!content.source.sources().is_empty());
    }

    #[test]
    fn a_malformed_runtime_root_fails_closed_with_a_warning() {
        let dir = tempdir().expect("a temporary directory");
        std::fs::create_dir_all(dir.path().join("broken")).expect("a namespace directory");
        let content = load(dir.path());
        assert!(content.warning.is_some());
        assert!(content.source.sources().is_empty());
    }

    #[test]
    fn the_empty_source_has_no_entries() {
        let source = EmptySource;
        let query = SearchQuery {
            text: "fire",
            kinds: &[],
            limit: 10,
        };
        assert!(source.search(&query).expect("empty search").is_empty());
        assert!(
            source
                .autocomplete(&query)
                .expect("empty autocomplete")
                .is_empty()
        );
        assert!(source.sources().is_empty());
    }
}
