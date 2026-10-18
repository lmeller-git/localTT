//! Modifier collection: sources, condition gates, group expansion, labels and
//! warning scoping (RFC `rules.md` 5.3, 5.4, 5.5, 5.7 and section 6).

use localtt_core::types::{
    CampaignTime,
    Condition,
    FeatureEntryId,
    Modifier,
    ModifierOp,
    StatKey,
    StatblockEntity,
    StatusDuration,
};

use crate::{
    error::{ElementRef, Warning, WarningCode},
    rules::ContributionSource,
};

/// One modifier that passed its condition gate, carrying its unevaluated
/// operand until the derivation stage it targets.
pub(super) struct Pending {
    /// The stat or group the modifier targets.
    pub(super) target: StatKey,
    /// The element the modifier came from.
    pub(super) source: ContributionSource,
    /// Display attribution: the modifier's note, else the source's name.
    pub(super) label: String,
    /// The operation, carrying the unevaluated operand.
    pub(super) op: ModifierOp,
}

/// Everything one collection pass yields: applicable modifiers in source
/// order and the warnings its gates raised.
#[derive(Default)]
pub(super) struct Collected {
    /// Applicable modifiers.
    pub(super) pendings: Vec<Pending>,
    /// Entity-scoped warnings raised while collecting.
    pub(super) warnings: Vec<Warning>,
}

/// Collects the applicable modifiers of one entity at one instant.
///
/// TODO: inventory entries and statuses contribute through the same gates;
/// only the feature source is wired so far.
pub(super) fn collect(entity: &StatblockEntity, now: CampaignTime) -> Collected {
    let mut collected = Collected::default();
    for feature in &entity.body.features {
        for modifier in &feature.modifiers {
            match condition_outcome(&modifier.when, entity, now) {
                ConditionOutcome::Applies => collected.pendings.push(Pending {
                    target: modifier.target.clone(),
                    source: ContributionSource::Feature(feature.id),
                    label: label_for(modifier, &feature.name),
                    op: modifier.op.clone(),
                }),
                ConditionOutcome::Skipped => {}
                ConditionOutcome::Unmodeled => {
                    collected
                        .warnings
                        .push(unmodeled_condition(entity, feature.id, &feature.name));
                }
            }
        }
    }
    collected
}

/// How a condition gate resolved (`rules.md` 5.5).
enum ConditionOutcome {
    /// The modifier applies.
    Applies,
    /// The modifier is skipped silently.
    Skipped,
    /// The condition is unmodeled: skip and warn.
    Unmodeled,
}

/// Evaluates a condition gate for a feature-sourced modifier.
///
/// `Equipped`/`Attuned` are no-ops for features; `StatusActive` matches a
/// name snapshot case-insensitively with soft expiry; `Custom` is unmodeled.
fn condition_outcome(
    when: &Option<Condition>,
    entity: &StatblockEntity,
    now: CampaignTime,
) -> ConditionOutcome {
    match when {
        None => ConditionOutcome::Applies,
        Some(Condition::Equipped | Condition::Attuned) => ConditionOutcome::Skipped,
        Some(Condition::StatusActive { name }) => {
            if has_active_status(entity, name, now) {
                ConditionOutcome::Applies
            } else {
                ConditionOutcome::Skipped
            }
        }
        Some(Condition::Custom { .. }) => ConditionOutcome::Unmodeled,
    }
}

/// Whether an unexpired status matches `name` case-insensitively.
fn has_active_status(entity: &StatblockEntity, name: &str, now: CampaignTime) -> bool {
    entity
        .body
        .statuses
        .iter()
        .any(|status| status.name.eq_ignore_ascii_case(name) && !expired(&status.duration, now))
}

/// Soft expiry: only `Timed` statuses expire, at `now >= expires_at`.
fn expired(duration: &StatusDuration, now: CampaignTime) -> bool {
    match duration {
        StatusDuration::Timed { expires_at } => now >= *expires_at,
        StatusDuration::UntilRest { .. }
        | StatusDuration::Indefinite
        | StatusDuration::Manual { .. } => false,
    }
}

/// The display attribution: the modifier's note, else the source's name.
fn label_for(modifier: &Modifier, source_name: &str) -> String {
    match &modifier.note {
        Some(note) => note.clone(),
        None => source_name.to_owned(),
    }
}

/// The warning for a skipped `Condition::Custom` contribution.
fn unmodeled_condition(
    entity: &StatblockEntity,
    feature: FeatureEntryId,
    feature_name: &str,
) -> Warning {
    Warning {
        code: WarningCode::UnmodeledCondition,
        message: format!("the condition on {feature_name} is not modeled"),
        entity: Some(entity.id),
        element: Some(ElementRef::Feature(feature)),
    }
}

#[cfg(test)]
mod tests {
    use localtt_core::{
        builders::{StatblockBuilder, stat},
        types::{
            Abilities,
            EntityId,
            EntityKind,
            Ext,
            FeatureCategory,
            FeatureEntry,
            FeatureEntryId,
            Formula,
            Modifier,
            ModifierOp,
            PlayerId,
            Status,
            StatusDuration,
            StatusId,
            StatusKind,
        },
    };

    use super::*;

    fn feature(name: &str, modifiers: Vec<Modifier>) -> FeatureEntry {
        FeatureEntry {
            id: FeatureEntryId::new(),
            category: FeatureCategory::Feat,
            name: name.to_owned(),
            reference: None,
            text: None,
            resource_id: None,
            modifiers,
            ext: Ext::default(),
        }
    }

    fn modifier(when: Option<Condition>, note: Option<&str>) -> Modifier {
        Modifier {
            target: stat("ability_mod.str").expect("a literal stat key"),
            op: ModifierOp::Add {
                value: Formula::Const(1),
            },
            when,
            note: note.map(ToOwned::to_owned),
        }
    }

    fn hero(features: Vec<FeatureEntry>) -> StatblockEntity {
        let mut builder = StatblockBuilder::character();
        builder.abilities(Abilities::all(10)).hp(20, 20);
        for feature in features {
            builder.feature(feature);
        }
        builder
            .build_entity(EntityKind::Pc, EntityId::new(), "Tester", PlayerId::new())
            .expect("abilities and hp are set")
    }

    fn status(name: &str, duration: StatusDuration) -> Status {
        Status {
            id: StatusId::new(),
            kind: StatusKind::Effect,
            name: name.to_owned(),
            reference: None,
            text: None,
            started_at: CampaignTime::from_seconds(0),
            duration,
            source: None,
            modifiers: Vec::new(),
        }
    }

    #[test]
    fn feature_modifiers_are_collected_with_their_labels() {
        let feature = feature(
            "Lucky",
            vec![modifier(None, None), modifier(None, Some("Charm"))],
        );
        let id = feature.id;
        let entity = hero(vec![feature]);

        let collected = collect(&entity, CampaignTime::from_seconds(0));

        assert!(collected.warnings.is_empty());
        let labels: Vec<_> = collected
            .pendings
            .iter()
            .map(|pending| (pending.source, pending.label.as_str()))
            .collect();
        assert_eq!(
            labels,
            vec![
                (ContributionSource::Feature(id), "Lucky"),
                (ContributionSource::Feature(id), "Charm")
            ]
        );
    }

    #[test]
    fn feature_conditions_gate_silently() {
        let mut entity = hero(vec![feature(
            "Armored",
            vec![
                modifier(Some(Condition::Equipped), None),
                modifier(Some(Condition::Attuned), None),
            ],
        )]);
        entity
            .body
            .statuses
            .push(status("Bless", StatusDuration::Indefinite));
        entity.body.features.push(feature(
            "Blessed",
            vec![modifier(
                Some(Condition::StatusActive {
                    name: "bless".to_owned(),
                }),
                None,
            )],
        ));

        let collected = collect(&entity, CampaignTime::from_seconds(0));

        assert!(collected.warnings.is_empty());
        assert_eq!(collected.pendings.len(), 1);
        assert_eq!(collected.pendings[0].label, "Blessed");
    }

    #[test]
    fn expired_statuses_do_not_gate_a_contribution_in() {
        let mut entity = hero(vec![feature(
            "Fading",
            vec![modifier(
                Some(Condition::StatusActive {
                    name: "Bless".to_owned(),
                }),
                None,
            )],
        )]);
        entity.body.statuses.push(status(
            "Bless",
            StatusDuration::Timed {
                expires_at: CampaignTime::from_seconds(100),
            },
        ));

        let collected = collect(&entity, CampaignTime::from_seconds(100));

        assert!(collected.warnings.is_empty());
        assert!(collected.pendings.is_empty());
    }

    #[test]
    fn custom_conditions_warn_scoped_to_the_feature() {
        let feature = feature(
            "Raging",
            vec![modifier(
                Some(Condition::Custom {
                    name: "while raging".to_owned(),
                }),
                None,
            )],
        );
        let id = feature.id;
        let entity = hero(vec![feature]);

        let collected = collect(&entity, CampaignTime::from_seconds(0));

        assert!(collected.pendings.is_empty());
        assert_eq!(collected.warnings.len(), 1);
        assert_eq!(collected.warnings[0].code, WarningCode::UnmodeledCondition);
        assert_eq!(collected.warnings[0].entity, Some(entity.id));
        assert_eq!(collected.warnings[0].element, Some(ElementRef::Feature(id)));
    }
}
