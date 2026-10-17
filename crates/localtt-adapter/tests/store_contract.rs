//! The store contract suite: one shared set of checks, run against every
//! `CampaignStore` implementation (`docs/rfcs/store.md` section 6.3; design
//! section 5.9).
//!
//! Fixtures are adapter-local and built through `localtt_core::builders`; the
//! golden cast stays in `localtt-integration`. The SQLite-only block
//! (`open`/`backup`/`sync`, DDL checks, raw-SQL fabrication) drives the
//! concrete store through this crate's own `rusqlite` dependency.

use localtt_engine::ports::store::CampaignStore;

/// Supplies the shared suite with the stores it drives.
///
/// The harness owns implementation details (temp dirs, file paths) so the
/// suite itself only ever sees the [`CampaignStore`] port.
trait StoreHarness {
    /// Returns a fresh, empty store; every call is independent of earlier
    /// ones.
    fn fresh(&mut self) -> Box<dyn CampaignStore>;
}

/// Runs the shared contract suite against `harness`'s implementation.
fn store_contract(harness: &mut dyn StoreHarness) {
    todo!()
}

#[test]
fn memory_store_satisfies_the_contract() {
    todo!()
}

#[test]
fn sqlite_store_satisfies_the_contract() {
    todo!()
}
