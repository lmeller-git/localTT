//! End-to-end session tests: golden fixtures through a real store and a real
//! `Executor` with the shipped RNG and content adapters, then out through the
//! read services, across a close/reopen.
//!
//! These are exploratory tests for the toy server phase: they join the pieces
//! the unit and target suites pin in isolation (fixtures, store, executor,
//! commands, dice, authz, services) the way a client session would. They do not
//! edit any frozen target test or engine semantics; friction and bugs found
//! here are reported, not worked around.

use std::collections::{BTreeMap, BTreeSet};

use localtt_adapter::{
    reference::FixtureSource,
    rng::ChaChaRng,
    store::{memory::MemoryStore, sqlite::SqliteStore},
};
use localtt_core::{
    builders::stat,
    types::{
        AclEntry,
        AclLevel,
        AnyEntity,
        Audience,
        Campaign,
        CampaignTime,
        DiceResult,
        EntityId,
        EntityKind,
        FeedEntryId,
        FeedPayload,
        FieldMask,
        Player,
        PlayerId,
        Resource,
        ResourceId,
        Role,
        RollContext,
        RollKind,
        StatKey,
        StatblockEntity,
        StatusDuration,
        StatusId,
        StatusKind,
        UnixTime,
        ViewField,
    },
};
use localtt_engine::{
    authz::{EntityView, EntityViewBody, FeedEntryView},
    commands::{Command, CommandEnv, Event, RollSpec, StatusSpec},
    content::ReferenceSource,
    dice::{self, RollPlan},
    error::{ElementRef, EngineError},
    executor::Executor,
    ports::store::{CampaignSnapshot, CampaignStore, StatePatch},
    services,
    state::{LoadedCampaign, WorkingSet},
};
use localtt_integration::{fixtures, roundtrip::to_json};
use serde::Serialize;
use tempfile::TempDir;

/// The fixed wall-clock stamp the fixture cast uses.
const FIXED_TIME: i64 = 1_750_000_000;

// ---------------------------------------------------------------------------
// Session environment and shared helpers
// ---------------------------------------------------------------------------

/// The session seam over the shipped adapters: a frozen wall clock, the
/// fixture content catalog and a seeded ChaCha RNG. This is what a composition
/// root has to assemble by hand today.
struct SessionEnv {
    rng: ChaChaRng,
    reference: FixtureSource,
}

impl SessionEnv {
    /// A deterministic session environment.
    fn new(seed: [u8; 32]) -> Self {
        Self {
            rng: ChaChaRng::from_seed(seed),
            reference: FixtureSource::new(),
        }
    }
}

impl CommandEnv for SessionEnv {
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

/// The entities' whole initial durable state as one creation patch.
fn seeding_patch<'a>(campaign: &'a Campaign, entities: &'a [AnyEntity]) -> StatePatch<'a> {
    StatePatch {
        campaign: Some(campaign),
        upserts: entities.iter().collect(),
        removals: Vec::new(),
        feed_appends: Vec::new(),
        feed_keep_from: None,
    }
}

/// Appends one ordinary member to the roster; there is no roster command yet.
fn add_member(campaign: &mut Campaign, display_name: &str) -> PlayerId {
    let id = PlayerId::new();
    campaign.players.push(Player {
        id,
        display_name: display_name.to_owned(),
        role: Role::Player,
        token_hash: None,
        created_at: UnixTime::from_seconds(FIXED_TIME),
        last_seen: None,
    });
    id
}

/// A read share limited to the given fields.
fn masked_read(player: PlayerId, fields: &[ViewField]) -> AclEntry {
    let mut mask = FieldMask::default();
    for &field in fields {
        mask.insert(field);
    }
    AclEntry {
        player,
        level: AclLevel::Read,
        field_mask: Some(mask),
    }
}

/// A read share over every section.
fn full_read(player: PlayerId) -> AclEntry {
    AclEntry {
        player,
        level: AclLevel::Read,
        field_mask: None,
    }
}

/// Serializes a core document; comparisons are byte-for-byte.
fn bytes<T: Serialize>(value: &T) -> String {
    to_json(value).expect("core documents serialize")
}

/// The durable shape of a working set: campaign bytes, entities keyed by id
/// (comparison order-independent) and feed entries in ascending id order.
type Documents = (
    String,
    BTreeMap<EntityId, String>,
    Vec<(FeedEntryId, String)>,
);

fn state_documents(state: &dyn WorkingSet) -> Documents {
    let entities = state
        .headers()
        .into_iter()
        .map(|header| {
            let entity = state
                .entity(header.id)
                .expect("the working set resolves its own headers")
                .expect("a header implies a body");
            (header.id, bytes(&entity))
        })
        .collect();
    let feed = state
        .feed()
        .into_iter()
        .map(|entry| (entry.id, bytes(&entry)))
        .collect();
    (bytes(state.campaign()), entities, feed)
}

fn snapshot_documents(snapshot: &CampaignSnapshot) -> Documents {
    (
        bytes(&snapshot.campaign),
        snapshot
            .entities
            .iter()
            .map(|entity| (entity.id(), bytes(entity)))
            .collect(),
        snapshot
            .feed
            .iter()
            .map(|entry| (entry.id, bytes(entry)))
            .collect(),
    )
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

/// Every `ViewField`, so the masked-view assertions cannot drift.
fn all_view_fields() -> BTreeSet<ViewField> {
    [
        ViewField::Name,
        ViewField::Identity,
        ViewField::Abilities,
        ViewField::Saves,
        ViewField::Skills,
        ViewField::Proficiencies,
        ViewField::Hp,
        ViewField::Ac,
        ViewField::Speed,
        ViewField::DeathSaves,
        ViewField::Coins,
        ViewField::Overrides,
        ViewField::Resources,
        ViewField::Spells,
        ViewField::Inventory,
        ViewField::Attacks,
        ViewField::Features,
        ViewField::Statuses,
        ViewField::Notes,
        ViewField::Ext,
    ]
    .into_iter()
    .collect()
}

/// Every field not listed: the hidden set a mask over `fields` implies.
fn all_but(fields: &[ViewField]) -> BTreeSet<ViewField> {
    let visible: BTreeSet<ViewField> = fields.iter().copied().collect();
    all_view_fields().difference(&visible).copied().collect()
}

/// The ids of a listed entity set.
fn entity_ids(views: &[EntityView]) -> Vec<EntityId> {
    views.iter().map(|view| view.id).collect()
}

/// The sorted ids of an entity slice.
fn sorted_ids(entities: &[AnyEntity]) -> Vec<EntityId> {
    let mut ids = entities.iter().map(AnyEntity::id).collect::<Vec<_>>();
    ids.sort();
    ids
}

/// The ids of a feed page, ascending.
fn feed_ids(views: &[FeedEntryView]) -> Vec<u64> {
    views.iter().map(|view| view.id.get()).collect()
}

/// A literal stat key, for the few commands that address one.
fn key(raw: &str) -> StatKey {
    stat(raw).expect("a literal stat key")
}

// ---------------------------------------------------------------------------
// 1. A whole session over SQLite, closed and reopened
// ---------------------------------------------------------------------------

#[test]
fn a_session_over_sqlite_survives_close_and_reopen() {
    // Arrange: the golden party plus Lyra in a file-backed store, a third
    // member with no grants, and the shipped adapters behind the executor.
    let dir = TempDir::new().expect("a temp dir for the store");
    let path = dir.path().join("campaign.sqlite");
    let party = fixtures::party();
    let mut campaign = party.campaign.clone();
    let stranger = add_member(&mut campaign, "Stranger");
    let hero = party.pc.clone();
    let lyra = fixtures::golden_warlock(party.player);
    let pact = lyra.body.resources[0].id;
    let mut entities = party.entities();
    entities.push(AnyEntity::from(lyra.clone()));
    let mut store = SqliteStore::open(&path).expect("a new file opens");
    store
        .apply(&seeding_patch(&campaign, &entities))
        .expect("a fresh store accepts the party");
    let state = LoadedCampaign::from_snapshot(store.load().expect("the party was just committed"));
    let mut executor = Executor::new(
        Box::new(state),
        &mut store,
        Box::new(SessionEnv::new([1; 32])),
    );

    // Act 1: the DM crosses Hex's expiry instant.
    let executed = executor
        .execute_as(party.dm, &Command::AdvanceClock { by_seconds: 3_600 })
        .expect("the DM moves the clock");
    assert_eq!(executed.warnings, Vec::new());
    assert_eq!(
        executed.events,
        vec![
            Event::CampaignChanged,
            Event::FeedAppended {
                ids: vec![FeedEntryId::new(1)],
            },
        ]
    );

    // Act 2: the owner rolls Aria's Wisdom save; the shipped ChaCha RNG
    // supplies the d20, so only the expression's shape is asserted.
    let executed = executor
        .execute_as(
            party.player,
            &Command::Roll {
                entity: Some(hero.id),
                spec: RollSpec::Check {
                    stat: key("save.wis"),
                    target_dc: Some(15),
                },
                audience: None,
            },
        )
        .expect("rolling a check is bookkeeping");
    assert_eq!(
        executed.events,
        vec![Event::FeedAppended {
            ids: vec![FeedEntryId::new(2)],
        }]
    );

    // Act 3: a public free roll, so the feed has an entry every member sees.
    let expression = dice::parse("1d6+1").expect("a literal dice expression");
    executor
        .execute_as(
            party.player,
            &Command::Roll {
                entity: None,
                spec: RollSpec::Expression {
                    expr: expression,
                    target: None,
                },
                audience: Some(Audience::Public),
            },
        )
        .expect("a free roll only needs membership");

    // Act 4: damage, spend, then rest, the ordinary handling loop.
    executor
        .execute_as(
            party.player,
            &Command::ApplyDamage {
                entity: hero.id,
                amount: 9,
                expected_rev: None,
            },
        )
        .expect("the owner may damage their character");
    executor
        .execute_as(
            party.player,
            &Command::SpendResource {
                entity: lyra.id,
                resource: pact,
                amount: 1,
                expected_rev: None,
            },
        )
        .expect("the owner may spend their pact slot");
    let executed = executor
        .execute_as(
            party.player,
            &Command::ShortRest {
                entity: lyra.id,
                advance_seconds: Some(600),
                expected_rev: None,
            },
        )
        .expect("a short rest is bookkeeping");
    assert_eq!(executed.warnings, Vec::new());
    assert_eq!(
        executed.events,
        vec![
            Event::CampaignChanged,
            Event::EntityChanged {
                id: lyra.id,
                rev: 2,
                kind: EntityKind::Pc,
            },
        ]
    );

    // Act 5: a member with no grants is refused, and nothing moves.
    let before_denial = state_documents(executor.state());
    let denied = executor.execute_as(
        stranger,
        &Command::ApplyDamage {
            entity: hero.id,
            amount: 5,
            expected_rev: None,
        },
    );
    assert!(matches!(
        denied,
        Err(EngineError::Unauthorized { actor, entity: Some(target) })
            if actor == stranger && target == hero.id
    ));
    assert_eq!(state_documents(executor.state()), before_denial);

    // Assert: the session's durable and visible effects.
    let working = executor.state();
    assert_eq!(
        working.campaign().clock.now_s,
        CampaignTime::from_seconds(4_200)
    );
    assert_eq!(statblock_of(working, hero.id).body.hp.current, 23);
    assert_eq!(statblock_of(working, hero.id).rev, 1);
    assert_eq!(resource_of(working, lyra.id, pact).current, 2);
    assert_eq!(statblock_of(working, lyra.id).rev, 2);
    assert!(
        statblock_of(working, lyra.id)
            .body
            .statuses
            .iter()
            .any(|status| status.name == "Hex"),
        "a soft expiry keeps the status on the sheet"
    );

    let feed = working.feed();
    assert_eq!(feed.len(), 3);
    assert_eq!(feed[0].audience, Audience::RollerAndDm);
    assert_eq!(
        feed[0].payload,
        FeedPayload::StatusExpired {
            status: lyra.body.statuses[0].id,
            name: "Hex".to_owned(),
        }
    );
    let FeedPayload::Roll { roll } = &feed[1].payload else {
        panic!("the second entry is a roll, got {:?}", feed[1].payload);
    };
    assert_eq!(roll.roller, party.player);
    assert_eq!(roll.entity, Some(hero.id));
    assert_eq!(
        roll.context,
        Some(RollContext {
            kind: RollKind::Save,
            stat: Some(key("save.wis")),
            target_dc: Some(15),
        })
    );
    let die = roll.result.terms[0].dice[0].value;
    assert!((1..=20).contains(&die), "a d20 face, got {die}");
    assert_eq!(roll.result.terms[1].total, 4, "Aria's Wisdom save modifier");
    assert_eq!(roll.result.total, die + 4);
    assert_eq!(feed[2].audience, Audience::Public);

    // Assert: the read services agree with the working set, per actor.
    let dm_views = services::list_entities(working, party.dm);
    assert_eq!(entity_ids(&dm_views), sorted_ids(&entities));
    let owner_views = services::list_entities(working, party.player);
    let mut expected_owner = vec![hero.id, party.companion.id, party.familiar.id, lyra.id];
    expected_owner.sort();
    assert_eq!(entity_ids(&owner_views), expected_owner);
    assert!(services::list_entities(working, stranger).is_empty());

    let dm_feed = services::feed_after(working, party.dm, None, 10);
    assert_eq!(feed_ids(&dm_feed), vec![1, 2, 3]);
    // The stranger is a member: the public roll is visible, the rest is not.
    assert_eq!(
        feed_ids(&services::feed_after(working, stranger, None, 10)),
        vec![3]
    );
    // Cursor paging is exclusive and applies to the visible page.
    assert_eq!(
        feed_ids(&services::feed_after(working, party.player, None, 1)),
        vec![1]
    );
    assert_eq!(
        feed_ids(&services::feed_after(
            working,
            party.player,
            Some(FeedEntryId::new(1)),
            10,
        )),
        vec![2, 3]
    );

    // Persist and publish: the store mirrors the working set byte for byte.
    let before = state_documents(working);
    drop(executor);
    let loaded = store.load().expect("the session persisted");
    assert_eq!(snapshot_documents(&loaded), before);
    drop(store);

    // Restart: the same session, from the file, through the same services.
    let reopened = SqliteStore::open(&path).expect("reopening the campaign");
    let loaded = reopened.load().expect("the session persisted");
    assert_eq!(snapshot_documents(&loaded), before);
    let state = LoadedCampaign::from_snapshot(loaded);
    assert_eq!(services::list_entities(&state, party.dm), dm_views);
    assert_eq!(services::list_entities(&state, party.player), owner_views);
    assert!(services::list_entities(&state, stranger).is_empty());
    assert_eq!(services::feed_after(&state, party.dm, None, 10), dm_feed);
    assert_eq!(
        feed_ids(&services::feed_after(&state, stranger, None, 10)),
        vec![3]
    );
}

// ---------------------------------------------------------------------------
// 2. Multi-actor authorization over a persisted party
// ---------------------------------------------------------------------------

#[test]
fn authorization_holds_across_a_persisted_party() {
    // Arrange: DM, owner, a masked reader, a grantless member and a
    // non-member over the persisted golden party.
    let party = fixtures::party();
    let mut campaign = party.campaign.clone();
    let reader = add_member(&mut campaign, "Reader");
    let stranger = add_member(&mut campaign, "Stranger");
    let outsider = PlayerId::new();

    let mut pc = party.pc.clone();
    pc.acl = vec![masked_read(reader, &[ViewField::Name, ViewField::Hp])];
    let mut note = party.note.clone();
    note.acl = vec![full_read(reader)];
    let entities = vec![
        AnyEntity::from(pc),
        AnyEntity::from(party.companion.clone()),
        AnyEntity::from(party.familiar.clone()),
        AnyEntity::from(note),
        AnyEntity::from(party.handout.clone()),
    ];

    let mut store = MemoryStore::new();
    store
        .apply(&seeding_patch(&campaign, &entities))
        .expect("a fresh store accepts the party");
    let state = LoadedCampaign::from_snapshot(store.load().expect("the party was just committed"));
    let mut executor = Executor::new(
        Box::new(state),
        &mut store,
        Box::new(SessionEnv::new([2; 32])),
    );

    // Act 1: the owner damages the PC; the masked reader sees the new value.
    executor
        .execute_as(
            party.player,
            &Command::ApplyDamage {
                entity: party.pc.id,
                amount: 5,
                expected_rev: None,
            },
        )
        .expect("the owner may damage");

    // Act 2: three non-writers are refused, and nothing moves.
    let before_denials = state_documents(executor.state());
    let damage = Command::ApplyDamage {
        entity: party.pc.id,
        amount: 5,
        expected_rev: None,
    };
    for actor in [reader, stranger, outsider] {
        let result = executor.execute_as(actor, &damage);
        assert!(
            matches!(
                result,
                Err(EngineError::Unauthorized { actor: denied, entity: Some(target) })
                    if denied == actor && target == party.pc.id
            ),
            "{actor} must not damage the pc, got {result:?}"
        );
    }
    assert_eq!(state_documents(executor.state()), before_denials);

    // Act 3: the DM is not bound; a damaged sheet is what the reader sees.
    executor
        .execute_as(party.dm, &damage)
        .expect("the DM may damage anything");

    // Act 4: a masked reader can read, so the pre-check lets a private roll
    // through; a public free roll by the owner follows.
    executor
        .execute_as(
            reader,
            &Command::Roll {
                entity: Some(party.pc.id),
                spec: RollSpec::Check {
                    stat: key("save.wis"),
                    target_dc: None,
                },
                audience: Some(Audience::RollerOnly),
            },
        )
        .expect("the mask authorizes the roll");
    let expression = dice::parse("1d4").expect("a literal dice expression");
    executor
        .execute_as(
            party.player,
            &Command::Roll {
                entity: None,
                spec: RollSpec::Expression {
                    expr: expression,
                    target: None,
                },
                audience: Some(Audience::Public),
            },
        )
        .expect("a free roll only needs membership");

    // Assert: list_entities is exactly the readable set per actor.
    let working = executor.state();
    assert_eq!(
        entity_ids(&services::list_entities(working, party.dm)),
        sorted_ids(&entities)
    );
    let mut expected_owner = vec![party.pc.id, party.companion.id, party.familiar.id];
    expected_owner.sort();
    assert_eq!(
        entity_ids(&services::list_entities(working, party.player)),
        expected_owner
    );
    let mut expected_reader = vec![party.pc.id, party.note.id];
    expected_reader.sort();
    assert_eq!(
        entity_ids(&services::list_entities(working, reader)),
        expected_reader
    );
    assert!(services::list_entities(working, stranger).is_empty());
    assert!(services::list_entities(working, outsider).is_empty());

    // Assert: the masked view carries exactly Name and the damaged Hp.
    let view = services::get_entity(working, party.pc.id, reader).expect("the masked share reads");
    assert_eq!(view.name.as_deref(), Some("Aria"));
    assert_eq!(view.hidden, all_but(&[ViewField::Name, ViewField::Hp]));
    assert!(!view.writable);
    let Some(EntityViewBody::Statblock(body)) = &view.body else {
        panic!("a statblock view, got {:?}", view.body);
    };
    assert_eq!(
        body.hp.expect("Hp is shared").current,
        22,
        "both damages landed"
    );
    assert!(body.abilities.is_none());

    // Assert: the feed audience matrix over command-generated entries.
    assert_eq!(
        feed_ids(&services::feed_after(working, party.dm, None, 10)),
        vec![2],
        "the DM never sees the reader's RollerOnly entry"
    );
    assert_eq!(
        feed_ids(&services::feed_after(working, reader, None, 10)),
        vec![1, 2]
    );
    assert_eq!(
        feed_ids(&services::feed_after(working, party.player, None, 10)),
        vec![2]
    );
    assert_eq!(
        feed_ids(&services::feed_after(working, stranger, None, 10)),
        vec![2]
    );
    assert!(
        services::feed_after(working, outsider, None, 10).is_empty(),
        "a non-member sees no feed, however public the entry"
    );
    assert_eq!(
        feed_ids(&services::feed_after(working, reader, None, 1)),
        vec![1]
    );
    assert_eq!(
        feed_ids(&services::feed_after(
            working,
            reader,
            Some(FeedEntryId::new(1)),
            10,
        )),
        vec![2]
    );
}

// ---------------------------------------------------------------------------
// 3. Refused commands leave the SQLite file byte-identical
// ---------------------------------------------------------------------------

#[test]
fn failed_commands_leave_the_sqlite_file_unchanged() {
    // Arrange: a seeded file with one committed damage and one feed entry, so
    // both the entity and the feed carry state a failed command could spoil.
    let dir = TempDir::new().expect("a temp dir for the store");
    let path = dir.path().join("campaign.sqlite");
    let party = fixtures::party();
    let mut campaign = party.campaign.clone();
    let stranger = add_member(&mut campaign, "Stranger");
    let outsider = PlayerId::new();
    let entities = party.entities();
    let mut store = SqliteStore::open(&path).expect("a new file opens");
    store
        .apply(&seeding_patch(&campaign, &entities))
        .expect("a fresh store accepts the party");
    let state = LoadedCampaign::from_snapshot(store.load().expect("the party was just committed"));
    let mut executor = Executor::new(
        Box::new(state),
        &mut store,
        Box::new(SessionEnv::new([3; 32])),
    );
    executor
        .execute_as(
            party.player,
            &Command::ApplyDamage {
                entity: party.pc.id,
                amount: 5,
                expected_rev: None,
            },
        )
        .expect("the owner may damage");
    let expression = dice::parse("1d4").expect("a literal dice expression");
    executor
        .execute_as(
            party.player,
            &Command::Roll {
                entity: None,
                spec: RollSpec::Expression {
                    expr: expression,
                    target: None,
                },
                audience: Some(Audience::Public),
            },
        )
        .expect("a free roll only needs membership");
    let before = state_documents(executor.state());

    // Act: one command per error class, every one refused.
    let malformed = executor.execute_as(
        party.player,
        &Command::ApplyDamage {
            entity: party.pc.id,
            amount: -1,
            expected_rev: None,
        },
    );
    assert!(matches!(malformed, Err(EngineError::Malformed { .. })));

    let stale = executor.execute_as(
        party.player,
        &Command::ApplyDamage {
            entity: party.pc.id,
            amount: 1,
            expected_rev: Some(999),
        },
    );
    match stale {
        Err(EngineError::Conflict {
            entity,
            expected,
            actual,
        }) => {
            assert_eq!(entity, party.pc.id);
            assert_eq!(expected, 999);
            assert_eq!(actual, 1);
        }
        other => panic!("a stale revision must conflict, got {other:?}"),
    }

    let denied = executor.execute_as(
        stranger,
        &Command::ApplyDamage {
            entity: party.pc.id,
            amount: 1,
            expected_rev: None,
        },
    );
    assert!(matches!(
        denied,
        Err(EngineError::Unauthorized { actor, entity: Some(_) }) if actor == stranger
    ));
    let non_member = executor.execute_as(
        outsider,
        &Command::ApplyDamage {
            entity: party.pc.id,
            amount: 1,
            expected_rev: None,
        },
    );
    assert!(matches!(
        non_member,
        Err(EngineError::Unauthorized { actor, entity: Some(_) }) if actor == outsider
    ));

    let missing = executor.execute_as(
        party.player,
        &Command::ApplyDamage {
            entity: EntityId::new(),
            amount: 1,
            expected_rev: None,
        },
    );
    assert!(matches!(
        missing,
        Err(EngineError::NotFound { element: None, .. })
    ));

    let wrong_body = executor.execute_as(
        party.dm,
        &Command::AddStatus {
            entity: party.note.id,
            status: StatusSpec {
                kind: StatusKind::Effect,
                name: "Note status".to_owned(),
                reference: None,
                text: None,
                duration: StatusDuration::Indefinite,
                source: None,
                modifiers: Vec::new(),
            },
            expected_rev: None,
        },
    );
    assert!(matches!(wrong_body, Err(EngineError::Malformed { .. })));

    let unknown_status = executor.execute_as(
        party.player,
        &Command::RemoveStatus {
            entity: party.pc.id,
            status: StatusId::new(),
            expected_rev: None,
        },
    );
    assert!(matches!(
        unknown_status,
        Err(EngineError::NotFound {
            entity,
            element: Some(ElementRef::Status(_)),
        }) if entity == party.pc.id
    ));

    // Assert: no refused command moved the durable image, in memory or on
    // disk, before or after a reopen.
    assert_eq!(state_documents(executor.state()), before);
    drop(executor);
    let loaded = store.load().expect("the refused commands wrote nothing");
    assert_eq!(snapshot_documents(&loaded), before);
    drop(store);

    let reopened = SqliteStore::open(&path).expect("reopening the campaign");
    let loaded = reopened.load().expect("the file is readable");
    assert_eq!(snapshot_documents(&loaded), before);
}
