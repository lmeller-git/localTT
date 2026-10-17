//! The content contract suite: one shared set of checks, run against every
//! `ReferenceSource` implementation (`docs/rfcs/content.md` section 6.6).
//!
//! The two implementations U2 ships are the in-memory `FixtureSource` and the
//! directory-backed `IndexedSources`; the second test also drives raw files
//! under a tempdir through `write_source` (missing files, malformed content,
//! version refusal, duplicate namespaces). Fixtures are adapter-local, like
//! `store_contract`: the golden cast stays in `localtt-integration`.

use localtt_engine::content::ReferenceSource;

/// Supplies the shared suite with the sources it drives.
///
/// The harness owns implementation details (temp dirs, snapshot files) so the
/// suite itself only ever sees the [`ReferenceSource`] port.
trait ReferenceHarness {
    /// Returns a fresh source with the fixture catalog; every call is
    /// independent of earlier ones.
    fn fresh(&mut self) -> Box<dyn ReferenceSource>;
}

/// Runs the shared contract suite against `harness`'s implementation.
fn reference_contract(harness: &mut dyn ReferenceHarness) {
    todo!()
}

#[test]
fn fixture_source_satisfies_the_contract() {
    todo!()
}

#[test]
fn indexed_sources_satisfy_the_contract() {
    todo!()
}
