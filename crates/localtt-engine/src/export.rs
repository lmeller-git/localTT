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
        Self {
            format: Self::FORMAT.to_owned(),
            format_version: Self::CURRENT_FORMAT_VERSION,
            exported_at,
            campaign: snapshot.campaign.clone(),
            entities: snapshot.entities.clone(),
            feed: snapshot.feed.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use localtt_core::{
        builders::CampaignBuilder,
        types::{
            AnyEntity,
            Audience,
            CampaignTime,
            Entity,
            EntityId,
            EntityKind,
            FeedEntry,
            FeedEntryId,
            FeedPayload,
            NoteBody,
            PlayerId,
            UnixTime,
        },
    };

    use super::*;

    const FIXED_TIME: i64 = 1_750_000_000;

    fn snapshot() -> CampaignSnapshot {
        let campaign = CampaignBuilder::new("Curse of the Vault").build();
        let note = AnyEntity::from(
            Entity::builder(
                EntityKind::Note,
                EntityId::new(),
                "Session 4",
                PlayerId::new(),
                NoteBody {
                    text: "The party opened the vault.".to_owned(),
                },
            )
            .created_at(UnixTime::from_seconds(FIXED_TIME))
            .updated_at(UnixTime::from_seconds(FIXED_TIME))
            .build()
            .expect("a note kind pairs with a note body"),
        );
        let feed = FeedEntry {
            id: FeedEntryId::new(1),
            ts: UnixTime::from_seconds(FIXED_TIME),
            clock_s: CampaignTime::from_seconds(0),
            actor: None,
            entity: None,
            audience: Audience::Public,
            payload: FeedPayload::Hint {
                text: "The innkeeper is lying".to_owned(),
            },
        };
        CampaignSnapshot {
            campaign,
            entities: vec![note],
            feed: vec![feed],
        }
    }

    #[test]
    fn from_snapshot_wraps_the_documents_with_the_format_header() {
        let snapshot = snapshot();

        let bundle = ExportBundle::from_snapshot(&snapshot, UnixTime::from_seconds(FIXED_TIME));

        assert_eq!(bundle.format, ExportBundle::FORMAT);
        assert_eq!(bundle.format_version, ExportBundle::CURRENT_FORMAT_VERSION);
        assert_eq!(bundle.exported_at, UnixTime::from_seconds(FIXED_TIME));
        assert_eq!(bundle.campaign, snapshot.campaign);
        assert_eq!(bundle.entities, snapshot.entities);
        assert_eq!(bundle.feed, snapshot.feed);
    }
}
