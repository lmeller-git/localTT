//! U5 target test: dice, rolls, casting and hit dice, as a client reads them.
//!
//! This file is the fixed point for `docs/rfcs/dice.md`: client code written
//! against the pinned API before any library code exists. It intentionally does
//! not compile until the U5 skeleton lands; that is the spec-phase contract,
//! not an oversight.

use std::collections::{BTreeMap, VecDeque};

use localtt_adapter::{rng::ChaChaRng, store::memory::MemoryStore};
use localtt_core::{
    builders::{content_ref, ref_id, stat},
    types::{
        Ability,
        AnyEntity,
        Audience,
        Campaign,
        CampaignTime,
        DiceExpr,
        DiceResult,
        DieResult,
        EntityId,
        EntityKind,
        Ext,
        FeedEntryId,
        FeedPayload,
        Formula,
        FormulaExpr,
        InventoryEntry,
        InventoryEntryId,
        InventoryItem,
        ItemState,
        Keep,
        Modifier,
        ModifierOp,
        Namespace,
        PlayerId,
        Recharge,
        RefId,
        RefOrText,
        Resource,
        ResourceId,
        ResourceKind,
        Roll,
        RollContext,
        RollKind,
        SpellEntry,
        SpellEntryId,
        SpellcastingBlock,
        SpellcastingBlockId,
        StatblockEntity,
        Status,
        StatusDuration,
        StatusId,
        StatusKind,
        TermResult,
        UnixTime,
    },
};
use localtt_engine::{
    commands::{Command, CommandEnv, Event, RollSpec, StatusSpec},
    content::{
        ContentData,
        ContentEntry,
        ContentFault,
        ContentKind,
        ContentSummary,
        Provenance,
        ReferenceSource,
        SearchQuery,
        SpellData,
    },
    dice::{self, RollPlan, TermPlan},
    error::{ElementRef, EngineError, WarningCode},
    executor::Executor,
    ports::{
        rng::Rng,
        store::{CampaignStore, StatePatch},
    },
    rules::{self, DiceTerm, RollHint, RollParts},
    state::{LoadedCampaign, WorkingSet},
};
use localtt_integration::fixtures;

/// The fixed wall-clock stamp the fixture cast uses.
const FIXED_TIME: i64 = 1_750_000_000;

// ---------------------------------------------------------------------------
// Test doubles and helpers
// ---------------------------------------------------------------------------

/// An `Rng` that returns scripted values in order and refuses impossible ones.
struct ScriptedRng {
    script: VecDeque<u32>,
}

impl ScriptedRng {
    fn new(script: &[u32]) -> Self {
        Self {
            script: script.iter().copied().collect(),
        }
    }
}

impl Rng for ScriptedRng {
    fn roll_die(&mut self, sides: u32) -> u32 {
        let value = self
            .script
            .pop_front()
            .unwrap_or_else(|| panic!("the script has no value left for a d{sides}"));
        assert!(
            (1..=sides).contains(&value),
            "the scripted value {value} is impossible on a d{sides}"
        );
        value
    }
}

/// A `ReferenceSource` over a literal map, optionally broken.
struct MapSource {
    entries: BTreeMap<RefId, ContentEntry>,
    fail_get: bool,
}

impl ReferenceSource for MapSource {
    fn get(&self, id: &RefId) -> Result<Option<ContentEntry>, ContentFault> {
        if self.fail_get {
            return Err(ContentFault::Unavailable {
                message: "the fixture source is down".to_owned(),
            });
        }
        Ok(self.entries.get(id).cloned())
    }

    fn search(&self, _query: &SearchQuery<'_>) -> Result<Vec<ContentSummary>, ContentFault> {
        Ok(Vec::new())
    }

    fn autocomplete(&self, _query: &SearchQuery<'_>) -> Result<Vec<ContentSummary>, ContentFault> {
        Ok(Vec::new())
    }

    fn sources(&self) -> Vec<Provenance> {
        Vec::new()
    }
}

/// A content map over the given entries, resolving everything it holds.
fn content(entries: Vec<ContentEntry>) -> MapSource {
    MapSource {
        entries: entries
            .into_iter()
            .map(|entry| (entry.id.clone(), entry))
            .collect(),
        fail_get: false,
    }
}

/// An empty, working content map.
fn no_content() -> MapSource {
    content(Vec::new())
}

/// A content map whose every lookup fails.
fn broken_content() -> MapSource {
    MapSource {
        entries: BTreeMap::new(),
        fail_get: true,
    }
}

/// The U5 environment seam: a scripted die source and a content map.
struct TestEnv {
    rng: ScriptedRng,
    reference: MapSource,
}

impl CommandEnv for TestEnv {
    fn now(&self) -> UnixTime {
        UnixTime::from_seconds(FIXED_TIME)
    }

    fn reference(&self) -> &dyn ReferenceSource {
        &self.reference
    }

    fn roll(&mut self, plan: &RollPlan) -> Result<DiceResult, EngineError> {
        Ok(dice::roll(plan, &mut self.rng))
    }
}

/// An environment over the trait defaults: the clock only. `roll` and
/// `reference` come from `CommandEnv`'s default bodies.
struct DefaultEnv;

impl CommandEnv for DefaultEnv {
    fn now(&self) -> UnixTime {
        UnixTime::from_seconds(FIXED_TIME)
    }
}

/// A test environment over the given script and content map.
fn env(script: &[u32], reference: MapSource) -> TestEnv {
    TestEnv {
        rng: ScriptedRng::new(script),
        reference,
    }
}

/// The party's entities as one creation patch; the tests start from an empty
/// feed.
fn seeding_patch<'a>(campaign: &'a Campaign, entities: &'a [AnyEntity]) -> StatePatch<'a> {
    StatePatch {
        campaign: Some(campaign),
        upserts: entities.iter().collect(),
        removals: Vec::new(),
        feed_appends: Vec::new(),
        feed_keep_from: None,
    }
}

/// A fresh in-memory store holding the campaign and the given entities.
fn seeded_store(campaign: &Campaign, entities: &[AnyEntity]) -> MemoryStore {
    let mut store = MemoryStore::new();
    store
        .apply(&seeding_patch(campaign, entities))
        .expect("a fresh store accepts the seed");
    store
}

/// An executor over a just-seeded store and the given environment.
fn seeded_executor<'a, E: CommandEnv + 'static>(
    store: &'a mut MemoryStore,
    env: E,
) -> Executor<'a> {
    let state = LoadedCampaign::from_snapshot(store.load().expect("the store was just seeded"));
    Executor::new(Box::new(state), store, Box::new(env))
}

/// One statblock as the working set holds it.
fn statblock_of(state: &dyn WorkingSet, id: EntityId) -> StatblockEntity {
    match state
        .entity(id)
        .expect("the working set resolves its own headers")
        .expect("the entity exists")
    {
        AnyEntity::Statblock(document) => document,
        other => panic!("{id} is a statblock, got {other:?}"),
    }
}

/// One statblock resource as the working set holds it.
fn resource_of(state: &dyn WorkingSet, entity: EntityId, resource: ResourceId) -> Resource {
    statblock_of(state, entity)
        .body
        .resources
        .into_iter()
        .find(|candidate| candidate.id == resource)
        .expect("the entity owns the resource")
}

/// The last feed entry of a working set, which the test expects to be a roll.
fn feed_roll(executor: &Executor<'_>) -> Roll {
    let feed = executor.state().feed();
    let Some(entry) = feed.last() else {
        panic!("the feed holds a roll");
    };
    let FeedPayload::Roll { roll } = entry.payload.clone() else {
        panic!("the last entry is a roll, got {:?}", entry.payload);
    };
    roll
}

/// One clash-free `Roll` command: a check of `key` against `target_dc`.
fn check(entity: EntityId, key: &str, target_dc: Option<i32>) -> Command {
    Command::Roll {
        entity: Some(entity),
        spec: RollSpec::Check {
            stat: stat(key).expect("a literal stat key"),
            target_dc,
        },
        audience: None,
    }
}

/// One `CastSpell` command with no compare-and-swap.
fn cast_spell(
    entity: EntityId,
    spell: SpellEntryId,
    slot: ResourceId,
    upcast_slots: u32,
    concentrate: bool,
) -> Command {
    Command::CastSpell {
        entity,
        spell,
        slot,
        upcast_slots,
        concentrate,
        expected_rev: None,
    }
}

/// A `SpendHitDie` command with no compare-and-swap.
fn spend_hit_die(entity: EntityId, resource: ResourceId) -> Command {
    Command::SpendHitDie {
        entity,
        resource,
        expected_rev: None,
    }
}

/// Runs one rolling command, which the caller expects to succeed.
fn run(executor: &mut Executor<'_>, actor: PlayerId, command: &Command) {
    executor
        .execute_as(actor, command)
        .expect("the roll is bookkeeping");
}

/// One die result.
fn die(value: i32, kept: bool) -> DieResult {
    DieResult { value, kept }
}

/// A plain d20 term.
fn d20() -> DiceExpr {
    DiceExpr::Roll {
        count: 1,
        sides: 20,
        keep: None,
    }
}

/// A plain d4 term.
fn d4() -> DiceExpr {
    DiceExpr::Roll {
        count: 1,
        sides: 4,
        keep: None,
    }
}

/// A `RollPlan` over a whole expression, as a command builds one for a free
/// expression: exactly one term with no attribution.
fn expression_plan(notation: &str) -> RollPlan {
    let expr = dice::parse(notation).expect("a literal dice notation");
    RollPlan {
        terms: vec![TermPlan {
            expr: expr.clone(),
            source: None,
        }],
        expression: expr,
    }
}

/// A left fold of `Add` over the terms, the shape `compose` pins.
fn fold_add(terms: Vec<DiceExpr>) -> DiceExpr {
    let mut terms = terms.into_iter();
    let first = terms.next().expect("a fold needs at least one term");
    terms.fold(first, |lhs, rhs| DiceExpr::Add {
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
    })
}

/// A balanced `Add` tree with `leaves` constant leaves, for cap tests.
fn adds(leaves: u32) -> DiceExpr {
    if leaves == 1 {
        return DiceExpr::Const { value: 1 };
    }
    let left = leaves / 2 + leaves % 2;
    DiceExpr::Add {
        lhs: Box::new(adds(left)),
        rhs: Box::new(adds(leaves - left)),
    }
}

/// A Bless-like status: `+1d4` to the save group, attributed `"Bless"`.
fn blessed() -> Status {
    Status {
        id: StatusId::new(),
        kind: StatusKind::Effect,
        name: "Bless".to_owned(),
        reference: None,
        text: None,
        started_at: CampaignTime::from_seconds(0),
        duration: StatusDuration::Indefinite,
        source: None,
        modifiers: vec![Modifier {
            target: stat("save").expect("a literal stat key"),
            op: ModifierOp::Add {
                value: Formula::Expr(FormulaExpr::Dice { expr: d4() }),
            },
            when: None,
            note: Some("Bless".to_owned()),
        }],
    }
}

/// A concentration status under the given name.
fn concentration_status(name: &str) -> Status {
    Status {
        id: StatusId::new(),
        kind: StatusKind::Concentration,
        name: name.to_owned(),
        reference: None,
        text: None,
        started_at: CampaignTime::from_seconds(0),
        duration: StatusDuration::Indefinite,
        source: None,
        modifiers: Vec::new(),
    }
}

/// A known spell entry pointing at a reference.
fn known_spell(id: SpellEntryId, name: &str, reference: &str) -> SpellEntry {
    SpellEntry {
        id,
        name: name.to_owned(),
        reference: Some(ref_id(reference).expect("a literal reference")),
        prepared: true,
    }
}

/// A fixed provenance for the test-local content entries.
fn provenance() -> Provenance {
    Provenance {
        namespace: Namespace::parse("fixture").expect("a literal namespace"),
        source: "dice target".to_owned(),
        source_revision: "v1".to_owned(),
        license: "CC0".to_owned(),
        attribution: String::new(),
        url: None,
    }
}

/// A spell payload with the usual defaults.
fn spell_data(level: u8, concentration: bool) -> SpellData {
    SpellData {
        level,
        school: "Evocation".to_owned(),
        concentration,
        ritual: false,
    }
}

/// A content entry wrapping one spell.
fn spell_content(id: &str, name: &str, level: u8, concentration: bool) -> ContentEntry {
    ContentEntry {
        id: ref_id(id).expect("a literal reference"),
        kind: ContentKind::Spell,
        name: name.to_owned(),
        description: String::new(),
        data: ContentData::Spell(spell_data(level, concentration)),
        provenance: provenance(),
        needs_review: false,
        review_notes: Vec::new(),
        ext: Ext::default(),
    }
}

/// A spell slot resource at a spell level.
fn spell_slot(id: ResourceId, name: &str, level: i32, max: i32) -> Resource {
    Resource {
        id,
        name: name.to_owned(),
        kind: ResourceKind::SpellSlot { level },
        max: Formula::Const(max),
        current: max,
        recharge: Recharge::Long,
    }
}

// ---------------------------------------------------------------------------
// The parser and the caps
// ---------------------------------------------------------------------------

#[test]
fn the_parser_builds_the_core_ast() {
    // A bare die and insignificant whitespace.
    assert_eq!(dice::parse("1d20").expect("a bare die"), d20());
    assert_eq!(
        dice::parse("  1d20  ").expect("whitespace is ignored"),
        d20()
    );

    // Addition, subtraction and multiplication with the usual precedence.
    assert_eq!(
        dice::parse("2d6+3").expect("a flat addend"),
        DiceExpr::Add {
            lhs: Box::new(DiceExpr::Roll {
                count: 2,
                sides: 6,
                keep: None,
            }),
            rhs: Box::new(DiceExpr::Const { value: 3 }),
        }
    );
    assert_eq!(
        dice::parse("4d6kh3+1d4-1").expect("a multi-term expression"),
        DiceExpr::Sub {
            lhs: Box::new(DiceExpr::Add {
                lhs: Box::new(DiceExpr::Roll {
                    count: 4,
                    sides: 6,
                    keep: Some(Keep::Highest { count: 3 }),
                }),
                rhs: Box::new(d4()),
            }),
            rhs: Box::new(DiceExpr::Const { value: 1 }),
        }
    );
    assert_eq!(
        dice::parse("1d20*2+3").expect("multiplication binds tighter"),
        DiceExpr::Add {
            lhs: Box::new(DiceExpr::Mul {
                lhs: Box::new(d20()),
                rhs: Box::new(DiceExpr::Const { value: 2 }),
            }),
            rhs: Box::new(DiceExpr::Const { value: 3 }),
        }
    );

    // Advantage and disadvantage wrap a parenthesized expression.
    assert_eq!(
        dice::parse("adv(1d20)").expect("advantage"),
        DiceExpr::Advantage {
            inner: Box::new(d20()),
        }
    );
    assert_eq!(
        dice::parse("dis ( adv( 1d20 + 2 ) )").expect("whitespace is ignored"),
        DiceExpr::Disadvantage {
            inner: Box::new(DiceExpr::Advantage {
                inner: Box::new(DiceExpr::Add {
                    lhs: Box::new(d20()),
                    rhs: Box::new(DiceExpr::Const { value: 2 }),
                }),
            }),
        }
    );

    // Lowercase only, no `d20` shorthand, `adv` needs its parentheses.
    assert!(matches!(
        dice::parse("1D20"),
        Err(EngineError::Malformed { .. })
    ));
    assert!(matches!(
        dice::parse("d20"),
        Err(EngineError::Malformed { .. })
    ));
    assert!(matches!(
        dice::parse("adv 1d20"),
        Err(EngineError::Malformed { .. })
    ));
    assert!(matches!(
        dice::parse(""),
        Err(EngineError::Malformed { .. })
    ));
    assert!(matches!(
        dice::parse("1d20+"),
        Err(EngineError::Malformed { .. })
    ));
}

#[test]
fn caps_gate_parsing_and_asts_from_the_wire() {
    // Parse-time caps.
    assert!(matches!(
        dice::parse("101d6"),
        Err(EngineError::TooLarge {
            what: "dice count",
            limit: 100,
        })
    ));
    assert!(matches!(
        dice::parse("0d6"),
        Err(EngineError::Malformed { .. })
    ));
    assert!(matches!(
        dice::parse("1d1001"),
        Err(EngineError::TooLarge {
            what: "dice sides",
            limit: 1000,
        })
    ));
    assert!(matches!(
        dice::parse("1d1"),
        Err(EngineError::Malformed { .. })
    ));
    assert!(matches!(
        dice::parse("1d20kh0"),
        Err(EngineError::Malformed { .. })
    ));
    assert!(matches!(
        dice::parse("1d20kh2"),
        Err(EngineError::Malformed { .. })
    ));

    // `validate` gates ASTs arriving from the wire, where `parse` never ran.
    let mut deep = d20();
    for _ in 0..16 {
        deep = DiceExpr::Advantage {
            inner: Box::new(deep),
        };
    }
    assert!(matches!(
        dice::validate(&deep),
        Err(EngineError::TooLarge {
            what: "expression depth",
            limit: 16,
        })
    ));
    assert!(matches!(
        dice::validate(&adds(17)),
        Err(EngineError::TooLarge {
            what: "expression terms",
            limit: 32,
        })
    ));
    assert!(matches!(
        dice::validate(&DiceExpr::Roll {
            count: 101,
            sides: 6,
            keep: None,
        }),
        Err(EngineError::TooLarge {
            what: "dice count",
            limit: 100,
        })
    ));
    assert!(matches!(
        dice::validate(&DiceExpr::Roll {
            count: 1,
            sides: 1001,
            keep: None,
        }),
        Err(EngineError::TooLarge {
            what: "dice sides",
            limit: 1000,
        })
    ));
    assert!(
        dice::validate(&dice::parse("4d6kh3").expect("a literal expression")).is_ok(),
        "a legal AST passes validation"
    );
}

// ---------------------------------------------------------------------------
// The roller
// ---------------------------------------------------------------------------

#[test]
fn keep_rules_drop_the_rest_and_prefer_earliest_ties() {
    // Keep the highest three of four: the lowest die drops.
    let mut rng = ScriptedRng::new(&[3, 5, 5, 1]);
    let result = dice::roll(&expression_plan("4d6kh3"), &mut rng);
    assert_eq!(
        result,
        DiceResult {
            terms: vec![TermResult {
                dice: vec![die(3, true), die(5, true), die(5, true), die(1, false)],
                total: 13,
                source: None,
            }],
            total: 13,
        }
    );

    // Keep the lowest one of three.
    let mut rng = ScriptedRng::new(&[4, 4, 2]);
    let result = dice::roll(&expression_plan("3d6kl1"), &mut rng);
    assert_eq!(
        result.terms,
        vec![TermResult {
            dice: vec![die(4, false), die(4, false), die(2, true)],
            total: 2,
            source: None,
        }]
    );

    // A tie keeps the earliest die in roll order, for both keep rules.
    let mut rng = ScriptedRng::new(&[3, 3]);
    let result = dice::roll(&expression_plan("2d6kh1"), &mut rng);
    assert_eq!(
        result.terms,
        vec![TermResult {
            dice: vec![die(3, true), die(3, false)],
            total: 3,
            source: None,
        }]
    );
    let mut rng = ScriptedRng::new(&[5, 5]);
    let result = dice::roll(&expression_plan("2d6kl1"), &mut rng);
    assert_eq!(
        result.terms,
        vec![TermResult {
            dice: vec![die(5, true), die(5, false)],
            total: 5,
            source: None,
        }]
    );
}

#[test]
fn advantage_and_disadvantage_keep_one_of_two_evaluations() {
    // The higher of two d20s wins; the loser's dice stay visible as unkept.
    let mut rng = ScriptedRng::new(&[13, 20]);
    let result = dice::roll(&expression_plan("adv(1d20)"), &mut rng);
    assert_eq!(
        result,
        DiceResult {
            terms: vec![TermResult {
                dice: vec![die(13, false), die(20, true)],
                total: 20,
                source: None,
            }],
            total: 20,
        }
    );

    // Disadvantage keeps the lower total; the bonus applies inside each
    // evaluation, not once.
    let mut rng = ScriptedRng::new(&[10, 18]);
    let result = dice::roll(&expression_plan("adv(1d20+2)"), &mut rng);
    assert_eq!(result.total, 20);
    assert_eq!(
        result.terms,
        vec![TermResult {
            dice: vec![die(10, false), die(18, true)],
            total: 20,
            source: None,
        }]
    );
    let mut rng = ScriptedRng::new(&[18, 10]);
    let result = dice::roll(&expression_plan("dis(1d20)"), &mut rng);
    assert_eq!(result.total, 10);
    assert_eq!(
        result.terms,
        vec![TermResult {
            dice: vec![die(18, false), die(10, true)],
            total: 10,
            source: None,
        }]
    );

    // Nat detection stays visible: the kept die carries its own value.
    let mut rng = ScriptedRng::new(&[1, 20]);
    let result = dice::roll(&expression_plan("adv(1d20)"), &mut rng);
    assert_eq!(
        result.terms,
        vec![TermResult {
            dice: vec![die(1, false), die(20, true)],
            total: 20,
            source: None,
        }]
    );
    let mut rng = ScriptedRng::new(&[1, 20]);
    let result = dice::roll(&expression_plan("dis(1d20)"), &mut rng);
    assert_eq!(
        result.terms,
        vec![TermResult {
            dice: vec![die(1, true), die(20, false)],
            total: 1,
            source: None,
        }]
    );
}

// ---------------------------------------------------------------------------
// Composition: the U4/U5 seam
// ---------------------------------------------------------------------------

#[test]
fn compose_builds_the_plan_from_roll_parts() {
    // The static side of the seam: the wizard's Wisdom save under Bless.
    let party = fixtures::party();
    let mut aria = fixtures::golden_wizard(party.player);
    aria.body.statuses.push(blessed());
    let save_wis = stat("save.wis").expect("a literal stat key");
    let parts = rules::roll_parts(
        &aria,
        &save_wis,
        &rules::RulesContext {
            now: CampaignTime::from_seconds(0),
            content: &BTreeMap::new(),
        },
    )
    .expect("the wizard's Wisdom save composes");

    // The roll side: the d20 base, the flat part as its own term, and every
    // attributed die.
    let plan = dice::compose(d20(), &parts);
    assert_eq!(
        plan.terms,
        vec![
            TermPlan {
                expr: d20(),
                source: None,
            },
            TermPlan {
                expr: DiceExpr::Const { value: 4 },
                source: None,
            },
            TermPlan {
                expr: d4(),
                source: Some("Bless".to_owned()),
            },
        ]
    );
    assert_eq!(
        plan.expression,
        fold_add(vec![d20(), DiceExpr::Const { value: 4 }, d4(),])
    );
    let mut rng = ScriptedRng::new(&[14, 3]);
    let result = dice::roll(&plan, &mut rng);
    assert_eq!(result.total, 21);
    assert_eq!(
        result.terms,
        vec![
            TermResult {
                dice: vec![die(14, true)],
                total: 14,
                source: None,
            },
            TermResult {
                dice: Vec::new(),
                total: 4,
                source: None,
            },
            TermResult {
                dice: vec![die(3, true)],
                total: 3,
                source: Some("Bless".to_owned()),
            },
        ]
    );

    // A zero flat part adds no term, so the plain check is one d20.
    let parts = RollParts {
        flat: 0,
        dice: Vec::new(),
        hint: RollHint::Normal,
    };
    let plan = dice::compose(d20(), &parts);
    assert_eq!(
        plan.terms,
        vec![TermPlan {
            expr: d20(),
            source: None,
        }]
    );
    assert_eq!(plan.expression, d20());

    // A net hint wraps the base term; the flat part stays a plain addend.
    let parts = RollParts {
        flat: 4,
        dice: vec![DiceTerm {
            source: Some("Bless".to_owned()),
            expr: d4(),
        }],
        hint: RollHint::Advantage,
    };
    let plan = dice::compose(d20(), &parts);
    assert_eq!(
        plan.terms,
        vec![
            TermPlan {
                expr: DiceExpr::Advantage {
                    inner: Box::new(d20()),
                },
                source: None,
            },
            TermPlan {
                expr: DiceExpr::Const { value: 4 },
                source: None,
            },
            TermPlan {
                expr: d4(),
                source: Some("Bless".to_owned()),
            },
        ]
    );

    // The wrapped base consumes two draws; the winner is kept.
    let mut rng = ScriptedRng::new(&[9, 14, 3]);
    let result = dice::roll(&plan, &mut rng);
    assert_eq!(result.total, 21);
    assert_eq!(
        result.terms,
        vec![
            TermResult {
                dice: vec![die(9, false), die(14, true)],
                total: 14,
                source: None,
            },
            TermResult {
                dice: Vec::new(),
                total: 4,
                source: None,
            },
            TermResult {
                dice: vec![die(3, true)],
                total: 3,
                source: Some("Bless".to_owned()),
            },
        ]
    );
}

// ---------------------------------------------------------------------------
// The Roll command
// ---------------------------------------------------------------------------

#[test]
fn a_check_roll_derives_its_flat_part_and_attributed_terms() {
    // Arrange: Aria under Bless; one scripted d20 and one scripted d4.
    let party = fixtures::party();
    let mut aria = fixtures::golden_wizard(party.player);
    aria.body.statuses.push(blessed());
    let entities = [AnyEntity::from(aria.clone())];
    let mut store = seeded_store(&party.campaign, &entities);
    let mut executor = seeded_executor(&mut store, env(&[14, 3], no_content()));

    // Act: roll her Wisdom save against DC 15.
    let executed = executor
        .execute_as(party.player, &check(aria.id, "save.wis", Some(15)))
        .expect("rolling a check is bookkeeping");

    // Assert: one feed entry, no entity writes, and the full roll record.
    assert_eq!(executed.warnings, Vec::new());
    assert_eq!(
        executed.events,
        vec![Event::FeedAppended {
            ids: vec![FeedEntryId::new(1)],
        }]
    );
    assert_eq!(
        statblock_of(executor.state(), aria.id).rev,
        0,
        "a roll never rewrites the entity"
    );
    let feed = executor.state().feed();
    assert_eq!(feed.len(), 1);
    let entry = &feed[0];
    assert_eq!(entry.id, FeedEntryId::new(1));
    assert_eq!(entry.ts, UnixTime::from_seconds(FIXED_TIME));
    assert_eq!(entry.clock_s, CampaignTime::from_seconds(0));
    assert_eq!(entry.actor, Some(party.player));
    assert_eq!(entry.entity, Some(aria.id));
    assert_eq!(entry.audience, Audience::RollerAndDm);
    let FeedPayload::Roll { roll } = &entry.payload else {
        panic!("the entry is a roll, got {:?}", entry.payload);
    };
    assert_eq!(roll.roller, party.player);
    assert_eq!(roll.entity, Some(aria.id));
    assert_eq!(roll.ts, UnixTime::from_seconds(FIXED_TIME));
    assert_eq!(roll.audience, Audience::RollerAndDm);
    assert_eq!(
        roll.expr,
        fold_add(vec![d20(), DiceExpr::Const { value: 4 }, d4()])
    );
    assert_eq!(
        roll.context,
        Some(RollContext {
            kind: RollKind::Save,
            stat: Some(stat("save.wis").expect("a literal stat key")),
            target_dc: Some(15),
        })
    );
    assert_eq!(roll.result.total, 21);
    assert_eq!(
        roll.result.terms,
        vec![
            TermResult {
                dice: vec![die(14, true)],
                total: 14,
                source: None,
            },
            TermResult {
                dice: Vec::new(),
                total: 4,
                source: None,
            },
            TermResult {
                dice: vec![die(3, true)],
                total: 3,
                source: Some("Bless".to_owned()),
            },
        ]
    );
}

#[test]
fn expression_rolls_take_context_and_audience() {
    let party = fixtures::party();
    let aria = fixtures::golden_wizard(party.player);
    let entities = [AnyEntity::from(aria.clone())];
    let mut store = seeded_store(&party.campaign, &entities);
    let mut executor = seeded_executor(&mut store, env(&[4, 5, 6, 7, 8, 9], no_content()));

    // A targeted expression: the target names the context, the audience
    // override is honored.
    run(
        &mut executor,
        party.player,
        &Command::Roll {
            entity: None,
            spec: RollSpec::Expression {
                expr: dice::parse("2d6+3").expect("a literal expression"),
                target: Some(stat("damage.fire").expect("a literal stat key")),
            },
            audience: Some(Audience::Public),
        },
    );
    let roll = feed_roll(&executor);
    assert_eq!(roll.entity, None);
    assert_eq!(roll.audience, Audience::Public);
    assert_eq!(
        roll.expr,
        dice::parse("2d6+3").expect("a literal expression")
    );
    assert_eq!(roll.result.total, 12);
    assert_eq!(
        roll.context,
        Some(RollContext {
            kind: RollKind::Damage,
            stat: Some(stat("damage.fire").expect("a literal stat key")),
            target_dc: None,
        })
    );

    // An untargeted expression has no context at all.
    run(
        &mut executor,
        party.player,
        &Command::Roll {
            entity: None,
            spec: RollSpec::Expression {
                expr: d20(),
                target: None,
            },
            audience: Some(Audience::RollerOnly),
        },
    );
    let roll = feed_roll(&executor);
    assert_eq!(roll.context, None);
    assert_eq!(roll.audience, Audience::RollerOnly);

    // Check rolls map their stat's family onto the roll kind.
    run(
        &mut executor,
        party.player,
        &check(aria.id, "skill.arcana", None),
    );
    assert_eq!(
        feed_roll(&executor)
            .context
            .expect("a check has context")
            .kind,
        RollKind::Check
    );
    run(
        &mut executor,
        party.player,
        &check(aria.id, "attack.spell", None),
    );
    assert_eq!(
        feed_roll(&executor)
            .context
            .expect("a check has context")
            .kind,
        RollKind::Attack
    );

    // A custom stat that is neither is `Other`.
    run(
        &mut executor,
        party.player,
        &Command::Roll {
            entity: None,
            spec: RollSpec::Expression {
                expr: d4(),
                target: Some(stat("house.luck").expect("a custom stat key")),
            },
            audience: None,
        },
    );
    assert_eq!(
        feed_roll(&executor)
            .context
            .expect("a target has context")
            .kind,
        RollKind::Other
    );

    // An oversized AST arriving from the wire is refused, not rolled.
    let result = executor.execute_as(
        party.player,
        &Command::Roll {
            entity: None,
            spec: RollSpec::Expression {
                expr: adds(17),
                target: None,
            },
            audience: None,
        },
    );
    assert!(matches!(
        result,
        Err(EngineError::TooLarge {
            what: "expression terms",
            limit: 32,
        })
    ));
}

#[test]
fn a_roll_requires_an_actor_and_a_real_entity() {
    let party = fixtures::party();
    let aria = fixtures::golden_wizard(party.player);
    let ghost = EntityId::new();
    let entities = [AnyEntity::from(aria.clone())];
    let mut store = seeded_store(&party.campaign, &entities);
    let mut executor = seeded_executor(&mut store, env(&[10], no_content()));

    // The trusted path carries no actor: a roll cannot be attributed.
    let result = executor.execute(&check(aria.id, "save.wis", None));
    assert!(
        matches!(result, Err(EngineError::Malformed { .. })),
        "a roll without an actor is malformed, got {result:?}"
    );

    // A check needs an entity to derive from.
    let result = executor.execute_as(
        party.player,
        &Command::Roll {
            entity: None,
            spec: RollSpec::Check {
                stat: stat("save.wis").expect("a literal stat key"),
                target_dc: None,
            },
            audience: None,
        },
    );
    assert!(matches!(result, Err(EngineError::Malformed { .. })));

    // An addressed entity must exist.
    let result = executor.execute_as(
        party.player,
        &Command::Roll {
            entity: Some(ghost),
            spec: RollSpec::Expression {
                expr: d20(),
                target: None,
            },
            audience: None,
        },
    );
    assert!(matches!(
        result,
        Err(EngineError::NotFound {
            entity,
            element: None,
        }) if entity == ghost
    ));
}

#[test]
fn a_broken_environment_or_content_source_fails_the_roll() {
    let party = fixtures::party();

    // The trait defaults: no roll port, and an empty content source.
    let mut default = DefaultEnv;
    assert!(matches!(
        default.roll(&expression_plan("1d20")),
        Err(EngineError::Environment { .. })
    ));
    assert!(
        default
            .reference()
            .get(&ref_id("srd2014:spell/fireball").expect("a literal reference"))
            .expect("the empty source answers")
            .is_none()
    );

    let aria = fixtures::golden_wizard(party.player);
    let entities = [AnyEntity::from(aria.clone())];
    let mut store = seeded_store(&party.campaign, &entities);
    let mut executor = seeded_executor(&mut store, DefaultEnv);
    let result = executor.execute_as(party.player, &check(aria.id, "save.wis", None));
    assert!(matches!(result, Err(EngineError::Environment { .. })));

    // A source that fails while the command materializes the content map is a
    // content fault, not a malformed command.
    let mut aria = fixtures::golden_wizard(party.player);
    aria.body.inventory.push(InventoryEntry {
        id: InventoryEntryId::new(),
        item: InventoryItem::Ref(
            content_ref("srd2014:item/ring-of-protection", "Ring of Protection")
                .expect("a literal reference"),
        ),
        qty: 1,
        state: ItemState::Equipped,
        attuned: false,
        notes: String::new(),
    });
    let entities = [AnyEntity::from(aria.clone())];
    let mut store = seeded_store(&party.campaign, &entities);
    let mut executor = seeded_executor(&mut store, env(&[], broken_content()));
    let result = executor.execute_as(party.player, &check(aria.id, "save.wis", None));
    assert!(matches!(result, Err(EngineError::Content(_))));
}

// ---------------------------------------------------------------------------
// The Rng contract
// ---------------------------------------------------------------------------

#[test]
fn the_rng_contract_holds_for_every_implementation() {
    /// Every `Rng` draws uniformly from `1..=sides`, and a one-sided die is
    /// the degenerate 1.
    fn contract(rng: &mut dyn Rng) {
        for _ in 0..256 {
            let value = rng.roll_die(20);
            assert!((1..=20).contains(&value), "out of range: {value}");
        }
        assert_eq!(rng.roll_die(1), 1, "a one-sided die always rolls one");
    }

    // The scripted double: scripted draws, contract-clean. Draw order is
    // exercised by the roller tests, which script exact sequences.
    let mut script = vec![20_u32; 256];
    script.push(1);
    contract(&mut ScriptedRng::new(&script));

    // The shipped ChaCha implementation is OS-seeded but reproducible when a
    // seed is given, so a campaign's rolls can be replayed.
    contract(&mut ChaChaRng::new());
    let mut first = ChaChaRng::from_seed([7; 32]);
    let mut second = ChaChaRng::from_seed([7; 32]);
    let rolled: Vec<u32> = (0..128).map(|_| first.roll_die(20)).collect();
    let replayed: Vec<u32> = (0..128).map(|_| second.roll_die(20)).collect();
    assert_eq!(rolled, replayed, "equal seeds replay equal sequences");
}

// ---------------------------------------------------------------------------
// Casting
// ---------------------------------------------------------------------------

#[test]
fn casting_spends_the_slot_and_replaces_concentration() {
    // Arrange: Aria knows Hex (a concentration spell) and already concentrates
    // on Hunter's Mark; she has a first-level slot to spend.
    let party = fixtures::party();
    let mut aria = fixtures::golden_wizard(party.player);
    let slot1 = ResourceId::new();
    aria.body
        .resources
        .push(spell_slot(slot1, "Spell Slots (1st)", 1, 4));
    let hex = SpellEntryId::new();
    aria.body.spellcasting[0]
        .spells
        .push(known_spell(hex, "Hex", "srd2014:spell/hex"));
    aria.body
        .statuses
        .push(concentration_status("Hunter's Mark"));
    let entries = vec![spell_content("srd2014:spell/hex", "Hex", 1, true)];
    let entities = [AnyEntity::from(aria.clone())];
    let mut store = seeded_store(&party.campaign, &entities);
    let mut executor = seeded_executor(&mut store, env(&[], content(entries)));

    // Act: cast Hex with the explicit flag off; the spell itself concentrates.
    let executed = executor
        .execute_as(party.player, &cast_spell(aria.id, hex, slot1, 0, false))
        .expect("casting a known spell is bookkeeping");

    // Assert: the slot is spent, the old concentration is replaced, and the
    // replacement is announced on the feed.
    assert_eq!(executed.warnings, Vec::new());
    assert_eq!(
        executed.events,
        vec![
            Event::EntityChanged {
                id: aria.id,
                rev: 1,
                kind: EntityKind::Pc,
            },
            Event::FeedAppended {
                ids: vec![FeedEntryId::new(1)],
            },
        ]
    );
    let updated = statblock_of(executor.state(), aria.id);
    assert_eq!(
        resource_of(executor.state(), aria.id, slot1).current,
        3,
        "the cast spends one slot"
    );
    let concentration: Vec<&Status> = updated
        .body
        .statuses
        .iter()
        .filter(|status| status.kind == StatusKind::Concentration)
        .collect();
    assert_eq!(concentration.len(), 1, "the cast leaves one concentration");
    let hex_status = concentration[0];
    assert_eq!(hex_status.name, "Hex");
    assert_eq!(
        hex_status.reference,
        Some(ref_id("srd2014:spell/hex").expect("a literal reference"))
    );
    assert_eq!(hex_status.duration, StatusDuration::Indefinite);
    assert_eq!(hex_status.started_at, CampaignTime::from_seconds(0));
    assert_eq!(hex_status.source, None);
    assert!(hex_status.modifiers.is_empty());
    let feed = executor.state().feed();
    assert_eq!(feed.len(), 1);
    assert_eq!(feed[0].audience, Audience::RollerAndDm);
    assert_eq!(feed[0].actor, Some(party.player));
    assert_eq!(feed[0].entity, Some(aria.id));
    assert_eq!(
        feed[0].payload,
        FeedPayload::ConcentrationEnded {
            name: "Hunter's Mark".to_owned(),
        }
    );
}

#[test]
fn cast_validation_rejects_bad_spells_and_slots() {
    // Arrange: Aria with slots of the first and third level, a cantrip, a
    // first-level spell, a third-level spell, and a non-slot resource.
    let party = fixtures::party();
    let mut aria = fixtures::golden_wizard(party.player);
    let slot1 = ResourceId::new();
    let slot3 = aria.body.resources[0].id;
    assert_eq!(
        aria.body.resources[0].kind,
        ResourceKind::SpellSlot { level: 3 }
    );
    aria.body
        .resources
        .push(spell_slot(slot1, "Spell Slots (1st)", 1, 4));
    let arcane = ResourceId::new();
    aria.body.resources.push(Resource {
        id: arcane,
        name: "Arcane Recovery".to_owned(),
        kind: ResourceKind::Feature,
        max: Formula::Const(1),
        current: 1,
        recharge: Recharge::Long,
    });
    let cantrip = SpellEntryId::new();
    let shield = aria.body.spellcasting[0].spells[1].id;
    assert_eq!(aria.body.spellcasting[0].spells[1].name, "Shield");
    let fireball = aria.body.spellcasting[0].spells[0].id;
    assert_eq!(aria.body.spellcasting[0].spells[0].name, "Fireball");
    aria.body.spellcasting[0].spells.push(known_spell(
        cantrip,
        "Acid Splash",
        "srd2014:spell/acid-splash",
    ));
    let entries = vec![
        spell_content("srd2014:spell/acid-splash", "Acid Splash", 0, false),
        spell_content("srd2014:spell/shield", "Shield", 1, false),
        spell_content("srd2014:spell/fireball", "Fireball", 3, false),
    ];
    let entities = [AnyEntity::from(aria.clone())];
    let mut store = seeded_store(&party.campaign, &entities);
    let mut executor = seeded_executor(&mut store, env(&[], content(entries)));

    // A cantrip is not cast with a slot.
    let result = executor.execute_as(party.player, &cast_spell(aria.id, cantrip, slot1, 0, false));
    assert!(matches!(result, Err(EngineError::Malformed { .. })));

    // A second-level-or-higher spell needs a slot at least that high.
    let result = executor.execute_as(
        party.player,
        &cast_spell(aria.id, fireball, slot1, 0, false),
    );
    assert!(matches!(result, Err(EngineError::Malformed { .. })));

    // Upcasting must be declared: three minus three is zero, not one.
    let result = executor.execute_as(
        party.player,
        &cast_spell(aria.id, fireball, slot3, 1, false),
    );
    assert!(matches!(result, Err(EngineError::Malformed { .. })));

    // Only spell slots and pact slots pay for spells.
    let result = executor.execute_as(party.player, &cast_spell(aria.id, shield, arcane, 0, false));
    assert!(matches!(result, Err(EngineError::Malformed { .. })));

    // Unknown elements are missing, not malformed.
    let ghost_spell = SpellEntryId::new();
    let result = executor.execute_as(
        party.player,
        &cast_spell(aria.id, ghost_spell, slot1, 0, false),
    );
    assert!(matches!(
        result,
        Err(EngineError::NotFound {
            entity,
            element: Some(ElementRef::Spell(spell)),
        }) if entity == aria.id && spell == ghost_spell
    ));
    let ghost_slot = ResourceId::new();
    let result = executor.execute_as(
        party.player,
        &cast_spell(aria.id, shield, ghost_slot, 0, false),
    );
    assert!(matches!(
        result,
        Err(EngineError::NotFound {
            entity,
            element: Some(ElementRef::Resource(resource)),
        }) if entity == aria.id && resource == ghost_slot
    ));

    // A non-concentration spell does not end an ongoing concentration.
    aria.body
        .statuses
        .push(concentration_status("Hunter's Mark"));
    let entities = [AnyEntity::from(aria.clone())];
    let entries = vec![spell_content(
        "srd2014:spell/fireball",
        "Fireball",
        3,
        false,
    )];
    let mut store = seeded_store(&party.campaign, &entities);
    let mut executor = seeded_executor(&mut store, env(&[], content(entries)));
    let executed = executor
        .execute_as(
            party.player,
            &cast_spell(aria.id, fireball, slot3, 0, false),
        )
        .expect("casting a third-level spell with a third-level slot");
    assert_eq!(executed.warnings, Vec::new());
    assert!(
        executed
            .events
            .iter()
            .all(|event| !matches!(event, Event::FeedAppended { .. }))
    );
    let updated = statblock_of(executor.state(), aria.id);
    assert_eq!(
        updated
            .body
            .statuses
            .iter()
            .filter(|status| status.kind == StatusKind::Concentration)
            .count(),
        1,
        "the Hunter's Mark survives a non-concentration cast"
    );
}

#[test]
fn pact_slots_skip_level_validation_but_not_upcast_nonsense() {
    // Arrange: Lyra alone, with one known Hex.
    let party = fixtures::party();
    let mut lyra = fixtures::golden_warlock(party.player);
    let pact = lyra.body.resources[0].id;
    assert_eq!(lyra.body.resources[0].kind, ResourceKind::PactSlot);
    let hex = SpellEntryId::new();
    lyra.body.spellcasting.push(SpellcastingBlock {
        id: SpellcastingBlockId::new(),
        source: RefOrText::Text("Warlock".to_owned()),
        ability: Ability::Charisma,
        spells: vec![known_spell(hex, "Hex", "srd2014:spell/hex")],
    });
    let entries = vec![spell_content("srd2014:spell/hex", "Hex", 1, true)];
    let entities = [AnyEntity::from(lyra.clone())];
    let mut store = seeded_store(&party.campaign, &entities);
    let mut executor = seeded_executor(&mut store, env(&[], content(entries)));

    // A pact slot's level lives in the class table, so the cast is allowed.
    let executed = executor
        .execute_as(party.player, &cast_spell(lyra.id, hex, pact, 0, true))
        .expect("a pact slot casts the known spell");
    assert_eq!(executed.warnings, Vec::new());
    assert_eq!(resource_of(executor.state(), lyra.id, pact).current, 1);

    // Upcasting cannot be validated without the class table: refuse it.
    let result = executor.execute_as(party.player, &cast_spell(lyra.id, hex, pact, 1, true));
    assert!(matches!(result, Err(EngineError::Malformed { .. })));
}

#[test]
fn an_unresolved_spell_casts_with_a_warning() {
    // Arrange: a homebrew spell entry with no content reference.
    let party = fixtures::party();
    let mut aria = fixtures::golden_wizard(party.player);
    let slot1 = ResourceId::new();
    aria.body
        .resources
        .push(spell_slot(slot1, "Spell Slots (1st)", 1, 4));
    let bolt = SpellEntryId::new();
    aria.body.spellcasting[0].spells.push(SpellEntry {
        id: bolt,
        name: "Homebrew Bolt".to_owned(),
        reference: None,
        prepared: true,
    });
    let entities = [AnyEntity::from(aria.clone())];
    let mut store = seeded_store(&party.campaign, &entities);
    let mut executor = seeded_executor(&mut store, env(&[], no_content()));

    // Act: cast it, asking for concentration explicitly.
    let executed = executor
        .execute_as(party.player, &cast_spell(aria.id, bolt, slot1, 0, true))
        .expect("an unresolved spell still casts");

    // Assert: one warning, the slot is spent, and the flag decides
    // concentration because the level and the spell data are unknown.
    assert_eq!(executed.warnings.len(), 1);
    let warning = &executed.warnings[0];
    assert_eq!(warning.code, WarningCode::UnresolvedReference);
    assert_eq!(warning.entity, Some(aria.id));
    assert_eq!(warning.element, Some(ElementRef::Spell(bolt)));
    assert!(!warning.message.is_empty());
    assert_eq!(resource_of(executor.state(), aria.id, slot1).current, 3);
    let updated = statblock_of(executor.state(), aria.id);
    let concentration: Vec<&Status> = updated
        .body
        .statuses
        .iter()
        .filter(|status| status.kind == StatusKind::Concentration)
        .collect();
    assert_eq!(concentration.len(), 1);
    assert_eq!(concentration[0].name, "Homebrew Bolt");
    assert_eq!(concentration[0].reference, None);
    assert_eq!(concentration[0].duration, StatusDuration::Indefinite);
    assert_eq!(concentration[0].started_at, CampaignTime::from_seconds(0));
}

#[test]
fn requesting_concentration_on_a_non_concentration_spell_warns() {
    // Arrange: Fireball is not a concentration spell, but the client asks.
    let party = fixtures::party();
    let aria = fixtures::golden_wizard(party.player);
    let fireball = aria.body.spellcasting[0].spells[0].id;
    let slot3 = aria.body.resources[0].id;
    let entries = vec![spell_content(
        "srd2014:spell/fireball",
        "Fireball",
        3,
        false,
    )];
    let entities = [AnyEntity::from(aria.clone())];
    let mut store = seeded_store(&party.campaign, &entities);
    let mut executor = seeded_executor(&mut store, env(&[], content(entries)));

    // Act.
    let executed = executor
        .execute_as(party.player, &cast_spell(aria.id, fireball, slot3, 0, true))
        .expect("casting is bookkeeping even when the flag is odd");

    // Assert: warned, and the concentration started anyway.
    assert_eq!(executed.warnings.len(), 1);
    let warning = &executed.warnings[0];
    assert_eq!(warning.code, WarningCode::ConcentrationUnsupported);
    assert_eq!(warning.entity, Some(aria.id));
    assert_eq!(warning.element, Some(ElementRef::Spell(fireball)));
    let updated = statblock_of(executor.state(), aria.id);
    assert_eq!(
        updated
            .body
            .statuses
            .iter()
            .filter(|status| status.kind == StatusKind::Concentration)
            .count(),
        1
    );
    assert_eq!(
        updated.body.statuses[0].reference,
        Some(ref_id("srd2014:spell/fireball").expect("a literal reference"))
    );
}

#[test]
fn begin_and_end_concentration_replace_and_clear() {
    let party = fixtures::party();
    let mut aria = fixtures::golden_wizard(party.player);
    aria.body
        .statuses
        .push(concentration_status("Hunter's Mark"));
    let entities = [AnyEntity::from(aria.clone())];
    let mut store = seeded_store(&party.campaign, &entities);
    let mut executor = seeded_executor(&mut store, env(&[], no_content()));

    // A concentration command must actually be a concentration status.
    let result = executor.execute_as(
        party.player,
        &Command::BeginConcentration {
            entity: aria.id,
            status: StatusSpec {
                kind: StatusKind::Effect,
                name: "Bless".to_owned(),
                reference: None,
                text: None,
                duration: StatusDuration::Indefinite,
                source: None,
                modifiers: Vec::new(),
            },
            expected_rev: None,
        },
    );
    assert!(matches!(result, Err(EngineError::Malformed { .. })));

    // Begin replaces the old concentration and announces the ending.
    let executed = executor
        .execute_as(
            party.player,
            &Command::BeginConcentration {
                entity: aria.id,
                status: StatusSpec {
                    kind: StatusKind::Concentration,
                    name: "Bless".to_owned(),
                    reference: Some(ref_id("srd2014:spell/bless").expect("a literal reference")),
                    text: None,
                    duration: StatusDuration::Indefinite,
                    source: None,
                    modifiers: Vec::new(),
                },
                expected_rev: None,
            },
        )
        .expect("beginning concentration replaces the old one");
    assert_eq!(executed.warnings, Vec::new());
    assert!(matches!(
        executed.events.last(),
        Some(Event::FeedAppended { ids }) if ids == &vec![FeedEntryId::new(1)]
    ));
    let updated = statblock_of(executor.state(), aria.id);
    let concentration: Vec<&Status> = updated
        .body
        .statuses
        .iter()
        .filter(|status| status.kind == StatusKind::Concentration)
        .collect();
    assert_eq!(concentration.len(), 1);
    assert_eq!(concentration[0].name, "Bless");
    assert_eq!(
        executor.state().feed()[0].payload,
        FeedPayload::ConcentrationEnded {
            name: "Hunter's Mark".to_owned(),
        }
    );

    // End clears it and announces that too.
    let executed = executor
        .execute_as(
            party.player,
            &Command::EndConcentration {
                entity: aria.id,
                expected_rev: None,
            },
        )
        .expect("ending concentration is bookkeeping");
    assert!(executed.warnings.is_empty());
    let updated = statblock_of(executor.state(), aria.id);
    assert!(
        updated
            .body
            .statuses
            .iter()
            .all(|status| status.kind != StatusKind::Concentration)
    );
    let feed = executor.state().feed();
    assert_eq!(feed.len(), 2);
    assert_eq!(
        feed[1].payload,
        FeedPayload::ConcentrationEnded {
            name: "Bless".to_owned(),
        }
    );

    // Ending what is not there is missing, not malformed.
    let result = executor.execute_as(
        party.player,
        &Command::EndConcentration {
            entity: aria.id,
            expected_rev: None,
        },
    );
    assert!(matches!(
        result,
        Err(EngineError::NotFound {
            entity,
            element: None,
        }) if entity == aria.id
    ));
}

// ---------------------------------------------------------------------------
// Damage hints and hit dice
// ---------------------------------------------------------------------------

#[test]
fn damage_to_a_concentrating_entity_suggests_the_save() {
    let party = fixtures::party();
    let mut aria = fixtures::golden_wizard(party.player);
    aria.body.statuses.push(concentration_status("Hex"));
    let entities = [AnyEntity::from(aria.clone())];
    let mut store = seeded_store(&party.campaign, &entities);
    let mut executor = seeded_executor(&mut store, env(&[], no_content()));

    // 24 damage asks for DC 12.
    let executed = executor
        .execute_as(
            party.player,
            &Command::ApplyDamage {
                entity: aria.id,
                amount: 24,
                expected_rev: None,
            },
        )
        .expect("damage is bookkeeping");
    assert_eq!(executed.warnings, Vec::new());
    assert!(matches!(
        executed.events.last(),
        Some(Event::FeedAppended { ids }) if ids == &vec![FeedEntryId::new(1)]
    ));
    let feed = executor.state().feed();
    assert_eq!(feed.len(), 1);
    assert_eq!(feed[0].audience, Audience::RollerAndDm);
    assert_eq!(feed[0].actor, Some(party.player));
    assert_eq!(feed[0].entity, Some(aria.id));
    assert_eq!(
        feed[0].payload,
        FeedPayload::Hint {
            text: "Concentration: CON save DC 12".to_owned(),
        }
    );

    // Small damage floors the DC at 10.
    executor
        .execute_as(
            party.player,
            &Command::ApplyDamage {
                entity: aria.id,
                amount: 5,
                expected_rev: None,
            },
        )
        .expect("damage is bookkeeping");
    let feed = executor.state().feed();
    assert_eq!(feed.len(), 2);
    assert_eq!(feed[1].ts, UnixTime::from_seconds(FIXED_TIME));
    assert_eq!(feed[1].clock_s, CampaignTime::from_seconds(0));
    assert_eq!(
        feed[1].payload,
        FeedPayload::Hint {
            text: "Concentration: CON save DC 10".to_owned(),
        }
    );

    // Zero damage suggests nothing.
    executor
        .execute_as(
            party.player,
            &Command::ApplyDamage {
                entity: aria.id,
                amount: 0,
                expected_rev: None,
            },
        )
        .expect("zero damage is legal");
    assert_eq!(executor.state().feed().len(), 2);

    // An entity that is not concentrating gets no hint either.
    let borin = fixtures::golden_fighter(party.player);
    let entities = [AnyEntity::from(borin.clone())];
    let mut store = seeded_store(&party.campaign, &entities);
    let mut executor = seeded_executor(&mut store, env(&[], no_content()));
    executor
        .execute_as(
            party.player,
            &Command::ApplyDamage {
                entity: borin.id,
                amount: 24,
                expected_rev: None,
            },
        )
        .expect("damage is bookkeeping");
    assert!(executor.state().feed().is_empty());
}

#[test]
fn spending_a_hit_die_rolls_heals_and_deducts() {
    // Arrange: wounded Aria with five d6 hit dice and a +2 Constitution.
    let party = fixtures::party();
    let mut aria = fixtures::golden_wizard(party.player);
    aria.body.hp.current = 20;
    let hit_die = ResourceId::new();
    aria.body.resources.push(Resource {
        id: hit_die,
        name: "Hit Dice (d6)".to_owned(),
        kind: ResourceKind::HitDie { sides: 6 },
        max: Formula::Const(5),
        current: 5,
        recharge: Recharge::Long,
    });
    let entities = [AnyEntity::from(aria.clone())];
    let mut store = seeded_store(&party.campaign, &entities);
    let mut executor = seeded_executor(&mut store, env(&[5], no_content()));

    // Act.
    let executed = executor
        .execute_as(party.player, &spend_hit_die(aria.id, hit_die))
        .expect("spending a hit die is bookkeeping");

    // Assert: one die, +2 Constitution, healed and deducted, and a roll on the
    // feed attributed to the spending actor.
    assert_eq!(executed.warnings, Vec::new());
    assert_eq!(
        executed.events,
        vec![
            Event::EntityChanged {
                id: aria.id,
                rev: 1,
                kind: EntityKind::Pc,
            },
            Event::FeedAppended {
                ids: vec![FeedEntryId::new(1)],
            },
        ]
    );
    let updated = statblock_of(executor.state(), aria.id);
    assert_eq!(updated.body.hp.current, 27);
    assert_eq!(resource_of(executor.state(), aria.id, hit_die).current, 4);
    let roll = feed_roll(&executor);
    assert_eq!(roll.roller, party.player);
    assert_eq!(roll.entity, Some(aria.id));
    assert_eq!(roll.audience, Audience::RollerAndDm);
    assert_eq!(
        roll.expr,
        DiceExpr::Add {
            lhs: Box::new(DiceExpr::Roll {
                count: 1,
                sides: 6,
                keep: None,
            }),
            rhs: Box::new(DiceExpr::Const { value: 2 }),
        }
    );
    assert_eq!(
        roll.context,
        Some(RollContext {
            kind: RollKind::Other,
            stat: None,
            target_dc: None,
        })
    );
    assert_eq!(roll.result.total, 7);
    assert_eq!(
        roll.result.terms,
        vec![
            TermResult {
                dice: vec![die(5, true)],
                total: 5,
                source: None,
            },
            TermResult {
                dice: Vec::new(),
                total: 2,
                source: None,
            },
        ]
    );
}

#[test]
fn hit_die_overspending_warns_and_bad_requests_fail() {
    let party = fixtures::party();
    let mut aria = fixtures::golden_wizard(party.player);
    aria.body.hp.current = 20;
    let hit_die = ResourceId::new();
    aria.body.resources.push(Resource {
        id: hit_die,
        name: "Hit Dice (d6)".to_owned(),
        kind: ResourceKind::HitDie { sides: 6 },
        max: Formula::Const(5),
        current: 0,
        recharge: Recharge::Long,
    });
    let entities = [AnyEntity::from(aria.clone())];
    let mut store = seeded_store(&party.campaign, &entities);
    let mut executor = seeded_executor(&mut store, env(&[1], no_content()));

    // Spending at zero is allowed and warned.
    let executed = executor
        .execute_as(party.player, &spend_hit_die(aria.id, hit_die))
        .expect("overspending is warned, never blocked");
    assert_eq!(executed.warnings.len(), 1);
    assert_eq!(executed.warnings[0].code, WarningCode::ResourceOverspent);
    assert_eq!(executed.warnings[0].entity, Some(aria.id));
    assert_eq!(
        executed.warnings[0].element,
        Some(ElementRef::Resource(hit_die))
    );
    assert_eq!(resource_of(executor.state(), aria.id, hit_die).current, -1);
    assert_eq!(statblock_of(executor.state(), aria.id).body.hp.current, 23);

    // A resource that is not a hit die cannot be spent as one.
    let arcane = ResourceId::new();
    aria.body.resources.push(Resource {
        id: arcane,
        name: "Arcane Recovery".to_owned(),
        kind: ResourceKind::Feature,
        max: Formula::Const(1),
        current: 1,
        recharge: Recharge::Long,
    });
    let entities = [AnyEntity::from(aria.clone())];
    let mut store = seeded_store(&party.campaign, &entities);
    let mut executor = seeded_executor(&mut store, env(&[1], no_content()));
    let result = executor.execute_as(party.player, &spend_hit_die(aria.id, arcane));
    assert!(matches!(result, Err(EngineError::Malformed { .. })));

    // An unknown resource is missing, not malformed.
    let ghost = ResourceId::new();
    let result = executor.execute_as(party.player, &spend_hit_die(aria.id, ghost));
    assert!(matches!(
        result,
        Err(EngineError::NotFound {
            entity,
            element: Some(ElementRef::Resource(resource)),
        }) if entity == aria.id && resource == ghost
    ));

    // The trusted path carries no actor.
    let result = executor.execute(&spend_hit_die(aria.id, hit_die));
    assert!(matches!(result, Err(EngineError::Malformed { .. })));
}

// ---------------------------------------------------------------------------
// Determinism
// ---------------------------------------------------------------------------

#[test]
fn equal_scripts_replay_equal_rolls() {
    // Two identical worlds and identical scripts produce identical rolls.
    let party = fixtures::party();
    let aria = fixtures::golden_wizard(party.player);
    let entities = [AnyEntity::from(aria.clone())];

    let mut first_store = seeded_store(&party.campaign, &entities);
    let mut first = seeded_executor(&mut first_store, env(&[14, 3], no_content()));
    let mut second_store = seeded_store(&party.campaign, &entities);
    let mut second = seeded_executor(&mut second_store, env(&[14, 3], no_content()));
    let first_rolled = {
        first
            .execute_as(party.player, &check(aria.id, "save.wis", Some(15)))
            .expect("rolling a check is bookkeeping");
        feed_roll(&first)
    };
    let second_rolled = {
        second
            .execute_as(party.player, &check(aria.id, "save.wis", Some(15)))
            .expect("rolling a check is bookkeeping");
        feed_roll(&second)
    };
    assert_eq!(first_rolled.expr, second_rolled.expr);
    assert_eq!(first_rolled.result, second_rolled.result);
    assert_eq!(first_rolled.context, second_rolled.context);
    assert_eq!(first_rolled.audience, second_rolled.audience);

    // The roller itself is a pure function of the plan and the rng.
    let plan = expression_plan("adv(1d20+2)");
    let mut first = ScriptedRng::new(&[10, 18]);
    let mut second = ScriptedRng::new(&[10, 18]);
    assert_eq!(
        dice::roll(&plan, &mut first),
        dice::roll(&plan, &mut second)
    );
}
