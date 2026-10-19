//! Modifier collection: sources, condition gates, labels and warning scoping
//! (RFC `rules.md` 5.3, 5.5, 5.7 and section 6).

use localtt_core::types::{
    CampaignTime,
    Condition,
    EntityId,
    InlineItem,
    InventoryEntry,
    InventoryItem,
    ItemProperties,
    ItemState,
    Modifier,
    ModifierOp,
    StatKey,
    StatblockEntity,
    StatusDuration,
};

use crate::{
    content::{ContentData, ContentKind},
    error::{ElementRef, Warning, WarningCode},
    rules::{ContributionSource, RulesContext, element_for},
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

/// One inventory entry with its resolved item definition.
pub(super) struct ItemSource<'a> {
    /// The entry itself.
    pub(super) entry: &'a InventoryEntry,
    /// The item definition; `None` when a reference did not resolve.
    pub(super) item: Option<&'a InlineItem>,
    /// Whether the entry's quantity lets it contribute (`rules.md` 5.7).
    pub(super) usable: bool,
}

/// Everything one collection pass yields: applicable modifiers in source
/// order, resolved inventory entries in document order, and the warnings the
/// gates raised.
pub(super) struct Collected<'a> {
    /// Applicable modifiers.
    pub(super) pendings: Vec<Pending>,
    /// Every inventory entry, resolved or not, in `inventory` order.
    pub(super) items: Vec<ItemSource<'a>>,
    /// Entity-scoped warnings raised while collecting.
    pub(super) warnings: Vec<Warning>,
}

/// Collects the applicable modifiers and resolved items of one entity at one
/// instant (`rules.md` 5.3): features, then inventory, then statuses.
pub(super) fn collect<'a>(entity: &'a StatblockEntity, ctx: &'a RulesContext<'a>) -> Collected<'a> {
    let mut collector = Collector {
        entity,
        now: ctx.now,
        pendings: Vec::new(),
        items: Vec::with_capacity(entity.body.inventory.len()),
        warnings: Vec::new(),
    };
    collector.features();
    collector.inventory(ctx);
    collector.statuses();
    collector.attunement();
    Collected {
        pendings: collector.pendings,
        items: collector.items,
        warnings: collector.warnings,
    }
}

/// The mutable state of one collection pass.
struct Collector<'a> {
    /// The entity being collected.
    entity: &'a StatblockEntity,
    /// The campaign instant used for status expiry.
    now: CampaignTime,
    /// Applicable modifiers so far, in source order.
    pendings: Vec<Pending>,
    /// Resolved inventory entries so far.
    items: Vec<ItemSource<'a>>,
    /// Warnings so far.
    warnings: Vec<Warning>,
}

impl<'a> Collector<'a> {
    /// Feature modifiers in `features` order.
    fn features(&mut self) {
        let entity = self.entity;
        for feature in &entity.body.features {
            for modifier in &feature.modifiers {
                self.push(
                    modifier,
                    Origin {
                        source: ContributionSource::Feature(feature.id),
                        name: &feature.name,
                        state: SourceState::Inert,
                    },
                );
            }
        }
    }

    /// Item modifiers in `inventory` order, resolving references through the
    /// context (`rules.md` 5.6) and gating on quantity (`rules.md` 5.7).
    fn inventory(&mut self, ctx: &'a RulesContext<'a>) {
        let entity = self.entity;
        for entry in &entity.body.inventory {
            let item = resolve_item(entry, ctx, entity.id, &mut self.warnings);
            let usable = quantity_usable(entry, item, entity.id, &mut self.warnings);
            self.items.push(ItemSource {
                entry,
                item,
                usable,
            });
            let Some(item) = item else {
                continue;
            };
            if !usable {
                continue;
            }
            let state = SourceState::Item {
                equipped: entry.state == ItemState::Equipped,
                attuned: entry.attuned,
            };
            for modifier in &item.modifiers {
                self.push(
                    modifier,
                    Origin {
                        source: ContributionSource::Item(entry.id),
                        name: &item.name,
                        state,
                    },
                );
            }
        }
    }

    /// Status modifiers in `statuses` order. An expired `Timed` status stays
    /// on the sheet but contributes nothing (soft expiry, `rules.md` 5.5).
    fn statuses(&mut self) {
        let entity = self.entity;
        for status in &entity.body.statuses {
            if expired(&status.duration, self.now) {
                continue;
            }
            for modifier in &status.modifiers {
                self.push(
                    modifier,
                    Origin {
                        source: ContributionSource::Status(status.id),
                        name: &status.name,
                        state: SourceState::Inert,
                    },
                );
            }
        }
    }

    /// The over-attunement warning: more than three attuned entries warn once
    /// per derivation, element-less (`rules.md` 5.7).
    fn attunement(&mut self) {
        let attuned = self
            .entity
            .body
            .inventory
            .iter()
            .filter(|entry| entry.attuned)
            .count();
        if attuned > 3 {
            self.warnings.push(Warning {
                code: WarningCode::OverAttunement,
                message: format!("{attuned} attuned entries exceed the limit of 3"),
                entity: Some(self.entity.id),
                element: None,
            });
        }
    }

    /// Applies one modifier's condition gate and records it when it applies.
    fn push(&mut self, modifier: &Modifier, origin: Origin<'_>) {
        match condition_outcome(&modifier.when, origin.state, self.entity, self.now) {
            ConditionOutcome::Applies => self.pendings.push(Pending {
                target: modifier.target.clone(),
                source: origin.source,
                label: label_for(modifier, origin.name),
                op: modifier.op.clone(),
            }),
            ConditionOutcome::Skipped => {}
            ConditionOutcome::Unmodeled => self.warnings.push(Warning {
                code: WarningCode::UnmodeledCondition,
                message: format!("the condition on {} is not modeled", origin.name),
                entity: Some(self.entity.id),
                element: element_for(origin.source),
            }),
        }
    }
}

/// Where a candidate modifier comes from.
struct Origin<'a> {
    /// The element the modifier came from.
    source: ContributionSource,
    /// The source's display name.
    name: &'a str,
    /// What the source's own state can satisfy.
    state: SourceState,
}

/// What a source can satisfy `Equipped`/`Attuned` conditions against
/// (`rules.md` 5.5).
#[derive(Clone, Copy)]
enum SourceState {
    /// A feature or status: neither condition is ever satisfiable.
    Inert,
    /// An inventory entry and its state flags.
    Item {
        /// Whether the entry is equipped.
        equipped: bool,
        /// Whether the entry is attuned.
        attuned: bool,
    },
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

/// Evaluates a condition gate for one source.
fn condition_outcome(
    when: &Option<Condition>,
    state: SourceState,
    entity: &StatblockEntity,
    now: CampaignTime,
) -> ConditionOutcome {
    match when {
        None => ConditionOutcome::Applies,
        Some(Condition::Equipped) => match state {
            SourceState::Item { equipped: true, .. } => ConditionOutcome::Applies,
            SourceState::Item {
                equipped: false, ..
            }
            | SourceState::Inert => ConditionOutcome::Skipped,
        },
        Some(Condition::Attuned) => match state {
            SourceState::Item { attuned: true, .. } => ConditionOutcome::Applies,
            SourceState::Item { attuned: false, .. } | SourceState::Inert => {
                ConditionOutcome::Skipped
            }
        },
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

/// Resolves an entry's item definition: an inline payload as-is, a reference
/// through `ContentData::Item`. A reference that does not resolve to an item
/// warns `UnresolvedReference` scoped to the entry and yields nothing.
fn resolve_item<'a>(
    entry: &'a InventoryEntry,
    ctx: &'a RulesContext<'a>,
    entity: EntityId,
    warnings: &mut Vec<Warning>,
) -> Option<&'a InlineItem> {
    match &entry.item {
        InventoryItem::Inline(item) => Some(item),
        InventoryItem::Ref(reference) => {
            let resolved = ctx.content.get(&reference.id).and_then(|content| {
                if content.kind != ContentKind::Item {
                    return None;
                }
                match &content.data {
                    ContentData::Item(item) => Some(item),
                    _ => None,
                }
            });
            if resolved.is_none() {
                warnings.push(Warning {
                    code: WarningCode::UnresolvedReference,
                    message: format!("`{}` does not resolve to an item", reference.name),
                    entity: Some(entity),
                    element: Some(ElementRef::Item(entry.id)),
                });
            }
            resolved
        }
    }
}

/// Whether an entry's quantity lets it contribute. A non-positive entry that
/// carries modifiers or armor/shield properties warns `ZeroQuantity`; one that
/// could not contribute anyway stays silent (`rules.md` 5.7).
fn quantity_usable(
    entry: &InventoryEntry,
    item: Option<&InlineItem>,
    entity: EntityId,
    warnings: &mut Vec<Warning>,
) -> bool {
    if entry.qty > 0 {
        return true;
    }
    if let Some(item) = item {
        let carries_contribution = !item.modifiers.is_empty()
            || matches!(
                item.properties,
                Some(ItemProperties::Armor(_) | ItemProperties::Shield(_))
            );
        if carries_contribution {
            warnings.push(Warning {
                code: WarningCode::ZeroQuantity,
                message: format!(
                    "`{}` has quantity {} and contributes nothing",
                    item.name, entry.qty
                ),
                entity: Some(entity),
                element: Some(ElementRef::Item(entry.id)),
            });
        }
    }
    false
}

/// The display attribution: the modifier's note, else the source's name.
fn label_for(modifier: &Modifier, source_name: &str) -> String {
    match &modifier.note {
        Some(note) => note.clone(),
        None => source_name.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use localtt_core::{
        builders::{StatblockBuilder, content_ref, stat},
        types::{
            Abilities,
            EntityId,
            EntityKind,
            Ext,
            FeatureCategory,
            FeatureEntry,
            FeatureEntryId,
            Formula,
            InventoryEntry,
            InventoryEntryId,
            InventoryItem,
            ItemKind,
            ItemState,
            Modifier,
            ModifierOp,
            PlayerId,
            RefId,
            Status,
            StatusDuration,
            StatusId,
            StatusKind,
        },
    };

    use super::*;
    use crate::{content::ContentEntry, rules::RulesContext};

    fn context(content: &BTreeMap<RefId, ContentEntry>) -> RulesContext<'_> {
        RulesContext {
            now: CampaignTime::from_seconds(0),
            content,
        }
    }

    fn hero() -> StatblockEntity {
        let mut builder = StatblockBuilder::character();
        builder.abilities(Abilities::all(10)).hp(20, 20);
        builder
            .build_entity(EntityKind::Pc, EntityId::new(), "Tester", PlayerId::new())
            .expect("abilities and hp are set")
    }

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

    fn inline(name: &str, modifiers: Vec<Modifier>) -> InventoryItem {
        InventoryItem::Inline(InlineItem {
            name: name.to_owned(),
            description: String::new(),
            kind: ItemKind::Wondrous,
            requires_attunement: false,
            weight_oz: None,
            value: None,
            properties: None,
            modifiers,
            resource: None,
            ext: Ext::default(),
        })
    }

    fn entry(
        id: InventoryEntryId,
        item: InventoryItem,
        qty: i32,
        state: ItemState,
        attuned: bool,
    ) -> InventoryEntry {
        InventoryEntry {
            id,
            item,
            qty,
            state,
            attuned,
            notes: String::new(),
        }
    }

    fn status(name: &str, duration: StatusDuration, modifiers: Vec<Modifier>) -> Status {
        Status {
            id: StatusId::new(),
            kind: StatusKind::Effect,
            name: name.to_owned(),
            reference: None,
            text: None,
            started_at: CampaignTime::from_seconds(0),
            duration,
            source: None,
            modifiers,
        }
    }

    #[test]
    fn modifiers_from_all_three_sources_are_collected_in_order() {
        let mut entity = hero();
        let brave = feature("Brave", vec![modifier(None, None)]);
        let brave_id = brave.id;
        entity.body.features.push(brave);
        let blade = InventoryEntryId::new();
        entity.body.inventory.push(entry(
            blade,
            inline("Lucky Blade", vec![modifier(None, None)]),
            1,
            ItemState::Carried,
            false,
        ));
        let bless = status(
            "Bless",
            StatusDuration::Indefinite,
            vec![modifier(None, None)],
        );
        let bless_id = bless.id;
        entity.body.statuses.push(bless);
        let content = BTreeMap::new();

        let ctx = context(&content);
        let collected = collect(&entity, &ctx);

        assert!(collected.warnings.is_empty());
        let sources: Vec<_> = collected
            .pendings
            .iter()
            .map(|pending| pending.source)
            .collect();
        assert_eq!(
            sources,
            vec![
                ContributionSource::Feature(brave_id),
                ContributionSource::Item(blade),
                ContributionSource::Status(bless_id),
            ]
        );
    }

    #[test]
    fn item_equipped_and_attuned_conditions_gate_on_the_entry_state() {
        let mut entity = hero();
        entity.body.inventory.push(entry(
            InventoryEntryId::new(),
            inline(
                "Ring of War",
                vec![
                    modifier(Some(Condition::Equipped), None),
                    modifier(Some(Condition::Attuned), None),
                ],
            ),
            1,
            ItemState::Carried,
            false,
        ));
        let content = BTreeMap::new();

        let ctx = context(&content);
        let collected = collect(&entity, &ctx);
        assert!(collected.pendings.is_empty());

        entity.body.inventory[0].state = ItemState::Equipped;
        entity.body.inventory[0].attuned = true;
        let ctx = context(&content);
        let collected = collect(&entity, &ctx);
        assert_eq!(collected.pendings.len(), 2);
    }

    #[test]
    fn feature_and_status_equipped_conditions_gate_silently() {
        let mut entity = hero();
        entity.body.features.push(feature(
            "Armored",
            vec![
                modifier(Some(Condition::Equipped), None),
                modifier(Some(Condition::Attuned), None),
            ],
        ));
        entity.body.statuses.push(status(
            "Warded",
            StatusDuration::Indefinite,
            vec![modifier(Some(Condition::Equipped), None)],
        ));
        let content = BTreeMap::new();

        let ctx = context(&content);
        let collected = collect(&entity, &ctx);

        assert!(collected.warnings.is_empty());
        assert!(collected.pendings.is_empty());
    }

    #[test]
    fn status_active_matches_case_insensitively_and_ignores_expiry() {
        let mut entity = hero();
        entity.body.features.push(feature(
            "Blessed",
            vec![modifier(
                Some(Condition::StatusActive {
                    name: "bless".to_owned(),
                }),
                None,
            )],
        ));
        entity.body.statuses.push(status(
            "Bless",
            StatusDuration::Timed {
                expires_at: CampaignTime::from_seconds(100),
            },
            Vec::new(),
        ));
        let content = BTreeMap::new();

        let before = context(&content);
        assert_eq!(collect(&entity, &before).pendings.len(), 1);

        let after = RulesContext {
            now: CampaignTime::from_seconds(100),
            content: &content,
        };
        assert!(collect(&entity, &after).pendings.is_empty());
    }

    #[test]
    fn expired_status_modifiers_stop_counting() {
        let mut entity = hero();
        entity.body.statuses.push(status(
            "Bless",
            StatusDuration::Timed {
                expires_at: CampaignTime::from_seconds(100),
            },
            vec![modifier(None, None)],
        ));
        let content = BTreeMap::new();

        let before = context(&content);
        assert_eq!(collect(&entity, &before).pendings.len(), 1);

        let after = RulesContext {
            now: CampaignTime::from_seconds(100),
            content: &content,
        };
        assert!(collect(&entity, &after).pendings.is_empty());
    }

    #[test]
    fn custom_conditions_warn_scoped_to_their_source() {
        let mut entity = hero();
        entity.body.features.push(feature(
            "Raging",
            vec![modifier(
                Some(Condition::Custom {
                    name: "while raging".to_owned(),
                }),
                None,
            )],
        ));
        let rage = status(
            "Raging",
            StatusDuration::Indefinite,
            vec![modifier(
                Some(Condition::Custom {
                    name: "while raging".to_owned(),
                }),
                None,
            )],
        );
        let rage_id = rage.id;
        entity.body.statuses.push(rage);
        let content = BTreeMap::new();

        let ctx = context(&content);
        let collected = collect(&entity, &ctx);

        assert!(collected.pendings.is_empty());
        assert_eq!(collected.warnings.len(), 2);
        assert!(
            collected
                .warnings
                .iter()
                .all(|warning| warning.code == WarningCode::UnmodeledCondition)
        );
        assert!(
            collected
                .warnings
                .iter()
                .any(|warning| warning.element == Some(ElementRef::Status(rage_id)))
        );
    }

    #[test]
    fn labels_fall_back_from_note_to_source_name() {
        let mut entity = hero();
        entity.body.inventory.push(entry(
            InventoryEntryId::new(),
            inline(
                "Lucky Blade",
                vec![modifier(None, None), modifier(None, Some("Charm"))],
            ),
            1,
            ItemState::Carried,
            false,
        ));
        let content = BTreeMap::new();

        let ctx = context(&content);
        let collected = collect(&entity, &ctx);

        let labels: Vec<&str> = collected
            .pendings
            .iter()
            .map(|pending| pending.label.as_str())
            .collect();
        assert_eq!(labels, vec!["Lucky Blade", "Charm"]);
    }

    #[test]
    fn unresolved_references_warn_once_per_entry() {
        let mut entity = hero();
        let missing = InventoryEntryId::new();
        entity.body.inventory.push(entry(
            missing,
            InventoryItem::Ref(
                content_ref("fixture:item/ring-of-protection", "Ring of Protection")
                    .expect("a literal reference"),
            ),
            1,
            ItemState::Equipped,
            true,
        ));
        let content = BTreeMap::new();

        let ctx = context(&content);
        let collected = collect(&entity, &ctx);

        assert!(collected.items[0].item.is_none());
        assert_eq!(collected.warnings.len(), 1);
        assert_eq!(collected.warnings[0].code, WarningCode::UnresolvedReference);
        assert_eq!(
            collected.warnings[0].element,
            Some(ElementRef::Item(missing))
        );
    }

    #[test]
    fn zero_quantity_warns_only_when_the_entry_could_contribute() {
        let mut entity = hero();
        let empty = InventoryEntryId::new();
        entity.body.inventory.push(entry(
            empty,
            inline("Empty Vial", Vec::new()),
            0,
            ItemState::Carried,
            false,
        ));
        let ghost = InventoryEntryId::new();
        entity.body.inventory.push(entry(
            ghost,
            inline("Phantom Ring", vec![modifier(None, None)]),
            0,
            ItemState::Equipped,
            false,
        ));
        let content = BTreeMap::new();

        let ctx = context(&content);
        let collected = collect(&entity, &ctx);

        assert!(collected.pendings.is_empty());
        assert!(collected.items.iter().all(|source| !source.usable));
        assert_eq!(collected.warnings.len(), 1);
        assert_eq!(collected.warnings[0].code, WarningCode::ZeroQuantity);
        assert_eq!(collected.warnings[0].element, Some(ElementRef::Item(ghost)));
    }

    #[test]
    fn over_attunement_warns_once_element_less() {
        let mut entity = hero();
        for index in 0..4 {
            entity.body.inventory.push(entry(
                InventoryEntryId::new(),
                inline(&format!("Charm {index}"), Vec::new()),
                1,
                ItemState::Carried,
                true,
            ));
        }
        let content = BTreeMap::new();

        let ctx = context(&content);
        let collected = collect(&entity, &ctx);

        let over_attunement: Vec<_> = collected
            .warnings
            .iter()
            .filter(|warning| warning.code == WarningCode::OverAttunement)
            .collect();
        assert_eq!(over_attunement.len(), 1);
        assert_eq!(over_attunement[0].element, None);
    }
}
