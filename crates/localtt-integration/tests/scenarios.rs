//! The scenario suite: `crates/localtt-integration/DESIGN.md` (sections 3, 4
//! and 6).
//!
//! These tests drive the core, engine and adapter the way a real client would
//! and pin the golden-character catalog. When a test and the design disagree,
//! decide which is right and change the other; do not work around it silently.
//!
//! Coverage, across the crates as they land:
//!
//! - character creation through `localtt_core::builders` (a hand-built
//!   character) and this crate's `fixtures` (the golden-character catalog,
//!   DESIGN.md section 3);
//! - handling/transformation through `localtt_engine::commands` (damage, temp
//!   HP, heal, resource spend, concentration replace, level up, summon, handout
//!   give);
//! - serialization/deserialization through this crate's `roundtrip` and the
//!   schema gate in `localtt_adapter::load`, including whole-party round trips;
//! - failure cases: a newer schema major is refused, a hand-edited
//!   kind/body mismatch is rejected, an invalid reference is rejected.
//!
//! - catalog coverage: the DESIGN.md section 3 store-column rows that had no
//!   test (statuses and durations, rolls and the feed, the full modifier and
//!   formula surface, multiclass and duplicate classes, custom stats, boundary
//!   values, the entity kind/body matrix, the canonical stat-key vocabulary).
//!
//! It does not assert derived values: proficiency bonus, AC, save DC and spell
//! slots need `localtt-engine` and are catalogued in DESIGN.md section 3 (the
//! deferred column). The suite is plain data, so there is no contention
//! case; the concurrency contract is a static `Send + Sync` bound, asserted
//! here and in the core test.

use localtt_adapter::load::{LoadError, load_checked};
use localtt_core::{
    builders::{CampaignBuilder, StatblockBuilder, content_ref, dice_pool, ref_id, stat},
    types::{
        Abilities,
        Ability,
        AclEntry,
        AclLevel,
        AnyEntity,
        AttackKind,
        Audience,
        Campaign,
        CampaignTime,
        ChallengeRating,
        Condition,
        DeathSaves,
        DiceExpr,
        DiceResult,
        DieResult,
        Entity,
        EntityError,
        EntityId,
        EntityKind,
        FeatureCategory,
        FeatureEntry,
        FeatureEntryId,
        FeedEntry,
        FeedEntryId,
        FeedPayload,
        FieldMask,
        Formula,
        FormulaExpr,
        HandoutBody,
        HandoutContent,
        InlineItem,
        InventoryEntry,
        InventoryEntryId,
        InventoryItem,
        ItemKind,
        ItemState,
        Keep,
        KnownStat,
        Modifier,
        ModifierOp,
        NoteBody,
        OverrideValue,
        PlayerId,
        Profile,
        Recharge,
        RefId,
        RefOrText,
        Resource,
        ResourceId,
        ResourceKind,
        RestKind,
        Role,
        Roll,
        RollContext,
        RollId,
        RollKind,
        RulesetId,
        SchemaVersion,
        Size,
        Skill,
        SkillProficiency,
        StatGroup,
        StatKey,
        StatblockEntity,
        Status,
        StatusDuration,
        StatusId,
        StatusKind,
        StatusSource,
        TermResult,
        UnixTime,
        ViewField,
    },
};
use localtt_engine::commands::{
    apply_damage,
    begin_concentration,
    give_handout,
    heal,
    level_up,
    set_temp_hp,
    spend_resource,
    summon_from_template,
};
use localtt_integration::{
    fixtures::{goblin, golden_fighter, golden_multiclass, golden_warlock, golden_wizard, party},
    roundtrip::{from_json, to_json},
};
use serde_json::{Value, json};

// -- character creation -----------------------------------------------------

#[test]
fn a_character_can_be_typed_in_with_the_builders() {
    // Arrange / Act: the "new sheet" path, one hand-built level-1 character.
    let owner = PlayerId::new();
    let mut builder = StatblockBuilder::character();
    let longbow = builder.item(
        InventoryItem::Ref(content_ref("srd2014:item/longbow", "Longbow").expect("literal ref")),
        1,
        ItemState::Equipped,
        false,
    );
    builder
        .abilities(Abilities::all(12))
        .skill(Skill::Perception, SkillProficiency::Proficient)
        .save_proficiency(Ability::Dexterity)
        .hp(11, 11)
        .override_stat(
            stat("ac").expect("literal stat key"),
            OverrideValue::Int(14),
        )
        .attack("Longbow", Some(4), "1d8+1 piercing", Some(longbow))
        .notes("A hunter from the northern woods.");
    // An attuned item contributing only while the condition holds, and a
    // feature whose value is a formula, exercise the modifier pipeline inputs.
    let ring = builder.item(
        InventoryItem::Inline(InlineItem {
            name: "Ring of Warmth".to_owned(),
            description: "Resistance to cold damage; +1 AC while attuned.".to_owned(),
            kind: ItemKind::Wondrous,
            requires_attunement: true,
            weight_oz: None,
            value: None,
            properties: None,
            modifiers: vec![Modifier::add_when(
                stat("ac").expect("literal stat key"),
                Formula::Const(1),
                Condition::Attuned,
            )],
            resource: None,
            ext: Default::default(),
        }),
        1,
        ItemState::Equipped,
        true,
    );
    builder.feature(FeatureEntry {
        id: FeatureEntryId::new(),
        category: FeatureCategory::Class,
        name: "Natural Explorer".to_owned(),
        reference: None,
        text: None,
        resource_id: None,
        modifiers: vec![
            Modifier::add(
                stat("skill.nature").expect("literal stat key"),
                Formula::proficiency_bonus(),
            ),
            Modifier::add(
                stat("initiative").expect("literal stat key"),
                Formula::ability_mod(Ability::Dexterity),
            ),
        ],
        ext: Default::default(),
    });
    // The single client path: the body and its entity header in one call.
    let entity: StatblockEntity = builder
        .build_entity(EntityKind::Pc, EntityId::new(), "Rowan", owner)
        .expect("a pc kind pairs with a statblock body");

    // Assert: the stored paper-sheet boxes are what we typed in.
    let body = &entity.body;
    assert_eq!(body.abilities.dexterity, 12);
    assert_eq!(body.hp.max, 11);
    assert_eq!(body.attacks[0].from_inventory, Some(longbow));
    assert_eq!(
        body.inventory[0].state,
        ItemState::Equipped,
        "equip state is a stored flag, not derived"
    );
    assert_eq!(
        body.overrides.get(&stat("ac").expect("literal stat key")),
        Some(&OverrideValue::Int(14))
    );
    // T7: the other setters must land too.
    assert_eq!(
        body.proficiencies.skills.get(&Skill::Perception),
        Some(&SkillProficiency::Proficient)
    );
    assert!(body.proficiencies.saves.contains(&Ability::Dexterity));
    assert_eq!(body.notes, "A hunter from the northern woods.");
    assert_eq!(body.attacks[0].name, "Longbow");
    assert_eq!(body.attacks[0].bonus, Some(4));
    assert_eq!(body.attacks[0].damage_text, "1d8+1 piercing");

    // The ring's contribution is conditional, and the feature carries a formula.
    let ring_entry = body.inventory.iter().find(|e| e.id == ring).unwrap();
    let InventoryItem::Inline(ring_item) = &ring_entry.item else {
        panic!("the ring is an inline item");
    };
    let ring_modifier = ring_item
        .modifiers
        .first()
        .expect("the ring has a modifier");
    assert_eq!(ring_modifier.when, Some(Condition::Attuned));
    assert_eq!(
        body.features[0].modifiers[0].op,
        ModifierOp::Add {
            value: Formula::proficiency_bonus()
        }
    );

    // The entity header is assembled by core's own builder, via `build_entity`.
    assert_eq!(entity.kind(), EntityKind::Pc);
}

#[test]
fn the_golden_characters_are_distinct_and_complete() {
    // Arrange / Act.
    let owner = PlayerId::new();
    let wizard = golden_wizard(owner);
    let fighter = golden_fighter(owner);
    let warlock = golden_warlock(owner);
    let multiclass = golden_multiclass(owner);

    // Assert: each fixture stores the base data its section 3 rules row needs.
    assert_eq!(wizard.body.levels[0].level, 5);
    assert_eq!(multiclass.body.levels.len(), 2, "wizard 3 / cleric 2");

    // T10: the cast members are distinct, not the same sheet under two names.
    assert_ne!(wizard.id, fighter.id);
    assert_ne!(wizard.body.levels[0].class, fighter.body.levels[0].class);

    // T17: a multiclass caster carries one spell block per class.
    assert_eq!(
        multiclass.body.spellcasting.len(),
        2,
        "wizard and cleric blocks"
    );

    // Fighter: short-rest feature resources are stored, not derived.
    assert!(
        fighter
            .body
            .resources
            .iter()
            .any(|r| matches!(r.kind, ResourceKind::Feature) && r.recharge == Recharge::Short)
    );

    // Warlock: a pact slot that returns on a short rest, and concentration.
    assert!(
        warlock
            .body
            .resources
            .iter()
            .any(|r| matches!(r.kind, ResourceKind::PactSlot) && r.recharge == Recharge::Short)
    );
    assert!(
        warlock
            .body
            .statuses
            .iter()
            .any(|s| s.kind == StatusKind::Concentration)
    );
}

#[test]
fn the_campaign_builder_wires_players_and_settings() {
    // Arrange / Act.
    let mut builder = CampaignBuilder::new("Curse of the Vault");
    let dm = builder.dm("Dungeon Master");
    let player = builder.player("Alex");
    builder.dawn_hour(6).feed_capacity(200);
    let campaign: Campaign = builder.build();

    // Assert.
    assert_eq!(campaign.name, "Curse of the Vault");
    assert_eq!(campaign.schema_version, SchemaVersion::CURRENT);
    assert_eq!(campaign.default_ruleset, RulesetId::dnd5e_2014());
    assert_eq!(campaign.players.len(), 2);
    assert_ne!(dm, player);
    assert_eq!(campaign.settings.dawn_hour, 6);
    assert_eq!(campaign.settings.feed_capacity, 200);

    // T8: both seats carry their role and the name the table sees.
    let dm_seat = campaign
        .players
        .iter()
        .find(|p| p.id == dm)
        .expect("the dm is a participant");
    assert_eq!(dm_seat.role, Role::Dm);
    assert_eq!(dm_seat.display_name, "Dungeon Master");
    let player_seat = campaign
        .players
        .iter()
        .find(|p| p.id == player)
        .expect("the player is a participant");
    assert_eq!(player_seat.role, Role::Player);
    assert_eq!(player_seat.display_name, "Alex");
}

#[test]
fn a_creature_uses_the_same_body_shape() {
    // Act.
    let goblin = goblin(PlayerId::new());

    // Assert: character-only sections are empty, creature-only ones are set.
    assert_eq!(goblin.body.profile, Profile::Creature);
    assert_eq!(goblin.body.cr, Some(ChallengeRating::parse("1/4").unwrap()));
    assert!(goblin.body.levels.is_empty());
    assert!(goblin.body.creature_type.is_some());
    // T9: the row's size, senses, action line and family kind.
    assert_eq!(goblin.kind(), EntityKind::Monster);
    assert_eq!(goblin.body.size, Size::Small, "a goblin is Small");
    assert!(
        !goblin.body.senses.is_empty(),
        "the catalog row stores senses as text"
    );
    assert!(
        goblin
            .body
            .features
            .iter()
            .any(|f| f.category == FeatureCategory::Action),
        "the monster's action line is stored"
    );

    let value: Value = serde_json::from_str(&to_json(&goblin).unwrap()).unwrap();
    assert_eq!(
        value["body"]["cr"],
        json!("1/4"),
        "challenge rating is a conventional string on the wire"
    );
}

// -- serialization / deserialization ----------------------------------------

#[test]
fn a_whole_party_round_trips_through_json() {
    // Arrange.
    let party = party();
    let before = party.entities();
    let campaign_before = to_json(&party.campaign).expect("campaign serializes");
    let entities_before = to_json(&before).expect("entities serialize");

    // Act: through a string, as storage would.
    let campaign_back: Campaign = from_json(&campaign_before).expect("campaign reloads");
    let entities_back: Vec<AnyEntity> = from_json(&entities_before).expect("entities reload");

    // Assert.
    assert_eq!(campaign_back, party.campaign);
    assert_eq!(entities_back, before);
    // The runtime-tagged round trip keeps each body reachable.
    assert!(entities_back.iter().any(|e| e.as_statblock().is_some()));
    assert!(entities_back.iter().any(|e| e.as_note().is_some()));
    assert!(entities_back.iter().any(|e| e.as_handout().is_some()));
}

#[test]
fn fixtures_are_deterministic_byte_for_byte() {
    // The determinism guarantee (DESIGN.md section 4) is what makes snapshot tests
    // and diffable exports possible later.
    assert_eq!(
        to_json(&party().campaign).expect("serializes"),
        to_json(&party().campaign).expect("serializes")
    );
    // T4: the cast and handout carry most of the ids, so cover them too.
    let a = party();
    let b = party();
    assert_eq!(
        to_json(&a.entities()).expect("serializes"),
        to_json(&b.entities()).expect("serializes")
    );
    assert_eq!(
        to_json(&a.handout).expect("serializes"),
        to_json(&b.handout).expect("serializes")
    );
    // Pinning a fixed expected string (a byte-for-byte snapshot of the fixture
    // bytes) is deferred until the fixture bytes are considered frozen
    // (`crates/localtt-integration/DESIGN.md` section 8, Deferred work).
}

#[test]
fn a_newer_schema_major_is_refused() {
    // Arrange: a document this build must not partially load.
    let mut value = serde_json::to_value(&party().pc).expect("entity serializes");
    value["schema_version"] = json!({"major": 2, "minor": 0});
    let raw = serde_json::to_string(&value).expect("reserializes");

    // Act.
    let result: Result<StatblockEntity, LoadError> = load_checked(&raw);

    // Assert.
    assert!(matches!(
        result,
        Err(LoadError::Schema(SchemaVersion { major: 2, .. }))
    ));
}

#[test]
fn load_checked_accepts_same_and_older_majors() {
    // T3: the gate must not reject everything. A same-major file with a newer
    // minor is additive-compatible, and an older major is still readable.
    let wizard = party().pc;
    let current = SchemaVersion::CURRENT;

    let mut value = serde_json::to_value(&wizard).expect("entity serializes");
    value["schema_version"] = json!({"major": current.major, "minor": current.minor + 1});
    let raw = serde_json::to_string(&value).expect("reserializes");
    let loaded: StatblockEntity = load_checked(&raw).expect("a same-major file loads");
    assert_eq!(loaded, wizard);

    value["schema_version"] = json!({"major": current.major.saturating_sub(1), "minor": 0});
    let raw = serde_json::to_string(&value).expect("reserializes");
    let older: StatblockEntity = load_checked(&raw).expect("an older major still loads");
    assert_eq!(older, wizard);
}

#[test]
fn a_campaign_loads_at_the_current_version_regardless_of_the_file() {
    // The entity path re-stamps the version (T3); the campaign singleton must
    // behave the same, or a same-major save would round-trip to a different
    // version than the build that wrote it.
    let campaign = party().campaign;
    let current = SchemaVersion::CURRENT;

    let mut value = serde_json::to_value(&campaign).expect("campaign serializes");
    value["schema_version"] = json!({"major": current.major, "minor": current.minor + 1});
    let raw = serde_json::to_string(&value).expect("reserializes");

    // Act.
    let loaded: Campaign = load_checked(&raw).expect("a same-major campaign loads");

    // Assert: the build's version, not the file's.
    assert_eq!(loaded.schema_version, SchemaVersion::CURRENT);
    assert_eq!(loaded, campaign);
}

#[test]
fn a_hand_edited_kind_body_mismatch_is_rejected() {
    // Arrange: the "someone edited the campaign JSON" failure.
    let mut value = serde_json::to_value(&party().pc).expect("entity serializes");
    value["kind"] = json!("note");
    let raw = serde_json::to_string(&value).expect("reserializes");

    // Act / Assert: the failure is what matters, not the serde error text.
    assert!(load_checked::<StatblockEntity>(&raw).is_err());

    // T2: the same pairing rule is a *typed* error at the core level. The load
    // path cannot name it because the family check runs inside `Entity`'s
    // `Deserialize`, so serde erases the error type (review B3); here we pin
    // the classification the core builder does expose.
    let owner = PlayerId::new();
    let body = golden_wizard(owner).body;
    let mismatch =
        Entity::builder(EntityKind::Note, EntityId::new(), "mismatch", owner, body).build();
    assert!(matches!(
        mismatch,
        Err(EntityError::KindBodyMismatch { .. })
    ));
}

#[test]
fn an_invalid_reference_is_rejected() {
    // A pack that ships a malformed id must fail at the boundary, not at use.
    // T22: this asserts core's `RefId` grammar directly; it is kept here as the
    // integration entry-point invariant (the unit-level cases live in core).
    assert!(RefId::parse("pack.mytome:item/Uppercase Slug").is_err());
    assert!(RefId::parse("not-a-ref").is_err());
    assert!(RefId::parse("srd2014:spell/fireball").is_ok());
}

// -- handling / transformation ----------------------------------------------

#[test]
fn damage_temp_hp_and_death_saves_are_representable() {
    // Arrange.
    let mut pc = golden_wizard(PlayerId::new());
    let start = pc.body.hp.current;

    // Act: temp HP absorbs first and does not stack; current may go negative.
    set_temp_hp(&mut pc, 5);
    assert_eq!(pc.body.hp.temp, 5, "the ward is stored");
    set_temp_hp(&mut pc, 3);
    assert_eq!(pc.body.hp.temp, 5, "temp HP keeps the higher of the two");
    apply_damage(&mut pc, 12);
    assert_eq!(pc.body.hp.temp, 0, "temp HP absorbs first");
    assert_eq!(pc.body.hp.current, start - 7, "5 temp + 7 real");
    heal(&mut pc, 4);

    // Assert the full progression, not just "some damage happened".
    assert_eq!(pc.body.hp.current, start - 3, "heal does not touch temp HP");
    assert_eq!(pc.body.hp.temp, 0);
    pc.body.death_saves = DeathSaves {
        successes: 2,
        failures: 1,
    };
    assert_eq!(
        pc.body.death_saves,
        DeathSaves {
            successes: 2,
            failures: 1
        }
    );

    // The state is representable and survives a round trip; it is the engine's
    // job to warn, never to block (RFC-0001 2.2).
    assert_eq!(pc.body.hp.max, 32);
    let back: StatblockEntity = from_json(&to_json(&pc).unwrap()).unwrap();
    assert_eq!(back, pc);
}

#[test]
fn spending_past_zero_is_allowed_and_round_trips() {
    // Arrange.
    let mut pc = golden_wizard(PlayerId::new());
    let slot_id: ResourceId = pc
        .body
        .resources
        .iter()
        .find(|r| matches!(r.kind, ResourceKind::SpellSlot { .. }) && r.current > 0)
        .expect("the wizard has a charged slot")
        .id;

    // Act 1: a normal spend decrements by exactly the amount (T11).
    let before = pc
        .body
        .resources
        .iter()
        .find(|r| r.id == slot_id)
        .unwrap()
        .current;
    spend_resource(&mut pc, slot_id, 1);
    let after = pc
        .body
        .resources
        .iter()
        .find(|r| r.id == slot_id)
        .unwrap()
        .current;
    assert_eq!(after, before - 1);
    let before_max = pc
        .body
        .resources
        .iter()
        .find(|r| r.id == slot_id)
        .unwrap()
        .max
        .clone();

    // Act 2: spend more than is left; warned, never blocked.
    spend_resource(&mut pc, slot_id, 99);

    // Assert: negative is reachable, the cap definition is untouched, and the
    // state survives a round trip.
    let slot = pc.body.resources.iter().find(|r| r.id == slot_id).unwrap();
    assert!(slot.current < 0);
    assert_eq!(slot.max, before_max);
    let back: StatblockEntity = from_json(&to_json(&pc).unwrap()).unwrap();
    assert_eq!(back, pc);
}

#[test]
fn beginning_concentration_replaces_the_existing_one() {
    // Arrange: a status holding the old concentration.
    fn concentration(name: &str, reference: &str, start_s: i64, end_s: i64) -> Status {
        Status {
            id: StatusId::new(),
            kind: StatusKind::Concentration,
            name: name.to_owned(),
            reference: Some(ref_id(reference).expect("literal ref")),
            text: None,
            started_at: CampaignTime::from_seconds(start_s),
            duration: StatusDuration::Timed {
                expires_at: CampaignTime::from_seconds(end_s),
            },
            source: None,
            modifiers: Vec::new(),
        }
    }
    let mut pc = golden_wizard(PlayerId::new());

    // Act: Hex replaces Haste (RFC-0001 5.4, command-layer replace semantics).
    let mut hex = concentration("Hex", "srd2014:spell/hex", 60, 3600);
    hex.modifiers.push(Modifier::add(
        stat("attack").expect("literal stat key"),
        Formula::dice(dice_pool(1, 6)),
    ));
    begin_concentration(
        &mut pc,
        concentration("Haste", "srd2014:spell/haste", 0, 60),
    );
    begin_concentration(&mut pc, hex);

    // Assert: exactly one concentration status, and it is the new one.
    let concentrations: Vec<&Status> = pc
        .body
        .statuses
        .iter()
        .filter(|s| s.kind == StatusKind::Concentration)
        .collect();
    assert_eq!(concentrations.len(), 1);
    assert_eq!(concentrations[0].name, "Hex");
}

#[test]
fn level_up_bumps_base_data_and_keeps_overrides() {
    // Arrange: a multiclass character, so "which class advanced" is observable.
    let mut pc = golden_multiclass(PlayerId::new());
    assert_eq!(pc.body.levels.len(), 2, "wizard 3 / cleric 2");
    let target_class = pc.body.levels[1].id;
    let other_class = pc.body.levels[0].id;
    let before_target = pc.body.levels[1].level;
    let before_other = pc.body.levels[0].level;
    let before_override = pc
        .body
        .overrides
        .get(&stat("ac").expect("literal stat key"))
        .cloned();

    // Act: level up the *second* class by its id (keyed by id, docs/decisions.md
    // ARCH-5).
    level_up(&mut pc, target_class, before_target + 1);

    // Assert: only the named class advanced; the stale override survives.
    assert_eq!(
        pc.body
            .levels
            .iter()
            .find(|l| l.id == target_class)
            .unwrap()
            .level,
        before_target + 1
    );
    assert_eq!(
        pc.body
            .levels
            .iter()
            .find(|l| l.id == other_class)
            .unwrap()
            .level,
        before_other,
        "the other class is untouched"
    );
    assert_eq!(
        pc.body
            .overrides
            .get(&stat("ac").expect("literal stat key"))
            .cloned(),
        before_override
    );
}

#[test]
fn level_up_keys_on_the_class_id_not_a_name() {
    // B1 + T6: a class stored as a reference is found by id, not by name text.
    let owner = PlayerId::new();
    let mut builder = StatblockBuilder::character();
    builder
        .class_level(
            RefOrText::Ref(content_ref("srd2014:class/wizard", "Wizard").expect("literal ref")),
            3,
            None,
        )
        .abilities(Abilities::all(12))
        .hp(20, 20);
    let mut pc: StatblockEntity = builder
        .build_entity(EntityKind::Pc, EntityId::new(), "Refy", owner)
        .expect("a pc kind pairs with a statblock body");

    let class = pc.body.levels[0].id;
    assert!(matches!(pc.body.levels[0].class, RefOrText::Ref(_)));

    level_up(&mut pc, class, 4);

    assert_eq!(pc.body.levels[0].level, 4);
}

#[test]
fn a_summon_is_a_fresh_copy_linked_to_its_caster() {
    // Arrange.
    let caster = golden_wizard(PlayerId::new());
    let template = goblin(PlayerId::new());

    // Act.
    let summon = summon_from_template(&template, &caster);

    // Assert: a copy, not a reference (RFC-0001 5.12); ids are fresh.
    assert_eq!(summon.kind(), EntityKind::Summon);
    assert_eq!(summon.parent, Some(caster.id));
    assert_eq!(summon.owner, caster.owner);
    assert_ne!(summon.id, template.id);
    assert!(
        !template.body.inventory.is_empty(),
        "the template must carry inventory for the fresh-id check to bite (T5)"
    );
    assert!(
        summon
            .body
            .inventory
            .iter()
            .zip(template.body.inventory.iter())
            .all(|(a, b)| a.id != b.id),
        "element ids are regenerated on copy"
    );
    assert_eq!(
        summon.body.abilities, template.body.abilities,
        "the copy starts from the template's numbers"
    );
}

#[test]
fn a_summon_regenerates_nested_ids_and_remaps_links() {
    // Arrange: a template whose element references must keep resolving inside
    // the copy (nested item charge, feature resource, status source link).
    let owner = PlayerId::new();
    let template_resource = ResourceId::new();
    let template_status = StatusId::new();
    let mut builder = StatblockBuilder::character();
    builder.item(
        InventoryItem::Inline(InlineItem {
            name: "Wand of Sparks".to_owned(),
            description: String::new(),
            kind: ItemKind::Wondrous,
            requires_attunement: false,
            weight_oz: None,
            value: None,
            properties: None,
            modifiers: Vec::new(),
            resource: Some(Resource {
                id: ResourceId::new(),
                name: "Charges".to_owned(),
                kind: ResourceKind::Item,
                max: Formula::Const(3),
                current: 3,
                recharge: Recharge::Manual,
            }),
            ext: Default::default(),
        }),
        1,
        ItemState::Carried,
        false,
    );
    builder
        .abilities(Abilities::all(10))
        .hp(10, 10)
        .resource(Resource {
            id: template_resource,
            name: "Focus".to_owned(),
            kind: ResourceKind::Feature,
            max: Formula::Const(1),
            current: 1,
            recharge: Recharge::Long,
        })
        .status(Status {
            id: template_status,
            kind: StatusKind::Concentration,
            name: "Hex".to_owned(),
            reference: None,
            text: None,
            started_at: CampaignTime::from_seconds(0),
            duration: StatusDuration::Indefinite,
            source: Some(StatusSource {
                entity: None,
                status: Some(template_status),
            }),
            modifiers: Vec::new(),
        });
    builder.feature(FeatureEntry {
        id: FeatureEntryId::new(),
        category: FeatureCategory::Class,
        name: "Focus".to_owned(),
        reference: None,
        text: None,
        resource_id: Some(template_resource),
        modifiers: Vec::new(),
        ext: Default::default(),
    });
    let template: StatblockEntity = builder
        .build_entity(EntityKind::Npc, EntityId::new(), "Template", owner)
        .expect("a npc kind pairs with a statblock body");
    let caster = golden_wizard(owner);

    // Act.
    let summon = summon_from_template(&template, &caster);

    // Assert: the nested item's charge definition got a fresh id.
    let nested_charge = |e: &StatblockEntity| match &e.body.inventory[0].item {
        InventoryItem::Inline(i) => i.resource.as_ref().map(|r| r.id),
        InventoryItem::Ref(_) => None,
    };
    assert!(
        nested_charge(&template).is_some(),
        "the template sets up the check"
    );
    assert_ne!(
        nested_charge(&summon),
        nested_charge(&template),
        "a nested resource id is an element id too, and is regenerated"
    );

    // The feature points at the copied (fresh) resource, not the template's.
    assert_ne!(summon.body.resources[0].id, template_resource);
    assert_eq!(
        summon.body.features[0].resource_id,
        Some(summon.body.resources[0].id)
    );

    // The status's source link resolves to the copied status, not the template's.
    let copied_status = summon.body.statuses[0].id;
    assert_ne!(copied_status, template_status);
    let source_status = summon.body.statuses[0]
        .source
        .as_ref()
        .and_then(|s| s.status);
    assert_eq!(source_status, Some(copied_status));
    assert!(
        summon
            .body
            .statuses
            .iter()
            .any(|s| Some(s.id) == source_status),
        "the remapped source resolves inside the summon"
    );
}

#[test]
fn hp_and_resource_arithmetic_saturate_at_the_i32_bounds() {
    // Extreme amounts must not panic in debug or wrap in release; they saturate
    // instead (RFC-0001 2.2: warn, never block).
    let mut pc = golden_wizard(PlayerId::new());
    pc.body.hp.current = 0;
    pc.body.hp.temp = 0;
    apply_damage(&mut pc, i32::MAX);
    assert_eq!(pc.body.hp.current, 0i32.saturating_sub(i32::MAX));

    pc.body.hp.current = i32::MIN;
    heal(&mut pc, i32::MAX);
    assert_eq!(
        pc.body.hp.current,
        i32::MIN.saturating_add(i32::MAX).min(pc.body.hp.max)
    );

    let id = pc.body.resources[0].id;
    pc.body.resources[0].current = 0;
    spend_resource(&mut pc, id, i32::MIN);
    assert_eq!(pc.body.resources[0].current, 0i32.saturating_sub(i32::MIN));
}

#[test]
fn giving_a_handout_copies_the_item_into_the_inventory() {
    // Arrange.
    let mut pc = golden_wizard(PlayerId::new());
    let handout = party().handout;
    let before = pc.body.inventory.len();

    // Act.
    let entry: InventoryEntryId = give_handout(&handout, &mut pc)
        .expect("the fixture handout carries an item, so it lands in the inventory");

    // Assert: the item lands as the recipient's own inline copy.
    assert_eq!(pc.body.inventory.len(), before + 1);
    let added = pc.body.inventory.iter().find(|e| e.id == entry).unwrap();
    assert_eq!(added.state, ItemState::Carried);
    // T12: the copy is the handout's own item definition.
    let source = match &handout.body.content {
        HandoutContent::Item { item } => item,
        HandoutContent::Text { .. } => panic!("the fixture handout is an item"),
    };
    let InventoryItem::Inline(copy) = &added.item else {
        panic!("the handout lands as an inline copy");
    };
    assert_eq!(copy, source, "the copy equals the handout's item");
}

// -- sharing (structure only; view_of is engine, phase E) --------------------

#[test]
fn a_read_share_carries_a_default_deny_mask() {
    // Arrange: a DM-owned monster shared read-only, name/hp/ac only.
    let mut mask = FieldMask::default();
    mask.insert(ViewField::Name);
    mask.insert(ViewField::Hp);
    mask.insert(ViewField::Ac);
    let mut monster = goblin(PlayerId::new());
    monster.acl = vec![AclEntry {
        player: PlayerId::new(),
        level: AclLevel::Read,
        field_mask: Some(mask),
    }];
    monster.body.overrides.insert(
        stat("ac").expect("literal stat key"),
        OverrideValue::Int(15),
    );
    monster.body.resources.push(Resource {
        id: ResourceId::new(),
        name: "Nimble Escape".to_owned(),
        kind: ResourceKind::Feature,
        max: Formula::Const(1),
        current: 1,
        recharge: Recharge::Short,
    });

    // Act / Assert: core stores the mask; the engine enforces it (phase E).
    let mask = monster.acl[0].field_mask.as_ref().unwrap();
    assert!(mask.contains(ViewField::Hp));
    assert!(!mask.contains(ViewField::Inventory));

    let value: Value = serde_json::from_str(&to_json(&monster).unwrap()).unwrap();
    // T13: the wire order is bound to `ViewField`'s declaration order; if that
    // enum is reordered, update this literal (it is a real compatibility pin).
    assert_eq!(
        value["acl"][0]["field_mask"],
        json!(["name", "hp", "ac"]),
        "the mask is stored in enum order"
    );
}

#[test]
fn a_handout_item_is_a_real_inline_definition() {
    // Arrange / Act.
    let handout = party().handout;

    // Assert: "give" hands over an item definition, not free text.
    assert!(matches!(&handout.body.content, HandoutContent::Item { .. }));
    let value: Value = serde_json::from_str(&to_json(&handout).unwrap()).unwrap();
    assert_eq!(value["body"]["content"]["type"], json!("item"));
}

// -- catalog coverage: DESIGN.md section 3 rows without a store-column test --

#[test]
fn statuses_cover_effects_conditions_and_durations() {
    // DESIGN.md section 3 rows "Bless riding a save" and "timed status vs clock".
    let mut pc = golden_wizard(PlayerId::new());
    let caster = pc.id;
    pc.body.statuses.extend([
        Status {
            id: StatusId::new(),
            kind: StatusKind::Effect,
            name: "Bless".to_owned(),
            reference: Some(ref_id("srd2014:spell/bless").expect("literal ref")),
            text: None,
            started_at: CampaignTime::from_seconds(10),
            duration: StatusDuration::Timed {
                expires_at: CampaignTime::from_seconds(70),
            },
            source: Some(StatusSource {
                entity: Some(caster),
                status: None,
            }),
            modifiers: vec![Modifier::add(
                stat("save").expect("literal stat key"),
                Formula::dice(dice_pool(1, 4)),
            )],
        },
        Status {
            id: StatusId::new(),
            kind: StatusKind::Condition,
            name: "Poisoned".to_owned(),
            reference: Some(ref_id("srd2014:condition/poisoned").expect("literal ref")),
            text: None,
            started_at: CampaignTime::from_seconds(10),
            duration: StatusDuration::UntilRest {
                rest: RestKind::Long,
            },
            source: None,
            modifiers: Vec::new(),
        },
        Status {
            id: StatusId::new(),
            kind: StatusKind::Custom,
            name: "Vault ward".to_owned(),
            reference: None,
            text: Some("Homebrew: disadvantage on the next check.".to_owned()),
            started_at: CampaignTime::from_seconds(10),
            duration: StatusDuration::Manual {
                text: "until the vault opens".to_owned(),
            },
            source: None,
            modifiers: Vec::new(),
        },
        Status {
            id: StatusId::new(),
            kind: StatusKind::Condition,
            name: "Cursed".to_owned(),
            reference: None,
            text: None,
            started_at: CampaignTime::from_seconds(10),
            duration: StatusDuration::Indefinite,
            source: None,
            modifiers: Vec::new(),
        },
    ]);

    let by_name = |n: &str| pc.body.statuses.iter().find(|s| s.name == n).unwrap();
    assert_eq!(by_name("Bless").kind, StatusKind::Effect);
    assert_eq!(
        by_name("Bless").duration,
        StatusDuration::Timed {
            expires_at: CampaignTime::from_seconds(70)
        },
        "the timed expiry is stored on the campaign clock"
    );
    assert_eq!(
        by_name("Bless").modifiers[0].target,
        stat("save").expect("literal stat key"),
        "Bless rides the save group"
    );
    assert_eq!(
        by_name("Bless").source,
        Some(StatusSource {
            entity: Some(caster),
            status: None
        })
    );
    assert_eq!(by_name("Poisoned").kind, StatusKind::Condition);
    assert_eq!(
        by_name("Poisoned").duration,
        StatusDuration::UntilRest {
            rest: RestKind::Long
        }
    );
    assert_eq!(by_name("Vault ward").kind, StatusKind::Custom);
    assert_eq!(
        by_name("Vault ward").duration,
        StatusDuration::Manual {
            text: "until the vault opens".to_owned()
        }
    );
    assert_eq!(by_name("Cursed").duration, StatusDuration::Indefinite);

    let back: StatblockEntity = from_json(&to_json(&pc).unwrap()).unwrap();
    assert_eq!(back, pc);
}

#[test]
fn a_roll_and_a_feed_entry_round_trip() {
    // DESIGN.md section 3 row "Roll + feed"; this pins the breakdown shapes.
    let roller = PlayerId::new();
    let entity = EntityId::new();
    let roll = Roll {
        id: RollId::new(),
        roller,
        entity: Some(entity),
        ts: UnixTime::from_seconds(1_700_000_000),
        expr: DiceExpr::Roll {
            count: 4,
            sides: 6,
            keep: Some(Keep::Highest { count: 3 }),
        },
        result: DiceResult {
            terms: vec![TermResult {
                dice: vec![
                    DieResult {
                        value: 6,
                        kept: true,
                    },
                    DieResult {
                        value: 5,
                        kept: true,
                    },
                    DieResult {
                        value: 4,
                        kept: true,
                    },
                    DieResult {
                        value: 1,
                        kept: false,
                    },
                ],
                total: 15,
                source: None,
            }],
            total: 15,
        },
        context: Some(RollContext {
            kind: RollKind::Damage,
            stat: Some(stat("attack.melee").expect("literal stat key")),
            target_dc: None,
        }),
        audience: Audience::RollerAndDm,
    };
    let entry = FeedEntry {
        id: FeedEntryId::new(1),
        ts: UnixTime::from_seconds(1_700_000_000),
        clock_s: CampaignTime::from_seconds(0),
        actor: Some(roller),
        entity: Some(entity),
        audience: Audience::RollerAndDm,
        payload: FeedPayload::Roll { roll: roll.clone() },
    };

    // The dice breakdown survives, including which die was dropped.
    let roll_back: Roll = from_json(&to_json(&roll).unwrap()).unwrap();
    assert_eq!(roll_back, roll);
    assert_eq!(roll_back.result.total, 15);
    assert_eq!(
        roll_back.result.terms[0]
            .dice
            .iter()
            .filter(|d| d.kept)
            .count(),
        3
    );

    // The feed entry keeps its tag, audience and nested roll.
    let entry_back: FeedEntry = from_json(&to_json(&entry).unwrap()).unwrap();
    assert_eq!(entry_back, entry);
    let value: Value = serde_json::from_str(&to_json(&entry).unwrap()).unwrap();
    assert_eq!(
        value["payload"]["type"],
        json!("roll"),
        "the payload is tagged"
    );
    assert_eq!(value["audience"], json!("roller_and_dm"));
}

#[test]
fn modifier_ops_and_formula_arithmetic_are_storable() {
    // T16: the ops beyond `Add`, the arithmetic and level leaves; T22: the
    // remaining closed conditions ride along.
    let spec: Vec<(&str, ModifierOp)> = vec![
        (
            "save",
            ModifierOp::Set {
                value: Formula::Const(15),
            },
        ),
        (
            "skill.stealth",
            ModifierOp::Min {
                value: Formula::Const(10),
            },
        ),
        (
            "ac",
            ModifierOp::Max {
                value: Formula::Const(20),
            },
        ),
        ("initiative", ModifierOp::Advantage),
        ("attack.melee", ModifierOp::Disadvantage),
    ];
    let mut modifiers: Vec<Modifier> = spec
        .into_iter()
        .map(|(raw, op)| Modifier {
            target: stat(raw).expect("literal stat key"),
            op,
            when: None,
            note: None,
        })
        .collect();
    modifiers.push(Modifier {
        target: stat("ac").expect("literal stat key"),
        op: ModifierOp::Add {
            value: Formula::Const(1),
        },
        when: Some(Condition::Equipped),
        note: None,
    });
    modifiers.push(Modifier {
        target: stat("ac").expect("literal stat key"),
        op: ModifierOp::Add {
            value: Formula::Const(1),
        },
        when: Some(Condition::StatusActive {
            name: "Bless".to_owned(),
        }),
        note: None,
    });

    // Every arithmetic leaf, over total level and class level.
    let lvl = Formula::Expr(FormulaExpr::Level);
    for (i, expr) in [
        FormulaExpr::Add {
            lhs: Box::new(lvl.clone()),
            rhs: Box::new(Formula::Const(1)),
        },
        FormulaExpr::Sub {
            lhs: Box::new(lvl.clone()),
            rhs: Box::new(Formula::Const(1)),
        },
        FormulaExpr::Mul {
            lhs: Box::new(lvl.clone()),
            rhs: Box::new(Formula::Const(2)),
        },
        FormulaExpr::Min {
            lhs: Box::new(lvl.clone()),
            rhs: Box::new(Formula::Const(20)),
        },
        FormulaExpr::Max {
            lhs: Box::new(lvl.clone()),
            rhs: Box::new(Formula::Const(1)),
        },
        FormulaExpr::FloorDiv {
            lhs: Box::new(lvl.clone()),
            rhs: Box::new(Formula::Const(2)),
        },
    ]
    .into_iter()
    .enumerate()
    {
        modifiers.push(Modifier {
            target: stat("speed").expect("literal stat key"),
            op: ModifierOp::Add {
                value: Formula::Expr(expr),
            },
            when: None,
            note: Some(format!("term {i}")),
        });
    }
    modifiers.push(Modifier {
        target: stat("spell.dc").expect("literal stat key"),
        op: ModifierOp::Add {
            value: Formula::Expr(FormulaExpr::ClassLevel {
                class: "Wizard".to_owned(),
            }),
        },
        when: None,
        note: None,
    });

    let mut pc = golden_wizard(PlayerId::new());
    pc.body.features.push(FeatureEntry {
        id: FeatureEntryId::new(),
        category: FeatureCategory::Feat,
        name: "Kitchen sink".to_owned(),
        reference: None,
        text: None,
        resource_id: None,
        modifiers,
        ext: Default::default(),
    });

    let feature = pc.body.features.last().unwrap();
    assert!(matches!(&feature.modifiers[0].op, ModifierOp::Set { .. }));
    assert!(matches!(&feature.modifiers[1].op, ModifierOp::Min { .. }));
    assert!(matches!(&feature.modifiers[2].op, ModifierOp::Max { .. }));
    assert_eq!(feature.modifiers[3].op, ModifierOp::Advantage);
    assert_eq!(feature.modifiers[4].op, ModifierOp::Disadvantage);
    assert_eq!(feature.modifiers[5].when, Some(Condition::Equipped));
    assert_eq!(
        feature.modifiers[6].when,
        Some(Condition::StatusActive {
            name: "Bless".to_owned()
        })
    );

    let back: StatblockEntity = from_json(&to_json(&pc).unwrap()).unwrap();
    assert_eq!(back, pc);
}

#[test]
fn a_three_class_and_a_duplicate_class_are_storable() {
    // T17: weird multiclass shapes are stored, not normalized away.
    let owner = PlayerId::new();
    let mut builder = StatblockBuilder::character();
    builder
        .class_level(RefOrText::Text("Wizard".to_owned()), 3, None)
        .class_level(RefOrText::Text("Cleric".to_owned()), 2, None)
        .class_level(RefOrText::Text("Rogue".to_owned()), 1, None)
        // A duplicate entry and a zero level are stored as-is; the engine
        // decides validity.
        .class_level(RefOrText::Text("Wizard".to_owned()), 1, None)
        .class_level(RefOrText::Text("Bard".to_owned()), 0, None)
        .abilities(Abilities::all(10))
        .hp(10, 10);
    let pc: StatblockEntity = builder
        .build_entity(EntityKind::Pc, EntityId::new(), "Multiclass", owner)
        .expect("a pc kind pairs with a statblock body");

    assert_eq!(pc.body.levels.len(), 5);
    assert_eq!(
        pc.body
            .levels
            .iter()
            .filter(|l| l.class == RefOrText::Text("Wizard".to_owned()))
            .count(),
        2,
        "the duplicate class entry survives"
    );
    assert!(pc.body.levels.iter().any(|l| l.level == 0));

    let back: StatblockEntity = from_json(&to_json(&pc).unwrap()).unwrap();
    assert_eq!(back, pc);
}

#[test]
fn custom_stats_and_homebrew_values_are_storable() {
    // T18: `StatKey::Custom` and non-integer overrides.
    let luck = StatKey::parse("house.luck").expect("a valid custom stat key");
    assert!(matches!(luck, StatKey::Custom(_)));

    let mut pc = golden_wizard(PlayerId::new());
    pc.body
        .overrides
        .insert(luck.clone(), OverrideValue::Bool(true));
    pc.body.overrides.insert(
        stat("passive.perception").expect("literal stat key"),
        OverrideValue::Text("18".to_owned()),
    );
    // A reference-less, text-only homebrew feature.
    pc.body.features.push(FeatureEntry {
        id: FeatureEntryId::new(),
        category: FeatureCategory::Other,
        name: "Lucky".to_owned(),
        reference: None,
        text: Some("Reroll one d20.".to_owned()),
        resource_id: None,
        modifiers: Vec::new(),
        ext: Default::default(),
    });

    assert_eq!(
        pc.body.overrides.get(&luck),
        Some(&OverrideValue::Bool(true))
    );
    assert_eq!(
        pc.body
            .overrides
            .get(&stat("passive.perception").expect("literal stat key")),
        Some(&OverrideValue::Text("18".to_owned()))
    );
    assert_eq!(
        pc.body.features.last().unwrap().category,
        FeatureCategory::Other
    );

    let back: StatblockEntity = from_json(&to_json(&pc).unwrap()).unwrap();
    assert_eq!(back, pc);
}

#[test]
fn boundary_values_are_storable() {
    // T19: rating edges, signed quantities, exhaustion and death saves.
    assert_eq!(
        ChallengeRating::parse("0").unwrap(),
        ChallengeRating::from_eighths(0).expect("0 is an official rating")
    );
    assert_eq!(
        ChallengeRating::parse("1/8").unwrap(),
        ChallengeRating::from_eighths(1).expect("1/8 is an official rating")
    );
    assert_eq!(
        ChallengeRating::parse("30").unwrap(),
        ChallengeRating::from_eighths(240).expect("30 is an official rating")
    );
    assert!(
        ChallengeRating::parse("1/3").is_err(),
        "1/3 is not a 5e rating"
    );
    assert!(
        ChallengeRating::from_eighths(3).is_err(),
        "3/8 is not a 5e rating"
    );

    let mut pc = golden_wizard(PlayerId::new());
    pc.body.inventory.push(InventoryEntry {
        id: InventoryEntryId::new(),
        item: InventoryItem::Inline(InlineItem {
            name: "Cursed coin".to_owned(),
            description: String::new(),
            kind: ItemKind::Other,
            requires_attunement: false,
            weight_oz: None,
            value: None,
            properties: None,
            modifiers: Vec::new(),
            resource: None,
            ext: Default::default(),
        }),
        qty: -1,
        state: ItemState::Carried,
        attuned: false,
        notes: String::new(),
    });
    pc.body.exhaustion = 6;
    pc.body.death_saves = DeathSaves {
        successes: 3,
        failures: 3,
    };
    // Negative current HP, driven through the damage path.
    apply_damage(&mut pc, 100);

    assert_eq!(pc.body.inventory.last().unwrap().qty, -1);
    assert_eq!(pc.body.exhaustion, 6);
    assert_eq!(
        pc.body.death_saves,
        DeathSaves {
            successes: 3,
            failures: 3
        }
    );
    assert!(pc.body.hp.current < 0);

    let back: StatblockEntity = from_json(&to_json(&pc).unwrap()).unwrap();
    assert_eq!(back, pc);
}

#[test]
fn the_kind_body_matrix_accepts_each_family() {
    // T20: every statblock-family kind, a positive note/handout build, and the
    // header setters core exposes.
    let owner = PlayerId::new();
    let body = golden_wizard(owner).body;
    for kind in [
        EntityKind::Pc,
        EntityKind::Companion,
        EntityKind::Summon,
        EntityKind::Familiar,
        EntityKind::Npc,
        EntityKind::Monster,
    ] {
        let built = Entity::builder(kind, EntityId::new(), "Kind", owner, body.clone()).build();
        assert!(built.is_ok(), "{kind:?} pairs with a statblock body");
    }

    let note = Entity::builder(
        EntityKind::Note,
        EntityId::new(),
        "Session 4",
        owner,
        NoteBody {
            text: "# Recap".to_owned(),
        },
    )
    .rev(7)
    .parent(EntityId::new())
    .acl(vec![AclEntry {
        player: PlayerId::new(),
        level: AclLevel::Write,
        field_mask: None,
    }])
    .schema_version(SchemaVersion { major: 1, minor: 2 })
    .build()
    .expect("a note kind pairs with a note body");
    assert_eq!(note.rev, 7);
    assert!(note.parent.is_some());
    assert_eq!(note.acl[0].level, AclLevel::Write);
    assert_eq!(note.schema_version, SchemaVersion { major: 1, minor: 2 });

    let handout = Entity::builder(
        EntityKind::Handout,
        EntityId::new(),
        "A map",
        owner,
        HandoutBody {
            content: HandoutContent::Text {
                text: "See attached".to_owned(),
            },
        },
    )
    .build()
    .expect("a handout kind pairs with a handout body");
    assert_eq!(
        handout.body.content,
        HandoutContent::Text {
            text: "See attached".to_owned()
        }
    );

    // A mismatched family is rejected, not coerced (contrast T2).
    let bad = Entity::builder(
        EntityKind::Note,
        EntityId::new(),
        "Mixed up",
        owner,
        HandoutBody {
            content: HandoutContent::Text {
                text: "x".to_owned(),
            },
        },
    )
    .build();
    assert!(matches!(bad, Err(EntityError::KindBodyMismatch { .. })));
}

#[test]
fn the_canonical_statkeys_parse() {
    // T21: pin the vocabulary the engine and UI rely on.
    assert_eq!(
        StatKey::parse("save.wis"),
        Ok(StatKey::Known(KnownStat::Save(Ability::Wisdom)))
    );
    assert_eq!(
        StatKey::parse("skill.stealth"),
        Ok(StatKey::Known(KnownStat::Skill(Skill::Stealth)))
    );
    assert_eq!(
        StatKey::parse("ability_mod.str"),
        Ok(StatKey::Known(KnownStat::AbilityModifier(
            Ability::Strength
        )))
    );
    assert_eq!(
        StatKey::parse("passive.perception"),
        Ok(StatKey::Known(KnownStat::PassiveScore(Skill::Perception)))
    );
    assert_eq!(
        StatKey::parse("attack.melee"),
        Ok(StatKey::Known(KnownStat::Attack(AttackKind::Melee)))
    );
    assert_eq!(
        StatKey::parse("spell.dc"),
        Ok(StatKey::Known(KnownStat::SpellDc))
    );
    assert_eq!(
        StatKey::parse("save"),
        Ok(StatKey::Known(KnownStat::Group(StatGroup::Save)))
    );
}

#[test]
fn the_suite_types_are_send_and_sync() {
    // These cross the server boundary; keep the contract local to the
    // suite rather than relying only on the core test.
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<localtt_integration::fixtures::Party>();
    assert_send_sync::<StatblockEntity>();
    assert_send_sync::<AnyEntity>();
    assert_send_sync::<Campaign>();
    assert_send_sync::<Roll>();
    assert_send_sync::<FeedEntry>();
}
