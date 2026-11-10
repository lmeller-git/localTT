//! Initiative queue commands (see `docs/engine.md` section 2).
//!
//! The queue is plain campaign state: the DM chooses participants and may edit
//! every value; a player may update a queued entity they own. A turn advances
//! the current participant; wrapping back to the first one moves the campaign
//! clock by one round, so status durations stay coherent with the order.

use std::cmp::Reverse;

use localtt_core::types::{
    AnyEntity,
    CampaignTime,
    EntityId,
    Initiative,
    InitiativeEntry,
    PlayerId,
    SECONDS_PER_ROUND,
};

use super::{CommandEnv, Transition, campaign_change};
use crate::{error::EngineError, state::WorkingSet};

/// Adds a participant or replaces its value; a player may only update a queued
/// entity they own, while the DM may add any statblock.
pub(super) fn set_initiative_command(
    state: &dyn WorkingSet,
    entity: EntityId,
    value: i32,
    actor: Option<PlayerId>,
) -> Result<Transition, EngineError> {
    let campaign = state.campaign();
    let is_dm = actor.is_none_or(|actor| crate::authz::dm(campaign, actor));
    let Some(document) = state.entity(entity)? else {
        return Err(EngineError::NotFound {
            entity,
            element: None,
        });
    };
    let AnyEntity::Statblock(statblock) = document else {
        return Err(EngineError::Malformed {
            reason: format!("entity {entity} is not a statblock"),
        });
    };
    if let Some(actor) = actor
        && !is_dm
        && actor != statblock.owner
    {
        return Err(EngineError::Unauthorized {
            actor,
            entity: Some(entity),
        });
    }
    let mut campaign = campaign.clone();
    let position = campaign
        .initiative
        .entries
        .iter()
        .position(|entry| entry.entity == entity);
    match (is_dm, position) {
        (true, Some(index)) => {
            let entry = &mut campaign.initiative.entries[index];
            entry.name = statblock.name.clone();
            entry.owner = statblock.owner;
            entry.value = value;
        }
        (true, None) => campaign.initiative.entries.push(InitiativeEntry {
            entity,
            name: statblock.name.clone(),
            owner: statblock.owner,
            value,
        }),
        (false, Some(index)) => {
            let entry = &mut campaign.initiative.entries[index];
            entry.name = statblock.name.clone();
            entry.owner = statblock.owner;
            entry.value = value;
        }
        (false, None) => {
            return Err(EngineError::Malformed {
                reason: format!("entity {entity} is not in the initiative order"),
            });
        }
    }
    sort_entries(&mut campaign.initiative.entries);
    Ok(campaign_change(campaign))
}

/// Removes one participant; a missing entry is an empty transition.
pub(super) fn remove_initiative_command(
    state: &dyn WorkingSet,
    entity: EntityId,
) -> Result<Transition, EngineError> {
    let mut campaign = state.campaign().clone();
    let Some(index) = campaign
        .initiative
        .entries
        .iter()
        .position(|entry| entry.entity == entity)
    else {
        return Ok(Transition::empty());
    };
    let was_current = campaign.initiative.current == Some(entity);
    campaign.initiative.entries.remove(index);
    if campaign.initiative.entries.is_empty() {
        campaign.initiative.current = None;
        campaign.initiative.round = 0;
    } else if was_current {
        let next = index % campaign.initiative.entries.len();
        campaign.initiative.current = Some(campaign.initiative.entries[next].entity);
    }
    Ok(campaign_change(campaign))
}

/// Empties the queue; an already-empty queue is an empty transition.
pub(super) fn clear_initiative_command(state: &dyn WorkingSet) -> Result<Transition, EngineError> {
    let mut campaign = state.campaign().clone();
    if campaign.initiative == Initiative::default() {
        return Ok(Transition::empty());
    }
    campaign.initiative = Initiative::default();
    Ok(campaign_change(campaign))
}

/// Selects a specific participant without moving the clock.
pub(super) fn set_initiative_turn_command(
    state: &dyn WorkingSet,
    entity: EntityId,
) -> Result<Transition, EngineError> {
    let mut campaign = state.campaign().clone();
    if !campaign
        .initiative
        .entries
        .iter()
        .any(|entry| entry.entity == entity)
    {
        return Err(EngineError::Malformed {
            reason: format!("entity {entity} is not in the initiative order"),
        });
    }
    if campaign.initiative.current == Some(entity) {
        return Ok(Transition::empty());
    }
    campaign.initiative.current = Some(entity);
    if campaign.initiative.round == 0 {
        campaign.initiative.round = 1;
    }
    Ok(campaign_change(campaign))
}

/// Advances one place; starting the encounter and each non-wrapping turn leave
/// the clock alone, while wrapping back to the first participant ticks
/// [`SECONDS_PER_ROUND`].
pub(super) fn advance_initiative_command(
    state: &dyn WorkingSet,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError> {
    let campaign = state.campaign();
    let initiative = &campaign.initiative;
    if initiative.entries.is_empty() {
        return Ok(Transition::empty());
    }
    let (next, round, wrapped) = match initiative.current {
        None => (initiative.entries[0].entity, 1, false),
        Some(current) => match initiative
            .entries
            .iter()
            .position(|entry| entry.entity == current)
        {
            Some(index) if index + 1 == initiative.entries.len() => (
                initiative.entries[0].entity,
                initiative.round.saturating_add(1).max(1),
                true,
            ),
            Some(index) => (
                initiative.entries[index + 1].entity,
                initiative.round.max(1),
                false,
            ),
            None => (initiative.entries[0].entity, initiative.round.max(1), false),
        },
    };
    if !wrapped {
        let mut campaign = campaign.clone();
        campaign.initiative.current = Some(next);
        campaign.initiative.round = round;
        return Ok(campaign_change(campaign));
    }
    let old = campaign.clock.now_s;
    let Some(seconds) = old.seconds().checked_add(SECONDS_PER_ROUND) else {
        return Err(EngineError::Malformed {
            reason: "advancing the initiative round overflows campaign time".to_owned(),
        });
    };
    let to = CampaignTime::from_seconds(seconds);
    let Some(mut moved) = super::clock::clock_move(state, old, to)? else {
        let mut campaign = campaign.clone();
        campaign.initiative.current = Some(next);
        campaign.initiative.round = round;
        return Ok(campaign_change(campaign));
    };
    moved.campaign.initiative.current = Some(next);
    moved.campaign.initiative.round = round;
    Ok(super::clock::finish(state, moved, env))
}

/// Descending value; the stable sort keeps tied entries in their old relative
/// order, so the DM controls ties by the order participants were added.
fn sort_entries(entries: &mut [InitiativeEntry]) {
    entries.sort_by_key(|entry| Reverse(entry.value));
}

#[cfg(test)]
mod tests {
    use localtt_core::{
        builders::{CampaignBuilder, StatblockBuilder},
        types::{
            Abilities,
            Campaign,
            CampaignTime,
            Entity,
            EntityId,
            EntityKind,
            NoteBody,
            PlayerId,
            Status,
            StatusDuration,
            StatusId,
            StatusKind,
            UnixTime,
        },
    };

    use super::*;
    use crate::{ports::store::CampaignSnapshot, state::LoadedCampaign};

    const FIXED_TIME: i64 = 1_750_000_000;

    struct FixedEnv;

    impl CommandEnv for FixedEnv {
        fn now(&self) -> UnixTime {
            UnixTime::from_seconds(FIXED_TIME)
        }
    }

    fn party() -> (Campaign, PlayerId, PlayerId) {
        let mut builder = CampaignBuilder::new("Curse of the Vault");
        let dm = builder.dm("DM");
        let player = builder.player("Alex");
        (builder.build(), dm, player)
    }

    fn statblock(id: EntityId, name: &str, owner: PlayerId) -> AnyEntity {
        let mut builder = StatblockBuilder::character();
        builder.abilities(Abilities::all(10)).hp(12, 12);
        AnyEntity::from(
            builder
                .build_entity(EntityKind::Pc, id, name, owner)
                .expect("abilities and hp are set"),
        )
    }

    fn state_with(campaign: Campaign, entities: &[AnyEntity]) -> LoadedCampaign {
        LoadedCampaign::from_snapshot(CampaignSnapshot {
            campaign,
            entities: entities.to_vec(),
            feed: Vec::new(),
        })
    }

    fn set(
        state: &LoadedCampaign,
        entity: EntityId,
        value: i32,
        actor: Option<PlayerId>,
    ) -> Transition {
        set_initiative_command(state, entity, value, actor).expect("initiative is bookkeeping")
    }

    fn advance(state: &LoadedCampaign) -> Transition {
        advance_initiative_command(state, &mut FixedEnv).expect("initiative is bookkeeping")
    }

    #[test]
    fn the_dm_adds_entries_in_descending_order() {
        let (campaign, dm, player) = party();
        let slow = EntityId::new();
        let fast = EntityId::new();
        let entities = vec![
            statblock(slow, "Goblin", dm),
            statblock(fast, "Aria", player),
        ];

        let state = state_with(campaign, &entities);
        let first = set(&state, slow, 9, Some(dm));
        let campaign = first.campaign.expect("the campaign changed");
        let state = state_with(campaign, &entities);
        let transition = set(&state, fast, 18, Some(dm));

        let campaign = transition.campaign.expect("the campaign changed");
        assert_eq!(campaign.initiative.entries.len(), 2);
        assert_eq!(campaign.initiative.entries[0].entity, fast);
        assert_eq!(campaign.initiative.entries[0].name, "Aria");
        assert_eq!(campaign.initiative.entries[0].owner, player);
        assert_eq!(campaign.initiative.entries[0].value, 18);
        assert_eq!(campaign.initiative.entries[1].entity, slow);
        assert_eq!(
            transition.events,
            vec![super::super::Event::CampaignChanged]
        );
    }

    #[test]
    fn ties_keep_the_previous_relative_order() {
        let (campaign, dm, _player) = party();
        let first = EntityId::new();
        let second = EntityId::new();
        let entities = vec![
            statblock(first, "First", dm),
            statblock(second, "Second", dm),
        ];

        let state = state_with(campaign, &entities);
        let transition = set(&state, first, 12, Some(dm));
        let state = state_with(
            transition.campaign.expect("the campaign changed"),
            &entities,
        );
        let transition = set(&state, second, 12, Some(dm));

        let campaign = transition.campaign.expect("the campaign changed");
        assert_eq!(campaign.initiative.entries[0].entity, first);
        assert_eq!(campaign.initiative.entries[1].entity, second);
    }

    #[test]
    fn a_player_updates_only_their_own_queued_row() {
        let (campaign, dm, player) = party();
        let other = PlayerId::new();
        let own = EntityId::new();
        let foreign = EntityId::new();
        let entities = vec![
            statblock(own, "Aria", player),
            statblock(foreign, "Borin", other),
        ];
        let state = state_with(campaign, &entities);
        let transition = set(&state, own, 8, Some(dm));
        let state = state_with(
            transition.campaign.expect("the campaign changed"),
            &entities,
        );
        let transition = set(&state, foreign, 12, Some(dm));
        let state = state_with(
            transition.campaign.expect("the campaign changed"),
            &entities,
        );

        let transition = set(&state, own, 17, Some(player));
        let campaign = transition.campaign.expect("the campaign changed");
        assert_eq!(
            campaign
                .initiative
                .entries
                .iter()
                .find(|entry| entry.entity == own)
                .expect("the row exists")
                .value,
            17
        );

        let denied = set_initiative_command(&state, foreign, 20, Some(player));
        assert!(matches!(
            denied,
            Err(EngineError::Unauthorized { entity: Some(id), .. }) if id == foreign
        ));
    }

    #[test]
    fn a_player_cannot_add_a_participant() {
        let (campaign, _dm, player) = party();
        let own = EntityId::new();
        let entities = vec![statblock(own, "Aria", player)];
        let state = state_with(campaign, &entities);

        let denied = set_initiative_command(&state, own, 15, Some(player));
        assert!(matches!(denied, Err(EngineError::Malformed { .. })));
    }

    #[test]
    fn advancing_ticks_the_clock_only_when_the_order_wraps() {
        let (campaign, dm, player) = party();
        let fast = EntityId::new();
        let slow = EntityId::new();
        let entities = vec![
            statblock(fast, "Aria", player),
            statblock(slow, "Goblin", dm),
        ];
        let state = state_with(campaign, &entities);
        let transition = set(&state, fast, 18, Some(dm));
        let state = state_with(
            transition.campaign.expect("the campaign changed"),
            &entities,
        );
        let transition = set(&state, slow, 9, Some(dm));
        let state = state_with(
            transition.campaign.expect("the campaign changed"),
            &entities,
        );

        let first = advance(&state);
        let campaign = first.campaign.expect("the campaign changed");
        assert_eq!(campaign.initiative.current, Some(fast));
        assert_eq!(campaign.initiative.round, 1);
        assert_eq!(campaign.clock.now_s, CampaignTime::from_seconds(0));

        let state = state_with(campaign, &entities);
        let second = advance(&state);
        let campaign = second.campaign.expect("the campaign changed");
        assert_eq!(campaign.initiative.current, Some(slow));
        assert_eq!(campaign.initiative.round, 1);
        assert_eq!(campaign.clock.now_s, CampaignTime::from_seconds(0));

        let state = state_with(campaign, &entities);
        let third = advance(&state);
        let campaign = third.campaign.expect("the campaign changed");
        assert_eq!(campaign.initiative.current, Some(fast));
        assert_eq!(campaign.initiative.round, 2);
        assert_eq!(
            campaign.clock.now_s,
            CampaignTime::from_seconds(SECONDS_PER_ROUND)
        );

        let state = state_with(campaign, &entities);
        let fourth = advance(&state);
        let campaign = fourth.campaign.expect("the campaign changed");
        assert_eq!(campaign.initiative.current, Some(slow));
        assert_eq!(campaign.initiative.round, 2);
        assert_eq!(
            campaign.clock.now_s,
            CampaignTime::from_seconds(SECONDS_PER_ROUND)
        );
    }

    #[test]
    fn a_round_duration_survives_a_full_round_of_turns() {
        let (campaign, dm, player) = party();
        let marked = EntityId::new();
        let other = EntityId::new();
        let mut builder = StatblockBuilder::character();
        builder
            .abilities(Abilities::all(10))
            .hp(12, 12)
            .status(Status {
                id: StatusId::new(),
                kind: StatusKind::Condition,
                name: "Marked".to_owned(),
                reference: None,
                text: None,
                started_at: CampaignTime::from_seconds(0),
                duration: StatusDuration::Timed {
                    expires_at: CampaignTime::from_seconds(SECONDS_PER_ROUND),
                },
                source: None,
                modifiers: Vec::new(),
            });
        let entities = vec![
            AnyEntity::from(
                builder
                    .build_entity(EntityKind::Pc, marked, "Aria", player)
                    .expect("abilities and hp are set"),
            ),
            statblock(other, "Goblin", dm),
        ];
        let state = state_with(campaign, &entities);
        let transition = set(&state, marked, 18, Some(dm));
        let state = state_with(
            transition.campaign.expect("the campaign changed"),
            &entities,
        );
        let transition = set(&state, other, 9, Some(dm));
        let state = state_with(
            transition.campaign.expect("the campaign changed"),
            &entities,
        );

        let started = advance(&state);
        assert!(started.feed_appends.is_empty());
        let state = state_with(started.campaign.expect("the campaign changed"), &entities);

        let second_turn = advance(&state);
        assert!(
            second_turn.feed_appends.is_empty(),
            "a one-round status does not expire before the round wraps"
        );
        let state = state_with(
            second_turn.campaign.expect("the campaign changed"),
            &entities,
        );

        let wrapped = advance(&state);
        assert_eq!(
            wrapped
                .campaign
                .as_ref()
                .expect("the campaign changed")
                .clock
                .now_s,
            CampaignTime::from_seconds(SECONDS_PER_ROUND)
        );
        assert_eq!(
            wrapped.feed_appends.len(),
            1,
            "the round tick expires the timed status"
        );
    }

    #[test]
    fn the_dm_updates_an_existing_entry_and_refreshes_its_snapshot() {
        let (campaign, dm, player) = party();
        let id = EntityId::new();
        let entities = vec![statblock(id, "Aria", player)];
        let state = state_with(campaign, &entities);
        let transition = set(&state, id, 8, Some(dm));
        let state = state_with(
            transition.campaign.expect("the campaign changed"),
            &entities,
        );

        let transition = set(&state, id, 20, Some(dm));
        let campaign = transition.campaign.expect("the campaign changed");
        assert_eq!(campaign.initiative.entries.len(), 1);
        let entry = &campaign.initiative.entries[0];
        assert_eq!(entry.entity, id);
        assert_eq!(entry.name, "Aria");
        assert_eq!(entry.owner, player);
        assert_eq!(entry.value, 20);
    }

    #[test]
    fn a_non_statblock_participant_is_rejected() {
        let (campaign, dm, player) = party();
        let note = AnyEntity::from(
            Entity::builder(
                EntityKind::Note,
                EntityId::new(),
                "Session 1",
                player,
                NoteBody {
                    text: "The party opened the vault.".to_owned(),
                },
            )
            .build()
            .expect("a note kind pairs with a note body"),
        );
        let note_id = note.id();
        let state = state_with(campaign, &[note]);

        let result = set_initiative_command(&state, note_id, 12, Some(dm));
        assert!(matches!(result, Err(EngineError::Malformed { .. })));
    }

    #[test]
    fn removing_the_current_participant_selects_the_next() {
        let (campaign, dm, _player) = party();
        let first = EntityId::new();
        let second = EntityId::new();
        let entities = vec![
            statblock(first, "First", dm),
            statblock(second, "Second", dm),
        ];
        let state = state_with(campaign, &entities);
        let transition = set(&state, first, 18, Some(dm));
        let state = state_with(
            transition.campaign.expect("the campaign changed"),
            &entities,
        );
        let transition = set(&state, second, 9, Some(dm));
        let state = state_with(
            transition.campaign.expect("the campaign changed"),
            &entities,
        );
        let started = advance(&state);
        let state = state_with(started.campaign.expect("the campaign changed"), &entities);

        let transition = remove_initiative_command(&state, first).expect("removal is bookkeeping");
        let campaign = transition.campaign.expect("the campaign changed");
        assert_eq!(campaign.initiative.current, Some(second));
        assert_eq!(campaign.initiative.entries.len(), 1);
    }

    #[test]
    fn clearing_resets_the_turn_and_round() {
        let (campaign, dm, _player) = party();
        let id = EntityId::new();
        let entities = vec![statblock(id, "Aria", dm)];
        let state = state_with(campaign, &entities);
        let transition = set(&state, id, 12, Some(dm));
        let state = state_with(
            transition.campaign.expect("the campaign changed"),
            &entities,
        );
        let started = advance(&state);
        let state = state_with(started.campaign.expect("the campaign changed"), &entities);

        let transition = clear_initiative_command(&state).expect("clearing is bookkeeping");
        let campaign = transition.campaign.expect("the campaign changed");
        assert!(campaign.initiative.is_empty());
        assert_eq!(campaign.initiative.current, None);
        assert_eq!(campaign.initiative.round, 0);
    }

    #[test]
    fn setting_a_turn_starts_the_round_without_moving_the_clock() {
        let (campaign, dm, _player) = party();
        let id = EntityId::new();
        let entities = vec![statblock(id, "Aria", dm)];
        let state = state_with(campaign, &entities);
        let transition = set(&state, id, 12, Some(dm));
        let state = state_with(
            transition.campaign.expect("the campaign changed"),
            &entities,
        );

        let transition = set_initiative_turn_command(&state, id).expect("turns are bookkeeping");
        let campaign = transition.campaign.expect("the campaign changed");
        assert_eq!(campaign.initiative.current, Some(id));
        assert_eq!(campaign.initiative.round, 1);
        assert_eq!(campaign.clock.now_s, CampaignTime::from_seconds(0));
    }

    #[test]
    fn a_missing_entity_is_not_found() {
        let (campaign, dm, _player) = party();
        let state = state_with(campaign, &[]);
        let missing = EntityId::new();

        let result = set_initiative_command(&state, missing, 10, Some(dm));
        assert!(matches!(result, Err(EngineError::NotFound { .. })));
    }

    #[test]
    fn empty_and_missing_targets_are_empty_transitions() {
        let (campaign, _dm, _player) = party();
        let state = state_with(campaign, &[]);
        let missing = EntityId::new();

        assert!(
            remove_initiative_command(&state, missing)
                .expect("removing nothing is allowed")
                .campaign
                .is_none()
        );
        assert!(
            clear_initiative_command(&state)
                .expect("clearing nothing is allowed")
                .campaign
                .is_none()
        );
        assert!(
            advance_initiative_command(&state, &mut FixedEnv)
                .expect("advancing nothing is allowed")
                .campaign
                .is_none()
        );
        assert!(matches!(
            set_initiative_turn_command(&state, missing),
            Err(EngineError::Malformed { .. })
        ));
    }

    #[test]
    fn setting_the_current_turn_again_is_an_empty_transition() {
        let (campaign, dm, _player) = party();
        let id = EntityId::new();
        let entities = vec![statblock(id, "Aria", dm)];
        let state = state_with(campaign, &entities);
        let transition = set(&state, id, 12, Some(dm));
        let state = state_with(
            transition.campaign.expect("the campaign changed"),
            &entities,
        );
        let started = advance(&state);
        let state = state_with(started.campaign.expect("the campaign changed"), &entities);

        let transition =
            set_initiative_turn_command(&state, id).expect("selecting the current turn is allowed");
        assert!(transition.campaign.is_none());
    }

    #[test]
    fn removing_the_last_participant_resets_the_encounter() {
        let (campaign, dm, _player) = party();
        let id = EntityId::new();
        let entities = vec![statblock(id, "Aria", dm)];
        let state = state_with(campaign, &entities);
        let transition = set(&state, id, 12, Some(dm));
        let state = state_with(
            transition.campaign.expect("the campaign changed"),
            &entities,
        );
        let started = advance(&state);
        let state = state_with(started.campaign.expect("the campaign changed"), &entities);

        let transition = remove_initiative_command(&state, id).expect("removal is bookkeeping");
        let campaign = transition.campaign.expect("the campaign changed");
        assert!(campaign.initiative.is_empty());
        assert_eq!(campaign.initiative.current, None);
        assert_eq!(campaign.initiative.round, 0);
    }

    #[test]
    fn a_wrapping_tick_that_overflows_is_malformed() {
        let (mut campaign, dm, _player) = party();
        campaign.clock.now_s = CampaignTime::from_seconds(i64::MAX - 3);
        let id = EntityId::new();
        let entities = vec![statblock(id, "Aria", dm)];
        let state = state_with(campaign, &entities);
        let transition = set(&state, id, 12, Some(dm));
        let state = state_with(
            transition.campaign.expect("the campaign changed"),
            &entities,
        );
        let started = advance(&state);
        let state = state_with(started.campaign.expect("the campaign changed"), &entities);

        let result = advance_initiative_command(&state, &mut FixedEnv);
        assert!(matches!(result, Err(EngineError::Malformed { .. })));
    }
}
