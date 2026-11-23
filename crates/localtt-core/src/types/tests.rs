use std::collections::HashSet;

use serde_json::json;
use uuid::Uuid;

use super::{stats::custom_stat_key, *};

fn uuid(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

#[test]
fn campaign_looks_up_roster_seats_by_id() {
    let mut builder = crate::builders::CampaignBuilder::new("Campaign");
    let dm = builder.dm("DM");
    let player = builder.player("Aria");
    let campaign = builder.build();

    assert_eq!(campaign.player(player).expect("on the roster").id, player);
    assert_eq!(campaign.player(dm).expect("on the roster").role, Role::Dm);
    assert!(campaign.player(PlayerId::new()).is_none());
}

#[test]
fn id_wraps_a_uuid_and_serializes_transparently() {
    let raw = uuid(0x018f_2b3c_4d5e_7f80_9abc_def0_1234_5678);
    let id: EntityId = Id::from_uuid(raw);

    assert_eq!(id.as_uuid(), raw);
    assert_eq!(id, Id::from_uuid(raw));
    assert_ne!(id, Id::from_uuid(uuid(1)));
    assert_eq!(id.to_string(), raw.to_string());
    assert_eq!(
        serde_json::to_value(id).expect("id serializes"),
        json!(raw.to_string())
    );
    let back: EntityId = serde_json::from_value(json!(raw.to_string())).expect("id deserializes");
    assert_eq!(back, id);
}

#[test]
fn id_is_a_v7_uuid_and_orders_and_hashes_by_it() {
    let fresh = EntityId::new();
    assert_eq!(fresh.as_uuid().get_version_num(), 7);

    let small: EntityId = Id::from_uuid(uuid(1));
    let big: EntityId = Id::from_uuid(uuid(2));
    assert!(small < big);
    assert_eq!(
        HashSet::from([small, big]).len(),
        2,
        "distinct ids hash apart"
    );
}

#[test]
fn feed_entry_id_round_trips() {
    let id = FeedEntryId::new(41);
    assert_eq!(id.get(), 41);
    assert_eq!(serde_json::to_value(id).expect("serializes"), json!(41));
    assert_eq!(
        serde_json::from_value::<FeedEntryId>(json!(41)).expect("deserializes"),
        id
    );
}

#[test]
fn time_newtypes_round_trip_as_seconds() {
    let campaign = CampaignTime::from_seconds(3600);
    assert_eq!(campaign.seconds(), 3600);
    assert_eq!(campaign.to_string(), "3600");
    assert_eq!(
        serde_json::to_value(campaign).expect("serializes"),
        json!(3600)
    );

    let wall = UnixTime::from_seconds(1_750_000_000);
    assert_eq!(wall.seconds(), 1_750_000_000);
    assert_eq!(wall.to_string(), "1750000000");
    assert_eq!(
        serde_json::to_value(wall).expect("serializes"),
        json!(1_750_000_000)
    );
}

#[test]
fn campaign_time_derives_days_weeks_and_time_of_day() {
    let start = CampaignTime::from_seconds(0);
    assert_eq!(start.days_since_start(), 0);
    assert_eq!(start.weeks_since_start(), 0);
    assert_eq!(start.day_of_week(), 0);
    assert_eq!(start.time_of_day().to_string(), "00:00:00");

    let mid = CampaignTime::from_seconds(3 * SECONDS_PER_DAY + 10 * 3_600 + 61);
    assert_eq!(mid.days_since_start(), 3);
    assert_eq!(mid.day_of_week(), 3);
    assert_eq!(mid.time_of_day().to_string(), "10:01:01");
    assert_eq!(
        mid.time_of_day(),
        TimeOfDay {
            hour: 10,
            minute: 1,
            second: 1
        }
    );

    let week = CampaignTime::from_seconds(DAYS_PER_WEEK * SECONDS_PER_DAY);
    assert_eq!(week.days_since_start(), 10);
    assert_eq!(week.weeks_since_start(), 1);
    assert_eq!(week.day_of_week(), 0);

    let just_before = CampaignTime::from_seconds(-1);
    assert_eq!(just_before.days_since_start(), -1);
    assert_eq!(just_before.time_of_day().to_string(), "23:59:59");
}

#[test]
fn schema_version_accepts_same_or_older_major() {
    let current = SchemaVersion::CURRENT;
    assert!(current.accepts(SchemaVersion {
        major: current.major,
        minor: u16::MAX
    }));
    assert!(current.accepts(SchemaVersion { major: 0, minor: 0 }));
    assert!(!current.accepts(SchemaVersion {
        major: current.major + 1,
        minor: 0
    }));
}

#[test]
fn ref_id_parses_rejects_and_round_trips() {
    let r = RefId::parse("srd2014:spell/fireball").expect("valid ref");
    assert_eq!(r.namespace(), "srd2014");
    assert_eq!(r.kind(), "spell");
    assert_eq!(r.slug(), "fireball");
    assert_eq!(r.as_str(), "srd2014:spell/fireball");
    assert_eq!(r.to_string(), "srd2014:spell/fireball");
    assert_eq!(
        serde_json::to_value(&r).expect("serializes"),
        json!("srd2014:spell/fireball")
    );

    let pack = RefId::parse("pack.mytome:item/foo-bar").expect("valid pack ref");
    assert_eq!(pack.namespace(), "pack.mytome");
    assert_eq!(pack.slug(), "foo-bar");
    assert_eq!(
        serde_json::from_value::<RefId>(json!("pack.mytome:item/foo-bar")).expect("deserializes"),
        pack
    );

    for bad in [
        "",
        "not-a-ref",
        "srd2014/spell/fireball",       // no colon
        "srd2014:spell",                // no slash
        "srd2014:spell/Fire Ball",      // uppercase and space
        "srd2014:spell/fireball/extra", // extra segment
        ":spell/fireball",
        "srd2014:/fireball",
        "srd2014:spell/",
        "pack..mytome:item/foo",
    ] {
        assert!(RefId::parse(bad).is_err(), "{bad:?} must be rejected");
        assert!(
            serde_json::from_value::<RefId>(json!(bad)).is_err(),
            "{bad:?} must be rejected on deserialize too"
        );
    }
}

#[test]
fn stat_key_round_trips_byte_identically() {
    for raw in [
        "ac",
        "initiative",
        "speed",
        "proficiency_bonus",
        "ability.str",
        "ability_mod.dex",
        "save.wis",
        "skill.sleight_of_hand",
        "passive.perception",
        "attack.melee",
        "spell.dc",
        "check",
        "save",
        "attack",
        "house.luck",
    ] {
        let key = StatKey::parse(raw).unwrap_or_else(|_| panic!("{raw:?} parses"));
        assert_eq!(key.to_string(), raw);
        assert_eq!(serde_json::to_value(&key).expect("serializes"), json!(raw));
        assert_eq!(
            serde_json::from_value::<StatKey>(json!(raw)).expect("deserializes"),
            key
        );
    }

    assert_eq!(
        StatKey::parse("skill.stealth").expect("known"),
        StatKey::Known(KnownStat::Skill(Skill::Stealth))
    );
    assert!(matches!(
        StatKey::parse("house.luck").expect("custom"),
        StatKey::Custom(_)
    ));
}

#[test]
fn stat_key_rejects_malformed_custom_keys_at_the_length_boundary() {
    for bad in ["", "Bad Key!", "House.Luck", "house..luck", "lucky!"] {
        assert!(StatKey::parse(bad).is_err(), "{bad:?} must be rejected");
    }

    let mut longest = "a.".repeat(31);
    longest.push_str("aa");
    assert_eq!(longest.len(), 64);
    assert!(StatKey::parse(&longest).is_ok());
    assert!(StatKey::parse(&format!("{longest}a")).is_err());
}

#[test]
fn challenge_rating_round_trips_through_its_string_form() {
    let mut ratings = vec![0, 1, 2, 4];
    ratings.extend((1..=30).map(|whole| whole * 8));
    for eighths in ratings {
        let cr = ChallengeRating::from_eighths(eighths)
            .unwrap_or_else(|_| panic!("{eighths} eighths must be valid"));
        assert_eq!(cr.as_eighths(), eighths);
        assert_eq!(
            ChallengeRating::parse(&cr.to_string()).expect("display form re-parses"),
            cr,
            "{eighths} eighths must round-trip"
        );
    }
}

#[test]
fn challenge_rating_rejects_non_5e_values() {
    for bad in [3, 5, 6, 7, -8, 248] {
        assert!(
            ChallengeRating::from_eighths(bad).is_err(),
            "{bad} eighths is not a 5e rating"
        );
    }
    for raw in ["", "1/3", "1/16", "-1", "31", "0/8", "two", "+5", "05"] {
        assert!(
            ChallengeRating::parse(raw).is_err(),
            "{raw:?} is not a 5e rating"
        );
    }
}

#[test]
fn token_hash_parses_lowercase_hex_and_round_trips() {
    let hash = TokenHash::parse("0a1b2c3d").expect("lowercase hex is valid");
    assert_eq!(hash.as_str(), "0a1b2c3d");
    assert!(
        !hash.is_sha256(),
        "an eight-character digest is not SHA-256"
    );
    assert_eq!(
        serde_json::to_value(&hash).expect("serializes"),
        json!("0a1b2c3d")
    );
    assert_eq!(
        serde_json::from_value::<TokenHash>(json!("0a1b2c3d")).expect("deserializes"),
        hash
    );

    for bad in ["", "0A1B", "xyz", "0a1b2c3dg", " 0a1b"] {
        assert!(TokenHash::parse(bad).is_err(), "{bad:?} must be rejected");
    }
    assert!(
        serde_json::from_value::<TokenHash>(json!("0A1B")).is_err(),
        "deserialization validates the digest"
    );
}

#[test]
fn token_hash_recognises_a_sha256_digest() {
    let digest = "a".repeat(TokenHash::SHA256_HEX_LEN);
    let hash = TokenHash::parse(&digest).expect("64 lowercase hex characters are valid");
    assert!(hash.is_sha256());
    assert_eq!(hash.as_str(), digest);
}

#[test]
fn string_newtype_accessors_expose_their_value() {
    assert_eq!(
        RulesetId::parse("5e-2014").expect("valid ruleset").as_str(),
        "5e-2014"
    );
    assert_eq!(
        Namespace::parse("com.example.foo")
            .expect("valid namespace")
            .as_str(),
        "com.example.foo"
    );
    assert_eq!(
        custom_stat_key("house.luck")
            .expect("valid custom stat")
            .as_str(),
        "house.luck"
    );
}

fn header_acl() -> Vec<AclEntry> {
    vec![AclEntry {
        player: PlayerId::new(),
        level: AclLevel::Read,
        field_mask: None,
    }]
}

fn assert_header_copies_every_field<B: Body>(
    entity: &Entity<B>,
    erased: &AnyEntity,
    expected_kind: EntityKind,
    parent: EntityId,
    acl: &[AclEntry],
) {
    // Act
    let header = entity.header();

    // Assert
    assert_eq!(header.id, entity.id);
    assert_eq!(header.kind, expected_kind);
    assert_eq!(header.name, "Header probe");
    assert_eq!(header.owner, entity.owner);
    assert_eq!(header.parent, Some(parent));
    assert_eq!(header.acl, acl);
    assert_eq!(header.rev, 7);
    assert_eq!(header.created_at, UnixTime::from_seconds(100));
    assert_eq!(header.updated_at, UnixTime::from_seconds(200));
    assert_eq!(erased.header(), header);
}

#[test]
fn headers_copy_every_field_for_each_body_family() {
    // Arrange
    let owner = PlayerId::new();
    let parent = EntityId::new();
    let acl = header_acl();

    let mut sheet = crate::builders::StatblockBuilder::character();
    sheet.abilities(Abilities::all(10)).hp(10, 10);
    let statblock = sheet
        .build()
        .map(|body| Entity::builder(EntityKind::Pc, EntityId::new(), "Header probe", owner, body))
        .expect("a complete statblock builds");
    let note = Entity::builder(
        EntityKind::Note,
        EntityId::new(),
        "Header probe",
        owner,
        NoteBody {
            text: String::new(),
        },
    );
    let handout = Entity::builder(
        EntityKind::Handout,
        EntityId::new(),
        "Header probe",
        owner,
        HandoutBody {
            content: HandoutContent::Text {
                text: String::new(),
            },
        },
    );

    // Act and Assert
    macro_rules! finish {
        ($builder:expr) => {
            $builder
                .parent(parent)
                .acl(acl.clone())
                .rev(7)
                .created_at(UnixTime::from_seconds(100))
                .updated_at(UnixTime::from_seconds(200))
                .build()
                .expect("the kind matches the body")
        };
    }
    let statblock = finish!(statblock);
    let erased = AnyEntity::from(statblock.clone());
    assert_header_copies_every_field(&statblock, &erased, EntityKind::Pc, parent, &acl);
    let note = finish!(note);
    let erased = AnyEntity::from(note.clone());
    assert_header_copies_every_field(&note, &erased, EntityKind::Note, parent, &acl);
    let handout = finish!(handout);
    let erased = AnyEntity::from(handout.clone());
    assert_header_copies_every_field(&handout, &erased, EntityKind::Handout, parent, &acl);
}
