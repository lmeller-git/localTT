//! The export bundle: the versioned sharing envelope for a campaign
//! (`docs/rfcs/store.md` section 4.2; RFC-0001 section 5.9).

use localtt_core::types::{AnyEntity, Campaign, FeedEntry, UnixTime};
use serde::Serialize;

use crate::ports::store::CampaignSnapshot;

/// Versioned envelope for sharing a campaign as JSON (RFC-0001 5.9).
///
/// Serialize-only by design: writing a bundle is plain `serde_json`
/// serialization, while reading goes through the adapter's gated
/// `decode_bundle`, so the un-gated parse path cannot be reached by accident.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ExportBundle {
    /// Always [`ExportBundle::FORMAT`].
    pub format: String,
    /// Envelope version, independent of document `schema_version`.
    pub format_version: u32,
    /// Excluded from round-trip comparisons (design section 5.1).
    pub exported_at: UnixTime,
    /// The campaign document.
    pub campaign: Campaign,
    /// Entities, ascending `EntityId`.
    pub entities: Vec<AnyEntity>,
    /// Feed entries, ascending `FeedEntryId`.
    pub feed: Vec<FeedEntry>,
}

impl ExportBundle {
    /// The envelope version this build writes.
    pub const CURRENT_FORMAT_VERSION: u32 = 1;
    /// The bundle format identifier stored in `format`.
    pub const FORMAT: &'static str = "localtt.campaign";

    /// Wraps a loaded snapshot; the documents are cloned, not transformed.
    ///
    /// `exported_at` is the wall-clock stamp for the envelope.
    #[must_use]
    #[inline]
    pub fn from_snapshot(snapshot: &CampaignSnapshot, exported_at: UnixTime) -> Self {
        todo!()
    }
}
