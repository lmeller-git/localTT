//! Read-only services over the working set (RFC `authz.md` 4.2, 5.5, 5.6).

use localtt_core::types::{EntityId, FeedEntryId, PlayerId};

use crate::{
    authz::{self, EntityView, FeedEntryView},
    error::EngineError,
    state::WorkingSet,
};

/// Every entity the viewer may read, ascending id.
///
/// Infallible: ids come from `WorkingSet::headers`, and an id its own working
/// set cannot materialize is skipped (it cannot happen; engine unit tests pin
/// the behavior).
#[inline]
pub fn list_entities(state: &dyn WorkingSet, viewer: PlayerId) -> Vec<EntityView> {
    let campaign = state.campaign();
    state
        .headers()
        .into_iter()
        .filter_map(|header| {
            let entity = state.entity(header.id).ok().flatten()?;
            authz::can_read(campaign, &entity, viewer)
                .then(|| authz::view_of(campaign, &entity, viewer))
        })
        .collect()
}

/// One entity view for one viewer: missing before unauthorized.
///
/// `NotFound { entity, element: None }` when the id is absent;
/// `Unauthorized { actor: viewer, entity: Some(id) }` when present but
/// unreadable; otherwise the same view `list_entities` would return.
#[inline]
pub fn get_entity(
    state: &dyn WorkingSet,
    id: EntityId,
    viewer: PlayerId,
) -> Result<EntityView, EngineError> {
    let Some(entity) = state.entity(id)? else {
        return Err(EngineError::NotFound {
            entity: id,
            element: None,
        });
    };
    let campaign = state.campaign();
    if !authz::can_read(campaign, &entity, viewer) {
        return Err(EngineError::Unauthorized {
            actor: viewer,
            entity: Some(id),
        });
    }
    Ok(authz::view_of(campaign, &entity, viewer))
}

/// One ascending page of feed entries the viewer may see.
///
/// `cursor` is exclusive (`id > cursor`), the cursor may name an invisible
/// entry, `limit == 0` is an empty page, and the returned views drop the
/// audience (section 5.5).
#[inline]
pub fn feed_after(
    state: &dyn WorkingSet,
    viewer: PlayerId,
    cursor: Option<FeedEntryId>,
    limit: usize,
) -> Vec<FeedEntryView> {
    state
        .feed()
        .into_iter()
        .filter(|entry| cursor.is_none_or(|cursor| entry.id > cursor))
        .filter(|entry| authz::entry_visible(state.campaign(), entry, viewer))
        .take(limit)
        .map(|entry| FeedEntryView {
            id: entry.id,
            ts: entry.ts,
            clock_s: entry.clock_s,
            actor: entry.actor,
            entity: entry.entity,
            payload: entry.payload,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use localtt_core::{
        builders::{CampaignBuilder, StatblockBuilder},
        types::{
            Abilities,
            AclEntry,
            AclLevel,
            AnyEntity,
            Audience,
            Campaign,
            CampaignTime,
            Entity,
            EntityKind,
            FeedEntry,
            FeedPayload,
            FieldMask,
            NoteBody,
            StatusId,
            UnixTime,
            ViewField,
        },
    };

    use super::*;
    use crate::{
        ports::store::CampaignSnapshot,
        state::{EntityHeader, LoadedCampaign},
    };

    const STAMP: i64 = 1_750_000_000;

    /// A campaign with a DM and one roller.
    fn campaign() -> (Campaign, PlayerId, PlayerId) {
        let mut builder = CampaignBuilder::new("Curse of the Vault");
        let dm = builder.dm("Dungeon Master");
        let roller = builder.player("Roller");
        (builder.build(), dm, roller)
    }

    /// One feed entry with a fixed stamp and no links.
    fn entry(id: u64, audience: Audience, actor: Option<PlayerId>) -> FeedEntry {
        FeedEntry {
            id: FeedEntryId::new(id),
            ts: UnixTime::from_seconds(STAMP),
            clock_s: CampaignTime::from_seconds(0),
            actor,
            entity: None,
            audience,
            payload: FeedPayload::Hint {
                text: format!("entry {id}"),
            },
        }
    }

    /// A campaign with a DM, an owner, a reader and a stranger.
    fn party() -> (Campaign, PlayerId, PlayerId, PlayerId, PlayerId) {
        let mut builder = CampaignBuilder::new("Curse of the Vault");
        let dm = builder.dm("Dungeon Master");
        let owner = builder.player("Owner");
        let reader = builder.player("Reader");
        let stranger = builder.player("Stranger");
        (builder.build(), dm, owner, reader, stranger)
    }

    /// A pc owned by `owner` and carrying `acl`.
    fn pc(id: EntityId, owner: PlayerId, acl: Vec<AclEntry>) -> AnyEntity {
        let mut builder = StatblockBuilder::character();
        builder.abilities(Abilities::all(10)).hp(32, 32);
        let mut entity = builder
            .build_entity(EntityKind::Pc, id, "Aria", owner)
            .expect("abilities and hp are set");
        entity.acl = acl;
        AnyEntity::from(entity)
    }

    /// A note owned by `owner` and carrying `acl`.
    fn note(id: EntityId, owner: PlayerId, acl: Vec<AclEntry>) -> AnyEntity {
        let mut entity = Entity::builder(
            EntityKind::Note,
            id,
            "Session 4",
            owner,
            NoteBody {
                text: "the party opened the vault".to_owned(),
            },
        )
        .build()
        .expect("a note kind pairs with a note body");
        entity.acl = acl;
        AnyEntity::from(entity)
    }

    /// A read grant over exactly `fields`.
    fn masked_read(player: PlayerId, fields: &[ViewField]) -> AclEntry {
        let mut mask = FieldMask::default();
        for &field in fields {
            mask.insert(field);
        }
        AclEntry {
            player,
            level: AclLevel::Read,
            field_mask: Some(mask),
        }
    }

    /// A read grant over every section.
    fn full_read(player: PlayerId) -> AclEntry {
        AclEntry {
            player,
            level: AclLevel::Read,
            field_mask: None,
        }
    }

    fn state(campaign: &Campaign, entities: &[AnyEntity], feed: &[FeedEntry]) -> LoadedCampaign {
        LoadedCampaign::from_snapshot(CampaignSnapshot {
            campaign: campaign.clone(),
            entities: entities.to_vec(),
            feed: feed.to_vec(),
        })
    }

    fn ids(views: &[FeedEntryView]) -> Vec<u64> {
        views.iter().map(|view| view.id.get()).collect()
    }

    fn entity_ids(views: &[EntityView]) -> Vec<EntityId> {
        views.iter().map(|view| view.id).collect()
    }

    #[test]
    fn a_page_is_visible_ascending_and_cursor_exclusive() {
        let (campaign, dm, roller) = campaign();
        let feed = vec![
            entry(1, Audience::Public, None),
            entry(2, Audience::RollerOnly, Some(roller)),
            entry(3, Audience::RollerAndDm, Some(roller)),
            entry(4, Audience::RollerAndDm, None),
        ];
        let state = state(&campaign, &[], &feed);

        assert_eq!(ids(&feed_after(&state, dm, None, 10)), vec![1, 3, 4]);
        assert_eq!(ids(&feed_after(&state, roller, None, 10)), vec![1, 2, 3]);

        // The cursor is exclusive, and an invisible position still pages by id.
        assert_eq!(
            ids(&feed_after(&state, dm, Some(FeedEntryId::new(2)), 10)),
            vec![3, 4]
        );
        assert_eq!(
            ids(&feed_after(&state, roller, Some(FeedEntryId::new(1)), 10)),
            vec![2, 3]
        );

        // The limit truncates the page; zero is an empty page, not everything.
        assert_eq!(ids(&feed_after(&state, dm, None, 2)), vec![1, 3]);
        assert!(feed_after(&state, dm, None, 0).is_empty());

        // A non-member sees nothing, however public the entry.
        assert!(feed_after(&state, PlayerId::new(), None, 10).is_empty());
    }

    #[test]
    fn a_view_mirrors_the_entry_without_the_audience() {
        let (campaign, dm, roller) = campaign();
        let state = state(&campaign, &[], &[entry(7, Audience::Public, Some(roller))]);

        let views = feed_after(&state, dm, None, 10);
        let view = views.first().expect("the public entry is visible");
        assert_eq!(view.id, FeedEntryId::new(7));
        assert_eq!(view.ts, UnixTime::from_seconds(STAMP));
        assert_eq!(view.clock_s, CampaignTime::from_seconds(0));
        assert_eq!(view.actor, Some(roller));
        assert_eq!(view.entity, None);
        assert_eq!(
            view.payload,
            FeedPayload::Hint {
                text: "entry 7".to_owned(),
            }
        );
    }

    #[test]
    fn read_services_filter_and_share_one_view() {
        let (campaign, dm, owner, reader, stranger) = party();
        let pc_id = EntityId::new();
        let note_id = EntityId::new();
        let entities = vec![
            pc(pc_id, owner, vec![masked_read(reader, &[ViewField::Hp])]),
            note(note_id, dm, vec![full_read(reader)]),
        ];
        let state = state(&campaign, &entities, &[]);
        let mut both = vec![pc_id, note_id];
        both.sort();

        let listed = list_entities(&state, dm);
        assert_eq!(entity_ids(&listed), both);
        assert!(listed.iter().all(|view| view.hidden.is_empty()));

        let listed = list_entities(&state, owner);
        assert_eq!(entity_ids(&listed), vec![pc_id]);
        assert!(listed[0].writable);

        let listed = list_entities(&state, reader);
        assert_eq!(entity_ids(&listed), both);
        let pc_view = listed
            .iter()
            .find(|view| view.id == pc_id)
            .expect("the mask still lists the pc");
        assert_eq!(pc_view.hidden.len(), 19, "the reader sees exactly Hp");

        assert!(list_entities(&state, stranger).is_empty());

        // The list view is the get view.
        let got = get_entity(&state, pc_id, reader).expect("the masked share reads");
        assert_eq!(got, *pc_view);
    }

    #[test]
    fn get_reports_missing_before_unauthorized() {
        let (campaign, dm, owner, reader, stranger) = party();
        let pc_id = EntityId::new();
        let entities = vec![pc(
            pc_id,
            owner,
            vec![masked_read(reader, &[ViewField::Hp])],
        )];
        let state = state(&campaign, &entities, &[]);

        assert!(get_entity(&state, pc_id, dm).is_ok());
        assert!(get_entity(&state, pc_id, reader).is_ok());

        assert!(matches!(
            get_entity(&state, pc_id, stranger),
            Err(EngineError::Unauthorized { actor, entity: Some(target) })
                if actor == stranger && target == pc_id
        ));
        assert!(matches!(
            get_entity(&state, pc_id, PlayerId::new()),
            Err(EngineError::Unauthorized { .. })
        ));

        let ghost = EntityId::new();
        assert!(matches!(
            get_entity(&state, ghost, stranger),
            Err(EngineError::NotFound { entity, element: None }) if entity == ghost
        ));
    }

    /// A working set whose only header names an id it cannot materialize.
    struct GhostHeaders {
        campaign: Campaign,
        header: EntityHeader,
    }

    impl WorkingSet for GhostHeaders {
        fn campaign(&self) -> &Campaign {
            &self.campaign
        }

        fn entity(&self, _id: EntityId) -> Result<Option<AnyEntity>, EngineError> {
            Ok(None)
        }

        fn headers(&self) -> Vec<EntityHeader> {
            vec![self.header.clone()]
        }

        fn feed(&self) -> Vec<FeedEntry> {
            Vec::new()
        }

        fn next_feed_id(&self) -> u64 {
            0
        }

        fn expiry_candidates_until(&self, _until: CampaignTime) -> Vec<(EntityId, StatusId)> {
            Vec::new()
        }
    }

    #[test]
    fn a_header_the_set_cannot_materialize_is_skipped() {
        let (campaign, dm, owner, _reader, _stranger) = party();
        let ghost = EntityId::new();
        let state = GhostHeaders {
            campaign,
            header: EntityHeader {
                id: ghost,
                kind: EntityKind::Pc,
                name: "Ghost".to_owned(),
                owner,
                parent: None,
                acl: Vec::new(),
                rev: 0,
                updated_at: UnixTime::from_seconds(STAMP),
            },
        };

        assert!(list_entities(&state, dm).is_empty());
        assert!(
            list_entities(&state, owner).is_empty(),
            "even the owner learns nothing the set cannot materialize"
        );
    }
}
