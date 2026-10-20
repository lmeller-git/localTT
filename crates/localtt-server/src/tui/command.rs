//! Prompt parsing into engine commands (`docs/rfcs/server.md` 3).
//!
//! The TUI addresses statuses, resources and spells by their 1-based position
//! in the selected entity's view. The parser is pure: it reads the target ids
//! and the current campaign clock, and returns one [`Command`] or a status-line
//! error string.

use localtt_core::types::{
    CampaignTime,
    DeathSaves,
    Entity,
    EntityId,
    EntityKind,
    Ext,
    HandoutBody,
    HandoutContent,
    InlineItem,
    InventoryEntryId,
    InventoryItem,
    ItemKind,
    ItemState,
    PlayerId,
    ResourceId,
    SpellEntryId,
    StatKey,
    StatusDuration,
    StatusId,
    StatusKind,
};
use localtt_engine::{
    commands::{Command, RollSpec, StatusSpec},
    dice,
};

/// The ids the prompt parser may address on the selected entity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionTarget {
    /// The selected entity.
    pub id: EntityId,
    /// Status ids in view order.
    pub statuses: Vec<StatusId>,
    /// Resource ids in view order.
    pub resources: Vec<ResourceId>,
    /// Spell ids in block order.
    pub spells: Vec<SpellEntryId>,
    /// Inventory entry ids in view order.
    pub items: Vec<InventoryEntryId>,
    /// Statblocks a handout can be given to: id and display name.
    pub recipients: Vec<(EntityId, String)>,
}

/// One input prompt's command grammar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptKind {
    /// Damage the selected entity.
    Damage,
    /// Heal the selected entity.
    Heal,
    /// Set the selected entity's temporary HP.
    TempHp,
    /// Replace the selected entity's death saves.
    DeathSaves,
    /// Roll an expression or a check.
    Roll,
    /// Add a custom status.
    AddStatus,
    /// Remove a status by 1-based index.
    RemoveStatus,
    /// Replace a status's timed duration.
    EditStatus,
    /// Spend a resource by 1-based index.
    Spend,
    /// Restore a resource by 1-based index.
    Restore,
    /// Apply a short rest.
    ShortRest,
    /// Apply a long rest.
    LongRest,
    /// Cast a spell by 1-based spell and slot indexes.
    Cast,
    /// Begin (or replace) concentration.
    BeginConcentration,
    /// Spend a hit-die resource.
    HitDie,
    /// Add an inline inventory item.
    AddItem,
    /// Edit one inventory entry field.
    EditItem,
    /// Remove one inventory entry.
    RemoveItem,
    /// Give the selected handout to a roster member.
    Give,
    /// Move the campaign clock.
    AdvanceClock,
    /// Set the campaign clock.
    SetClock,
}

impl PromptKind {
    /// The prompt label shown on the status line.
    #[must_use]
    #[inline]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Damage => "damage amount",
            Self::Heal => "heal amount",
            Self::TempHp => "temporary HP",
            Self::DeathSaves => "death saves: successes failures",
            Self::Roll => "roll: 1d20+3 or check <stat>",
            Self::AddStatus => "status name",
            Self::RemoveStatus => "remove status #",
            Self::EditStatus => "status # new duration seconds",
            Self::Spend => "spend resource # amount",
            Self::Restore => "restore resource # amount",
            Self::ShortRest => "short rest advance seconds (blank = 0)",
            Self::LongRest => "long rest advance seconds (blank = 0)",
            Self::Cast => "cast spell # slot # [upcast]",
            Self::BeginConcentration => "concentration status name",
            Self::HitDie => "hit die resource #",
            Self::AddItem => "item: name[, qty[, carried|equipped|stowed]]",
            Self::EditItem => "item # qty N | state S | attuned on|off | notes text",
            Self::RemoveItem => "remove item #",
            Self::Give => "give to player # or name",
            Self::AdvanceClock => "advance clock seconds",
            Self::SetClock => "set clock seconds",
        }
    }
}

/// Parses one prompt buffer into a command.
///
/// # Errors
///
/// Returns a short, user-facing message when the input is malformed, an index
/// is outside the selected view, or the command needs a selected entity.
#[inline]
pub fn parse(
    kind: PromptKind,
    input: &str,
    target: Option<&ActionTarget>,
    now: CampaignTime,
) -> Result<Command, String> {
    let input = input.trim();
    match kind {
        PromptKind::Damage => Ok(Command::ApplyDamage {
            entity: require_target(target)?.id,
            amount: parse_i32(input, "damage")?,
            expected_rev: None,
        }),
        PromptKind::Heal => Ok(Command::Heal {
            entity: require_target(target)?.id,
            amount: parse_i32(input, "healing")?,
            expected_rev: None,
        }),
        PromptKind::TempHp => Ok(Command::SetTempHp {
            entity: require_target(target)?.id,
            amount: parse_i32(input, "temporary HP")?,
            expected_rev: None,
        }),
        PromptKind::DeathSaves => {
            let target = require_target(target)?;
            let mut parts = input.split([' ', '/']).filter(|part| !part.is_empty());
            let successes = parts
                .next()
                .ok_or_else(|| "expected successes and failures".to_owned())?
                .parse::<u8>()
                .map_err(|_| "successes must be a number".to_owned())?;
            let failures = parts
                .next()
                .ok_or_else(|| "expected successes and failures".to_owned())?
                .parse::<u8>()
                .map_err(|_| "failures must be a number".to_owned())?;
            if parts.next().is_some() {
                return Err("expected exactly successes and failures".to_owned());
            }
            Ok(Command::SetDeathSaves {
                entity: target.id,
                value: DeathSaves {
                    successes,
                    failures,
                },
                expected_rev: None,
            })
        }
        PromptKind::Roll => {
            if let Some(stat) = input.strip_prefix("check ") {
                let target = require_target(target)?;
                let stat = StatKey::parse(stat.trim())
                    .map_err(|_| format!("unknown stat {}", stat.trim()))?;
                return Ok(Command::Roll {
                    entity: Some(target.id),
                    spec: RollSpec::Check {
                        stat,
                        target_dc: None,
                    },
                    audience: None,
                });
            }
            let expr = dice::parse(input).map_err(|error| error.to_string())?;
            Ok(Command::Roll {
                entity: target.map(|target| target.id),
                spec: RollSpec::Expression { expr, target: None },
                audience: None,
            })
        }
        PromptKind::AddStatus => {
            let target = require_target(target)?;
            let name = non_empty(input, "status name")?.to_owned();
            Ok(Command::AddStatus {
                entity: target.id,
                status: custom_status(name, StatusKind::Custom),
                expected_rev: None,
            })
        }
        PromptKind::RemoveStatus => {
            let target = require_target(target)?;
            let index = parse_index(input, target.statuses.len(), "status")?;
            Ok(Command::RemoveStatus {
                entity: target.id,
                status: target.statuses[index],
                expected_rev: None,
            })
        }
        PromptKind::EditStatus => {
            let target = require_target(target)?;
            let mut parts = input.split_whitespace();
            let index = parts
                .next()
                .ok_or_else(|| "expected a status index".to_owned())?;
            let index = parse_index(index, target.statuses.len(), "status")?;
            let seconds = parts
                .next()
                .ok_or_else(|| "expected new duration seconds".to_owned())?;
            if parts.next().is_some() {
                return Err("expected status # and seconds".to_owned());
            }
            let seconds = parse_i64(seconds, "duration")?;
            Ok(Command::EditStatusDuration {
                entity: target.id,
                status: target.statuses[index],
                duration: StatusDuration::Timed {
                    expires_at: CampaignTime::from_seconds(now.seconds().saturating_add(seconds)),
                },
                expected_rev: None,
            })
        }
        PromptKind::Spend => {
            let (target, index, amount) = resource_command(target, input)?;
            Ok(Command::SpendResource {
                entity: target.id,
                resource: target.resources[index],
                amount,
                expected_rev: None,
            })
        }
        PromptKind::Restore => {
            let (target, index, amount) = resource_command(target, input)?;
            Ok(Command::RestoreResource {
                entity: target.id,
                resource: target.resources[index],
                amount,
                expected_rev: None,
            })
        }
        PromptKind::ShortRest => Ok(Command::ShortRest {
            entity: require_target(target)?.id,
            advance_seconds: optional_seconds(input)?,
            expected_rev: None,
        }),
        PromptKind::LongRest => Ok(Command::LongRest {
            entity: require_target(target)?.id,
            advance_seconds: optional_seconds(input)?,
            expected_rev: None,
        }),
        PromptKind::Cast => {
            let target = require_target(target)?;
            let mut parts = input.split_whitespace();
            let spell = parts
                .next()
                .ok_or_else(|| "expected a spell index".to_owned())?;
            let spell = parse_index(spell, target.spells.len(), "spell")?;
            let slot = parts
                .next()
                .ok_or_else(|| "expected a slot resource index".to_owned())?;
            let slot = parse_index(slot, target.resources.len(), "slot resource")?;
            let upcast = match parts.next() {
                Some(raw) => raw
                    .parse::<u32>()
                    .map_err(|_| "upcast must be a non-negative number".to_owned())?,
                None => 0,
            };
            if parts.next().is_some() {
                return Err("expected spell #, slot # and optional upcast".to_owned());
            }
            Ok(Command::CastSpell {
                entity: target.id,
                spell: target.spells[spell],
                slot: target.resources[slot],
                upcast_slots: upcast,
                concentrate: false,
                expected_rev: None,
            })
        }
        PromptKind::BeginConcentration => {
            let target = require_target(target)?;
            let name = non_empty(input, "status name")?.to_owned();
            Ok(Command::BeginConcentration {
                entity: target.id,
                status: custom_status(name, StatusKind::Concentration),
                expected_rev: None,
            })
        }
        PromptKind::HitDie => {
            let target = require_target(target)?;
            let index = parse_index(input, target.resources.len(), "hit die resource")?;
            Ok(Command::SpendHitDie {
                entity: target.id,
                resource: target.resources[index],
                expected_rev: None,
            })
        }
        PromptKind::AddItem => {
            let target = require_target(target)?;
            let mut parts = input.splitn(3, ',');
            let name = parts.next().unwrap_or_default().trim();
            if name.is_empty() {
                return Err("expected an item name".to_owned());
            }
            let qty = match parts.next().map(str::trim).filter(|part| !part.is_empty()) {
                Some(raw) => parse_i32(raw, "quantity")?,
                None => 1,
            };
            let item_state = match parts.next().map(str::trim).filter(|part| !part.is_empty()) {
                Some(raw) => parse_item_state(raw)?,
                None => ItemState::Carried,
            };
            Ok(Command::AddInventoryItem {
                entity: target.id,
                item: Box::new(inline_item(name)),
                qty,
                state: item_state,
                attuned: false,
                notes: String::new(),
                expected_rev: None,
            })
        }
        PromptKind::EditItem => {
            let target = require_target(target)?;
            let mut parts = input.splitn(3, char::is_whitespace);
            let index = parts
                .next()
                .ok_or_else(|| "expected an item index".to_owned())?;
            let index = parse_index(index, target.items.len(), "item")?;
            let field = parts
                .next()
                .ok_or_else(|| "expected qty, state, attuned or notes".to_owned())?;
            let value = parts.next().unwrap_or_default().trim();
            let entry = target.items[index];
            match field {
                "qty" => Ok(Command::EditInventoryItem {
                    entity: target.id,
                    entry,
                    qty: Some(parse_i32(non_empty(value, "quantity")?, "quantity")?),
                    state: None,
                    attuned: None,
                    notes: None,
                    expected_rev: None,
                }),
                "state" => Ok(Command::EditInventoryItem {
                    entity: target.id,
                    entry,
                    qty: None,
                    state: Some(parse_item_state(non_empty(value, "state")?)?),
                    attuned: None,
                    notes: None,
                    expected_rev: None,
                }),
                "attuned" => Ok(Command::EditInventoryItem {
                    entity: target.id,
                    entry,
                    qty: None,
                    state: None,
                    attuned: Some(parse_bool(non_empty(value, "attuned on or off")?)?),
                    notes: None,
                    expected_rev: None,
                }),
                "notes" => Ok(Command::EditInventoryItem {
                    entity: target.id,
                    entry,
                    qty: None,
                    state: None,
                    attuned: None,
                    notes: Some(value.to_owned()),
                    expected_rev: None,
                }),
                other => Err(format!(
                    "unknown field {other}; use qty, state, attuned or notes"
                )),
            }
        }
        PromptKind::RemoveItem => {
            let target = require_target(target)?;
            let index = parse_index(input, target.items.len(), "item")?;
            Ok(Command::RemoveInventoryItem {
                entity: target.id,
                entry: target.items[index],
                expected_rev: None,
            })
        }
        PromptKind::Give => {
            let target = require_target(target)?;
            if target.recipients.is_empty() {
                return Err("the campaign has no roster members".to_owned());
            }
            let recipient = parse_recipient(input, &target.recipients)?;
            Ok(Command::GiveHandout {
                handout: target.id,
                recipient,
                expected_rev: None,
            })
        }
        PromptKind::AdvanceClock => Ok(Command::AdvanceClock {
            by_seconds: parse_i64(input, "clock advance")?,
        }),
        PromptKind::SetClock => Ok(Command::SetClock {
            to: CampaignTime::from_seconds(parse_i64(input, "clock value")?),
        }),
    }
}

/// Builds the handout command for the handout form: `kind` is `text` or
/// `item`, `name` is the document name, `content` is the text or item name.
///
/// # Errors
///
/// A short message when the kind is unknown, a name is empty, or the document
/// builder refuses the kind/body pairing.
#[inline]
pub fn handout_command(
    kind: &str,
    name: &str,
    content: &str,
    actor: PlayerId,
    ruleset: &localtt_core::types::RulesetId,
) -> Result<Command, String> {
    let name = name.trim();
    let content = content.trim();
    if name.is_empty() {
        return Err("the handout name must not be empty".to_owned());
    }
    if content.is_empty() {
        return Err("the handout content must not be empty".to_owned());
    }
    let body = match kind.trim().to_ascii_lowercase().as_str() {
        "text" => HandoutBody {
            content: HandoutContent::Text {
                text: content.to_owned(),
            },
        },
        "item" => HandoutBody {
            content: HandoutContent::Item {
                item: match inline_item(content) {
                    InventoryItem::Inline(item) => item,
                    InventoryItem::Ref(_) => unreachable!("the helper builds inline items"),
                },
            },
        },
        other => return Err(format!("handout kind {other} is not text or item")),
    };
    let mut entity = Entity::builder(EntityKind::Handout, EntityId::new(), name, actor, body)
        .build()
        .map_err(|error| format!("could not build handout: {error}"))?;
    entity.ruleset = ruleset.clone();
    Ok(Command::CreateEntity {
        entity: Box::new(localtt_core::types::AnyEntity::from(entity)),
    })
}

fn inline_item(name: &str) -> InventoryItem {
    InventoryItem::Inline(InlineItem {
        name: name.to_owned(),
        description: String::new(),
        kind: ItemKind::Gear,
        requires_attunement: false,
        weight_oz: None,
        value: None,
        properties: None,
        modifiers: Vec::new(),
        resource: None,
        ext: Ext::new(),
    })
}

fn parse_item_state(raw: &str) -> Result<ItemState, String> {
    match raw.to_ascii_lowercase().as_str() {
        "carried" => Ok(ItemState::Carried),
        "equipped" => Ok(ItemState::Equipped),
        "stowed" => Ok(ItemState::Stowed),
        other => Err(format!(
            "item state {other} is not carried, equipped or stowed"
        )),
    }
}

fn parse_bool(raw: &str) -> Result<bool, String> {
    match raw.to_ascii_lowercase().as_str() {
        "on" | "true" | "yes" => Ok(true),
        "off" | "false" | "no" => Ok(false),
        other => Err(format!("{other} is not on or off")),
    }
}

fn parse_recipient(raw: &str, recipients: &[(EntityId, String)]) -> Result<EntityId, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("expected a recipient index or name".to_owned());
    }
    if raw.parse::<usize>().is_ok() {
        let index = parse_index(raw, recipients.len(), "recipient")?;
        return Ok(recipients[index].0);
    }
    recipients
        .iter()
        .find(|(_, name)| name.eq_ignore_ascii_case(raw))
        .map(|(id, _)| *id)
        .ok_or_else(|| format!("{raw} is not on the roster"))
}

fn custom_status(name: String, kind: StatusKind) -> StatusSpec {
    StatusSpec {
        kind,
        name,
        reference: None,
        text: None,
        duration: StatusDuration::Indefinite,
        source: None,
        modifiers: Vec::new(),
    }
}

fn require_target(target: Option<&ActionTarget>) -> Result<&ActionTarget, String> {
    target.ok_or_else(|| "select an entity first".to_owned())
}

fn resource_command<'a>(
    target: Option<&'a ActionTarget>,
    input: &str,
) -> Result<(&'a ActionTarget, usize, i32), String> {
    let target = require_target(target)?;
    let mut parts = input.split_whitespace();
    let index = parts
        .next()
        .ok_or_else(|| "expected a resource index".to_owned())?;
    let index = parse_index(index, target.resources.len(), "resource")?;
    let amount = parts
        .next()
        .ok_or_else(|| "expected an amount".to_owned())?;
    if parts.next().is_some() {
        return Err("expected resource # and amount".to_owned());
    }
    Ok((target, index, parse_i32(amount, "amount")?))
}

fn non_empty<'a>(input: &'a str, what: &str) -> Result<&'a str, String> {
    if input.is_empty() {
        Err(format!("expected {what}"))
    } else {
        Ok(input)
    }
}

fn parse_index(input: &str, len: usize, what: &str) -> Result<usize, String> {
    let index = input
        .parse::<usize>()
        .map_err(|_| format!("{what} index must be a number"))?;
    if index == 0 || index > len {
        return Err(format!("{what} index must be 1..={len}"));
    }
    Ok(index - 1)
}

fn parse_i32(input: &str, what: &str) -> Result<i32, String> {
    input
        .parse()
        .map_err(|_| format!("{what} must be a number"))
}

fn parse_i64(input: &str, what: &str) -> Result<i64, String> {
    input
        .parse()
        .map_err(|_| format!("{what} must be a number"))
}

fn optional_seconds(input: &str) -> Result<Option<i64>, String> {
    if input.is_empty() {
        return Ok(None);
    }
    let seconds = parse_i64(input, "advance seconds")?;
    if seconds < 0 {
        return Err("advance seconds must not be negative".to_owned());
    }
    Ok(Some(seconds))
}

#[cfg(test)]
mod tests {
    use localtt_core::types::{
        CampaignTime,
        DeathSaves,
        EntityId,
        InventoryEntryId,
        ResourceId,
        SpellEntryId,
        StatusDuration,
        StatusId,
        StatusKind,
    };
    use localtt_engine::commands::{Command, RollSpec};

    use super::{ActionTarget, PromptKind, parse};

    fn target() -> ActionTarget {
        ActionTarget {
            id: EntityId::new(),
            statuses: vec![StatusId::new()],
            resources: vec![ResourceId::new(), ResourceId::new()],
            spells: vec![SpellEntryId::new()],
            items: vec![InventoryEntryId::new()],
            recipients: vec![(EntityId::new(), "Aria".to_owned())],
        }
    }

    #[test]
    fn hp_commands_need_a_selected_entity_and_an_amount() {
        let target = target();
        assert!(matches!(
            parse(PromptKind::Damage, "12", Some(&target), CampaignTime::from_seconds(0)),
            Ok(Command::ApplyDamage { entity, amount: 12, .. }) if entity == target.id
        ));
        assert!(
            parse(
                PromptKind::Heal,
                "x",
                Some(&target),
                CampaignTime::from_seconds(0)
            )
            .is_err()
        );
        assert!(parse(PromptKind::TempHp, "3", None, CampaignTime::from_seconds(0)).is_err());
    }

    #[test]
    fn death_saves_accept_space_or_slash() {
        let target = target();
        let expected = DeathSaves {
            successes: 1,
            failures: 2,
        };
        for input in ["1 2", "1/2"] {
            assert!(matches!(
                parse(
                    PromptKind::DeathSaves,
                    input,
                    Some(&target),
                    CampaignTime::from_seconds(0)
                ),
                Ok(Command::SetDeathSaves { value, .. }) if value == expected
            ));
        }
        assert!(
            parse(
                PromptKind::DeathSaves,
                "1",
                Some(&target),
                CampaignTime::from_seconds(0)
            )
            .is_err()
        );
    }

    #[test]
    fn rolls_parse_expressions_and_checks() {
        let target = target();
        assert!(matches!(
            parse(
                PromptKind::Roll,
                "1d20+3",
                Some(&target),
                CampaignTime::from_seconds(0)
            ),
            Ok(Command::Roll {
                entity: Some(id),
                spec: RollSpec::Expression { .. },
                ..
            }) if id == target.id
        ));
        assert!(matches!(
            parse(
                PromptKind::Roll,
                "check save.dex",
                Some(&target),
                CampaignTime::from_seconds(0)
            ),
            Ok(Command::Roll {
                entity: Some(id),
                spec: RollSpec::Check { .. },
                ..
            }) if id == target.id
        ));
        assert!(
            parse(
                PromptKind::Roll,
                "check save.dex",
                None,
                CampaignTime::from_seconds(0)
            )
            .is_err()
        );
    }

    #[test]
    fn statuses_and_resources_use_one_based_indexes() {
        let target = target();
        assert!(matches!(
            parse(
                PromptKind::AddStatus,
                "Poisoned",
                Some(&target),
                CampaignTime::from_seconds(0)
            ),
            Ok(Command::AddStatus { status, .. })
                if status.kind == StatusKind::Custom && status.name == "Poisoned"
        ));
        assert!(matches!(
            parse(
                PromptKind::RemoveStatus,
                "1",
                Some(&target),
                CampaignTime::from_seconds(0)
            ),
            Ok(Command::RemoveStatus { status, .. }) if status == target.statuses[0]
        ));
        assert!(
            parse(
                PromptKind::RemoveStatus,
                "2",
                Some(&target),
                CampaignTime::from_seconds(0)
            )
            .is_err()
        );
        assert!(matches!(
            parse(
                PromptKind::Spend,
                "2 3",
                Some(&target),
                CampaignTime::from_seconds(0)
            ),
            Ok(Command::SpendResource { resource, amount: 3, .. })
                if resource == target.resources[1]
        ));
        assert!(matches!(
            parse(
                PromptKind::EditStatus,
                "1 600",
                Some(&target),
                CampaignTime::from_seconds(100)
            ),
            Ok(Command::EditStatusDuration {
                duration: StatusDuration::Timed { expires_at },
                ..
            }) if expires_at == CampaignTime::from_seconds(700)
        ));
    }

    #[test]
    fn casting_needs_both_indexes_and_optional_upcast() {
        let target = target();
        assert!(matches!(
            parse(
                PromptKind::Cast,
                "1 1 2",
                Some(&target),
                CampaignTime::from_seconds(0)
            ),
            Ok(Command::CastSpell {
                spell,
                slot,
                upcast_slots: 2,
                ..
            }) if spell == target.spells[0] && slot == target.resources[0]
        ));
        assert!(
            parse(
                PromptKind::Cast,
                "1",
                Some(&target),
                CampaignTime::from_seconds(0)
            )
            .is_err()
        );
    }

    #[test]
    fn item_prompts_build_inventory_commands() {
        let target = target();
        assert!(matches!(
            parse(
                PromptKind::AddItem,
                "Rope, 2, stowed",
                Some(&target),
                CampaignTime::from_seconds(0)
            ),
            Ok(Command::AddInventoryItem { entity, qty: 2, state, .. })
                if entity == target.id && state == localtt_core::types::ItemState::Stowed
        ));
        assert!(matches!(
            parse(
                PromptKind::AddItem,
                "Torch",
                Some(&target),
                CampaignTime::from_seconds(0)
            ),
            Ok(Command::AddInventoryItem { qty: 1, .. })
        ));
        assert!(matches!(
            parse(
                PromptKind::EditItem,
                "1 state equipped",
                Some(&target),
                CampaignTime::from_seconds(0)
            ),
            Ok(Command::EditInventoryItem { entry, state: Some(localtt_core::types::ItemState::Equipped), .. })
                if entry == target.items[0]
        ));
        assert!(matches!(
            parse(
                PromptKind::RemoveItem,
                "1",
                Some(&target),
                CampaignTime::from_seconds(0)
            ),
            Ok(Command::RemoveInventoryItem { entry, .. }) if entry == target.items[0]
        ));
        assert!(
            parse(
                PromptKind::EditItem,
                "1 color blue",
                Some(&target),
                CampaignTime::from_seconds(0)
            )
            .is_err()
        );
    }

    #[test]
    fn giving_a_handout_accepts_an_index_or_a_display_name() {
        let target = target();
        assert!(matches!(
            parse(
                PromptKind::Give,
                "1",
                Some(&target),
                CampaignTime::from_seconds(0)
            ),
            Ok(Command::GiveHandout { handout, recipient, .. })
                if handout == target.id && recipient == target.recipients[0].0
        ));
        assert!(matches!(
            parse(
                PromptKind::Give,
                "aria",
                Some(&target),
                CampaignTime::from_seconds(0)
            ),
            Ok(Command::GiveHandout { recipient, .. })
                if recipient == target.recipients[0].0
        ));
        assert!(
            parse(
                PromptKind::Give,
                "nobody",
                Some(&target),
                CampaignTime::from_seconds(0)
            )
            .is_err()
        );
    }

    #[test]
    fn clocks_and_rests_parse_signed_and_optional_offsets() {
        let target = target();
        assert!(matches!(
            parse(
                PromptKind::AdvanceClock,
                "-60",
                Some(&target),
                CampaignTime::from_seconds(0)
            ),
            Ok(Command::AdvanceClock { by_seconds: -60 })
        ));
        assert!(matches!(
            parse(
                PromptKind::ShortRest,
                "",
                Some(&target),
                CampaignTime::from_seconds(0)
            ),
            Ok(Command::ShortRest {
                advance_seconds: None,
                ..
            })
        ));
        assert!(
            parse(
                PromptKind::LongRest,
                "-1",
                Some(&target),
                CampaignTime::from_seconds(0)
            )
            .is_err()
        );
    }
}
