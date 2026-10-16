//! Target test for RFC-0003: `crates/localtt-integration/RFC-0003-integration.md`.
//!
//! Spec phase. This file intentionally does not compile until the skeleton
//! phase lands both the `localtt-integration` crate (section 5 of the RFC) and
//! the additive core constructors (section 4). Do not add scaffolding here to
//! make it build; change the RFC sketch instead.
//!
//! Coverage, across the crates as they land:
//!
//! - character creation through `localtt_core::builders` (a hand-built
//!   character) and this crate's `fixtures` (the golden-character catalog of
//!   RFC-0003 section 3);
//! - handling/transformation through `localtt_engine::commands` (damage, temp
//!   HP, heal, resource spend, concentration replace, level up, summon, handout
//!   give);
//! - serialization/deserialization through this crate's `roundtrip` and the
//!   schema gate in `localtt_adapter::load`, including whole-party round trips;
//! - failure cases: a newer schema major is refused, a hand-edited
//!   kind/body mismatch is rejected, an invalid reference is rejected.
//!
//! It does not assert derived values: proficiency bonus, AC, save DC and spell
//! slots need `localtt-engine` and are catalogued in RFC-0003 section 3 (phase
//! E) for the follow-up RFC. The suite is plain data, so there is no contention
//! case; the concurrency contract is the static `Send + Sync` bound already
//! asserted by RFC-0002's test.

use localtt_adapter::load::{LoadError, load_checked};
use localtt_core::{
    builders::{CampaignBuilder, StatblockBuilder, content_ref, dice_pool, ref_id, stat},
    types::{
        Abilities,
        Ability,
        AclEntry,
        AclLevel,
        AnyEntity,
        Campaign,
        CampaignTime,
        ChallengeRating,
        Condition,
        EntityKind,
        FeatureCategory,
        FeatureEntry,
        FeatureEntryId,
        FieldMask,
        Formula,
        HandoutContent,
        InlineItem,
        InventoryEntryId,
        InventoryItem,
        ItemKind,
        ItemState,
        Modifier,
        OverrideValue,
        PlayerId,
        Profile,
        Recharge,
        RefId,
        Resource,
        ResourceId,
        ResourceKind,
        RulesetId,
        SchemaVersion,
        Skill,
        SkillProficiency,
        StatblockEntity,
        Status,
        StatusDuration,
        StatusId,
        StatusKind,
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
        .build_entity(
            EntityKind::Pc,
            localtt_core::types::EntityId::new(),
            "Rowan",
            owner,
        )
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
        localtt_core::types::ModifierOp::Add {
            value: Formula::proficiency_bonus()
        }
    );

    // The entity header is assembled by core's own builder, via `build_entity`.
    assert_eq!(entity.kind, EntityKind::Pc);
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
    // The determinism guarantee (RFC-0003 6.2) is what makes snapshot tests
    // and diffable exports possible later.
    assert_eq!(
        to_json(&party().campaign).expect("serializes"),
        to_json(&party().campaign).expect("serializes")
    );
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
fn a_hand_edited_kind_body_mismatch_is_rejected() {
    // Arrange: the "someone edited the campaign JSON" failure.
    let mut value = serde_json::to_value(&party().pc).expect("entity serializes");
    value["kind"] = json!("note");
    let raw = serde_json::to_string(&value).expect("reserializes");

    // Act / Assert: the failure is what matters, not the serde error text.
    assert!(load_checked::<StatblockEntity>(&raw).is_err());
}

#[test]
fn an_invalid_reference_is_rejected() {
    // A pack that ships a malformed id must fail at the boundary, not at use.
    assert!(RefId::parse("pack.mytome:item/Uppercase Slug").is_err());
    assert!(RefId::parse("not-a-ref").is_err());
    assert!(RefId::parse("srd2014:spell/fireball").is_ok());
}

// -- handling / transformation ----------------------------------------------

#[test]
fn damage_temp_hp_and_death_saves_are_representable() {
    // Arrange.
    let mut pc = golden_wizard(PlayerId::new());

    // Act: temp HP absorbs first and does not stack; current may go negative.
    set_temp_hp(&mut pc, 5);
    set_temp_hp(&mut pc, 3); // keeps the higher, 5
    apply_damage(&mut pc, 12);
    heal(&mut pc, 4);

    // Assert: the state is representable and survives a round trip; it is the
    // engine's job to warn, never to block (RFC-0001 2.2).
    assert_eq!(pc.body.hp.max, 32);
    assert!(pc.body.hp.current < pc.body.hp.max);
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
        .find(|r| matches!(r.kind, ResourceKind::SpellSlot { .. }))
        .expect("the wizard has slots")
        .id;

    // Act: spend more than is left; warned, never blocked.
    spend_resource(&mut pc, slot_id, 99);

    // Assert.
    let slot = pc.body.resources.iter().find(|r| r.id == slot_id).unwrap();
    assert!(slot.current < 0);
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
    // Arrange.
    let mut pc = golden_wizard(PlayerId::new());
    let before_level = pc.body.levels[0].level;
    let before_override = pc
        .body
        .overrides
        .get(&stat("ac").expect("literal stat key"))
        .cloned();

    // Act: level up the class by its id (RFC-0003 B1), not by a name string.
    let class = pc.body.levels[0].id;
    level_up(&mut pc, class, before_level + 1);

    // Assert: base data advanced, the stale override survives (RFC-0001 5.1).
    assert_eq!(pc.body.levels[0].level, before_level + 1);
    assert_eq!(
        pc.body
            .overrides
            .get(&stat("ac").expect("literal stat key"))
            .cloned(),
        before_override
    );
}

#[test]
fn a_summon_is_a_fresh_copy_linked_to_its_caster() {
    // Arrange.
    let caster = golden_wizard(PlayerId::new());
    let template = goblin(PlayerId::new());

    // Act.
    let summon = summon_from_template(&template, &caster);

    // Assert: a copy, not a reference (RFC-0001 5.12); ids are fresh.
    assert_eq!(summon.kind, EntityKind::Summon);
    assert_eq!(summon.parent, Some(caster.id));
    assert_eq!(summon.owner, caster.owner);
    assert_ne!(summon.id, template.id);
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
fn giving_a_handout_copies_the_item_into_the_inventory() {
    // Arrange.
    let mut pc = golden_wizard(PlayerId::new());
    let handout = party().handout;
    let before = pc.body.inventory.len();

    // Act.
    let entry: InventoryEntryId = give_handout(&handout, &mut pc);

    // Assert: the item lands as the recipient's own inline copy.
    assert_eq!(pc.body.inventory.len(), before + 1);
    let added = pc.body.inventory.iter().find(|e| e.id == entry).unwrap();
    assert!(matches!(&added.item, InventoryItem::Inline(_)));
    assert_eq!(added.state, ItemState::Carried);
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
