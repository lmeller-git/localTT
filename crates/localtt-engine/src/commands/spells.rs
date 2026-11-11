//! Spell-list CRUD: adding, editing and removing spell entries.
//!
//! The level is a sheet snapshot, so an unresolved or homebrew spell still
//! groups and casts. References are validated by kind but never required to
//! resolve; a missing source degrades to the name snapshot.

use localtt_core::types::{
    AnyEntity,
    EntityId,
    RefId,
    SpellEntry,
    SpellEntryId,
    SpellcastingBlock,
    SpellcastingBlockId,
};

use super::{CommandEnv, NewSpell, SpellPlacement, Transition, load_statblock, one_upsert, stamp};
use crate::{
    error::{ElementRef, EngineError},
    state::WorkingSet,
};

/// The `AddSpell` command: mints one entry and appends it to an existing or
/// new spellcasting block.
pub(super) fn add_spell_command(
    state: &dyn WorkingSet,
    entity: EntityId,
    placement: &SpellPlacement,
    new_spell: &NewSpell,
    expected_rev: Option<u64>,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError> {
    let mut statblock = load_statblock(state, entity, expected_rev)?;
    let name = validated_name(
        &new_spell.name,
        new_spell.reference.as_ref(),
        new_spell.level,
    )?;
    let entry = SpellEntry {
        id: SpellEntryId::new(),
        name,
        reference: new_spell.reference.clone(),
        level: new_spell.level,
        prepared: new_spell.prepared,
    };
    match placement {
        SpellPlacement::Existing(block) => {
            let Some(target) = statblock
                .body
                .spellcasting
                .iter_mut()
                .find(|candidate| candidate.id == *block)
            else {
                return Err(EngineError::NotFound {
                    entity,
                    element: Some(ElementRef::SpellcastingBlock(*block)),
                });
            };
            target.spells.push(entry);
        }
        SpellPlacement::New { source, ability } => {
            statblock.body.spellcasting.push(SpellcastingBlock {
                id: SpellcastingBlockId::new(),
                source: source.clone(),
                ability: *ability,
                spells: vec![entry],
            });
        }
    }
    let mut document = AnyEntity::from(statblock);
    stamp(&mut document, env.now());
    Ok(one_upsert(document, Vec::new()))
}

/// The fields of one [`super::Command::EditSpell`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SpellEdit<'a> {
    pub(super) name: &'a str,
    pub(super) reference: Option<&'a RefId>,
    pub(super) level: Option<u8>,
    pub(super) prepared: bool,
}

/// The `EditSpell` command: replaces every client-editable field.
pub(super) fn edit_spell_command(
    state: &dyn WorkingSet,
    entity: EntityId,
    spell: SpellEntryId,
    edit: SpellEdit<'_>,
    expected_rev: Option<u64>,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError> {
    let mut statblock = load_statblock(state, entity, expected_rev)?;
    let name = validated_name(edit.name, edit.reference, edit.level)?;
    let Some(stored) = statblock
        .body
        .spellcasting
        .iter_mut()
        .flat_map(|block| block.spells.iter_mut())
        .find(|candidate| candidate.id == spell)
    else {
        return Err(EngineError::NotFound {
            entity,
            element: Some(ElementRef::Spell(spell)),
        });
    };
    stored.name = name;
    stored.reference = edit.reference.cloned();
    stored.level = edit.level;
    stored.prepared = edit.prepared;
    let mut document = AnyEntity::from(statblock);
    stamp(&mut document, env.now());
    Ok(one_upsert(document, Vec::new()))
}

/// The `RemoveSpell` command: removes one entry from its block. The block
/// itself is kept, because it carries the source and ability.
pub(super) fn remove_spell_command(
    state: &dyn WorkingSet,
    entity: EntityId,
    spell: SpellEntryId,
    expected_rev: Option<u64>,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError> {
    let mut statblock = load_statblock(state, entity, expected_rev)?;
    let Some((block_index, spell_index)) =
        statblock
            .body
            .spellcasting
            .iter()
            .enumerate()
            .find_map(|(block_index, block)| {
                block
                    .spells
                    .iter()
                    .position(|candidate| candidate.id == spell)
                    .map(|spell_index| (block_index, spell_index))
            })
    else {
        return Err(EngineError::NotFound {
            entity,
            element: Some(ElementRef::Spell(spell)),
        });
    };
    statblock.body.spellcasting[block_index]
        .spells
        .remove(spell_index);
    let mut document = AnyEntity::from(statblock);
    stamp(&mut document, env.now());
    Ok(one_upsert(document, Vec::new()))
}

/// Trims the name and checks the level and reference kind.
fn validated_name(
    name: &str,
    reference: Option<&RefId>,
    level: Option<u8>,
) -> Result<String, EngineError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(EngineError::Malformed {
            reason: "a spell name must not be empty".to_owned(),
        });
    }
    if let Some(level) = level
        && level > 9
    {
        return Err(EngineError::Malformed {
            reason: format!("spell level {level} is outside 0..=9"),
        });
    }
    if let Some(reference) = reference
        && reference.kind() != "spell"
    {
        return Err(EngineError::Malformed {
            reason: format!("reference {reference} is not a spell"),
        });
    }
    Ok(name.to_owned())
}
