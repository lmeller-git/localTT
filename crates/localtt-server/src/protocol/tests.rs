//! Golden JSON and mapping tests for the transport DTOs.

use std::collections::{BTreeMap, BTreeSet};

use localtt_core::{
    builders::StatblockBuilder,
    types::{
        Abilities,
        AnyEntity,
        Audience,
        CampaignId,
        CampaignTime,
        ContentRef,
        DeathSaves,
        DiceExpr,
        EntityKind,
        FeatureEntryId,
        FeedEntryId,
        FeedPayload,
        Id,
        InventoryEntryId,
        InventoryItem,
        ItemState,
        Namespace,
        PlayerId,
        RefId,
        ResourceId,
        Role,
        RulesetId,
        SchemaVersion,
        SpellEntryId,
        StatKey,
        StatusDuration,
        StatusId,
        StatusKind,
        UnixTime,
    },
};
use localtt_engine::{
    authz::{CampaignSettingsView, CampaignView, EntityView, FeedEntryView, RosterEntryView},
    commands::{Command, Event, RollSpec},
    content::{ContentData, ContentEntry, ContentFault, ContentKind, ContentSummary, Provenance},
    error::{ElementRef, EngineError, Warning, WarningCode},
    executor::Executed,
    ports::store::StoreFault,
};
use serde_json::json;
use uuid::Uuid;

use super::*;
use crate::{
    app::{CommandOutcome, FeedPage, StreamEvent},
    error::AppError,
};

fn id<T>(value: u128) -> Id<T> {
    Id::from_uuid(Uuid::from_u128(value))
}

fn entity_id() -> localtt_core::types::EntityId {
    id(1)
}

fn player_id() -> PlayerId {
    id(2)
}

fn status_id() -> StatusId {
    id(3)
}

fn resource_id() -> ResourceId {
    id(4)
}

fn entry_id() -> InventoryEntryId {
    id(5)
}

fn spell_entry_id() -> SpellEntryId {
    id(6)
}

fn feature_entry_id() -> FeatureEntryId {
    id(7)
}

fn campaign_id() -> CampaignId {
    id(8)
}

fn recipient_id() -> localtt_core::types::EntityId {
    id(15)
}

fn inventory_item() -> InventoryItem {
    InventoryItem::Ref(ContentRef {
        id: RefId::parse("srd2014:item/potion").expect("a valid reference"),
        name: "Potion".to_owned(),
    })
}

fn status_spec() -> StatusSpecDto {
    StatusSpecDto {
        kind: StatusKind::Condition,
        name: "Prone".to_owned(),
        reference: None,
        text: None,
        duration: StatusDuration::Indefinite,
        source: None,
        modifiers: Vec::new(),
    }
}

fn sample_entity() -> AnyEntity {
    let mut builder = StatblockBuilder::character();
    builder.abilities(Abilities::all(10)).hp(12, 12);
    builder
        .build_entity(EntityKind::Pc, entity_id(), "Aria", player_id())
        .expect("the test statblock has its required sections")
        .into()
}

fn provenance() -> Provenance {
    Provenance {
        namespace: Namespace::parse("srd2014").expect("a valid namespace"),
        source: "SRD 5.1".to_owned(),
        source_revision: "2024-01-01".to_owned(),
        license: "CC-BY-4.0".to_owned(),
        attribution: "Example attribution".to_owned(),
        url: None,
    }
}

fn assert_command_json(dto: CommandDto, expected: serde_json::Value) {
    assert_eq!(
        serde_json::to_value(&dto).expect("the DTO serializes"),
        expected
    );
    let decoded: CommandDto =
        serde_json::from_value(expected).expect("the golden JSON deserializes");
    Command::try_from(decoded).expect("every known command converts");
}

#[test]
fn command_golden_json_covers_every_variant() {
    let entity = entity_id();
    let item = inventory_item();

    assert_command_json(
        CommandDto::AdvanceClock { by_seconds: 60 },
        json!({"type": "advance_clock", "by_seconds": 60}),
    );
    assert_command_json(
        CommandDto::SetClock {
            to: CampaignTime::from_seconds(120),
        },
        json!({"type": "set_clock", "to": 120}),
    );

    let entity_document = sample_entity();
    let entity_json = serde_json::to_value(&entity_document).expect("the entity serializes");
    assert_command_json(
        CommandDto::CreateEntity {
            entity: Box::new(entity_document),
        },
        json!({"type": "create_entity", "entity": entity_json}),
    );

    assert_command_json(
        CommandDto::AddInventoryItem {
            entity,
            item: Box::new(item),
            qty: 2,
            state: ItemState::Equipped,
            attuned: true,
            notes: "belt".to_owned(),
            expected_rev: Some(3),
        },
        json!({
            "type": "add_inventory_item",
            "entity": entity,
            "item": {"type": "ref", "id": "srd2014:item/potion", "name": "Potion"},
            "qty": 2,
            "state": "equipped",
            "attuned": true,
            "notes": "belt",
            "expected_rev": 3
        }),
    );
    assert_command_json(
        CommandDto::EditInventoryItem {
            entity,
            entry: entry_id(),
            qty: Some(1),
            state: Some(ItemState::Stowed),
            attuned: Some(false),
            notes: Some("packed".to_owned()),
            expected_rev: Some(4),
        },
        json!({
            "type": "edit_inventory_item",
            "entity": entity,
            "entry": entry_id(),
            "qty": 1,
            "state": "stowed",
            "attuned": false,
            "notes": "packed",
            "expected_rev": 4
        }),
    );
    assert_command_json(
        CommandDto::RemoveInventoryItem {
            entity,
            entry: entry_id(),
            expected_rev: None,
        },
        json!({"type": "remove_inventory_item", "entity": entity, "entry": entry_id()}),
    );
    assert_command_json(
        CommandDto::GiveHandout {
            handout: entity,
            recipient: recipient_id(),
            expected_rev: Some(5),
        },
        json!({
            "type": "give_handout",
            "handout": entity,
            "recipient": recipient_id(),
            "expected_rev": 5
        }),
    );
    assert_command_json(
        CommandDto::AddStatus {
            entity,
            status: status_spec(),
            expected_rev: None,
        },
        json!({
            "type": "add_status",
            "entity": entity,
            "status": {"kind": "condition", "name": "Prone", "duration": {"type": "indefinite"}}
        }),
    );
    assert_command_json(
        CommandDto::RemoveStatus {
            entity,
            status: status_id(),
            expected_rev: Some(6),
        },
        json!({
            "type": "remove_status",
            "entity": entity,
            "status": status_id(),
            "expected_rev": 6
        }),
    );
    assert_command_json(
        CommandDto::EditStatusDuration {
            entity,
            status: status_id(),
            duration: StatusDuration::Timed {
                expires_at: CampaignTime::from_seconds(600),
            },
            expected_rev: None,
        },
        json!({
            "type": "edit_status_duration",
            "entity": entity,
            "status": status_id(),
            "duration": {"type": "timed", "expires_at": 600}
        }),
    );
    assert_command_json(
        CommandDto::ApplyDamage {
            entity,
            amount: 7,
            expected_rev: Some(7),
        },
        json!({"type": "apply_damage", "entity": entity, "amount": 7, "expected_rev": 7}),
    );
    assert_command_json(
        CommandDto::Heal {
            entity,
            amount: 4,
            expected_rev: None,
        },
        json!({"type": "heal", "entity": entity, "amount": 4}),
    );
    assert_command_json(
        CommandDto::SetTempHp {
            entity,
            amount: 3,
            expected_rev: None,
        },
        json!({"type": "set_temp_hp", "entity": entity, "amount": 3}),
    );
    assert_command_json(
        CommandDto::SetDeathSaves {
            entity,
            value: DeathSaves {
                successes: 1,
                failures: 2,
            },
            expected_rev: None,
        },
        json!({
            "type": "set_death_saves",
            "entity": entity,
            "value": {"successes": 1, "failures": 2}
        }),
    );
    assert_command_json(
        CommandDto::SpendResource {
            entity,
            resource: resource_id(),
            amount: 1,
            expected_rev: None,
        },
        json!({
            "type": "spend_resource",
            "entity": entity,
            "resource": resource_id(),
            "amount": 1
        }),
    );
    assert_command_json(
        CommandDto::RestoreResource {
            entity,
            resource: resource_id(),
            amount: 2,
            expected_rev: None,
        },
        json!({
            "type": "restore_resource",
            "entity": entity,
            "resource": resource_id(),
            "amount": 2
        }),
    );
    assert_command_json(
        CommandDto::ShortRest {
            entity,
            advance_seconds: Some(3600),
            expected_rev: None,
        },
        json!({"type": "short_rest", "entity": entity, "advance_seconds": 3600}),
    );
    assert_command_json(
        CommandDto::LongRest {
            entity,
            advance_seconds: None,
            expected_rev: None,
        },
        json!({"type": "long_rest", "entity": entity}),
    );
    assert_command_json(
        CommandDto::Roll {
            entity: Some(entity),
            spec: RollSpecDto::Check {
                stat: StatKey::parse("save.wis").expect("a known stat"),
                target_dc: Some(15),
            },
            audience: Some(Audience::Public),
        },
        json!({
            "type": "roll",
            "entity": entity,
            "spec": {"type": "check", "stat": "save.wis", "target_dc": 15},
            "audience": "public"
        }),
    );
    assert_command_json(
        CommandDto::CastSpell {
            entity,
            spell: spell_entry_id(),
            slot: resource_id(),
            upcast_slots: 1,
            concentrate: true,
            expected_rev: Some(8),
        },
        json!({
            "type": "cast_spell",
            "entity": entity,
            "spell": spell_entry_id(),
            "slot": resource_id(),
            "upcast_slots": 1,
            "concentrate": true,
            "expected_rev": 8
        }),
    );
    assert_command_json(
        CommandDto::BeginConcentration {
            entity,
            status: status_spec(),
            expected_rev: None,
        },
        json!({
            "type": "begin_concentration",
            "entity": entity,
            "status": {"kind": "condition", "name": "Prone", "duration": {"type": "indefinite"}}
        }),
    );
    assert_command_json(
        CommandDto::EndConcentration {
            entity,
            expected_rev: None,
        },
        json!({"type": "end_concentration", "entity": entity}),
    );
    assert_command_json(
        CommandDto::SpendHitDie {
            entity,
            resource: resource_id(),
            expected_rev: None,
        },
        json!({
            "type": "spend_hit_die",
            "entity": entity,
            "resource": resource_id()
        }),
    );
}

#[test]
fn the_roll_expression_variant_maps_its_dice_and_target() {
    let dto = CommandDto::Roll {
        entity: None,
        spec: RollSpecDto::Expression {
            expr: DiceExpr::Roll {
                count: 1,
                sides: 20,
                keep: None,
            },
            target: Some(StatKey::parse("attack").expect("a known stat")),
        },
        audience: None,
    };

    let command = Command::try_from(dto).expect("a known command");
    let Command::Roll {
        entity,
        spec,
        audience,
    } = command
    else {
        panic!("the DTO maps to a roll");
    };
    assert!(entity.is_none());
    assert!(audience.is_none());
    let RollSpec::Expression { expr, target } = spec else {
        panic!("the DTO maps to an expression roll");
    };
    assert_eq!(
        expr,
        DiceExpr::Roll {
            count: 1,
            sides: 20,
            keep: None
        }
    );
    assert_eq!(
        target,
        Some(StatKey::parse("attack").expect("a known stat"))
    );
}

#[test]
fn inventory_field_mapping_is_not_swapped() {
    let dto = CommandDto::EditInventoryItem {
        entity: entity_id(),
        entry: entry_id(),
        qty: Some(9),
        state: Some(ItemState::Carried),
        attuned: Some(true),
        notes: Some("notes".to_owned()),
        expected_rev: Some(11),
    };

    let command = Command::try_from(dto).expect("a known command");
    let Command::EditInventoryItem {
        entity,
        entry,
        qty,
        state,
        attuned,
        notes,
        expected_rev,
    } = command
    else {
        panic!("the DTO maps to an inventory edit");
    };
    assert_eq!(entity, entity_id());
    assert_eq!(entry, entry_id());
    assert_eq!(qty, Some(9));
    assert_eq!(state, Some(ItemState::Carried));
    assert_eq!(attuned, Some(true));
    assert_eq!(notes.as_deref(), Some("notes"));
    assert_eq!(expected_rev, Some(11));
}

#[test]
fn every_known_command_maps_to_its_engine_variant() {
    macro_rules! maps {
        ($dto:expr, $expected:expr) => {
            assert_eq!(Command::try_from($dto).expect("a known command"), $expected);
        };
    }

    let entity = entity_id();
    let duration = StatusDuration::Timed {
        expires_at: CampaignTime::from_seconds(600),
    };
    let entity_document = sample_entity();

    maps!(
        CommandDto::AdvanceClock { by_seconds: 60 },
        Command::AdvanceClock { by_seconds: 60 }
    );
    maps!(
        CommandDto::SetClock {
            to: CampaignTime::from_seconds(120)
        },
        Command::SetClock {
            to: CampaignTime::from_seconds(120)
        }
    );
    maps!(
        CommandDto::CreateEntity {
            entity: Box::new(entity_document.clone())
        },
        Command::CreateEntity {
            entity: Box::new(entity_document)
        }
    );
    maps!(
        CommandDto::AddInventoryItem {
            entity,
            item: Box::new(inventory_item()),
            qty: 2,
            state: ItemState::Equipped,
            attuned: true,
            notes: "belt".to_owned(),
            expected_rev: Some(3),
        },
        Command::AddInventoryItem {
            entity,
            item: Box::new(inventory_item()),
            qty: 2,
            state: ItemState::Equipped,
            attuned: true,
            notes: "belt".to_owned(),
            expected_rev: Some(3),
        }
    );
    maps!(
        CommandDto::EditInventoryItem {
            entity,
            entry: entry_id(),
            qty: Some(1),
            state: Some(ItemState::Stowed),
            attuned: Some(false),
            notes: Some("packed".to_owned()),
            expected_rev: Some(4),
        },
        Command::EditInventoryItem {
            entity,
            entry: entry_id(),
            qty: Some(1),
            state: Some(ItemState::Stowed),
            attuned: Some(false),
            notes: Some("packed".to_owned()),
            expected_rev: Some(4),
        }
    );
    maps!(
        CommandDto::RemoveInventoryItem {
            entity,
            entry: entry_id(),
            expected_rev: None,
        },
        Command::RemoveInventoryItem {
            entity,
            entry: entry_id(),
            expected_rev: None,
        }
    );
    maps!(
        CommandDto::GiveHandout {
            handout: entity,
            recipient: recipient_id(),
            expected_rev: Some(5),
        },
        Command::GiveHandout {
            handout: entity,
            recipient: recipient_id(),
            expected_rev: Some(5),
        }
    );
    maps!(
        CommandDto::AddStatus {
            entity,
            status: status_spec(),
            expected_rev: None,
        },
        Command::AddStatus {
            entity,
            status: status_spec().into(),
            expected_rev: None,
        }
    );
    maps!(
        CommandDto::RemoveStatus {
            entity,
            status: status_id(),
            expected_rev: Some(6),
        },
        Command::RemoveStatus {
            entity,
            status: status_id(),
            expected_rev: Some(6),
        }
    );
    maps!(
        CommandDto::EditStatusDuration {
            entity,
            status: status_id(),
            duration: duration.clone(),
            expected_rev: None,
        },
        Command::EditStatusDuration {
            entity,
            status: status_id(),
            duration,
            expected_rev: None,
        }
    );
    maps!(
        CommandDto::ApplyDamage {
            entity,
            amount: 7,
            expected_rev: Some(7),
        },
        Command::ApplyDamage {
            entity,
            amount: 7,
            expected_rev: Some(7),
        }
    );
    maps!(
        CommandDto::Heal {
            entity,
            amount: 4,
            expected_rev: None,
        },
        Command::Heal {
            entity,
            amount: 4,
            expected_rev: None,
        }
    );
    maps!(
        CommandDto::SetTempHp {
            entity,
            amount: 3,
            expected_rev: None,
        },
        Command::SetTempHp {
            entity,
            amount: 3,
            expected_rev: None,
        }
    );
    maps!(
        CommandDto::SetDeathSaves {
            entity,
            value: DeathSaves {
                successes: 1,
                failures: 2,
            },
            expected_rev: None,
        },
        Command::SetDeathSaves {
            entity,
            value: DeathSaves {
                successes: 1,
                failures: 2,
            },
            expected_rev: None,
        }
    );
    maps!(
        CommandDto::SpendResource {
            entity,
            resource: resource_id(),
            amount: 1,
            expected_rev: None,
        },
        Command::SpendResource {
            entity,
            resource: resource_id(),
            amount: 1,
            expected_rev: None,
        }
    );
    maps!(
        CommandDto::RestoreResource {
            entity,
            resource: resource_id(),
            amount: 2,
            expected_rev: None,
        },
        Command::RestoreResource {
            entity,
            resource: resource_id(),
            amount: 2,
            expected_rev: None,
        }
    );
    maps!(
        CommandDto::ShortRest {
            entity,
            advance_seconds: Some(3600),
            expected_rev: None,
        },
        Command::ShortRest {
            entity,
            advance_seconds: Some(3600),
            expected_rev: None,
        }
    );
    maps!(
        CommandDto::LongRest {
            entity,
            advance_seconds: None,
            expected_rev: None,
        },
        Command::LongRest {
            entity,
            advance_seconds: None,
            expected_rev: None,
        }
    );
    maps!(
        CommandDto::Roll {
            entity: Some(entity),
            spec: RollSpecDto::Check {
                stat: StatKey::parse("save.wis").expect("a known stat"),
                target_dc: Some(15),
            },
            audience: Some(Audience::Public),
        },
        Command::Roll {
            entity: Some(entity),
            spec: RollSpec::Check {
                stat: StatKey::parse("save.wis").expect("a known stat"),
                target_dc: Some(15),
            },
            audience: Some(Audience::Public),
        }
    );
    maps!(
        CommandDto::CastSpell {
            entity,
            spell: spell_entry_id(),
            slot: resource_id(),
            upcast_slots: 1,
            concentrate: true,
            expected_rev: Some(8),
        },
        Command::CastSpell {
            entity,
            spell: spell_entry_id(),
            slot: resource_id(),
            upcast_slots: 1,
            concentrate: true,
            expected_rev: Some(8),
        }
    );
    maps!(
        CommandDto::BeginConcentration {
            entity,
            status: status_spec(),
            expected_rev: None,
        },
        Command::BeginConcentration {
            entity,
            status: status_spec().into(),
            expected_rev: None,
        }
    );
    maps!(
        CommandDto::EndConcentration {
            entity,
            expected_rev: None,
        },
        Command::EndConcentration {
            entity,
            expected_rev: None,
        }
    );
    maps!(
        CommandDto::SpendHitDie {
            entity,
            resource: resource_id(),
            expected_rev: None,
        },
        Command::SpendHitDie {
            entity,
            resource: resource_id(),
            expected_rev: None,
        }
    );
}

#[test]
fn unknown_command_tags_are_rejected() {
    let dto: CommandDto = serde_json::from_value(json!({
        "type": "future_command",
        "anything": [1, 2, 3]
    }))
    .expect("an unknown tag has a fallback");

    assert_eq!(dto, CommandDto::Unknown);
    assert!(Command::try_from(dto).is_err());
}

#[test]
fn a_missing_required_field_is_rejected() {
    let error = serde_json::from_value::<CommandDto>(json!({
        "type": "apply_damage",
        "entity": entity_id()
    }))
    .expect_err("amount is required");

    assert!(error.to_string().contains("amount"));
}

#[test]
fn warning_and_event_golden_json() {
    let warning = WarningDto::from(Warning {
        code: WarningCode::NegativeHp,
        message: "hit points below zero".to_owned(),
        entity: Some(entity_id()),
        element: Some(ElementRef::Status(status_id())),
    });
    assert_eq!(
        serde_json::to_value(warning).expect("the warning serializes"),
        json!({
            "code": "negative_hp",
            "message": "hit points below zero",
            "entity": entity_id(),
            "element": {"type": "status", "id": status_id()}
        })
    );

    assert_eq!(
        serde_json::to_value(EventDto::from(Event::EntityChanged {
            id: entity_id(),
            rev: 2,
            kind: EntityKind::Pc,
        }))
        .expect("the event serializes"),
        json!({"type": "entity_changed", "id": entity_id(), "rev": 2, "kind": "pc"})
    );
    assert_eq!(
        serde_json::to_value(EventDto::from(Event::FeedAppended {
            ids: vec![FeedEntryId::new(7), FeedEntryId::new(8)],
        }))
        .expect("the event serializes"),
        json!({"type": "feed_appended", "ids": [7, 8]})
    );
    assert_eq!(
        serde_json::to_value(EventDto::from(Event::CampaignChanged)).expect("the event serializes"),
        json!({"type": "campaign_changed"})
    );
    assert_eq!(
        serde_json::to_value(EventDto::from(Event::EntityRemoved { id: entity_id() }))
            .expect("the event serializes"),
        json!({"type": "entity_removed", "id": entity_id()})
    );
}

#[test]
fn every_element_reference_has_a_stable_tag() {
    let cases = [
        (
            ElementRef::Status(status_id()),
            json!({"type": "status", "id": status_id()}),
        ),
        (
            ElementRef::Resource(resource_id()),
            json!({"type": "resource", "id": resource_id()}),
        ),
        (
            ElementRef::Item(entry_id()),
            json!({"type": "item", "id": entry_id()}),
        ),
        (
            ElementRef::Feature(feature_entry_id()),
            json!({"type": "feature", "id": feature_entry_id()}),
        ),
        (
            ElementRef::Spell(spell_entry_id()),
            json!({"type": "spell", "id": spell_entry_id()}),
        ),
    ];

    for (element, expected) in cases {
        assert_eq!(
            serde_json::to_value(ElementRefDto::from(element)).expect("the element serializes"),
            expected
        );
    }
}

#[test]
fn every_warning_code_has_a_stable_name() {
    let cases = [
        (WarningCode::ResourceOverspent, "resource_overspent"),
        (WarningCode::NegativeHp, "negative_hp"),
        (WarningCode::MultipleConcentration, "multiple_concentration"),
        (WarningCode::DawnRewind, "dawn_rewind"),
        (WarningCode::FormulaUnevaluated, "formula_unevaluated"),
        (WarningCode::DiceInStaticContext, "dice_in_static_context"),
        (WarningCode::UnresolvedReference, "unresolved_reference"),
        (WarningCode::OverArmored, "over_armored"),
        (WarningCode::OverAttunement, "over_attunement"),
        (WarningCode::StrengthRequirement, "strength_requirement"),
        (WarningCode::ZeroQuantity, "zero_quantity"),
        (WarningCode::UnmodeledCondition, "unmodeled_condition"),
        (WarningCode::UnmodeledClass, "unmodeled_class"),
        (
            WarningCode::ConcentrationUnsupported,
            "concentration_unsupported",
        ),
    ];

    for (code, expected) in cases {
        assert_eq!(
            serde_json::to_value(WarningCodeDto::from(code)).expect("the code serializes"),
            json!(expected)
        );
    }
}

#[test]
fn error_mapping_uses_safe_fields_and_stable_statuses() {
    let conflict = ErrorResponse::from(&AppError::Engine(EngineError::Conflict {
        entity: entity_id(),
        expected: 1,
        actual: 2,
    }));
    assert_eq!(conflict.status, HttpStatus::Conflict);
    assert_eq!(
        serde_json::to_value(&conflict.body).expect("the body serializes"),
        json!({
            "code": "conflict",
            "entity": entity_id(),
            "expected_rev": 1,
            "actual_rev": 2
        })
    );

    let already_exists = ErrorResponse::from(&AppError::Engine(EngineError::AlreadyExists {
        entity: entity_id(),
    }));
    assert_eq!(already_exists.status, HttpStatus::Conflict);
    assert_eq!(already_exists.status.as_u16(), 409);
    assert_eq!(
        serde_json::to_value(&already_exists.body).expect("the body serializes"),
        json!({"code": "already_exists", "entity": entity_id()})
    );

    let unauthorized = ErrorResponse::from(&AppError::Engine(EngineError::Unauthorized {
        actor: player_id(),
        entity: Some(entity_id()),
    }));
    assert_eq!(unauthorized.status, HttpStatus::Forbidden);
    assert_eq!(
        serde_json::to_value(&unauthorized.body).expect("the body serializes"),
        json!({"code": "unauthorized", "entity": entity_id()})
    );

    let missing = ErrorResponse::from(&AppError::Engine(EngineError::NotFound {
        entity: entity_id(),
        element: Some(ElementRef::Item(entry_id())),
    }));
    assert_eq!(missing.status, HttpStatus::NotFound);
    assert_eq!(
        serde_json::to_value(&missing.body).expect("the body serializes"),
        json!({
            "code": "not_found",
            "entity": entity_id(),
            "element": {"type": "item", "id": entry_id()}
        })
    );

    let malformed = ErrorResponse::from(&AppError::Engine(EngineError::Malformed {
        reason: "secret diagnostic".to_owned(),
    }));
    assert_eq!(malformed.status, HttpStatus::BadRequest);
    let malformed_json = serde_json::to_string(&malformed.body).expect("the body serializes");
    assert!(malformed_json.contains("\"malformed\""));
    assert!(!malformed_json.contains("secret"));

    let too_large = ErrorResponse::from(&AppError::Engine(EngineError::TooLarge {
        what: "dice count",
        limit: 20,
    }));
    assert_eq!(too_large.status, HttpStatus::BadRequest);
    assert_eq!(
        serde_json::to_value(&too_large.body).expect("the body serializes"),
        json!({"code": "too_large", "what": "dice count", "limit": 20})
    );

    let unavailable = ErrorResponse::from(&AppError::Store(StoreFault::Unavailable {
        message: "secret path".to_owned(),
    }));
    assert_eq!(unavailable.status, HttpStatus::ServiceUnavailable);
    assert_eq!(unavailable.body.code, ErrorCode::StoreUnavailable);
    let unavailable_json = serde_json::to_string(&unavailable.body).expect("the body serializes");
    assert!(!unavailable_json.contains("secret"));

    let content = ErrorResponse::from(&AppError::Content(ContentFault::Corrupt {
        message: "secret content".to_owned(),
    }));
    assert_eq!(content.status, HttpStatus::InternalServerError);
    assert_eq!(content.body.code, ErrorCode::ContentCorrupt);

    let version = ErrorResponse::from(&AppError::Store(StoreFault::VersionRefused {
        found: SchemaVersion { major: 2, minor: 0 },
        supported: SchemaVersion::CURRENT,
    }));
    assert_eq!(version.status, HttpStatus::Conflict);
    assert_eq!(version.status.as_u16(), 409);
    assert_eq!(version.body.code, ErrorCode::StoreVersionRefused);

    let format = ErrorResponse::from(&AppError::Store(StoreFault::FormatRefused {
        found: 2,
        supported: 1,
    }));
    assert_eq!(format.status, HttpStatus::Conflict);
    assert_eq!(format.body.code, ErrorCode::StoreFormatRefused);

    let task = ErrorResponse::from(&AppError::ContentTask);
    assert_eq!(task.status, HttpStatus::ServiceUnavailable);
    assert_eq!(task.body.code, ErrorCode::ContentTaskFailed);

    let gone = ErrorResponse::from(&AppError::ActorGone);
    assert_eq!(gone.status, HttpStatus::ServiceUnavailable);
    assert_eq!(gone.body.code, ErrorCode::ActorGone);

    let unsupported = ErrorResponse::unsupported_command();
    assert_eq!(unsupported.status, HttpStatus::BadRequest);
    assert_eq!(unsupported.body.code, ErrorCode::UnsupportedCommand);

    let from_unsupported = ErrorResponse::from(UnsupportedCommand);
    assert_eq!(from_unsupported, unsupported);

    let auth = ErrorResponse::new(HttpStatus::Unauthorized, ErrorCode::Unauthorized);
    assert_eq!(auth.status.as_u16(), 401);
    assert_eq!(auth.body.code, ErrorCode::Unauthorized);

    let body_cap = ErrorResponse::payload_too_large();
    assert_eq!(body_cap.status.as_u16(), 413);
    assert_eq!(body_cap.body.code, ErrorCode::PayloadTooLarge);

    let malformed_request = ErrorResponse::malformed();
    assert_eq!(malformed_request.status, HttpStatus::BadRequest);
    assert_eq!(malformed_request.body.code, ErrorCode::Malformed);
}

#[test]
fn store_and_content_faults_map_to_stable_statuses() {
    let future = SchemaVersion { major: 2, minor: 0 };
    let current = SchemaVersion::CURRENT;
    let store_cases = [
        (
            StoreFault::NoCampaign,
            HttpStatus::InternalServerError,
            ErrorCode::StoreNoCampaign,
        ),
        (
            StoreFault::StoreNotEmpty,
            HttpStatus::Conflict,
            ErrorCode::StoreNotEmpty,
        ),
        (
            StoreFault::VersionRefused {
                found: future,
                supported: current,
            },
            HttpStatus::Conflict,
            ErrorCode::StoreVersionRefused,
        ),
        (
            StoreFault::Corrupt {
                message: "secret".to_owned(),
            },
            HttpStatus::InternalServerError,
            ErrorCode::StoreCorrupt,
        ),
        (
            StoreFault::DdlVersionRefused {
                found: 2,
                supported: 1,
            },
            HttpStatus::Conflict,
            ErrorCode::StoreDdlVersionRefused,
        ),
        (
            StoreFault::DocumentMigration {
                from: current,
                to: future,
                message: "secret".to_owned(),
            },
            HttpStatus::InternalServerError,
            ErrorCode::StoreMigrationFailed,
        ),
        (
            StoreFault::DdlMigration {
                from: 1,
                to: 2,
                message: "secret".to_owned(),
            },
            HttpStatus::InternalServerError,
            ErrorCode::StoreMigrationFailed,
        ),
        (
            StoreFault::FormatRefused {
                found: 2,
                supported: 1,
            },
            HttpStatus::Conflict,
            ErrorCode::StoreFormatRefused,
        ),
        (
            StoreFault::Unavailable {
                message: "secret".to_owned(),
            },
            HttpStatus::ServiceUnavailable,
            ErrorCode::StoreUnavailable,
        ),
        (
            StoreFault::Backend {
                message: "secret".to_owned(),
            },
            HttpStatus::InternalServerError,
            ErrorCode::StoreBackend,
        ),
        (
            StoreFault::InvalidPatch {
                message: "secret".to_owned(),
            },
            HttpStatus::InternalServerError,
            ErrorCode::StoreInvalidPatch,
        ),
    ];
    for (fault, expected_status, expected_code) in store_cases {
        let response = ErrorResponse::from(&AppError::Store(fault));
        assert_eq!(response.status, expected_status);
        assert_eq!(response.body.code, expected_code);
    }

    let content_cases = [
        (
            ContentFault::Unavailable {
                message: "secret".to_owned(),
            },
            HttpStatus::ServiceUnavailable,
            ErrorCode::ContentUnavailable,
        ),
        (
            ContentFault::Corrupt {
                message: "secret".to_owned(),
            },
            HttpStatus::InternalServerError,
            ErrorCode::ContentCorrupt,
        ),
        (
            ContentFault::SnapshotVersionRefused {
                found: 2,
                supported: 1,
            },
            HttpStatus::InternalServerError,
            ErrorCode::ContentVersionRefused,
        ),
        (
            ContentFault::NamespaceConflict {
                namespace: Namespace::parse("srd2014").expect("a valid namespace"),
            },
            HttpStatus::InternalServerError,
            ErrorCode::ContentNamespaceConflict,
        ),
    ];
    for (fault, expected_status, expected_code) in content_cases {
        let response = ErrorResponse::from(&AppError::Content(fault));
        assert_eq!(response.status, expected_status);
        assert_eq!(response.body.code, expected_code);
    }
}

#[test]
fn content_golden_json_uses_the_transport_shape() {
    let summary = ContentSummaryDto::from(ContentSummary {
        id: RefId::parse("srd2014:spell/fireball").expect("a valid reference"),
        kind: ContentKind::Spell,
        name: "Fireball".to_owned(),
    });
    let summary_json = serde_json::to_value(&summary).expect("the summary serializes");
    assert_eq!(
        summary_json,
        json!({
            "id": "srd2014:spell/fireball",
            "kind": "spell",
            "name": "Fireball"
        })
    );
    assert_eq!(
        serde_json::from_value::<ContentSummaryDto>(summary_json).expect("the summary round-trips"),
        summary
    );

    let source = SourceInfoDto::from(provenance());
    assert_eq!(
        serde_json::to_value(&source).expect("the source serializes"),
        json!({
            "namespace": "srd2014",
            "source": "SRD 5.1",
            "source_revision": "2024-01-01",
            "license": "CC-BY-4.0",
            "attribution": "Example attribution"
        })
    );

    let entry = ContentEntryDto::from(ContentEntry {
        id: RefId::parse("srd2014:spell/fireball").expect("a valid reference"),
        kind: ContentKind::Spell,
        name: "Fireball".to_owned(),
        description: "A bright streak".to_owned(),
        data: ContentData::Text,
        provenance: provenance(),
        needs_review: true,
        review_notes: vec!["checked".to_owned()],
        ext: BTreeMap::new(),
    });
    let entry_json = serde_json::to_value(&entry).expect("the entry serializes");
    assert_eq!(
        entry_json,
        json!({
            "id": "srd2014:spell/fireball",
            "kind": "spell",
            "name": "Fireball",
            "description": "A bright streak",
            "data": {"type": "text"},
            "provenance": {
                "namespace": "srd2014",
                "source": "SRD 5.1",
                "source_revision": "2024-01-01",
                "license": "CC-BY-4.0",
                "attribution": "Example attribution"
            },
            "needs_review": true,
            "review_notes": ["checked"],
            "ext": {}
        })
    );
    assert_eq!(
        serde_json::from_value::<ContentEntryDto>(entry_json).expect("the entry round-trips"),
        entry
    );
}

#[test]
fn outcome_feed_and_stream_golden_json() {
    let outcome = CommandOutcome {
        executed: Executed {
            warnings: vec![Warning {
                code: WarningCode::MultipleConcentration,
                message: "already concentrating".to_owned(),
                entity: Some(entity_id()),
                element: None,
            }],
            events: vec![Event::EntityChanged {
                id: entity_id(),
                rev: 3,
                kind: EntityKind::Pc,
            }],
        },
        views: Vec::new(),
        campaign: None,
        feed: Vec::new(),
    };
    assert_eq!(
        serde_json::to_value(CommandOutcomeDto::from(outcome)).expect("the outcome serializes"),
        json!({
            "executed": {
                "warnings": [{
                    "code": "multiple_concentration",
                    "message": "already concentrating",
                    "entity": entity_id()
                }],
                "events": [{
                    "type": "entity_changed",
                    "id": entity_id(),
                    "rev": 3,
                    "kind": "pc"
                }]
            },
            "views": [],
            "feed": []
        })
    );

    let page = FeedPageDto::from(FeedPage {
        entries: Vec::new(),
        trimmed_before: Some(FeedEntryId::new(7)),
    });
    assert_eq!(
        serde_json::to_value(page).expect("the page serializes"),
        json!({"entries": [], "trimmed_before": 7})
    );

    let stream_cases = [
        (
            StreamEvent::EntityChanged(entity_id()),
            json!({"type": "entity_changed", "id": entity_id()}),
        ),
        (
            StreamEvent::Forgotten(entity_id()),
            json!({"type": "forgotten", "id": entity_id()}),
        ),
        (
            StreamEvent::CampaignChanged,
            json!({"type": "campaign_changed"}),
        ),
        (
            StreamEvent::FeedAppended(FeedEntryId::new(9)),
            json!({"type": "feed_appended", "id": 9}),
        ),
        (StreamEvent::Lagged, json!({"type": "lagged"})),
    ];
    for (event, expected) in stream_cases {
        assert_eq!(
            serde_json::to_value(StreamEventDto::from(event)).expect("the stream event serializes"),
            expected
        );
    }
}

#[test]
fn view_golden_json_is_per_viewer() {
    let entity = EntityView {
        id: entity_id(),
        kind: EntityKind::Note,
        owner: player_id(),
        parent: None,
        rev: 1,
        writable: true,
        hidden: BTreeSet::new(),
        name: Some("Session note".to_owned()),
        ext: None,
        body: None,
    };
    assert_eq!(
        serde_json::to_value(entity).expect("the entity view serializes"),
        json!({
            "id": entity_id(),
            "kind": "note",
            "owner": player_id(),
            "parent": null,
            "rev": 1,
            "writable": true,
            "hidden": [],
            "name": "Session note"
        })
    );

    let entry = FeedEntryView {
        id: FeedEntryId::new(7),
        ts: UnixTime::from_seconds(100),
        clock_s: CampaignTime::from_seconds(200),
        actor: Some(player_id()),
        entity: None,
        payload: FeedPayload::Hint {
            text: "hint".to_owned(),
        },
    };
    assert_eq!(
        serde_json::to_value(entry).expect("the feed view serializes"),
        json!({
            "id": 7,
            "ts": 100,
            "clock_s": 200,
            "actor": player_id(),
            "payload": {"type": "hint", "text": "hint"}
        })
    );

    let campaign = CampaignView {
        id: campaign_id(),
        name: "The Vault".to_owned(),
        default_ruleset: RulesetId::parse("5e-2014").expect("a valid ruleset"),
        clock: CampaignTime::from_seconds(300),
        settings: CampaignSettingsView {
            player_roll_audience_default: Audience::RollerAndDm,
            feed_capacity: 1000,
            dawn_hour: 6,
        },
        roster: vec![RosterEntryView {
            id: player_id(),
            display_name: "Dungeon Master".to_owned(),
            role: Role::Dm,
        }],
    };
    assert_eq!(
        serde_json::to_value(campaign).expect("the campaign view serializes"),
        json!({
            "id": campaign_id(),
            "name": "The Vault",
            "default_ruleset": "5e-2014",
            "clock": 300,
            "settings": {
                "player_roll_audience_default": "roller_and_dm",
                "feed_capacity": 1000,
                "dawn_hour": 6
            },
            "roster": [{
                "id": player_id(),
                "display_name": "Dungeon Master",
                "role": "dm"
            }]
        })
    );
}
