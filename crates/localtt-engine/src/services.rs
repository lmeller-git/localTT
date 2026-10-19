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
    todo!()
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
    todo!()
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
        builders::CampaignBuilder,
        types::{Audience, Campaign, CampaignTime, FeedEntry, FeedPayload, UnixTime},
    };

    use super::*;
    use crate::{ports::store::CampaignSnapshot, state::LoadedCampaign};

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

    fn state(campaign: &Campaign, feed: &[FeedEntry]) -> LoadedCampaign {
        LoadedCampaign::from_snapshot(CampaignSnapshot {
            campaign: campaign.clone(),
            entities: Vec::new(),
            feed: feed.to_vec(),
        })
    }

    fn ids(views: &[FeedEntryView]) -> Vec<u64> {
        views.iter().map(|view| view.id.get()).collect()
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
        let state = state(&campaign, &feed);

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
        let state = state(&campaign, &[entry(7, Audience::Public, Some(roller))]);

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
}
