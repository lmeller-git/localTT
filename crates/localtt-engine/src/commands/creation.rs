//! The `CreateEntity` command (F3, `docs/rfcs/server.md` milestone M2).
//!
//! Creation is deliberately strict: the header is client-supplied, but revision
//! and timestamps are engine-stamped, the owner must be a roster member, and a
//! player cannot create a document for someone else or grant extra access.

use localtt_core::types::{AnyEntity, PlayerId, SchemaVersion, UnixTime};

use super::{Transition, one_upsert};
use crate::{authz, error::EngineError, state::WorkingSet};

/// Creates one entity from a draft document.
///
/// # Errors
///
/// [`EngineError::AlreadyExists`] for a colliding id, [`EngineError::Unauthorized`]
/// for a non-member, another actor's owner, a non-DM ACL, or an unreadable
/// parent, and [`EngineError::Malformed`] for an invalid draft header.
pub(super) fn create_entity_command(
    state: &dyn WorkingSet,
    mut entity: AnyEntity,
    actor: Option<PlayerId>,
    now: UnixTime,
) -> Result<Transition, EngineError> {
    let Some(actor) = actor else {
        return Err(EngineError::Malformed {
            reason: "creating an entity requires an actor".to_owned(),
        });
    };
    let campaign = state.campaign();
    if !authz::member(campaign, actor) {
        return Err(EngineError::Unauthorized {
            actor,
            entity: None,
        });
    }

    let id = entity.id();
    if state.entity(id)?.is_some() {
        return Err(EngineError::AlreadyExists { entity: id });
    }
    if entity.rev() != 0 {
        return Err(EngineError::Malformed {
            reason: format!(
                "a new entity must start at revision 0, got {}",
                entity.rev()
            ),
        });
    }
    let name = entity.name().trim();
    if name.is_empty() {
        return Err(EngineError::Malformed {
            reason: "an entity name must not be empty".to_owned(),
        });
    }
    if name.chars().count() > 200 {
        return Err(EngineError::Malformed {
            reason: "an entity name must be at most 200 characters".to_owned(),
        });
    }

    let owner = entity.owner();
    if !campaign.players.iter().any(|player| player.id == owner) {
        return Err(EngineError::Malformed {
            reason: format!("owner {owner} is not on the campaign roster"),
        });
    }
    let dm = authz::dm(campaign, actor);
    if !dm && owner != actor {
        return Err(EngineError::Unauthorized {
            actor,
            entity: Some(id),
        });
    }
    if !dm && !entity.acl().is_empty() {
        return Err(EngineError::Unauthorized {
            actor,
            entity: Some(id),
        });
    }
    for grant in entity.acl() {
        if !campaign
            .players
            .iter()
            .any(|player| player.id == grant.player)
        {
            return Err(EngineError::Malformed {
                reason: format!("ACL grantee {} is not on the campaign roster", grant.player),
            });
        }
    }
    if let Some(parent) = entity.parent() {
        if parent == id {
            return Err(EngineError::Malformed {
                reason: "an entity cannot be its own parent".to_owned(),
            });
        }
        let Some(parent_entity) = state.entity(parent)? else {
            return Err(EngineError::NotFound {
                entity: parent,
                element: None,
            });
        };
        if !authz::can_read(campaign, &parent_entity, actor) {
            return Err(EngineError::Unauthorized {
                actor,
                entity: Some(parent),
            });
        }
    }

    stamp_create(&mut entity, now);
    Ok(one_upsert(entity, Vec::new()))
}

fn stamp_create(entity: &mut AnyEntity, now: UnixTime) {
    match entity {
        AnyEntity::Statblock(document) => {
            document.rev = 1;
            document.schema_version = SchemaVersion::CURRENT;
            document.created_at = now;
            document.updated_at = now;
        }
        AnyEntity::Note(document) => {
            document.rev = 1;
            document.schema_version = SchemaVersion::CURRENT;
            document.created_at = now;
            document.updated_at = now;
        }
        AnyEntity::Handout(document) => {
            document.rev = 1;
            document.schema_version = SchemaVersion::CURRENT;
            document.created_at = now;
            document.updated_at = now;
        }
    }
}

#[cfg(test)]
mod tests {
    use localtt_core::{
        builders::{CampaignBuilder, StatblockBuilder},
        types::{
            Abilities,
            AclEntry,
            AclLevel,
            Campaign,
            EntityId,
            EntityKind,
            PlayerId,
            SchemaVersion,
            UnixTime,
        },
    };

    use super::create_entity_command;
    use crate::{
        commands::Event,
        error::EngineError,
        ports::store::CampaignSnapshot,
        state::LoadedCampaign,
    };

    const NOW: i64 = 1_750_000_000;

    fn campaign() -> (Campaign, PlayerId, PlayerId) {
        let mut builder = CampaignBuilder::new("Creation");
        let dm = builder.dm("DM");
        let player = builder.player("Player");
        (builder.build(), dm, player)
    }

    fn state_with(
        campaign: Campaign,
        entities: Vec<localtt_core::types::AnyEntity>,
    ) -> LoadedCampaign {
        LoadedCampaign::from_snapshot(CampaignSnapshot {
            campaign,
            entities,
            feed: Vec::new(),
        })
    }

    fn draft(owner: PlayerId) -> localtt_core::types::AnyEntity {
        let mut builder = StatblockBuilder::character();
        builder.abilities(Abilities::all(12)).hp(12, 12);
        localtt_core::types::AnyEntity::from(
            builder
                .build_entity(EntityKind::Pc, EntityId::new(), "Aria", owner)
                .expect("abilities and hp are set"),
        )
    }

    #[test]
    fn a_player_creates_an_owned_document_at_revision_one() {
        let (campaign, _dm, player) = campaign();
        let state = state_with(campaign, Vec::new());
        let entity = draft(player);

        let transition = create_entity_command(
            &state,
            entity.clone(),
            Some(player),
            UnixTime::from_seconds(NOW),
        )
        .expect("a player may create their own character");

        assert_eq!(transition.upserts.len(), 1);
        let created = &transition.upserts[0];
        assert_eq!(created.id(), entity.id());
        assert_eq!(created.rev(), 1);
        assert_eq!(created.owner(), player);
        assert_eq!(created.created_at(), UnixTime::from_seconds(NOW));
        assert_eq!(created.updated_at(), UnixTime::from_seconds(NOW));
        assert_eq!(created.schema_version(), SchemaVersion::CURRENT);
        assert_eq!(
            transition.events,
            vec![Event::EntityChanged {
                id: entity.id(),
                rev: 1,
                kind: EntityKind::Pc,
            }]
        );
    }

    #[test]
    fn a_dm_may_create_for_another_roster_member() {
        let (campaign, dm, player) = campaign();
        let state = state_with(campaign, Vec::new());

        let transition =
            create_entity_command(&state, draft(player), Some(dm), UnixTime::from_seconds(NOW))
                .expect("the DM may seed a player's character");

        assert_eq!(transition.upserts[0].owner(), player);
    }

    #[test]
    fn a_player_cannot_create_for_another_owner_or_grant_access() {
        let (campaign, _dm, player) = campaign();
        let other = PlayerId::new();
        let state = state_with(campaign, Vec::new());
        let entity = draft(player);

        let result = create_entity_command(
            &state,
            entity.clone(),
            Some(other),
            UnixTime::from_seconds(NOW),
        );
        assert!(matches!(
            result,
            Err(EngineError::Unauthorized { actor, entity: None }) if actor == other
        ));

        let mut builder = CampaignBuilder::new("Creation");
        let player = builder.player("Player");
        let other = builder.player("Other");
        let state = state_with(builder.build(), Vec::new());
        let mut entity = draft(player);
        match &mut entity {
            localtt_core::types::AnyEntity::Statblock(document) => {
                document.acl = vec![AclEntry {
                    player: other,
                    level: AclLevel::Read,
                    field_mask: None,
                }];
            }
            other => panic!("the draft is a statblock, got {other:?}"),
        }

        let result =
            create_entity_command(&state, entity, Some(player), UnixTime::from_seconds(NOW));
        assert!(matches!(
            result,
            Err(EngineError::Unauthorized {
                actor,
                entity: Some(_)
            }) if actor == player
        ));
    }

    #[test]
    fn duplicate_ids_and_nonzero_revisions_are_refused() {
        let (campaign, dm, player) = campaign();
        let existing = draft(player);
        let state = state_with(campaign.clone(), vec![existing.clone()]);

        let result = create_entity_command(
            &state,
            existing.clone(),
            Some(dm),
            UnixTime::from_seconds(NOW),
        );
        assert!(matches!(
            result,
            Err(EngineError::AlreadyExists { entity }) if entity == existing.id()
        ));

        let mut revised = draft(player);
        match &mut revised {
            localtt_core::types::AnyEntity::Statblock(document) => document.rev = 4,
            other => panic!("the draft is a statblock, got {other:?}"),
        }
        let state = state_with(campaign, Vec::new());
        assert!(matches!(
            create_entity_command(&state, revised, Some(dm), UnixTime::from_seconds(NOW)),
            Err(EngineError::Malformed { .. })
        ));
    }

    #[test]
    fn creation_requires_an_actor() {
        let (campaign, _dm, player) = campaign();
        let state = state_with(campaign, Vec::new());
        assert!(matches!(
            create_entity_command(&state, draft(player), None, UnixTime::from_seconds(NOW)),
            Err(EngineError::Malformed { .. })
        ));
    }
}
