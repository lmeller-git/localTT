//! Command dispatch: one pure function from `(state, command)` to a [`Transition`].

use localtt_core::types::PlayerId;

use crate::{
    commands::{
        Command,
        CommandEnv,
        Transition,
        casting,
        clock,
        creation,
        dice_cmd,
        hp,
        initiative,
        inventory,
        players,
        resources,
        rests,
        spells,
        statuses,
        trading,
    },
    error::EngineError,
    state::WorkingSet,
};

/// The unauthenticated form: computes what should happen and what it costs with
/// no actor and no authorization pre-check. State is not modified.
///
/// Test support only: production code reaches commands through
/// [`Executor::plan_as`](crate::executor::Executor::plan_as), which checks the
/// actor first.
#[cfg(any(test, feature = "testing"))]
#[inline]
pub fn apply(
    state: &dyn WorkingSet,
    command: &Command,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError> {
    apply_as(state, command, env, None)
}

/// Computes one command's transition (see `docs/engine.md` section 4): U5
/// records the actor for roll attribution, and the actor-requiring handlers
/// reject `None`.
pub(crate) fn apply_as(
    state: &dyn WorkingSet,
    command: &Command,
    env: &mut dyn CommandEnv,
    actor: Option<PlayerId>,
) -> Result<Transition, EngineError> {
    match command {
        Command::AdvanceClock { by_seconds } => {
            clock::advance_clock_command(state, *by_seconds, env)
        }
        Command::SetClock { to } => clock::set_clock_command(state, *to, env),
        Command::SetInitiative { entity, value } => {
            initiative::set_initiative_command(state, *entity, *value, actor)
        }
        Command::RemoveInitiative { entity } => {
            initiative::remove_initiative_command(state, *entity)
        }
        Command::ClearInitiative => initiative::clear_initiative_command(state),
        Command::AdvanceInitiative => initiative::advance_initiative_command(state, env),
        Command::SetInitiativeTurn { entity } => {
            initiative::set_initiative_turn_command(state, *entity)
        }
        Command::CreateEntity { entity } => {
            creation::create_entity_command(state, entity.as_ref().clone(), actor, env.now())
        }
        Command::AddInventoryItem {
            entity,
            item,
            qty,
            state: item_state,
            attuned,
            notes,
            expected_rev,
        } => inventory::add_inventory_item_command(
            state,
            *entity,
            inventory::NewItem {
                item: item.as_ref().clone(),
                qty: *qty,
                state: *item_state,
                attuned: *attuned,
                notes: notes.clone(),
            },
            *expected_rev,
            env,
        ),
        Command::EditInventoryItem {
            entity,
            entry,
            qty,
            state: item_state,
            attuned,
            notes,
            expected_rev,
        } => inventory::edit_inventory_item_command(
            state,
            *entity,
            *entry,
            inventory::ItemEdit {
                qty: *qty,
                state: *item_state,
                attuned: *attuned,
                notes: notes.clone(),
            },
            *expected_rev,
            env,
        ),
        Command::RemoveInventoryItem {
            entity,
            entry,
            expected_rev,
        } => inventory::remove_inventory_item_command(state, *entity, *entry, *expected_rev, env),
        Command::GiveHandout {
            handout,
            recipient,
            expected_rev,
        } => inventory::give_handout_command(state, *handout, *recipient, *expected_rev, env),
        Command::TransferItems {
            source,
            moves,
            recipient,
            destination,
            expected_source_rev,
            expected_destination_rev,
        } => trading::transfer_items_command(
            state,
            trading::ItemTransfer {
                source: *source,
                moves,
                recipient: *recipient,
                destination: *destination,
                expected_source_rev: *expected_source_rev,
                expected_destination_rev: *expected_destination_rev,
            },
            actor,
            env,
        ),
        Command::TransferCoins {
            source,
            amount,
            recipient,
            destination,
            expected_source_rev,
            expected_destination_rev,
        } => trading::transfer_coins_command(
            state,
            trading::CoinTransfer {
                source: *source,
                amount: *amount,
                recipient: *recipient,
                destination: *destination,
                expected_source_rev: *expected_source_rev,
                expected_destination_rev: *expected_destination_rev,
            },
            actor,
            env,
        ),
        Command::TransferEntity {
            entity,
            recipient,
            expected_rev,
        } => trading::transfer_entity_command(
            state,
            trading::EntityTransfer {
                entity: *entity,
                recipient: *recipient,
                expected_rev: *expected_rev,
            },
            actor,
            env,
        ),
        Command::AddStatus {
            entity,
            status,
            expected_rev,
        } => statuses::add_status_command(state, *entity, status, *expected_rev, env),
        Command::RemoveStatus {
            entity,
            status,
            expected_rev,
        } => statuses::remove_status_command(state, *entity, *status, *expected_rev, env),
        Command::EditStatusDuration {
            entity,
            status,
            duration,
            expected_rev,
        } => statuses::edit_status_duration_command(
            state,
            *entity,
            *status,
            duration,
            *expected_rev,
            env,
        ),
        Command::ApplyDamage {
            entity,
            amount,
            expected_rev,
        } => hp::apply_damage_command(state, *entity, *amount, *expected_rev, env),
        Command::Heal {
            entity,
            amount,
            expected_rev,
        } => hp::heal_command(state, *entity, *amount, *expected_rev, env),
        Command::SetTempHp {
            entity,
            amount,
            expected_rev,
        } => hp::set_temp_hp_command(state, *entity, *amount, *expected_rev, env),
        Command::SetDeathSaves {
            entity,
            value,
            expected_rev,
        } => hp::set_death_saves_command(state, *entity, *value, *expected_rev, env),
        Command::SpendResource {
            entity,
            resource,
            amount,
            expected_rev,
        } => resources::spend_resource_command(
            state,
            *entity,
            *resource,
            *amount,
            *expected_rev,
            env,
        ),
        Command::RestoreResource {
            entity,
            resource,
            amount,
            expected_rev,
        } => resources::restore_resource_command(
            state,
            *entity,
            *resource,
            *amount,
            *expected_rev,
            env,
        ),
        Command::AddResource {
            entity,
            name,
            kind,
            max,
            current,
            recharge,
            expected_rev,
        } => resources::add_resource_command(
            state,
            *entity,
            resources::NewResource {
                name,
                kind: *kind,
                max,
                current: *current,
                recharge: *recharge,
            },
            *expected_rev,
            env,
        ),
        Command::EditResource {
            entity,
            resource,
            name,
            max,
            current,
            recharge,
            expected_rev,
        } => resources::edit_resource_command(
            state,
            *entity,
            *resource,
            resources::ResourceEdit {
                name: name.clone(),
                max: max.clone(),
                current: *current,
                recharge: *recharge,
            },
            *expected_rev,
            env,
        ),
        Command::RemoveResource {
            entity,
            resource,
            expected_rev,
        } => resources::remove_resource_command(state, *entity, *resource, *expected_rev, env),
        Command::ShortRest {
            entity,
            advance_seconds,
            expected_rev,
        } => rests::short_rest_command(state, *entity, *advance_seconds, *expected_rev, env),
        Command::LongRest {
            entity,
            advance_seconds,
            expected_rev,
        } => rests::long_rest_command(state, *entity, *advance_seconds, *expected_rev, env),
        Command::Roll {
            entity,
            spec,
            audience,
        } => dice_cmd::roll_command(state, *entity, spec, *audience, actor, env),
        Command::CastSpell {
            entity,
            spell,
            slot,
            upcast_slots,
            concentrate,
            expected_rev,
        } => casting::cast_spell_command(
            state,
            *entity,
            *spell,
            *slot,
            casting::CastOptions {
                upcast_slots: *upcast_slots,
                concentrate: *concentrate,
            },
            *expected_rev,
            env,
        ),
        Command::AddSpell {
            entity,
            placement,
            spell,
            expected_rev,
        } => spells::add_spell_command(state, *entity, placement, spell, *expected_rev, env),
        Command::EditSpell {
            entity,
            spell,
            name,
            reference,
            level,
            prepared,
            expected_rev,
        } => spells::edit_spell_command(
            state,
            *entity,
            *spell,
            spells::SpellEdit {
                name,
                reference: reference.as_ref(),
                level: *level,
                prepared: *prepared,
            },
            *expected_rev,
            env,
        ),
        Command::RemoveSpell {
            entity,
            spell,
            expected_rev,
        } => spells::remove_spell_command(state, *entity, *spell, *expected_rev, env),
        Command::BeginConcentration {
            entity,
            status,
            expected_rev,
        } => casting::begin_concentration_command(state, *entity, status, *expected_rev, env),
        Command::EndConcentration {
            entity,
            expected_rev,
        } => casting::end_concentration_command(state, *entity, *expected_rev, env),
        Command::SpendHitDie {
            entity,
            resource,
            expected_rev,
        } => resources::spend_hit_die_command(state, *entity, *resource, *expected_rev, actor, env),
        Command::AddPlayer {
            id,
            display_name,
            token_hash,
        } => players::add_player_command(state, *id, display_name, token_hash.as_ref(), env),
        Command::SetPlayerToken { player, token_hash } => {
            players::set_player_token_command(state, *player, token_hash.as_ref())
        }
        Command::RotateDmKey { player, token_hash } => {
            players::rotate_dm_key_command(state, *player, token_hash)
        }
        Command::RemovePlayer {
            player,
            reassign_to,
        } => players::remove_player_command(state, *player, *reassign_to, env),
    }
}
