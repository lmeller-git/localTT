//! Bundle decode and import over the engine's store port (RFC section 4.3;
//! STORE-5, STORE-9).

use localtt_core::types::{AnyEntity, Campaign, FeedEntry, UnixTime};
use localtt_engine::{
    export::ExportBundle,
    ports::store::{CampaignStore, StatePatch, StoreFault},
};
use serde::Deserialize;
use serde_json::Value;

use crate::store::parse_value;

/// The bundle wire shape. Adapter-private: the engine's [`ExportBundle`]
/// deliberately does not derive `Deserialize`, and the documents stay raw
/// JSON values so each can run the schema gate before it is parsed.
#[derive(Deserialize)]
struct BundleWire {
    format: String,
    format_version: u32,
    exported_at: UnixTime,
    campaign: Value,
    entities: Vec<Value>,
    feed: Vec<Value>,
}

/// Decodes a bundle text: envelope format/version, then every document through
/// the same schema gate as `load` (same-or-older major accepted and re-stamped,
/// newer refused).
///
/// A foreign `format` is `Corrupt`; a newer `format_version` is
/// `FormatRefused`. Entities and feed entries come out sorted ascending, the
/// order [`ExportBundle`] documents.
#[inline]
pub fn decode_bundle(text: &str) -> Result<ExportBundle, StoreFault> {
    let wire: BundleWire = serde_json::from_str(text).map_err(|error| StoreFault::Corrupt {
        message: format!("the bundle did not parse: {error}"),
    })?;
    if wire.format != ExportBundle::FORMAT {
        return Err(StoreFault::Corrupt {
            message: format!(
                "bundle format {:?} is not {}",
                wire.format,
                ExportBundle::FORMAT
            ),
        });
    }
    if wire.format_version > ExportBundle::CURRENT_FORMAT_VERSION {
        return Err(StoreFault::FormatRefused {
            found: wire.format_version,
            supported: ExportBundle::CURRENT_FORMAT_VERSION,
        });
    }
    if wire.format_version < ExportBundle::CURRENT_FORMAT_VERSION {
        return Err(StoreFault::Corrupt {
            message: format!(
                "bundle format version {} is not a format this build knows",
                wire.format_version
            ),
        });
    }

    let campaign = parse_value::<Campaign>(&wire.campaign)?;
    let mut entities = wire
        .entities
        .iter()
        .map(parse_value::<AnyEntity>)
        .collect::<Result<Vec<_>, _>>()?;
    entities.sort_by_key(AnyEntity::id);
    let mut feed = wire
        .feed
        .iter()
        .map(parse_value::<FeedEntry>)
        .collect::<Result<Vec<_>, _>>()?;
    feed.sort_by_key(|entry| entry.id);

    Ok(ExportBundle {
        format: wire.format,
        format_version: wire.format_version,
        exported_at: wire.exported_at,
        campaign,
        entities,
        feed,
    })
}

/// Imports a decoded bundle into an empty store: one `apply`, all-or-nothing.
///
/// A store that already holds a campaign is refused with `StoreNotEmpty`; the
/// emptiness check matches `Err(NoCampaign)` from `load` and lets every other
/// fault through.
#[inline]
pub fn import_into(store: &mut dyn CampaignStore, bundle: &ExportBundle) -> Result<(), StoreFault> {
    match store.load() {
        Ok(_) => return Err(StoreFault::StoreNotEmpty),
        Err(StoreFault::NoCampaign) => {}
        Err(other) => return Err(other),
    }

    let patch = StatePatch {
        campaign: Some(&bundle.campaign),
        upserts: bundle.entities.iter().collect(),
        removals: Vec::new(),
        feed_appends: bundle.feed.iter().collect(),
        feed_keep_from: None,
    };
    store.apply(&patch)
}

#[cfg(test)]
mod tests {
    use localtt_core::{
        builders::CampaignBuilder,
        types::{
            Audience,
            CampaignTime,
            Entity,
            EntityId,
            EntityKind,
            FeedEntryId,
            FeedPayload,
            NoteBody,
            PlayerId,
        },
    };
    use localtt_engine::ports::store::CampaignSnapshot;
    use serde::Serialize;
    use serde_json::json;

    use super::*;
    use crate::store::MemoryStore;

    const FIXED_TIME: i64 = 1_750_000_000;

    /// A bundle with one campaign, one note, and one feed entry.
    fn bundle() -> ExportBundle {
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
        ExportBundle::from_snapshot(
            &CampaignSnapshot {
                campaign,
                entities: vec![note],
                feed: vec![feed],
            },
            UnixTime::from_seconds(FIXED_TIME),
        )
    }

    fn bytes<T: Serialize>(value: &T) -> String {
        serde_json::to_string(value).expect("documents serialize")
    }

    /// The bundle's wire JSON with one envelope field replaced.
    fn tampered(rewrite: impl FnOnce(&mut Value)) -> String {
        let mut wire: Value = serde_json::from_str(&bytes(&bundle())).expect("the bundle is JSON");
        rewrite(&mut wire);
        wire.to_string()
    }

    #[test]
    fn a_round_trip_through_the_wire_reproduces_the_bundle() {
        let first = bundle();

        let decoded = decode_bundle(&bytes(&first)).expect("bundles are portable JSON");

        assert_eq!(decoded, first);
    }

    #[test]
    fn a_foreign_format_is_refused() {
        let text = tampered(|wire| wire["format"] = json!("some.other.tool"));

        match decode_bundle(&text) {
            Err(StoreFault::Corrupt { .. }) => {}
            other => panic!("a foreign format must be Corrupt, got {other:?}"),
        }
    }

    #[test]
    fn a_newer_format_version_is_refused() {
        let text = tampered(|wire| wire["format_version"] = json!(2));

        match decode_bundle(&text) {
            Err(StoreFault::FormatRefused { found, supported }) => {
                assert_eq!(found, 2);
                assert_eq!(supported, ExportBundle::CURRENT_FORMAT_VERSION);
            }
            other => panic!("a newer format version must be FormatRefused, got {other:?}"),
        }
    }

    #[test]
    fn a_newer_document_major_is_refused() {
        let text = tampered(|wire| {
            wire["campaign"]["schema_version"] = json!({ "major": 2, "minor": 0 });
        });

        match decode_bundle(&text) {
            Err(StoreFault::VersionRefused { found, .. }) => {
                assert_eq!(found.major, 2);
            }
            other => panic!("a newer document major must be VersionRefused, got {other:?}"),
        }
    }

    #[test]
    fn import_round_trips_into_an_empty_store() {
        let bundle = bundle();
        let mut store = MemoryStore::new();

        import_into(&mut store, &bundle).expect("an empty store accepts the bundle");

        let loaded = store.load().expect("the import just persisted");
        let reexported = ExportBundle::from_snapshot(&loaded, bundle.exported_at);
        assert_eq!(bytes(&reexported), bytes(&bundle));
    }

    #[test]
    fn import_refuses_a_store_that_already_holds_a_campaign() {
        let bundle = bundle();
        let mut store = MemoryStore::new();
        import_into(&mut store, &bundle).expect("an empty store accepts the bundle");

        match import_into(&mut store, &bundle) {
            Err(StoreFault::StoreNotEmpty) => {}
            other => panic!("a non-empty store must be StoreNotEmpty, got {other:?}"),
        }
    }
}
