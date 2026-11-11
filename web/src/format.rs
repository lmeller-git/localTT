//! Display formatting shared by the browser components.

use std::collections::BTreeMap;

use localtt_core::types::{
    CampaignTime,
    EntityKind,
    FeedPayload,
    InventoryItem,
    ItemState,
    OverrideValue,
    PlayerId,
    SpellEntry,
    StatKey,
};
use localtt_protocol::{
    ContributionDto,
    ContributionOpDto,
    EntityView,
    FeedEntryView,
    RosterEntryView,
    SheetViewDto,
};

/// A human-readable entity kind.
#[must_use]
pub fn kind_label(kind: EntityKind) -> &'static str {
    match kind {
        EntityKind::Pc => "Player character",
        EntityKind::Companion => "Companion",
        EntityKind::Summon => "Summon",
        EntityKind::Familiar => "Familiar",
        EntityKind::Npc => "NPC",
        EntityKind::Monster => "Monster",
        EntityKind::Note => "Note",
        EntityKind::Handout => "Handout",
    }
}

/// The campaign clock as a day and time.
#[must_use]
pub fn clock(value: CampaignTime) -> String {
    format!("Day {} {}", value.days_since_start(), value.time_of_day())
}

/// The visible name of an entity view.
#[must_use]
pub fn entity_name(view: &EntityView) -> String {
    view.name.clone().unwrap_or_else(|| "Unnamed".to_owned())
}

/// A player's display name, or their id when the roster does not know them.
#[must_use]
pub fn player_name(id: PlayerId, roster: &[RosterEntryView]) -> String {
    roster
        .iter()
        .find(|entry| entry.id == id)
        .map(|entry| entry.display_name.clone())
        .unwrap_or_else(|| id.to_string())
}

/// Formats an override or derived value.
#[must_use]
pub fn value_text(value: Option<&OverrideValue>) -> String {
    match value {
        Some(OverrideValue::Int(value)) => value.to_string(),
        Some(OverrideValue::Bool(value)) => {
            if *value {
                "yes".to_owned()
            } else {
                "no".to_owned()
            }
        }
        Some(OverrideValue::Text(value)) => value.clone(),
        None => "-".to_owned(),
    }
}

/// Reads one effective derived value from a sheet.
#[must_use]
pub fn stat_value(sheet: &SheetViewDto, key: &str) -> String {
    let Ok(key) = StatKey::parse(key) else {
        return "-".to_owned();
    };
    sheet
        .sheet
        .values
        .get(&key)
        .map(|value| value_text(value.effective.as_ref()))
        .unwrap_or_else(|| "-".to_owned())
}

/// Formats one contribution in a derived-value breakdown.
#[must_use]
pub fn contribution_text(contribution: &ContributionDto) -> String {
    let op = match contribution.op {
        ContributionOpDto::Set => "set",
        ContributionOpDto::Add => "add",
        ContributionOpDto::Min => "min",
        ContributionOpDto::Max => "max",
        ContributionOpDto::Advantage => "advantage",
        ContributionOpDto::Disadvantage => "disadvantage",
    };
    match contribution.value {
        Some(value) => format!("{op} {value} ({})", contribution.label),
        None => format!("{op} ({})", contribution.label),
    }
}

/// The name of an inventory entry's item.
#[must_use]
pub fn item_name(item: &InventoryItem) -> String {
    match item {
        InventoryItem::Ref(reference) => reference.name.clone(),
        InventoryItem::Inline(inline) => inline.name.clone(),
    }
}

/// A human-readable inventory state.
#[must_use]
pub const fn item_state_label(state: ItemState) -> &'static str {
    match state {
        ItemState::Carried => "carried",
        ItemState::Equipped => "equipped",
        ItemState::Stowed => "stowed",
    }
}

/// One readable feed line.
#[must_use]
pub fn feed_line(entry: &FeedEntryView, roster: &[RosterEntryView]) -> String {
    let time = clock(entry.clock_s);
    let text = match &entry.payload {
        FeedPayload::Roll { roll } => {
            let who = player_name(roll.roller, roster);
            let what = roll
                .context
                .as_ref()
                .map(|context| {
                    context
                        .stat
                        .as_ref()
                        .map(ToString::to_string)
                        .unwrap_or_else(|| format!("{:?}", context.kind).to_lowercase())
                })
                .unwrap_or_else(|| "roll".to_owned());
            format!("{who} rolled {} ({what})", roll.result.total)
        }
        FeedPayload::StatusExpired { name, .. } => {
            let who = entry
                .actor
                .map(|actor| player_name(actor, roster))
                .unwrap_or_else(|| "A status".to_owned());
            format!("{who}: {name} expired")
        }
        FeedPayload::ConcentrationEnded { name } => format!("Concentration ended: {name}"),
        FeedPayload::Hint { text } => format!("Hint: {text}"),
    };
    format!("{time}  {text}")
}

/// The sort rank that keeps unknown-level spells after the leveled ones.
const UNLEVELED_RANK: u16 = 1000;

/// A spell level as the paper sheet labels it: `0` is "Cantrips", `None` is
/// "Unleveled", and leveled spells use an ordinal.
#[must_use]
pub fn spell_level_label(level: Option<u8>) -> String {
    match level {
        None => "Unleveled".to_owned(),
        Some(0) => "Cantrips".to_owned(),
        Some(level) => format!("{level}{}", ordinal_suffix(level)),
    }
}

/// One level group of a spell list: its sort rank, its label and its spells.
///
/// Cantrips come first, then levels 1 to 9 in order, then unknown levels.
#[must_use]
pub fn spell_groups(spells: &[SpellEntry]) -> Vec<(u16, String, Vec<SpellEntry>)> {
    let mut groups: BTreeMap<u16, Vec<SpellEntry>> = BTreeMap::new();
    for spell in spells {
        let rank = spell.level.map_or(UNLEVELED_RANK, u16::from);
        groups.entry(rank).or_default().push(spell.clone());
    }
    groups
        .into_iter()
        .map(|(rank, spells)| {
            let label = if rank == UNLEVELED_RANK {
                spell_level_label(None)
            } else {
                spell_level_label(u8::try_from(rank).ok())
            };
            (rank, label, spells)
        })
        .collect()
}

fn ordinal_suffix(value: u8) -> &'static str {
    match (value % 10, value % 100) {
        (1, 11) | (2, 12) | (3, 13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    }
}

#[cfg(test)]
mod tests {
    use localtt_core::types::{EntityId, FeedEntryId, UnixTime};

    use super::*;

    #[test]
    fn the_clock_is_readable_and_handles_the_origin() {
        assert_eq!(clock(CampaignTime::from_seconds(0)), "Day 0 00:00:00");
        assert_eq!(clock(CampaignTime::from_seconds(90_061)), "Day 1 01:01:01");
    }

    #[test]
    fn values_cover_each_override_shape() {
        assert_eq!(value_text(Some(&OverrideValue::Int(3))), "3");
        assert_eq!(value_text(Some(&OverrideValue::Bool(true))), "yes");
        assert_eq!(
            value_text(Some(&OverrideValue::Text("blessed".to_owned()))),
            "blessed"
        );
        assert_eq!(value_text(None), "-");
    }

    #[test]
    fn hint_feed_lines_carry_the_clock_and_text() {
        let entry = FeedEntryView {
            id: FeedEntryId::new(1),
            ts: UnixTime::from_seconds(0),
            clock_s: CampaignTime::from_seconds(3_600),
            actor: None,
            entity: Some(EntityId::new()),
            payload: FeedPayload::Hint {
                text: "remember concentration".to_owned(),
            },
        };
        assert_eq!(
            feed_line(&entry, &[]),
            "Day 0 01:00:00  Hint: remember concentration"
        );
    }

    #[test]
    fn every_entity_kind_has_a_label() {
        for kind in [
            EntityKind::Pc,
            EntityKind::Companion,
            EntityKind::Summon,
            EntityKind::Familiar,
            EntityKind::Npc,
            EntityKind::Monster,
            EntityKind::Note,
            EntityKind::Handout,
        ] {
            assert!(!kind_label(kind).is_empty());
        }
    }

    #[test]
    fn inventory_states_have_form_labels() {
        assert_eq!(item_state_label(ItemState::Carried), "carried");
        assert_eq!(item_state_label(ItemState::Equipped), "equipped");
        assert_eq!(item_state_label(ItemState::Stowed), "stowed");
    }

    fn spell(name: &str, level: Option<u8>) -> SpellEntry {
        SpellEntry {
            id: localtt_core::types::SpellEntryId::new(),
            name: name.to_owned(),
            reference: None,
            level,
            prepared: true,
        }
    }

    #[test]
    fn spell_levels_use_cantrips_and_ordinals() {
        assert_eq!(spell_level_label(None), "Unleveled");
        assert_eq!(spell_level_label(Some(0)), "Cantrips");
        assert_eq!(spell_level_label(Some(1)), "1st");
        assert_eq!(spell_level_label(Some(2)), "2nd");
        assert_eq!(spell_level_label(Some(3)), "3rd");
        assert_eq!(spell_level_label(Some(4)), "4th");
        assert_eq!(spell_level_label(Some(11)), "11th");
        assert_eq!(spell_level_label(Some(12)), "12th");
        assert_eq!(spell_level_label(Some(13)), "13th");
        assert_eq!(spell_level_label(Some(21)), "21st");
        assert_eq!(spell_level_label(Some(22)), "22nd");
        assert_eq!(spell_level_label(Some(23)), "23rd");
    }

    #[test]
    fn spell_groups_order_cantrips_levels_then_unleveled() {
        let spells = vec![
            spell("Unknown", None),
            spell("Fireball", Some(3)),
            spell("Spark", Some(0)),
            spell("Shield", Some(1)),
        ];

        let groups = spell_groups(&spells);

        let labels: Vec<&str> = groups.iter().map(|(_, label, _)| label.as_str()).collect();
        assert_eq!(labels, ["Cantrips", "1st", "3rd", "Unleveled"]);
        assert_eq!(groups[2].2[0].name, "Fireball");
        assert_eq!(groups[0].2[0].name, "Spark");
    }
}
