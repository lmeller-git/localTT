//! Bundle decode and import over the engine's store port (RFC section 4.3;
//! STORE-5, STORE-9).

use localtt_engine::{
    export::ExportBundle,
    ports::store::{CampaignStore, StoreFault},
};

/// Decodes a bundle text: envelope format/version, then every document through
/// the same schema gate as `load` (same-or-older major accepted and re-stamped,
/// newer refused).
///
/// A foreign `format` is `Corrupt`; a newer `format_version` is
/// `FormatRefused`. Parses an adapter-private wire type: the engine's
/// [`ExportBundle`] deliberately does not derive `Deserialize`.
#[inline]
pub fn decode_bundle(text: &str) -> Result<ExportBundle, StoreFault> {
    todo!()
}

/// Imports a decoded bundle into an empty store: one `apply`, all-or-nothing.
///
/// A store that already holds a campaign is refused with `StoreNotEmpty`; the
/// emptiness check matches `Err(NoCampaign)` from `load` and lets every other
/// fault through.
#[inline]
pub fn import_into(store: &mut dyn CampaignStore, bundle: &ExportBundle) -> Result<(), StoreFault> {
    todo!()
}
