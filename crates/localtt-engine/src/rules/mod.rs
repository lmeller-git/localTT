//! The rules unit (RFC `rules.md`): static derivation of a sheet and the
//! roll-parts seam U5 consumes.
//!
//! [`derive()`] and [`roll_parts()`] are pure: the entity, the context and the
//! campaign instant are the only inputs, and nothing here does I/O
//! (RFC `rules.md` 5.1).

mod armor;
mod collect;
mod formula;
mod tables;

use std::collections::BTreeMap;

use localtt_core::types::{
    Abilities,
    Ability,
    CampaignTime,
    DiceExpr,
    EntityId,
    FeatureEntryId,
    Formula,
    InventoryEntryId,
    KnownStat,
    ModifierOp,
    OverrideValue,
    RefId,
    ResourceId,
    StatKey,
    StatblockEntity,
    StatusId,
};

use crate::{
    content::ContentEntry,
    error::{ElementRef, EngineError, Warning, WarningCode},
};

/// Inputs a derivation needs beyond the entity: the campaign instant and the
/// materialized content entries the entity's references resolve to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RulesContext<'a> {
    /// Campaign instant used for status expiry and time-gated rules.
    pub now: CampaignTime,
    /// Materialized entries keyed by reference id; the caller resolves them
    /// (engine architecture 7.4). Missing entries degrade with warnings.
    pub content: &'a BTreeMap<RefId, ContentEntry>,
}

/// Derives the full sheet: values, resources, class tables, warnings.
#[must_use]
#[inline]
pub fn derive(entity: &StatblockEntity, ctx: &RulesContext<'_>) -> DerivedSheet {
    let collected = collect::collect(entity, ctx.now);
    let classes = tables::class_snapshots(entity);
    let level = tables::total_level(&classes);
    let proficiency_bonus = tables::proficiency_bonus(entity.body.profile, entity.body.cr, level);
    let stored = stored_scores(&entity.body.abilities);
    let bootstrap = formula::EvalEnv {
        abilities: &stored,
        proficiency_bonus,
        level,
        classes: &classes,
    };

    let mut values = BTreeMap::new();
    let mut scores = BTreeMap::new();
    let mut warnings = collected.warnings;

    // Stage: proficiency bonus, then the ability scores. The environment for
    // this stage keeps `AbilityMod` leaves reading the stored scores until the
    // score stage settles (`rules.md` 5.4).
    {
        let mut stage = StageCtx {
            entity,
            pendings: &collected.pendings,
            env: &bootstrap,
            warnings: &mut warnings,
        };

        let key = StatKey::Known(KnownStat::ProficiencyBonus);
        let base = base_contribution(ContributionOp::Set, proficiency_bonus);
        let staged = stage.stage_key(&key, base);
        values.insert(key, staged.into_value());

        for ability in ABILITIES {
            let key = StatKey::Known(KnownStat::AbilityScore(ability));
            let base = base_contribution(ContributionOp::Set, stored[&ability]);
            let staged = stage.stage_key(&key, base);
            scores.insert(
                ability,
                staged.computed.expect("an ability score has a base `Set`"),
            );
            values.insert(key, staged.into_value());
        }
    }

    // Stage: ability modifiers off the settled scores.
    let settled = formula::EvalEnv {
        abilities: &scores,
        proficiency_bonus,
        level,
        classes: &classes,
    };
    {
        let mut stage = StageCtx {
            entity,
            pendings: &collected.pendings,
            env: &settled,
            warnings: &mut warnings,
        };

        for ability in ABILITIES {
            let key = StatKey::Known(KnownStat::AbilityModifier(ability));
            let modifier = formula::eval(&Formula::ability_mod(ability), &settled)
                .expect("the six ability scores are settled")
                .value;
            let base = base_contribution(ContributionOp::Set, modifier);
            let staged = stage.stage_key(&key, base);
            values.insert(key, staged.into_value());
        }
    }

    apply_overrides(&mut values, &entity.body.overrides);

    // Resources, slots and hit dice land with their own stages (`rules.md`
    // 5.9).
    DerivedSheet {
        values,
        resources: BTreeMap::new(),
        slots: SlotMaxima {
            spell: BTreeMap::new(),
            pact: None,
        },
        hit_dice: BTreeMap::new(),
        warnings,
    }
}

/// Builds the flat part, attributed dice and hint for one stat roll (U5).
///
/// Errors with `Malformed` when the stat has no integer effective value (no
/// derived value, or an override that is not an integer) or when a dice leaf
/// sits under an operation the roller cannot represent (section 5.8).
#[inline]
pub fn roll_parts(
    entity: &StatblockEntity,
    stat: &StatKey,
    ctx: &RulesContext<'_>,
) -> Result<RollParts, EngineError> {
    todo!()
}

/// The static sheet of one entity at one instant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DerivedSheet {
    /// Derived stat values by canonical key.
    pub values: BTreeMap<StatKey, DerivedValue>,
    /// Derived maxima for the entity's tracked resources, keyed by element id.
    pub resources: BTreeMap<ResourceId, DerivedResource>,
    /// Class-table spell-slot capacity (not the tracked resources).
    pub slots: SlotMaxima,
    /// Hit-dice pools by die sides: how many dice of that size the entity has.
    pub hit_dice: BTreeMap<u8, i32>,
    /// Non-blocking rule outcomes, entity-scoped.
    pub warnings: Vec<Warning>,
}

/// One derived stat.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DerivedValue {
    /// The computed value, when the rules know how to compute one.
    pub computed: Option<i32>,
    /// The sheet's sparse override, when present.
    pub override_value: Option<OverrideValue>,
    /// `override_value` when present, else the computed integer; `None` when
    /// neither exists. The only value a client shows as "the" value.
    pub effective: Option<OverrideValue>,
    /// The contributions that produced `computed`, in application order.
    pub breakdown: Vec<Contribution>,
    /// Net advantage/disadvantage from contributions; `None` when no hint
    /// contribution applied, `Some(RollHint::Normal)` when they cancelled.
    pub hint: Option<RollHint>,
}

/// One contribution to a derived value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Contribution {
    /// Where the contribution came from.
    pub source: ContributionSource,
    /// Display attribution: `modifier.note`, else the source's name.
    pub label: String,
    /// The operation the source applied.
    pub op: ContributionOp,
    /// The operand; `None` for advantage/disadvantage.
    pub value: Option<i32>,
}

/// The origin of one contribution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContributionSource {
    /// The rules' own base value.
    Base,
    /// One inventory entry.
    Item(InventoryEntryId),
    /// One feature entry.
    Feature(FeatureEntryId),
    /// One status.
    Status(StatusId),
}

/// What a contribution does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContributionOp {
    /// Replace the running value.
    Set,
    /// Add to the running value.
    Add,
    /// Clamp to at least the operand.
    Min,
    /// Clamp to at most the operand.
    Max,
    /// Grant advantage (a roll hint, not a number).
    Advantage,
    /// Grant disadvantage (a roll hint, not a number).
    Disadvantage,
}

/// A tracked resource's derived maximum.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DerivedResource {
    /// The evaluated maximum; `None` when the formula is unevaluable here.
    pub max: Option<i32>,
    /// The evaluated formula as a single `Set` contribution, or empty when
    /// unevaluable.
    pub breakdown: Vec<Contribution>,
}

/// Spell-slot capacity from the class tables.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotMaxima {
    /// Regular slots by spell level, 1 to 9.
    pub spell: BTreeMap<u8, i32>,
    /// Pact Magic, when the entity has warlock levels.
    pub pact: Option<PactSlots>,
}

/// Pact Magic capacity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PactSlots {
    /// Slot level.
    pub level: u8,
    /// Number of slots.
    pub count: i32,
}

/// Net advantage or disadvantage on a roll.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RollHint {
    /// Neither, or a pairwise cancellation.
    Normal,
    /// Net advantage.
    Advantage,
    /// Net disadvantage.
    Disadvantage,
}

/// The flat part, attributed dice and hint for one roll of a stat (U5 seam).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RollParts {
    /// The integer the roll starts from.
    pub flat: i32,
    /// Dice terms from applicable modifiers, in application order.
    pub dice: Vec<DiceTerm>,
    /// The net hint.
    pub hint: RollHint,
}

/// One attributed dice term.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiceTerm {
    /// Display attribution from the contributing modifier.
    pub source: Option<String>,
    /// The dice expression.
    pub expr: DiceExpr,
}

// ---------------------------------------------------------------------------
// Derivation internals
// ---------------------------------------------------------------------------

/// The six abilities in canonical order.
const ABILITIES: [Ability; 6] = [
    Ability::Strength,
    Ability::Dexterity,
    Ability::Constitution,
    Ability::Intelligence,
    Ability::Wisdom,
    Ability::Charisma,
];

/// Display attribution of the rules' own base contributions.
const BASE_LABEL: &str = "base";

/// The stored ability scores as a map: the bootstrap environment's abilities.
fn stored_scores(abilities: &Abilities) -> BTreeMap<Ability, i32> {
    BTreeMap::from([
        (Ability::Strength, abilities.strength),
        (Ability::Dexterity, abilities.dexterity),
        (Ability::Constitution, abilities.constitution),
        (Ability::Intelligence, abilities.intelligence),
        (Ability::Wisdom, abilities.wisdom),
        (Ability::Charisma, abilities.charisma),
    ])
}

/// A structural base value as a contribution.
fn base_contribution(op: ContributionOp, value: i32) -> Contribution {
    Contribution {
        source: ContributionSource::Base,
        label: BASE_LABEL.to_owned(),
        op,
        value: Some(value),
    }
}

/// Applies the sheet's sparse overrides: a derived value keeps its computed
/// value and gains the override; an override on a key no stage derived still
/// materializes (`rules.md` 5.3).
fn apply_overrides(
    values: &mut BTreeMap<StatKey, DerivedValue>,
    overrides: &BTreeMap<StatKey, OverrideValue>,
) {
    for (key, override_value) in overrides {
        let value = values.entry(key.clone()).or_insert_with(|| DerivedValue {
            computed: None,
            override_value: None,
            effective: None,
            breakdown: Vec::new(),
            hint: None,
        });
        value.override_value = Some(override_value.clone());
    }
    for value in values.values_mut() {
        value.effective = match (&value.override_value, value.computed) {
            (Some(fixed), _) => Some(fixed.clone()),
            (None, Some(computed)) => Some(OverrideValue::Int(computed)),
            (None, None) => None,
        };
    }
}

/// The inputs one derivation stage needs.
struct StageCtx<'a, 'b> {
    /// The entity being derived.
    entity: &'a StatblockEntity,
    /// Every applicable modifier from the one collection pass.
    pendings: &'a [collect::Pending],
    /// The evaluation environment of the stage.
    env: &'b formula::EvalEnv<'b>,
    /// The warning sink.
    warnings: &'b mut Vec<Warning>,
}

impl StageCtx<'_, '_> {
    /// Resolves and stages one key: its structural base first, then every
    /// applicable modifier in source order.
    fn stage_key(&mut self, key: &StatKey, base: Contribution) -> Staged {
        let mut contributions = vec![base];
        for pending in self
            .pendings
            .iter()
            .filter(|pending| pending.target == *key)
        {
            let resolved = resolve(pending, self.env, self.entity.id);
            if let Some(contribution) = resolved.contribution {
                contributions.push(contribution);
            }
            if let Some(warning) = resolved.warning {
                self.warnings.push(warning);
            }
        }
        stage(contributions)
    }
}

/// A pending modifier's operand resolved for one stage.
struct Resolved {
    /// The contribution; `None` when the operand was unevaluable.
    contribution: Option<Contribution>,
    /// The warning the resolution raised, when it raised one.
    warning: Option<Warning>,
}

/// Evaluates one pending modifier's operand in `env`.
fn resolve(pending: &collect::Pending, env: &formula::EvalEnv<'_>, entity: EntityId) -> Resolved {
    let (op, operand) = match &pending.op {
        ModifierOp::Add { value } => (ContributionOp::Add, Some(value)),
        ModifierOp::Set { value } => (ContributionOp::Set, Some(value)),
        ModifierOp::Min { value } => (ContributionOp::Min, Some(value)),
        ModifierOp::Max { value } => (ContributionOp::Max, Some(value)),
        ModifierOp::Advantage => (ContributionOp::Advantage, None),
        ModifierOp::Disadvantage => (ContributionOp::Disadvantage, None),
    };
    let Some(operand) = operand else {
        return Resolved {
            contribution: Some(Contribution {
                source: pending.source,
                label: pending.label.clone(),
                op,
                value: None,
            }),
            warning: None,
        };
    };
    match formula::eval(operand, env) {
        Some(evaluated) => Resolved {
            contribution: Some(Contribution {
                source: pending.source,
                label: pending.label.clone(),
                op,
                value: Some(evaluated.value),
            }),
            warning: evaluated
                .flattened_dice
                .then(|| flattened_dice_warning(entity, pending)),
        },
        None => Resolved {
            contribution: None,
            warning: Some(unevaluable_formula_warning(entity, pending)),
        },
    }
}

/// The element a contribution source maps to, for warning scoping.
fn element_for(source: ContributionSource) -> Option<ElementRef> {
    match source {
        ContributionSource::Base => None,
        ContributionSource::Item(id) => Some(ElementRef::Item(id)),
        ContributionSource::Feature(id) => Some(ElementRef::Feature(id)),
        ContributionSource::Status(id) => Some(ElementRef::Status(id)),
    }
}

/// The warning for a contribution whose dice leaf flattened to 0.
fn flattened_dice_warning(entity: EntityId, pending: &collect::Pending) -> Warning {
    Warning {
        code: WarningCode::DiceInStaticContext,
        message: format!(
            "`{}` flattens a dice term to 0 in the static sheet",
            pending.label
        ),
        entity: Some(entity),
        element: element_for(pending.source),
    }
}

/// The warning for a contribution whose formula was unevaluable.
fn unevaluable_formula_warning(entity: EntityId, pending: &collect::Pending) -> Warning {
    Warning {
        code: WarningCode::FormulaUnevaluated,
        message: format!(
            "`{}` contributes nothing: its formula is unevaluable",
            pending.label
        ),
        entity: Some(entity),
        element: element_for(pending.source),
    }
}

/// The result of one key's contribution pipeline (`rules.md` 5.3).
struct Staged {
    /// The computed value, when a contribution established one.
    computed: Option<i32>,
    /// Every applied contribution, in application order.
    breakdown: Vec<Contribution>,
    /// The net roll hint; `None` when no hint contribution applied.
    hint: Option<RollHint>,
}

impl Staged {
    /// Wraps the pipeline result as a derived value, before overrides apply.
    fn into_value(self) -> DerivedValue {
        DerivedValue {
            computed: self.computed,
            override_value: None,
            effective: self.computed.map(OverrideValue::Int),
            breakdown: self.breakdown,
            hint: self.hint,
        }
    }
}

/// Applies one key's contributions: `Set`s first (last wins), then `Add`s,
/// then `Min`/`Max` clamps, then pairwise hint cancellation. The breakdown is
/// the audit trail in application order (`rules.md` 5.3).
fn stage(mut contributions: Vec<Contribution>) -> Staged {
    contributions.sort_by_key(|contribution| match contribution.op {
        ContributionOp::Set => 0,
        ContributionOp::Add => 1,
        ContributionOp::Min | ContributionOp::Max => 2,
        ContributionOp::Advantage | ContributionOp::Disadvantage => 3,
    });

    let mut computed = None;
    let mut advantage = 0_u32;
    let mut disadvantage = 0_u32;
    let mut breakdown = Vec::with_capacity(contributions.len());
    for contribution in contributions {
        match contribution.op {
            ContributionOp::Set => computed = contribution.value,
            ContributionOp::Add => {
                let addend = contribution.value.unwrap_or(0);
                computed = Some(computed.unwrap_or(0).saturating_add(addend));
            }
            ContributionOp::Min => {
                if let (Some(running), Some(operand)) = (computed, contribution.value) {
                    computed = Some(running.max(operand));
                }
            }
            ContributionOp::Max => {
                if let (Some(running), Some(operand)) = (computed, contribution.value) {
                    computed = Some(running.min(operand));
                }
            }
            ContributionOp::Advantage => advantage += 1,
            ContributionOp::Disadvantage => disadvantage += 1,
        }
        breakdown.push(contribution);
    }

    let hint = if advantage == 0 && disadvantage == 0 {
        None
    } else if advantage > disadvantage {
        Some(RollHint::Advantage)
    } else if disadvantage > advantage {
        Some(RollHint::Disadvantage)
    } else {
        Some(RollHint::Normal)
    };

    Staged {
        computed,
        breakdown,
        hint,
    }
}

#[cfg(test)]
mod tests {
    use localtt_core::{
        builders::{StatblockBuilder, dice_pool, stat},
        types::{
            EntityId,
            EntityKind,
            Ext,
            FeatureCategory,
            FeatureEntry,
            FeatureEntryId,
            FormulaExpr,
            Modifier,
            PlayerId,
        },
    };

    use super::*;

    fn context(content: &BTreeMap<RefId, ContentEntry>) -> RulesContext<'_> {
        RulesContext {
            now: CampaignTime::from_seconds(0),
            content,
        }
    }

    fn hero(abilities: Abilities) -> StatblockEntity {
        let mut builder = StatblockBuilder::character();
        builder.abilities(abilities).hp(20, 20);
        builder
            .build_entity(EntityKind::Pc, EntityId::new(), "Tester", PlayerId::new())
            .expect("abilities and hp are set")
    }

    fn aria() -> StatblockEntity {
        hero(Abilities {
            strength: 8,
            dexterity: 14,
            constitution: 14,
            intelligence: 18,
            wisdom: 12,
            charisma: 10,
        })
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

    fn modifier(target: &str, op: ModifierOp) -> Modifier {
        Modifier {
            target: stat(target).expect("a literal stat key"),
            op,
            when: None,
            note: None,
        }
    }

    fn add(amount: i32) -> ModifierOp {
        ModifierOp::Add {
            value: Formula::Const(amount),
        }
    }

    fn value<'a>(sheet: &'a DerivedSheet, key: &str) -> &'a DerivedValue {
        sheet
            .values
            .get(&stat(key).expect("a literal stat key"))
            .unwrap_or_else(|| panic!("{key} is derived"))
    }

    #[test]
    fn scores_and_modifiers_derive_from_the_stored_sheet() {
        let mut entity = aria();
        entity
            .body
            .features
            .push(feature("Lucky", vec![modifier("ability_mod.int", add(2))]));
        let content = BTreeMap::new();

        let sheet = derive(&entity, &context(&content));

        assert_eq!(value(&sheet, "proficiency_bonus").computed, Some(2));
        let score = value(&sheet, "ability.int");
        assert_eq!(score.computed, Some(18));
        assert_eq!(score.effective, Some(OverrideValue::Int(18)));
        assert_eq!(
            score.breakdown,
            vec![Contribution {
                source: ContributionSource::Base,
                label: "base".to_owned(),
                op: ContributionOp::Set,
                value: Some(18),
            }]
        );
        assert_eq!(value(&sheet, "ability_mod.str").computed, Some(-1));

        let modifier_value = value(&sheet, "ability_mod.int");
        assert_eq!(modifier_value.computed, Some(6));
        assert_eq!(
            modifier_value
                .breakdown
                .iter()
                .map(|contribution| (contribution.op, contribution.value))
                .collect::<Vec<_>>(),
            vec![
                (ContributionOp::Set, Some(4)),
                (ContributionOp::Add, Some(2))
            ]
        );
        assert!(sheet.warnings.is_empty());
    }

    #[test]
    fn sets_apply_before_adds_and_the_breakdown_keeps_application_order() {
        let mut entity = aria();
        entity.body.features.push(feature(
            "Curse",
            vec![
                modifier("ability_mod.int", add(1)),
                modifier(
                    "ability_mod.int",
                    ModifierOp::Set {
                        value: Formula::Const(12),
                    },
                ),
            ],
        ));

        let sheet = derive(&entity, &context(&BTreeMap::new()));

        let modifier_value = value(&sheet, "ability_mod.int");
        assert_eq!(
            modifier_value.computed,
            Some(13),
            "the set wins, then the add"
        );
        assert_eq!(
            modifier_value
                .breakdown
                .iter()
                .map(|contribution| contribution.op)
                .collect::<Vec<_>>(),
            vec![
                ContributionOp::Set,
                ContributionOp::Set,
                ContributionOp::Add,
            ]
        );
    }

    #[test]
    fn advantage_and_disadvantage_cancel_pairwise() {
        let mut entity = aria();
        entity.body.features.push(feature(
            "Focused",
            vec![modifier("ability_mod.int", ModifierOp::Advantage)],
        ));
        entity.body.features.push(feature(
            "Shaken",
            vec![modifier("ability_mod.int", ModifierOp::Disadvantage)],
        ));
        let content = BTreeMap::new();

        let sheet = derive(&entity, &context(&content));
        let modifier_value = value(&sheet, "ability_mod.int");
        assert_eq!(
            modifier_value.hint,
            Some(RollHint::Normal),
            "the pair cancels"
        );
        assert_eq!(
            modifier_value.computed,
            Some(4),
            "hints never change the value"
        );

        entity.body.features.push(feature(
            "Inspired",
            vec![modifier("ability_mod.int", ModifierOp::Advantage)],
        ));
        let sheet = derive(&entity, &context(&content));
        assert_eq!(
            value(&sheet, "ability_mod.int").hint,
            Some(RollHint::Advantage)
        );
    }

    #[test]
    fn overrides_win_and_materialize_missing_keys() {
        let mut entity = aria();
        entity.body.overrides.insert(
            stat("ability.int").expect("a literal stat"),
            OverrideValue::Int(20),
        );
        entity.body.overrides.insert(
            stat("attack.melee").expect("a literal stat"),
            OverrideValue::Int(7),
        );

        let sheet = derive(&entity, &context(&BTreeMap::new()));

        let int = value(&sheet, "ability.int");
        assert_eq!(int.computed, Some(18), "the computed value stays visible");
        assert_eq!(int.override_value, Some(OverrideValue::Int(20)));
        assert_eq!(int.effective, Some(OverrideValue::Int(20)));

        let attack = value(&sheet, "attack.melee");
        assert_eq!(attack.computed, None);
        assert_eq!(attack.override_value, Some(OverrideValue::Int(7)));
        assert_eq!(attack.effective, Some(OverrideValue::Int(7)));
        assert!(attack.breakdown.is_empty());
    }

    #[test]
    fn unevaluable_and_dice_operands_warn_and_do_not_fail() {
        let mut entity = aria();
        let cursed = feature(
            "Cursed",
            vec![modifier(
                "ability_mod.int",
                ModifierOp::Add {
                    value: Formula::Expr(FormulaExpr::FloorDiv {
                        lhs: Box::new(Formula::Const(1)),
                        rhs: Box::new(Formula::Const(0)),
                    }),
                },
            )],
        );
        let cursed_id = cursed.id;
        entity.body.features.push(cursed);
        let blessed = feature(
            "Blessed",
            vec![modifier(
                "ability_mod.int",
                ModifierOp::Add {
                    value: Formula::dice(dice_pool(1, 4)),
                },
            )],
        );
        let blessed_id = blessed.id;
        entity.body.features.push(blessed);

        let sheet = derive(&entity, &context(&BTreeMap::new()));

        assert_eq!(value(&sheet, "ability_mod.int").computed, Some(4));
        assert!(sheet.warnings.iter().any(|warning| {
            warning.code == WarningCode::FormulaUnevaluated
                && warning.element == Some(ElementRef::Feature(cursed_id))
        }));
        assert!(sheet.warnings.iter().any(|warning| {
            warning.code == WarningCode::DiceInStaticContext
                && warning.element == Some(ElementRef::Feature(blessed_id))
        }));
        assert!(
            sheet
                .warnings
                .iter()
                .all(|warning| warning.entity == Some(entity.id))
        );
    }
}
