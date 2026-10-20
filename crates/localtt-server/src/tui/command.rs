//! Prompt parsing into engine commands (`docs/rfcs/server.md` 3).
//!
//! The TUI addresses statuses, resources and spells by their 1-based position
//! in the selected entity's view. The parser is pure: it reads the target ids
//! and the current campaign clock, and returns one [`Command`] or a status-line
//! error string.

use localtt_core::types::{
    CampaignTime,
    DeathSaves,
    EntityId,
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
        PromptKind::AdvanceClock => Ok(Command::AdvanceClock {
            by_seconds: parse_i64(input, "clock advance")?,
        }),
        PromptKind::SetClock => Ok(Command::SetClock {
            to: CampaignTime::from_seconds(parse_i64(input, "clock value")?),
        }),
    }
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
