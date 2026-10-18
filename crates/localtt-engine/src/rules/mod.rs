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
    AttackKind,
    CampaignTime,
    DiceExpr,
    EntityId,
    FeatureEntryId,
    Formula,
    FormulaExpr,
    InventoryEntryId,
    KnownStat,
    ModifierOp,
    OverrideValue,
    RefId,
    Resource,
    ResourceId,
    SkillProficiency,
    StatGroup,
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
    let collected = collect::collect(entity, ctx);
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
    // score stage settles (`rules.md` 5.2, 5.4).
    {
        let stage = FirstStage {
            entity: entity.id,
            pendings: &collected.pendings,
            env: &bootstrap,
        };

        let key = StatKey::Known(KnownStat::ProficiencyBonus);
        let staged = stage.stage_key(&key, proficiency_bonus, &mut warnings);
        values.insert(key, staged.into_value());

        for ability in ABILITIES {
            let key = StatKey::Known(KnownStat::AbilityScore(ability));
            let staged = stage.stage_key(&key, stored[&ability], &mut warnings);
            scores.insert(
                ability,
                staged.computed.expect("an ability score has a base `Set`"),
            );
            values.insert(key, staged.into_value());
        }
    }

    // One environment for everything downstream: `AbilityMod` reads the final
    // computed scores (`rules.md` 5.2).
    let settled = formula::EvalEnv {
        abilities: &scores,
        proficiency_bonus,
        level,
        classes: &classes,
    };

    // Resolve every post-bootstrap pending once, before staging: group members
    // share one resolution, so a warning fires once per contribution, not once
    // per member (`rules.md` 5.4).
    let mut resolved = Vec::with_capacity(collected.pendings.len());
    for pending in &collected.pendings {
        if is_first_stage_target(&pending.target) {
            resolved.push(None);
            continue;
        }
        let outcome = resolve(pending, &settled, entity.id);
        if let Some(warning) = outcome.warning {
            warnings.push(warning);
        }
        resolved.push(outcome.contribution);
    }

    // Ability modifiers.
    for ability in ABILITIES {
        let modifier = formula::ability_modifier(scores[&ability]);
        let key = StatKey::Known(KnownStat::AbilityModifier(ability));
        let base = vec![base_contribution(ContributionOp::Set, modifier)];
        let staged = stage_key(&key, base, &collected.pendings, &resolved);
        values.insert(key, staged.into_value());
    }

    // Saving throws.
    for ability in ABILITIES {
        let modifier = formula::ability_modifier(scores[&ability]);
        let mut base = vec![base_contribution(ContributionOp::Set, modifier)];
        if entity.body.proficiencies.saves.contains(&ability) {
            base.push(base_contribution(ContributionOp::Add, proficiency_bonus));
        }
        let key = StatKey::Known(KnownStat::Save(ability));
        let staged = stage_key(&key, base, &collected.pendings, &resolved);
        values.insert(key, staged.into_value());
    }

    // Skills.
    let mut skill_totals = BTreeMap::new();
    for skill in tables::SKILLS {
        let ability = tables::skill_ability(skill);
        let modifier = formula::ability_modifier(scores[&ability]);
        let mut base = vec![base_contribution(ContributionOp::Set, modifier)];
        match entity.body.proficiencies.skills.get(&skill) {
            Some(SkillProficiency::Proficient) => {
                base.push(base_contribution(ContributionOp::Add, proficiency_bonus));
            }
            Some(SkillProficiency::Expertise) => {
                base.push(base_contribution(ContributionOp::Add, proficiency_bonus));
                base.push(base_contribution(ContributionOp::Add, proficiency_bonus));
            }
            None => {}
        }
        let key = StatKey::Known(KnownStat::Skill(skill));
        let staged = stage_key(&key, base, &collected.pendings, &resolved);
        skill_totals.insert(skill, staged.computed.expect("a skill has a base `Set`"));
        values.insert(key, staged.into_value());
    }

    // Passive scores: 10 plus the computed skill.
    for skill in tables::SKILLS {
        let base = vec![
            base_contribution(ContributionOp::Set, 10),
            base_contribution(ContributionOp::Add, skill_totals[&skill]),
        ];
        let key = StatKey::Known(KnownStat::PassiveScore(skill));
        let staged = stage_key(&key, base, &collected.pendings, &resolved);
        values.insert(key, staged.into_value());
    }

    // AC: the armor base plus every modifier on `ac`.
    let dex_modifier = formula::ability_modifier(scores[&Ability::Dexterity]);
    let strength_score = scores[&Ability::Strength];
    let armor = armor::ac_base(&collected.items, dex_modifier, strength_score, entity.id);
    warnings.extend(armor.warnings);
    let key = StatKey::Known(KnownStat::Ac);
    let staged = stage_key(&key, armor.contributions, &collected.pendings, &resolved);
    values.insert(key, staged.into_value());

    // Initiative.
    let key = StatKey::Known(KnownStat::Initiative);
    let base = vec![base_contribution(ContributionOp::Set, dex_modifier)];
    let staged = stage_key(&key, base, &collected.pendings, &resolved);
    values.insert(key, staged.into_value());

    // Speed.
    let key = StatKey::Known(KnownStat::Speed);
    let base = vec![base_contribution(
        ContributionOp::Set,
        entity.body.speed.walk_ft,
    )];
    let staged = stage_key(&key, base, &collected.pendings, &resolved);
    values.insert(key, staged.into_value());

    // Spell DC and spell attack, driven by the first spellcasting block.
    if let Some(block) = entity.body.spellcasting.first() {
        let modifier = formula::ability_modifier(scores[&block.ability]);

        let key = StatKey::Known(KnownStat::SpellDc);
        let base = vec![
            base_contribution(ContributionOp::Set, 8),
            base_contribution(ContributionOp::Add, proficiency_bonus),
            base_contribution(ContributionOp::Add, modifier),
        ];
        let staged = stage_key(&key, base, &collected.pendings, &resolved);
        values.insert(key, staged.into_value());

        let key = StatKey::Known(KnownStat::Attack(AttackKind::Spell));
        let base = vec![
            base_contribution(ContributionOp::Set, modifier),
            base_contribution(ContributionOp::Add, proficiency_bonus),
        ];
        let staged = stage_key(&key, base, &collected.pendings, &resolved);
        values.insert(key, staged.into_value());
    }

    // Custom keys and deferred known keys appear when a contribution targets
    // them (`rules.md` 5.4); a group reaches its members even when the member
    // is deferred (`attack.melee`/`attack.ranged`).
    for pending in &collected.pendings {
        let targets: Vec<StatKey> = match &pending.target {
            StatKey::Known(KnownStat::Group(group)) => group_members(*group),
            target => vec![target.clone()],
        };
        for target in targets {
            if values.contains_key(&target) {
                continue;
            }
            let staged = stage_key(&target, Vec::new(), &collected.pendings, &resolved);
            values.insert(target, staged.into_value());
        }
    }

    apply_overrides(&mut values, &entity.body.overrides);

    // Resources: every entry derives its maximum; a dice leaf makes it
    // unevaluable (`rules.md` 5.2, 5.9).
    let mut resources = BTreeMap::new();
    for resource in &entity.body.resources {
        let derived = match formula::eval(&resource.max, &settled) {
            Some(evaluated) if !evaluated.flattened_dice => DerivedResource {
                max: Some(evaluated.value),
                breakdown: vec![Contribution {
                    source: ContributionSource::Base,
                    label: resource.name.clone(),
                    op: ContributionOp::Set,
                    value: Some(evaluated.value),
                }],
            },
            _ => {
                warnings.push(unevaluable_resource_warning(entity.id, resource));
                DerivedResource {
                    max: None,
                    breakdown: Vec::new(),
                }
            }
        };
        resources.insert(resource.id, derived);
    }

    // Class tables: slots and hit-dice pools, with unmodeled classes warned.
    let class_tables = tables::class_tables(&classes, entity.id);
    warnings.extend(class_tables.warnings);

    DerivedSheet {
        values,
        resources,
        slots: SlotMaxima {
            spell: class_tables.spell,
            pact: class_tables.pact,
        },
        hit_dice: class_tables.hit_dice,
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
    let sheet = derive(entity, ctx);
    let Some(value) = sheet.values.get(stat) else {
        return Err(EngineError::Malformed {
            reason: format!("`{stat}` has no derived value to roll"),
        });
    };
    let flat = match &value.effective {
        Some(OverrideValue::Int(flat)) => *flat,
        Some(other) => {
            return Err(EngineError::Malformed {
                reason: format!("`{stat}` has a non-integer effective value: {other:?}"),
            });
        }
        None => {
            return Err(EngineError::Malformed {
                reason: format!("`{stat}` has no effective value to roll"),
            });
        }
    };

    // The same collection filter as `derive` (`rules.md` 5.8): applicable
    // modifiers whose target is the stat itself or a group containing it.
    let collected = collect::collect(entity, ctx);
    let mut dice = Vec::new();
    for pending in &collected.pendings {
        if !target_matches(&pending.target, stat) {
            continue;
        }
        let formula = match &pending.op {
            ModifierOp::Add { value }
            | ModifierOp::Set { value }
            | ModifierOp::Min { value }
            | ModifierOp::Max { value } => value,
            ModifierOp::Advantage | ModifierOp::Disadvantage => continue,
        };
        extract_dice(formula, &pending.label, &mut dice)?;
    }

    Ok(RollParts {
        flat,
        dice,
        hint: value.hint.unwrap_or(RollHint::Normal),
    })
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
pub(super) fn base_contribution(op: ContributionOp, value: i32) -> Contribution {
    Contribution {
        source: ContributionSource::Base,
        label: BASE_LABEL.to_owned(),
        op,
        value: Some(value),
    }
}

/// Whether `target` reaches `key`: it is the key itself, or a group containing
/// it (`rules.md` 5.4).
fn target_matches(target: &StatKey, key: &StatKey) -> bool {
    if target == key {
        return true;
    }
    let StatKey::Known(KnownStat::Group(group)) = target else {
        return false;
    };
    let StatKey::Known(member) = key else {
        return false;
    };
    match group {
        StatGroup::Save => matches!(member, KnownStat::Save(_)),
        StatGroup::Check => matches!(member, KnownStat::Skill(_)),
        StatGroup::Attack => matches!(member, KnownStat::Attack(_)),
    }
}

/// Whether a target is staged in the bootstrap environment: proficiency bonus
/// or an ability score. No group contains either.
fn is_first_stage_target(target: &StatKey) -> bool {
    matches!(
        target,
        StatKey::Known(KnownStat::ProficiencyBonus | KnownStat::AbilityScore(_))
    )
}

/// The members a group key expands into (`rules.md` 5.4).
fn group_members(group: StatGroup) -> Vec<StatKey> {
    match group {
        StatGroup::Save => ABILITIES
            .iter()
            .map(|ability| StatKey::Known(KnownStat::Save(*ability)))
            .collect(),
        StatGroup::Check => tables::SKILLS
            .iter()
            .map(|skill| StatKey::Known(KnownStat::Skill(*skill)))
            .collect(),
        StatGroup::Attack => [AttackKind::Melee, AttackKind::Ranged, AttackKind::Spell]
            .iter()
            .map(|kind| StatKey::Known(KnownStat::Attack(*kind)))
            .collect(),
    }
}

/// Stages one post-bootstrap key from its base and the pre-resolved
/// contributions, in source order (`rules.md` 5.3).
fn stage_key(
    key: &StatKey,
    base: Vec<Contribution>,
    pendings: &[collect::Pending],
    resolved: &[Option<Contribution>],
) -> Staged {
    let mut contributions = base;
    for (pending, contribution) in pendings.iter().zip(resolved) {
        if let Some(contribution) = contribution
            && target_matches(&pending.target, key)
        {
            contributions.push(contribution.clone());
        }
    }
    stage(contributions)
}

/// The bootstrap stage's inputs: proficiency bonus and ability scores,
/// resolved before the scores settle.
struct FirstStage<'a, 'b> {
    /// The entity being derived.
    entity: EntityId,
    /// Every applicable modifier from the one collection pass.
    pendings: &'a [collect::Pending],
    /// The bootstrap evaluation environment.
    env: &'b formula::EvalEnv<'b>,
}

impl FirstStage<'_, '_> {
    /// Stages one key: its structural base, then every pending that targets it
    /// exactly.
    fn stage_key(&self, key: &StatKey, base: i32, warnings: &mut Vec<Warning>) -> Staged {
        let mut contributions = vec![base_contribution(ContributionOp::Set, base)];
        for pending in self
            .pendings
            .iter()
            .filter(|pending| pending.target == *key)
        {
            let resolved = resolve(pending, self.env, self.entity);
            if let Some(contribution) = resolved.contribution {
                contributions.push(contribution);
            }
            if let Some(warning) = resolved.warning {
                warnings.push(warning);
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
pub(super) fn element_for(source: ContributionSource) -> Option<ElementRef> {
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

/// The warning for a resource whose maximum is unevaluable.
fn unevaluable_resource_warning(entity: EntityId, resource: &Resource) -> Warning {
    Warning {
        code: WarningCode::FormulaUnevaluated,
        message: format!("`{}` has an unevaluable maximum", resource.name),
        entity: Some(entity),
        element: Some(ElementRef::Resource(resource.id)),
    }
}

/// Extracts representable dice terms from a modifier's formula in evaluation
/// order, left to right (`rules.md` 5.8).
fn extract_dice(
    formula: &Formula,
    label: &str,
    dice: &mut Vec<DiceTerm>,
) -> Result<(), EngineError> {
    match formula {
        Formula::Const(_) => Ok(()),
        Formula::Expr(expr) => extract_expr(expr, label, dice),
    }
}

/// Extracts the dice terms of a tagged formula expression.
///
/// A dice leaf is representable in an `Add`/`Sub` chain; under `Mul`,
/// `FloorDiv`, `Min` or `Max` the roller cannot express it as terms, so
/// extraction fails `Malformed`.
fn extract_expr(
    expr: &FormulaExpr,
    label: &str,
    dice: &mut Vec<DiceTerm>,
) -> Result<(), EngineError> {
    match expr {
        FormulaExpr::Dice { expr } => {
            dice.push(DiceTerm {
                source: Some(label.to_owned()),
                expr: expr.clone(),
            });
            Ok(())
        }
        FormulaExpr::Add { lhs, rhs } | FormulaExpr::Sub { lhs, rhs } => {
            extract_dice(lhs, label, dice)?;
            extract_dice(rhs, label, dice)
        }
        FormulaExpr::Mul { lhs, rhs }
        | FormulaExpr::Min { lhs, rhs }
        | FormulaExpr::Max { lhs, rhs }
        | FormulaExpr::FloorDiv { lhs, rhs } => {
            if contains_dice(lhs) || contains_dice(rhs) {
                return Err(EngineError::Malformed {
                    reason: format!(
                        "`{label}` scales or clamps a dice term, which a roll cannot express"
                    ),
                });
            }
            Ok(())
        }
        FormulaExpr::ProficiencyBonus
        | FormulaExpr::AbilityMod { .. }
        | FormulaExpr::Level
        | FormulaExpr::ClassLevel { .. } => Ok(()),
    }
}

/// Whether a formula contains a dice leaf anywhere.
fn contains_dice(formula: &Formula) -> bool {
    match formula {
        Formula::Const(_) => false,
        Formula::Expr(expr) => match expr {
            FormulaExpr::Dice { .. } => true,
            FormulaExpr::Add { lhs, rhs }
            | FormulaExpr::Sub { lhs, rhs }
            | FormulaExpr::Mul { lhs, rhs }
            | FormulaExpr::Min { lhs, rhs }
            | FormulaExpr::Max { lhs, rhs }
            | FormulaExpr::FloorDiv { lhs, rhs } => contains_dice(lhs) || contains_dice(rhs),
            FormulaExpr::ProficiencyBonus
            | FormulaExpr::AbilityMod { .. }
            | FormulaExpr::Level
            | FormulaExpr::ClassLevel { .. } => false,
        },
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

#[cfg(test)]
mod tests {
    use localtt_core::{
        builders::{StatblockBuilder, dice_pool, stat},
        types::{
            Abilities,
            Ability,
            CampaignTime,
            EntityId,
            EntityKind,
            Ext,
            FeatureCategory,
            FeatureEntry,
            FeatureEntryId,
            FormulaExpr,
            Modifier,
            PlayerId,
            Recharge,
            RefOrText,
            Resource,
            ResourceId,
            ResourceKind,
            Skill,
            SkillProficiency,
            SpellcastingBlock,
            SpellcastingBlockId,
            Status,
            StatusDuration,
            StatusId,
            StatusKind,
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

    fn status(name: &str, modifiers: Vec<Modifier>) -> Status {
        Status {
            id: StatusId::new(),
            kind: StatusKind::Effect,
            name: name.to_owned(),
            reference: None,
            text: None,
            started_at: CampaignTime::from_seconds(0),
            duration: StatusDuration::Indefinite,
            source: None,
            modifiers,
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

    #[test]
    fn structural_bases_derive_for_a_bare_character() {
        let entity = hero(Abilities::all(10));
        let content = BTreeMap::new();

        let sheet = derive(&entity, &context(&content));

        assert_eq!(value(&sheet, "proficiency_bonus").computed, Some(2));
        for ability in ["str", "dex", "con", "int", "wis", "cha"] {
            assert_eq!(value(&sheet, &format!("save.{ability}")).computed, Some(0));
            assert_eq!(
                value(&sheet, &format!("ability_mod.{ability}")).computed,
                Some(0)
            );
        }
        assert_eq!(value(&sheet, "skill.athletics").computed, Some(0));
        assert_eq!(value(&sheet, "passive.athletics").computed, Some(10));
        assert_eq!(value(&sheet, "ac").computed, Some(10));
        assert_eq!(value(&sheet, "initiative").computed, Some(0));
        assert_eq!(value(&sheet, "speed").computed, Some(30));
        assert!(
            !sheet
                .values
                .contains_key(&stat("spell.dc").expect("a literal stat")),
            "a sheet without a spellcasting block derives no spell DC"
        );
        assert!(sheet.slots.spell.is_empty());
        assert!(sheet.hit_dice.is_empty());
        assert!(sheet.warnings.is_empty());
    }

    #[test]
    fn proficiencies_shape_saves_skills_and_passives() {
        let mut entity = hero(Abilities {
            strength: 16,
            dexterity: 10,
            constitution: 10,
            intelligence: 10,
            wisdom: 10,
            charisma: 10,
        });
        entity.body.proficiencies.saves.insert(Ability::Strength);
        entity
            .body
            .proficiencies
            .skills
            .insert(Skill::Athletics, SkillProficiency::Proficient);
        entity
            .body
            .proficiencies
            .skills
            .insert(Skill::Stealth, SkillProficiency::Expertise);
        entity.body.levels.push(localtt_core::types::ClassLevel {
            id: localtt_core::types::ClassLevelId::new(),
            class: RefOrText::Text("Fighter".to_owned()),
            level: 5,
            subclass: None,
        });
        let content = BTreeMap::new();

        let sheet = derive(&entity, &context(&content));

        assert_eq!(value(&sheet, "proficiency_bonus").computed, Some(3));
        assert_eq!(value(&sheet, "save.str").computed, Some(6), "3 + prof 3");
        assert_eq!(value(&sheet, "save.dex").computed, Some(0));
        assert_eq!(
            value(&sheet, "skill.athletics").computed,
            Some(6),
            "3 + prof 3"
        );
        assert_eq!(
            value(&sheet, "skill.stealth").computed,
            Some(6),
            "0 + doubled prof 6"
        );
        assert_eq!(value(&sheet, "passive.athletics").computed, Some(16));
        assert_eq!(value(&sheet, "passive.stealth").computed, Some(16));
    }

    #[test]
    fn groups_reach_every_member_and_warn_once_per_contribution() {
        let mut entity = aria();
        entity.body.spellcasting.push(SpellcastingBlock {
            id: SpellcastingBlockId::new(),
            source: RefOrText::Text("Wizard".to_owned()),
            ability: Ability::Intelligence,
            spells: Vec::new(),
        });
        entity.body.statuses.push(status(
            "Bless",
            vec![
                modifier(
                    "save",
                    ModifierOp::Add {
                        value: Formula::dice(dice_pool(1, 4)),
                    },
                ),
                modifier("check", ModifierOp::Advantage),
                modifier("attack", ModifierOp::Advantage),
            ],
        ));
        entity.body.overrides.insert(
            stat("attack.melee").expect("a literal stat"),
            OverrideValue::Int(7),
        );
        let content = BTreeMap::new();

        let sheet = derive(&entity, &context(&content));

        for ability in ["str", "dex", "con", "int", "wis", "cha"] {
            assert!(
                value(&sheet, &format!("save.{ability}"))
                    .breakdown
                    .iter()
                    .any(|contribution| contribution.op == ContributionOp::Add),
                "the save group reaches every save"
            );
        }
        assert_eq!(
            sheet
                .warnings
                .iter()
                .filter(|warning| warning.code == WarningCode::DiceInStaticContext)
                .count(),
            1,
            "one warning per flattened contribution, not per member"
        );
        assert_eq!(
            value(&sheet, "skill.athletics").hint,
            Some(RollHint::Advantage)
        );
        assert_eq!(
            value(&sheet, "passive.perception").hint,
            None,
            "the check group expands into skills only"
        );
        assert_eq!(
            value(&sheet, "attack.spell").hint,
            Some(RollHint::Advantage)
        );
        let parts = roll_parts(
            &entity,
            &stat("attack.melee").expect("a literal stat"),
            &context(&content),
        )
        .expect("the overridden stat rolls");
        assert_eq!(parts.flat, 7);
        assert_eq!(parts.hint, RollHint::Advantage, "the attack group matches");
    }

    #[test]
    fn custom_and_deferred_keys_appear_when_a_contribution_targets_them() {
        let mut entity = aria();
        entity
            .body
            .features
            .push(feature("Lucky", vec![modifier("house.luck", add(2))]));
        entity
            .body
            .features
            .push(feature("Luckier", vec![modifier("house.luck", add(3))]));
        entity
            .body
            .features
            .push(feature("Trained", vec![modifier("attack.melee", add(1))]));

        let sheet = derive(&entity, &context(&BTreeMap::new()));

        let luck = value(&sheet, "house.luck");
        assert_eq!(luck.computed, Some(5), "adds start from 0");
        assert_eq!(luck.breakdown.len(), 2);
        assert_eq!(value(&sheet, "attack.melee").computed, Some(1));
        assert!(
            !sheet
                .values
                .contains_key(&stat("attack.ranged").expect("a literal stat")),
            "untargeted deferred keys stay absent"
        );
    }

    #[test]
    fn min_and_max_clamp_the_running_value() {
        let mut entity = aria();
        entity.body.features.push(feature(
            "Cursed",
            vec![
                modifier(
                    "ability_mod.int",
                    ModifierOp::Min {
                        value: Formula::Const(6),
                    },
                ),
                modifier(
                    "ability_mod.int",
                    ModifierOp::Max {
                        value: Formula::Const(2),
                    },
                ),
            ],
        ));

        let sheet = derive(&entity, &context(&BTreeMap::new()));

        let modifier_value = value(&sheet, "ability_mod.int");
        assert_eq!(
            modifier_value.computed,
            Some(2),
            "the min raises 4 to 6, the max lowers 6 to 2"
        );
        assert_eq!(
            modifier_value
                .breakdown
                .iter()
                .map(|contribution| contribution.op)
                .collect::<Vec<_>>(),
            vec![
                ContributionOp::Set,
                ContributionOp::Min,
                ContributionOp::Max,
            ]
        );
    }

    #[test]
    fn resources_derive_maxima_and_degrade_loudly() {
        let mut entity = aria();
        let constant = ResourceId::new();
        entity.body.resources.push(Resource {
            id: constant,
            name: "Second Wind".to_owned(),
            kind: ResourceKind::Feature,
            max: Formula::Const(3),
            current: 3,
            recharge: Recharge::Short,
        });
        let class_scaled = ResourceId::new();
        entity.body.resources.push(Resource {
            id: class_scaled,
            name: "Arcane Recovery".to_owned(),
            kind: ResourceKind::Feature,
            max: Formula::Expr(FormulaExpr::ClassLevel {
                class: "wizard".to_owned(),
            }),
            current: 0,
            recharge: Recharge::Long,
        });
        let rolled = ResourceId::new();
        entity.body.resources.push(Resource {
            id: rolled,
            name: "Wild Surge".to_owned(),
            kind: ResourceKind::Feature,
            max: Formula::dice(dice_pool(1, 4)),
            current: 0,
            recharge: Recharge::Long,
        });
        let broken = ResourceId::new();
        entity.body.resources.push(Resource {
            id: broken,
            name: "Broken".to_owned(),
            kind: ResourceKind::Feature,
            max: Formula::Expr(FormulaExpr::FloorDiv {
                lhs: Box::new(Formula::Const(1)),
                rhs: Box::new(Formula::Const(0)),
            }),
            current: 0,
            recharge: Recharge::Manual,
        });
        entity.body.levels.push(localtt_core::types::ClassLevel {
            id: localtt_core::types::ClassLevelId::new(),
            class: RefOrText::Text("Wizard".to_owned()),
            level: 4,
            subclass: None,
        });

        let sheet = derive(&entity, &context(&BTreeMap::new()));

        let resource = |id: ResourceId| {
            sheet
                .resources
                .get(&id)
                .unwrap_or_else(|| panic!("resource {id} is derived"))
        };
        assert_eq!(resource(constant).max, Some(3));
        assert_eq!(
            resource(constant).breakdown,
            vec![Contribution {
                source: ContributionSource::Base,
                label: "Second Wind".to_owned(),
                op: ContributionOp::Set,
                value: Some(3),
            }]
        );
        assert_eq!(resource(class_scaled).max, Some(4));
        assert_eq!(resource(rolled).max, None);
        assert_eq!(resource(broken).max, None);
        for id in [rolled, broken] {
            assert!(sheet.warnings.iter().any(|warning| {
                warning.code == WarningCode::FormulaUnevaluated
                    && warning.element == Some(ElementRef::Resource(id))
            }));
        }
    }

    #[test]
    fn spell_dc_and_attack_follow_the_first_spellcasting_block() {
        let mut builder = StatblockBuilder::character();
        builder
            .abilities(Abilities {
                strength: 10,
                dexterity: 10,
                constitution: 10,
                intelligence: 16,
                wisdom: 20,
                charisma: 10,
            })
            .hp(20, 20);
        builder.spellcasting_block_with_id(
            SpellcastingBlockId::new(),
            RefOrText::Text("Wizard".to_owned()),
            Ability::Intelligence,
            Vec::new(),
        );
        builder.spellcasting_block_with_id(
            SpellcastingBlockId::new(),
            RefOrText::Text("Cleric".to_owned()),
            Ability::Wisdom,
            Vec::new(),
        );
        let entity = builder
            .build_entity(EntityKind::Pc, EntityId::new(), "Tester", PlayerId::new())
            .expect("abilities and hp are set");

        let sheet = derive(&entity, &context(&BTreeMap::new()));

        assert_eq!(
            value(&sheet, "spell.dc").computed,
            Some(13),
            "8 + prof 2 + intelligence 3"
        );
        assert_eq!(
            value(&sheet, "attack.spell").computed,
            Some(5),
            "prof 2 + int 3"
        );
    }

    #[test]
    fn roll_parts_extract_additive_dice_in_order() {
        let mut entity = aria();
        entity.body.statuses.push(status(
            "Bless",
            vec![
                modifier(
                    "save.wis",
                    ModifierOp::Add {
                        value: Formula::Expr(FormulaExpr::Sub {
                            lhs: Box::new(Formula::dice(dice_pool(1, 4))),
                            rhs: Box::new(Formula::Const(1)),
                        }),
                    },
                ),
                modifier(
                    "save.wis",
                    ModifierOp::Add {
                        value: Formula::dice(dice_pool(1, 6)),
                    },
                ),
            ],
        ));
        let content = BTreeMap::new();

        let parts = roll_parts(
            &entity,
            &stat("save.wis").expect("a literal stat"),
            &context(&content),
        )
        .expect("the save derives");

        assert_eq!(
            parts.flat, 0,
            "wisdom 12 gives +1, the pending `Sub` constant contributes -1, and the dice leaves contribute 0"
        );
        assert_eq!(
            parts.dice,
            vec![
                DiceTerm {
                    source: Some("Bless".to_owned()),
                    expr: dice_pool(1, 4),
                },
                DiceTerm {
                    source: Some("Bless".to_owned()),
                    expr: dice_pool(1, 6),
                },
            ]
        );
        assert_eq!(parts.hint, RollHint::Normal);
    }

    #[test]
    fn roll_parts_reject_scaled_or_clamped_dice() {
        let mut entity = aria();
        entity.body.statuses.push(status(
            "Twisted",
            vec![modifier(
                "save.wis",
                ModifierOp::Add {
                    value: Formula::Expr(FormulaExpr::Mul {
                        lhs: Box::new(Formula::dice(dice_pool(1, 4))),
                        rhs: Box::new(Formula::Const(2)),
                    }),
                },
            )],
        ));
        let content = BTreeMap::new();

        let error = roll_parts(
            &entity,
            &stat("save.wis").expect("a literal stat"),
            &context(&content),
        )
        .expect_err("a scaled die is not representable");

        assert!(matches!(error, EngineError::Malformed { .. }));
    }

    #[test]
    fn roll_parts_require_an_integer_effective_value() {
        let mut entity = aria();
        let content = BTreeMap::new();

        let error = roll_parts(
            &entity,
            &stat("attack.melee").expect("a literal stat"),
            &context(&content),
        )
        .expect_err("attack.melee has no derived value");
        assert!(matches!(error, EngineError::Malformed { .. }));

        entity.body.overrides.insert(
            stat("attack.melee").expect("a literal stat"),
            OverrideValue::Text("two blades".to_owned()),
        );
        let error = roll_parts(
            &entity,
            &stat("attack.melee").expect("a literal stat"),
            &context(&content),
        )
        .expect_err("a text override has nothing to roll");
        assert!(matches!(error, EngineError::Malformed { .. }));
    }
}
