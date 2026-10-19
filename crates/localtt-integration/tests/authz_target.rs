//! U6 target test: authorization, entity views and read services, as a client
//! reads them.
//!
//! This file is the fixed point for `docs/rfcs/authz.md`: client code written
//! against the pinned API before any library code exists. It intentionally does
//! not compile until the U6 skeleton lands; that is the spec-phase contract,
//! not an oversight.

use std::collections::BTreeSet;

use localtt_adapter::store::memory::MemoryStore;
use localtt_core::{
    builders::{StatblockBuilder, ref_id, stat},
    types::{
        Abilities,
        Ability,
        AclEntry,
        AclLevel,
        AnyEntity,
        Audience,
        Campaign,
        CampaignTime,
        ChallengeRating,
        ClassLevelId,
        Coins,
        DiceExpr,
        Entity,
        EntityId,
        EntityKind,
        Ext,
        FeatureCategory,
        FeatureEntry,
        FeatureEntryId,
        FeedEntry,
        FeedEntryId,
        FeedPayload,
        FieldMask,
        Formula,
        HandoutEntity,
        Identity,
        ItemKind,
        ItemState,
        Namespace,
        NoteEntity,
        OverrideValue,
        Player,
        PlayerId,
        Recharge,
        RefOrText,
        Resource,
        ResourceId,
        ResourceKind,
        Role,
        Size,
        Skill,
        SkillProficiency,
        Speed,
        SpellEntry,
        SpellEntryId,
        StatKey,
        StatblockEntity,
        Status,
        StatusDuration,
        StatusId,
        StatusKind,
        UnixTime,
        ViewField,
    },
};
use localtt_engine::{
    authz::{
        self,
        EntityView,
        EntityViewBody,
        FeedEntryView,
        HandoutView,
        NoteView,
        StatblockView,
    },
    commands::{Command, CommandEnv, Event, RollSpec, StatusSpec},
    error::{ElementRef, EngineError},
    executor::Executor,
    ports::store::{CampaignSnapshot, CampaignStore, StatePatch},
    services,
    state::{LoadedCampaign, WorkingSet},
};
use localtt_integration::fixtures;

/// The fixed wall-clock stamp every fixture shares.
const FIXED_TIME: i64 = 1_750_000_000;

// ---------------------------------------------------------------------------
// Field, mask and grant helpers
// ---------------------------------------------------------------------------

/// A literal stat key, for the few commands that address one.
fn key(raw: &str) -> StatKey {
    stat(raw).expect("a literal stat key")
}

/// Every `ViewField`; one place so the mask helpers cannot drift from the
/// enum.
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

/// A set literal over a slice.
fn field_set(fields: &[ViewField]) -> BTreeSet<ViewField> {
    fields.iter().copied().collect()
}

/// Every field not listed: the hidden set a mask over `fields` implies.
fn all_but(fields: &[ViewField]) -> BTreeSet<ViewField> {
    let visible = field_set(fields);
    all_view_fields().difference(&visible).copied().collect()
}

/// A mask over the given fields.
fn set(fields: impl IntoIterator<Item = ViewField>) -> FieldMask {
    let mut mask = FieldMask::default();
    for field in fields {
        mask.insert(field);
    }
    mask
}

/// One ACL entry.
fn grant(player: PlayerId, level: AclLevel, field_mask: Option<FieldMask>) -> AclEntry {
    AclEntry {
        player,
        level,
        field_mask,
    }
}

/// A read share limited to the given fields.
fn masked_read(player: PlayerId, fields: impl IntoIterator<Item = ViewField>) -> AclEntry {
    grant(player, AclLevel::Read, Some(set(fields)))
}

/// A write share; the mask is deliberately garbage the pin says to ignore.
fn write_with_mask(player: PlayerId, fields: impl IntoIterator<Item = ViewField>) -> AclEntry {
    grant(player, AclLevel::Write, Some(set(fields)))
}

// ---------------------------------------------------------------------------
// Document and view downcasts
// ---------------------------------------------------------------------------

fn statblock(entity: &AnyEntity) -> &StatblockEntity {
    match entity {
        AnyEntity::Statblock(document) => document,
        other => panic!("a statblock, got {other:?}"),
    }
}

fn note_entity(entity: &AnyEntity) -> &NoteEntity {
    match entity {
        AnyEntity::Note(document) => document,
        other => panic!("a note, got {other:?}"),
    }
}

fn handout_entity(entity: &AnyEntity) -> &HandoutEntity {
    match entity {
        AnyEntity::Handout(document) => document,
        other => panic!("a handout, got {other:?}"),
    }
}

fn statblock_view(view: &EntityView) -> &StatblockView {
    match &view.body {
        Some(EntityViewBody::Statblock(body)) => body,
        other => panic!("a statblock view, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Executor and working-set helpers
// ---------------------------------------------------------------------------

/// The environment seam: the clock only; dice and content stay fail-closed
/// defaults, which is how a passed pre-check stays observable.
struct FixedEnv;

impl CommandEnv for FixedEnv {
    fn now(&self) -> UnixTime {
        UnixTime::from_seconds(FIXED_TIME)
    }
}

/// Appends one ordinary member to the roster.
fn add_player(campaign: &mut Campaign, display_name: &str) -> PlayerId {
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

/// The party's entities as one creation patch.
fn seeding_patch<'a>(
    campaign: &'a Campaign,
    entities: &'a [AnyEntity],
    feed: &'a [FeedEntry],
) -> StatePatch<'a> {
    StatePatch {
        campaign: Some(campaign),
        upserts: entities.iter().collect(),
        removals: Vec::new(),
        feed_appends: feed.iter().collect(),
        feed_keep_from: None,
    }
}

/// A fresh in-memory store holding the campaign, the entities and the feed.
fn seeded_store(campaign: &Campaign, entities: &[AnyEntity], feed: &[FeedEntry]) -> MemoryStore {
    let mut store = MemoryStore::new();
    store
        .apply(&seeding_patch(campaign, entities, feed))
        .expect("a fresh store accepts the seed");
    store
}

/// An executor over a just-seeded store and the fixed environment.
fn seeded_executor<'a>(store: &'a mut MemoryStore) -> Executor<'a> {
    let state = LoadedCampaign::from_snapshot(store.load().expect("the store was just seeded"));
    Executor::new(Box::new(state), store, Box::new(FixedEnv))
}

/// A loaded working set, for the read-service tests.
fn loaded_state(campaign: &Campaign, entities: &[AnyEntity], feed: &[FeedEntry]) -> LoadedCampaign {
    LoadedCampaign::from_snapshot(CampaignSnapshot {
        campaign: campaign.clone(),
        entities: entities.to_vec(),
        feed: feed.to_vec(),
    })
}

/// One statblock as the working set holds it.
fn statblock_of_state(state: &dyn WorkingSet, id: EntityId) -> StatblockEntity {
    match state
        .entity(id)
        .expect("the working set resolves its own headers")
        .expect("the entity exists")
    {
        AnyEntity::Statblock(document) => document,
        other => panic!("{id} is a statblock, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Feed helpers
// ---------------------------------------------------------------------------

/// One feed entry with the fixed stamp.
fn feed_entry(
    id: u64,
    audience: Audience,
    actor: Option<PlayerId>,
    entity: Option<EntityId>,
) -> FeedEntry {
    FeedEntry {
        id: FeedEntryId::new(id),
        ts: UnixTime::from_seconds(FIXED_TIME),
        clock_s: CampaignTime::from_seconds(0),
        actor,
        entity,
        audience,
        payload: FeedPayload::Hint {
            text: format!("entry {id}"),
        },
    }
}

/// Six entries covering every audience/actor combination the visibility rule
/// branches on.
fn feed_of(world: &World) -> Vec<FeedEntry> {
    let pc = world.pc.id();
    vec![
        feed_entry(1, Audience::Public, Some(world.player), Some(pc)),
        feed_entry(2, Audience::RollerAndDm, Some(world.player), Some(pc)),
        feed_entry(3, Audience::RollerAndDm, None, None),
        feed_entry(4, Audience::RollerOnly, Some(world.player), Some(pc)),
        feed_entry(5, Audience::RollerOnly, None, None),
        feed_entry(6, Audience::Public, None, None),
    ]
}

/// The ids of a feed page, ascending.
fn feed_ids(views: &[FeedEntryView]) -> Vec<u64> {
    views.iter().map(|view| view.id.get()).collect()
}

/// The ids of an entity list, in returned order.
fn entity_ids(views: &[EntityView]) -> Vec<EntityId> {
    views.iter().map(|view| view.id).collect()
}

/// One status spec, for the commands that take one.
fn spec(name: &str) -> StatusSpec {
    StatusSpec {
        kind: StatusKind::Effect,
        name: name.to_owned(),
        reference: None,
        text: None,
        duration: StatusDuration::Indefinite,
        source: None,
        modifiers: Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// The test world
// ---------------------------------------------------------------------------

/// The U6 test world: the golden party plus a roster of extra members, so one
/// entity can carry every ACL state under test.
struct World {
    campaign: Campaign,
    dm: PlayerId,
    player: PlayerId,
    stranger: PlayerId,
    writer: PlayerId,
    reader: PlayerId,
    masked: PlayerId,
    empty: PlayerId,
    union: PlayerId,
    overlap: PlayerId,
    write_empty: PlayerId,
    ac_only: PlayerId,
    sections: PlayerId,
    probe: PlayerId,
    unknown: PlayerId,
    pc: AnyEntity,
    note: AnyEntity,
    handout: AnyEntity,
}

fn world() -> World {
    let party = fixtures::party();
    let mut campaign = party.campaign.clone();
    let dm = party.dm;
    let player = party.player;
    let stranger = add_player(&mut campaign, "Stranger");
    let writer = add_player(&mut campaign, "Writer");
    let reader = add_player(&mut campaign, "Reader");
    let masked = add_player(&mut campaign, "Masked");
    let empty = add_player(&mut campaign, "Empty");
    let union = add_player(&mut campaign, "Union");
    let overlap = add_player(&mut campaign, "Overlap");
    let write_empty = add_player(&mut campaign, "Write Empty");
    let ac_only = add_player(&mut campaign, "AC Only");
    let sections = add_player(&mut campaign, "Sections");
    let probe = add_player(&mut campaign, "Probe");
    // Deliberately never added to the roster: membership comes first.
    let unknown = PlayerId::new();

    let mut pc = party.pc.clone();
    pc.acl = vec![
        grant(writer, AclLevel::Write, None),
        masked_read(masked, [ViewField::Name, ViewField::Hp]),
        masked_read(empty, []),
        masked_read(union, [ViewField::Name]),
        masked_read(union, [ViewField::Hp]),
        masked_read(ac_only, [ViewField::Ac]),
        masked_read(
            sections,
            all_but(&[ViewField::Name, ViewField::Ext, ViewField::Ac]),
        ),
        grant(overlap, AclLevel::Read, Some(set([ViewField::Name]))),
        grant(overlap, AclLevel::Write, Some(FieldMask::default())),
    ];

    let mut note = party.note.clone();
    note.acl = vec![
        grant(reader, AclLevel::Read, None),
        masked_read(masked, [ViewField::Hp]),
        write_with_mask(write_empty, []),
    ];

    let mut handout = party.handout.clone();
    handout.acl = vec![
        masked_read(union, [ViewField::Notes]),
        masked_read(union, []),
    ];

    World {
        campaign,
        dm,
        player,
        stranger,
        writer,
        reader,
        masked,
        empty,
        union,
        overlap,
        write_empty,
        ac_only,
        sections,
        probe,
        unknown,
        pc: AnyEntity::from(pc),
        note: AnyEntity::from(note),
        handout: AnyEntity::from(handout),
    }
}

// ---------------------------------------------------------------------------
// The marker creature
// ---------------------------------------------------------------------------

/// Every view section of one statblock, carrying a marker string or a value
/// that must not leak when the section is hidden.
fn marked_creature(owner: PlayerId) -> StatblockEntity {
    let mut ext = Ext::default();
    ext.insert(
        Namespace::parse("probe.test").expect("a literal namespace"),
        serde_json::json!({ "marker": "EXT-MARK" }),
    );

    let mut builder = StatblockBuilder::creature(
        ChallengeRating::parse("1/4").expect("a literal challenge rating"),
    );
    builder
        .abilities(Abilities {
            strength: 15,
            dexterity: 15,
            constitution: 15,
            intelligence: 15,
            wisdom: 15,
            charisma: 15,
        })
        .hp(40, 33)
        .size(Size::Small)
        .creature_type("TYPE-MARK")
        .senses("SENSES-MARK")
        .identity(Identity {
            race: Some(RefOrText::Text("RACE-MARK".to_owned())),
            background: Some(RefOrText::Text("BG-MARK".to_owned())),
            alignment: "IDENTITY-MARK".to_owned(),
        })
        .class_level_with_id(
            ClassLevelId::new(),
            RefOrText::Text("LEVEL-MARK".to_owned()),
            1,
            None,
        )
        .save_proficiency(Ability::Strength)
        .skill(Skill::Arcana, SkillProficiency::Proficient)
        .tools("TOOLS-MARK")
        .languages("LANGS-MARK")
        .speed(Speed {
            walk_ft: 40,
            fly_ft: Some(60),
            ..Speed::default()
        })
        .death_saves(1, 2)
        .exhaustion(2)
        .inspiration(true)
        .coins(Coins {
            cp: 1,
            sp: 2,
            ep: 3,
            gp: 4,
            pp: 5,
        })
        .override_stat(key("ac"), OverrideValue::Int(99))
        .notes("NOTES-MARK");
    builder.resource(Resource {
        id: ResourceId::new(),
        name: "RESOURCE-MARK".to_owned(),
        kind: ResourceKind::Feature,
        max: Formula::Const(3),
        current: 2,
        recharge: Recharge::Long,
    });
    builder.spellcasting_block(
        RefOrText::Text("SPELL-SOURCE".to_owned()),
        Ability::Charisma,
        vec![SpellEntry {
            id: SpellEntryId::new(),
            name: "SPELL-MARK".to_owned(),
            reference: Some(ref_id("probe.test:spell/mark").expect("a literal reference")),
            prepared: true,
        }],
    );
    builder.item_inline(
        "ITEM-MARK",
        ItemKind::Weapon,
        ItemState::Carried,
        false,
        false,
    );
    builder.attack("ATTACK-MARK", Some(5), "1d6 + 3", None);
    builder.feature(FeatureEntry {
        id: FeatureEntryId::new(),
        category: FeatureCategory::Trait,
        name: "FEATURE-MARK".to_owned(),
        reference: None,
        text: None,
        resource_id: None,
        modifiers: Vec::new(),
        ext: Ext::default(),
    });
    builder.status(Status {
        id: StatusId::new(),
        kind: StatusKind::Effect,
        name: "STATUS-MARK".to_owned(),
        reference: None,
        text: None,
        started_at: CampaignTime::from_seconds(0),
        duration: StatusDuration::Indefinite,
        source: None,
        modifiers: Vec::new(),
    });

    let mut body = builder.build().expect("abilities and hp are set");
    body.hp.temp = 5;
    Entity::builder(
        EntityKind::Monster,
        EntityId::new(),
        "NAME-MARK",
        owner,
        body,
    )
    .ext(ext)
    .created_at(UnixTime::from_seconds(FIXED_TIME))
    .updated_at(UnixTime::from_seconds(FIXED_TIME))
    .build()
    .expect("a monster kind pairs with a statblock body")
}

/// Every marker string of `marked_creature`, with the field that gates it.
const MARKERS: &[(&str, ViewField)] = &[
    ("NAME-MARK", ViewField::Name),
    ("RACE-MARK", ViewField::Identity),
    ("BG-MARK", ViewField::Identity),
    ("LEVEL-MARK", ViewField::Identity),
    ("IDENTITY-MARK", ViewField::Identity),
    ("TYPE-MARK", ViewField::Identity),
    ("SENSES-MARK", ViewField::Identity),
    ("TOOLS-MARK", ViewField::Proficiencies),
    ("LANGS-MARK", ViewField::Proficiencies),
    ("RESOURCE-MARK", ViewField::Resources),
    ("SPELL-MARK", ViewField::Spells),
    ("ITEM-MARK", ViewField::Inventory),
    ("ATTACK-MARK", ViewField::Attacks),
    ("FEATURE-MARK", ViewField::Features),
    ("STATUS-MARK", ViewField::Statuses),
    ("NOTES-MARK", ViewField::Notes),
    ("EXT-MARK", ViewField::Ext),
];

/// Every JSON pointer a visible statblock section adds, with the field that
/// gates it. `Ac` has no carrier (authz OQ-7); its visibility is asserted
/// only through `hidden`.
const POINTERS: &[(&str, ViewField)] = &[
    ("/name", ViewField::Name),
    ("/ext", ViewField::Ext),
    ("/body/profile", ViewField::Identity),
    ("/body/identity", ViewField::Identity),
    ("/body/levels", ViewField::Identity),
    ("/body/cr", ViewField::Identity),
    ("/body/creature_type", ViewField::Identity),
    ("/body/size", ViewField::Identity),
    ("/body/senses", ViewField::Identity),
    ("/body/abilities", ViewField::Abilities),
    ("/body/proficiencies/saves", ViewField::Saves),
    ("/body/proficiencies/skills", ViewField::Skills),
    ("/body/proficiencies/tools", ViewField::Proficiencies),
    ("/body/proficiencies/languages", ViewField::Proficiencies),
    ("/body/hp", ViewField::Hp),
    ("/body/speed", ViewField::Speed),
    ("/body/death_saves", ViewField::DeathSaves),
    ("/body/coins", ViewField::Coins),
    ("/body/overrides", ViewField::Overrides),
    ("/body/resources", ViewField::Resources),
    ("/body/spellcasting", ViewField::Spells),
    ("/body/inventory", ViewField::Inventory),
    ("/body/attacks", ViewField::Attacks),
    ("/body/features", ViewField::Features),
    ("/body/statuses", ViewField::Statuses),
    ("/body/notes", ViewField::Notes),
];

/// Whether a visible field forces the statblock body to exist: every section
/// except the name, the plugin bucket and the carrier-less `Ac`.
fn carries_body(field: ViewField) -> bool {
    !matches!(field, ViewField::Name | ViewField::Ac | ViewField::Ext)
}

// ---------------------------------------------------------------------------
// Membership and access levels
// ---------------------------------------------------------------------------

#[test]
fn roster_membership_gates_every_egress() {
    let world = world();

    // Owner and DM: full read and write.
    assert!(authz::can_read(&world.campaign, &world.pc, world.player));
    assert!(authz::can_write(&world.campaign, &world.pc, world.player));
    assert!(authz::can_read(&world.campaign, &world.pc, world.dm));
    assert!(authz::can_write(&world.campaign, &world.pc, world.dm));

    // A non-member is nobody, even when the document names them owner.
    let outsider = PlayerId::new();
    let owned = AnyEntity::from(fixtures::golden_wizard(outsider));
    assert!(!authz::can_read(&world.campaign, &owned, outsider));
    assert!(!authz::can_write(&world.campaign, &owned, outsider));
    assert!(
        authz::can_read(&world.campaign, &owned, world.dm),
        "the DM sees everything on the table"
    );

    // A member with no grant sees nothing and cannot write.
    assert!(!authz::can_read(&world.campaign, &world.pc, world.stranger));
    assert!(!authz::can_write(
        &world.campaign,
        &world.pc,
        world.stranger
    ));
    assert!(!authz::can_read(
        &world.campaign,
        &world.note,
        world.stranger
    ));
    assert!(!authz::can_read(
        &world.campaign,
        &world.handout,
        world.stranger
    ));

    // A non-member's view is the deny stub, headers mirrored only.
    let view = authz::view_of(&world.campaign, &world.pc, world.unknown);
    assert_eq!(view.hidden, all_view_fields());
    assert!(view.name.is_none());
    assert!(view.ext.is_none());
    assert!(view.body.is_none());
    assert!(!view.writable);

    // The membership gate covers feed egress too: a Public entry is visible
    // to any member and to nobody else.
    let entry = feed_entry(1, Audience::Public, Some(world.player), None);
    assert!(authz::entry_visible(
        &world.campaign,
        &entry,
        world.stranger
    ));
    assert!(!authz::entry_visible(
        &world.campaign,
        &entry,
        world.unknown
    ));
}

#[test]
fn full_readers_see_every_section_and_cannot_write() {
    let world = world();
    let pc = statblock(&world.pc);

    // The owner: every section, writable, header mirrored.
    let view = authz::view_of(&world.campaign, &world.pc, world.player);
    assert!(view.writable);
    assert!(view.hidden.is_empty());
    assert_eq!(view.id, pc.id);
    assert_eq!(view.kind, EntityKind::Pc);
    assert_eq!(view.owner, world.player);
    assert_eq!(view.parent, None);
    assert_eq!(view.rev, pc.rev);
    assert_eq!(view.name.as_deref(), Some(pc.name.as_str()));
    assert_eq!(view.ext, Some(pc.ext.clone()));
    let body = statblock_view(&view);
    assert_eq!(body.profile, Some(pc.body.profile));
    assert_eq!(body.identity, Some(pc.body.identity.clone()));
    assert_eq!(body.levels, Some(pc.body.levels.clone()));
    assert_eq!(body.cr, pc.body.cr);
    assert_eq!(body.creature_type, pc.body.creature_type.clone());
    assert_eq!(body.size, Some(pc.body.size));
    assert_eq!(body.senses, Some(pc.body.senses.clone()));
    assert_eq!(body.abilities, Some(pc.body.abilities));
    assert_eq!(body.hp, Some(pc.body.hp));
    assert_eq!(body.speed, Some(pc.body.speed));
    assert_eq!(body.death_saves, Some(pc.body.death_saves));
    assert_eq!(body.coins, Some(pc.body.coins));
    assert_eq!(body.overrides, Some(pc.body.overrides.clone()));
    assert_eq!(body.resources, Some(pc.body.resources.clone()));
    assert_eq!(body.spellcasting, Some(pc.body.spellcasting.clone()));
    assert_eq!(body.inventory, Some(pc.body.inventory.clone()));
    assert_eq!(body.attacks, Some(pc.body.attacks.clone()));
    assert_eq!(body.features, Some(pc.body.features.clone()));
    assert_eq!(body.statuses, Some(pc.body.statuses.clone()));
    assert_eq!(body.notes, Some(pc.body.notes.clone()));
    assert_eq!(body.exhaustion, pc.body.exhaustion);
    assert_eq!(body.inspiration, pc.body.inspiration);

    // The DM sees exactly what the owner sees.
    let dm = authz::view_of(&world.campaign, &world.pc, world.dm);
    assert_eq!(dm, view);

    // A plain read share: every section, read-only.
    let note = note_entity(&world.note);
    let view = authz::view_of(&world.campaign, &world.note, world.reader);
    assert!(authz::can_read(&world.campaign, &world.note, world.reader));
    assert!(!authz::can_write(
        &world.campaign,
        &world.note,
        world.reader
    ));
    assert!(!view.writable);
    assert!(view.hidden.is_empty());
    assert_eq!(
        view.body,
        Some(EntityViewBody::Note(NoteView {
            text: note.body.text.clone(),
        }))
    );

    // A member with no grant sees the stub, whatever the kind.
    let view = authz::view_of(&world.campaign, &world.note, world.stranger);
    assert_eq!(view.hidden, all_view_fields());
    assert!(view.name.is_none());
    assert!(view.ext.is_none());
    assert!(view.body.is_none());
    assert!(!view.writable);
}

#[test]
fn write_grants_ignore_their_masks() {
    let world = world();

    // Read plus Write, with the Write carrying an empty mask: the write grant
    // wins whole, the mask is not a cap.
    let view = authz::view_of(&world.campaign, &world.pc, world.overlap);
    assert!(authz::can_write(&world.campaign, &world.pc, world.overlap));
    assert!(view.writable);
    assert!(view.hidden.is_empty());
    assert_eq!(
        view.name.as_deref(),
        Some(statblock(&world.pc).name.as_str())
    );
    assert_eq!(statblock_view(&view).hp, Some(statblock(&world.pc).body.hp));

    // A lone Write grant with an empty mask on a note: full read, full write.
    let note = note_entity(&world.note);
    let view = authz::view_of(&world.campaign, &world.note, world.write_empty);
    assert!(authz::can_write(
        &world.campaign,
        &world.note,
        world.write_empty
    ));
    assert!(view.writable);
    assert!(view.hidden.is_empty());
    assert_eq!(
        view.body,
        Some(EntityViewBody::Note(NoteView {
            text: note.body.text.clone(),
        }))
    );
}

#[test]
fn read_masks_default_deny() {
    let world = world();
    let pc = statblock(&world.pc);

    // Exactly Name and Hp; everything else is hidden, not blank.
    let view = authz::view_of(&world.campaign, &world.pc, world.masked);
    assert!(authz::can_read(&world.campaign, &world.pc, world.masked));
    assert!(!authz::can_write(&world.campaign, &world.pc, world.masked));
    assert_eq!(view.hidden, all_but(&[ViewField::Name, ViewField::Hp]));
    assert_eq!(view.name.as_deref(), Some(pc.name.as_str()));
    assert!(view.ext.is_none());
    let body = statblock_view(&view);
    assert_eq!(body.hp, Some(pc.body.hp));
    assert_eq!(body.abilities, None);
    assert_eq!(body.identity, None);
    assert_eq!(body.proficiencies, None);
    assert_eq!(body.notes, None);

    // Ac is maskable but carries no view section: a grant that lists only Ac
    // authorizes the read and exposes no body.
    let view = authz::view_of(&world.campaign, &world.pc, world.ac_only);
    assert!(authz::can_read(&world.campaign, &world.pc, world.ac_only));
    assert_eq!(view.hidden, all_but(&[ViewField::Ac]));
    assert!(view.name.is_none());
    assert!(view.body.is_none());
    assert!(!view.writable);
}

#[test]
fn an_empty_mask_denies_like_no_grant() {
    let world = world();

    assert!(!authz::can_read(&world.campaign, &world.pc, world.empty));
    assert!(!authz::can_write(&world.campaign, &world.pc, world.empty));
    let denied = authz::view_of(&world.campaign, &world.pc, world.stranger);
    let emptied = authz::view_of(&world.campaign, &world.pc, world.empty);
    assert_eq!(emptied, denied, "an empty read mask leaks nothing");
    assert_eq!(emptied.hidden, all_view_fields());
    assert!(emptied.name.is_none());
    assert!(emptied.ext.is_none());
    assert!(emptied.body.is_none());
    assert!(!emptied.writable);
}

#[test]
fn grants_combine_permissively() {
    let world = world();

    // Two read grants union their masks; an empty grant never subtracts.
    let view = authz::view_of(&world.campaign, &world.pc, world.union);
    assert!(authz::can_read(&world.campaign, &world.pc, world.union));
    assert!(!view.writable);
    assert_eq!(view.hidden, all_but(&[ViewField::Name, ViewField::Hp]));
    assert!(view.name.is_some());
    let body = statblock_view(&view);
    assert!(body.hp.is_some());
    assert!(body.abilities.is_none());
    assert!(body.notes.is_none());

    // On the handout, {Notes} plus {} unions to {Notes}: the body comes back.
    let handout = handout_entity(&world.handout);
    let view = authz::view_of(&world.campaign, &world.handout, world.union);
    assert!(authz::can_read(
        &world.campaign,
        &world.handout,
        world.union
    ));
    assert_eq!(view.hidden, all_but(&[ViewField::Notes]));
    assert_eq!(
        view.body,
        Some(EntityViewBody::Handout(HandoutView {
            content: handout.body.content.clone(),
        }))
    );

    // Read plus Write resolves to Write; the read mask is not a cap.
    assert!(authz::can_write(&world.campaign, &world.pc, world.overlap));
    let view = authz::view_of(&world.campaign, &world.pc, world.overlap);
    assert!(view.hidden.is_empty());
}

#[test]
fn sections_map_into_the_statblock_view() {
    let world = world();
    let pc = statblock(&world.pc);
    let view = authz::view_of(&world.campaign, &world.pc, world.sections);

    assert!(authz::can_read(&world.campaign, &world.pc, world.sections));
    assert_eq!(
        view.hidden,
        field_set(&[ViewField::Name, ViewField::Ext, ViewField::Ac])
    );
    assert!(view.name.is_none(), "the name is its own field");
    assert!(view.ext.is_none());
    let body = statblock_view(&view);
    assert_eq!(body.profile, Some(pc.body.profile));
    assert_eq!(body.identity, Some(pc.body.identity.clone()));
    assert_eq!(body.levels, Some(pc.body.levels.clone()));
    assert_eq!(body.cr, pc.body.cr);
    assert_eq!(body.creature_type, pc.body.creature_type.clone());
    assert_eq!(body.size, Some(pc.body.size));
    assert_eq!(body.senses, Some(pc.body.senses.clone()));
    assert_eq!(body.abilities, Some(pc.body.abilities));
    let prof = body
        .proficiencies
        .as_ref()
        .expect("proficiencies are visible");
    assert_eq!(prof.saves, Some(pc.body.proficiencies.saves.clone()));
    assert_eq!(prof.skills, Some(pc.body.proficiencies.skills.clone()));
    assert_eq!(prof.tools, Some(pc.body.proficiencies.tools.clone()));
    assert_eq!(
        prof.languages,
        Some(pc.body.proficiencies.languages.clone())
    );
    assert_eq!(body.hp, Some(pc.body.hp));
    assert_eq!(body.speed, Some(pc.body.speed));
    assert_eq!(body.death_saves, Some(pc.body.death_saves));
    assert_eq!(body.coins, Some(pc.body.coins));
    assert_eq!(body.overrides, Some(pc.body.overrides.clone()));
    assert_eq!(body.resources, Some(pc.body.resources.clone()));
    assert_eq!(body.spellcasting, Some(pc.body.spellcasting.clone()));
    assert_eq!(body.inventory, Some(pc.body.inventory.clone()));
    assert_eq!(body.attacks, Some(pc.body.attacks.clone()));
    assert_eq!(body.features, Some(pc.body.features.clone()));
    assert_eq!(body.statuses, Some(pc.body.statuses.clone()));
    assert_eq!(body.notes, Some(pc.body.notes.clone()));
    assert_eq!(body.exhaustion, pc.body.exhaustion);
    assert_eq!(body.inspiration, pc.body.inspiration);

    // The identity group travels together: hiding Identity hides all seven
    // sections.
    let view = authz::view_of(&world.campaign, &world.pc, world.masked);
    let body = statblock_view(&view);
    assert_eq!(body.profile, None);
    assert_eq!(body.identity, None);
    assert_eq!(body.levels, None);
    assert_eq!(body.cr, None);
    assert_eq!(body.creature_type, None);
    assert_eq!(body.size, None);
    assert_eq!(body.senses, None);
}

#[test]
fn notes_gate_note_and_handout_bodies() {
    let world = world();
    let note = note_entity(&world.note);

    // A note's only body carrier is Notes.
    let view = authz::view_of(&world.campaign, &world.note, world.reader);
    assert!(view.hidden.is_empty());
    assert_eq!(
        view.body,
        Some(EntityViewBody::Note(NoteView {
            text: note.body.text.clone(),
        }))
    );
    let value = serde_json::to_value(&view).expect("views serialize");
    assert_eq!(
        value.pointer("/body/type").and_then(|tag| tag.as_str()),
        Some("note")
    );
    let view = authz::view_of(&world.campaign, &world.note, world.masked);
    assert_eq!(view.hidden, all_but(&[ViewField::Hp]));
    assert!(
        view.body.is_none(),
        "a mask without Notes hides the note body"
    );

    // A statblock with only Notes visible keeps the statblock body, notes and
    // all; the other sections stay absent.
    let mut solo = statblock(&world.pc).clone();
    solo.body.notes = "NOTES-MARK".to_owned();
    solo.acl = vec![masked_read(world.stranger, [ViewField::Notes])];
    let solo = AnyEntity::from(solo);
    let view = authz::view_of(&world.campaign, &solo, world.stranger);
    assert_eq!(view.hidden, all_but(&[ViewField::Notes]));
    let body = statblock_view(&view);
    assert_eq!(body.notes.as_deref(), Some("NOTES-MARK"));
    assert!(body.hp.is_none());
    assert!(body.abilities.is_none());

    // The handout body follows the same gate.
    let handout = handout_entity(&world.handout);
    let view = authz::view_of(&world.campaign, &world.handout, world.union);
    assert_eq!(
        view.body,
        Some(EntityViewBody::Handout(HandoutView {
            content: handout.body.content.clone(),
        }))
    );
    let value = serde_json::to_value(&view).expect("views serialize");
    assert_eq!(
        value.pointer("/body/type").and_then(|tag| tag.as_str()),
        Some("handout")
    );
}

#[test]
fn serialized_views_never_carry_hidden_sections() {
    let world = world();
    let all = all_view_fields();
    let cases: [BTreeSet<ViewField>; 8] = [
        field_set(&[ViewField::Name]),
        field_set(&[ViewField::Hp]),
        field_set(&[ViewField::Identity]),
        field_set(&[ViewField::Abilities, ViewField::Proficiencies]),
        field_set(&[ViewField::Ac]),
        field_set(&[ViewField::Notes]),
        field_set(&[ViewField::Saves, ViewField::Skills]),
        all.clone(),
    ];

    for visible in &cases {
        let mut creature = marked_creature(world.probe);
        creature.acl = vec![grant(
            world.masked,
            AclLevel::Read,
            Some(set(visible.iter().copied())),
        )];
        let creature = AnyEntity::from(creature);
        let view = authz::view_of(&world.campaign, &creature, world.masked);
        let hidden: BTreeSet<ViewField> = all.difference(visible).copied().collect();
        assert_eq!(view.hidden, hidden, "hidden set for {visible:?}");
        assert!(!view.writable);
        assert_eq!(view.name.is_some(), visible.contains(&ViewField::Name));
        assert_eq!(view.ext.is_some(), visible.contains(&ViewField::Ext));
        let has_body = visible.iter().copied().any(carries_body);
        assert_eq!(
            view.body.is_some(),
            has_body,
            "body presence for {visible:?}"
        );

        let value = serde_json::to_value(&view).expect("views serialize");
        for (pointer, field) in POINTERS {
            let header = matches!(*field, ViewField::Name | ViewField::Ext);
            let expected = visible.contains(field) && (header || has_body);
            assert_eq!(
                value.pointer(pointer).is_some(),
                expected,
                "{pointer} for {visible:?}"
            );
        }
        // The body tag is part of the body gate: the serialized body is a
        // statblock exactly when the body exists.
        assert_eq!(
            value.pointer("/body/type").and_then(|tag| tag.as_str()),
            has_body.then_some("statblock"),
            "the body tag for {visible:?}"
        );
        let text = value.to_string();
        for (marker, field) in MARKERS {
            assert_eq!(
                text.contains(marker),
                visible.contains(field),
                "{marker} for {visible:?}"
            );
        }
        if has_body {
            let body = statblock_view(&view);
            assert_eq!(body.exhaustion, 2, "the riders are not maskable");
            assert!(body.inspiration, "the riders are not maskable");
        }
    }
}

// ---------------------------------------------------------------------------
// Read services
// ---------------------------------------------------------------------------

#[test]
fn list_entities_filters_by_read_and_sorts_by_id() {
    let world = world();
    let entities = vec![world.pc.clone(), world.note.clone(), world.handout.clone()];
    let state = loaded_state(&world.campaign, &entities, &[]);

    // The DM sees everything, ascending by id, unmasked.
    let listed = services::list_entities(&state, world.dm);
    let mut expected = vec![world.pc.id(), world.note.id(), world.handout.id()];
    expected.sort();
    assert_eq!(entity_ids(&listed), expected);
    assert!(listed.iter().all(|view| view.hidden.is_empty()));

    // The owner sees the character; nothing else is shared with them.
    let listed = services::list_entities(&state, world.player);
    assert_eq!(entity_ids(&listed), vec![world.pc.id()]);
    assert!(listed[0].writable);

    // A plain reader sees the note; a masked reader sees both shares masked.
    let listed = services::list_entities(&state, world.reader);
    assert_eq!(entity_ids(&listed), vec![world.note.id()]);
    assert!(listed[0].hidden.is_empty());
    let listed = services::list_entities(&state, world.masked);
    let mut expected = vec![world.pc.id(), world.note.id()];
    expected.sort();
    assert_eq!(entity_ids(&listed), expected);
    let pc_view = listed
        .iter()
        .find(|view| view.id == world.pc.id())
        .expect("the pc is listed");
    assert_eq!(pc_view.hidden, all_but(&[ViewField::Name, ViewField::Hp]));
    let note_view = listed
        .iter()
        .find(|view| view.id == world.note.id())
        .expect("the note is listed");
    assert_eq!(note_view.hidden, all_but(&[ViewField::Hp]));

    // No grant: nothing; non-member: nothing.
    assert!(services::list_entities(&state, world.stranger).is_empty());
    assert!(services::list_entities(&state, world.unknown).is_empty());

    // The list view is the get view: both go through `view_of`.
    let got =
        services::get_entity(&state, world.pc.id(), world.masked).expect("the masked share reads");
    assert_eq!(got, *pc_view);
}

#[test]
fn get_entity_reports_missing_before_unauthorized() {
    let world = world();
    let entities = vec![world.pc.clone(), world.note.clone()];
    let state = loaded_state(&world.campaign, &entities, &[]);

    // Readable: the same masked view the list produces.
    let view =
        services::get_entity(&state, world.pc.id(), world.masked).expect("the masked share reads");
    assert_eq!(view.hidden, all_but(&[ViewField::Name, ViewField::Hp]));

    // Existing but unreadable is Unauthorized, with the entity named.
    let result = services::get_entity(&state, world.pc.id(), world.stranger);
    assert!(matches!(
        result,
        Err(EngineError::Unauthorized { actor, entity: Some(entity) })
            if actor == world.stranger && entity == world.pc.id()
    ));

    // A non-member gets the same answer as a grantless member.
    let result = services::get_entity(&state, world.pc.id(), world.unknown);
    assert!(matches!(
        result,
        Err(EngineError::Unauthorized { actor, entity: Some(entity) })
            if actor == world.unknown && entity == world.pc.id()
    ));

    // Missing beats unauthorized: there is nothing to authorize.
    let ghost = EntityId::new();
    let result = services::get_entity(&state, ghost, world.stranger);
    assert!(matches!(
        result,
        Err(EngineError::NotFound { entity, element: None }) if entity == ghost
    ));
}

#[test]
fn feed_visibility_follows_the_audience_matrix() {
    let world = world();
    let feed = feed_of(&world);
    let expected = [
        // dm, player (the actor), stranger, unknown (a non-member)
        [true, true, true, false],    // 1: Public, actor player
        [true, true, false, false],   // 2: RollerAndDm, actor player
        [true, false, false, false],  // 3: RollerAndDm, no actor: DM only
        [false, true, false, false],  // 4: RollerOnly, actor player: DM out
        [false, false, false, false], // 5: RollerOnly, no actor: nobody
        [true, true, true, false],    // 6: Public, no actor
    ];
    let viewers = [world.dm, world.player, world.stranger, world.unknown];
    for (index, entry) in feed.iter().enumerate() {
        for (slot, viewer) in viewers.iter().enumerate() {
            assert_eq!(
                authz::entry_visible(&world.campaign, entry, *viewer),
                expected[index][slot],
                "entry {} for viewer {slot}",
                entry.id.get()
            );
        }
    }
}

#[test]
fn feed_after_pages_visible_entries_ascending() {
    let world = world();
    let feed = feed_of(&world);
    let state = loaded_state(&world.campaign, &[], &feed);

    // The DM sees public entries and the RollerAndDm pair; the roller sees
    // their own RollerOnly entry and the public pair; a third member sees the
    // public pair.
    assert_eq!(
        feed_ids(&services::feed_after(&state, world.dm, None, 10)),
        vec![1, 2, 3, 6]
    );
    assert_eq!(
        feed_ids(&services::feed_after(&state, world.player, None, 10)),
        vec![1, 2, 4, 6]
    );
    assert_eq!(
        feed_ids(&services::feed_after(&state, world.stranger, None, 10)),
        vec![1, 6]
    );
    assert!(services::feed_after(&state, world.unknown, None, 10).is_empty());

    // Paging: the cursor is exclusive, the limit applies to the page.
    assert_eq!(
        feed_ids(&services::feed_after(&state, world.dm, None, 2)),
        vec![1, 2]
    );
    assert_eq!(
        feed_ids(&services::feed_after(
            &state,
            world.dm,
            Some(FeedEntryId::new(2)),
            10
        )),
        vec![3, 6]
    );
    assert_eq!(
        feed_ids(&services::feed_after(
            &state,
            world.dm,
            Some(FeedEntryId::new(5)),
            10
        )),
        vec![6]
    );
    // A cursor on an invisible position still pages by id.
    assert_eq!(
        feed_ids(&services::feed_after(
            &state,
            world.player,
            Some(FeedEntryId::new(5)),
            10
        )),
        vec![6]
    );
    // A zero limit is an empty page, never everything.
    assert!(services::feed_after(&state, world.dm, None, 0).is_empty());

    // A view mirrors the entry and never carries the audience.
    let views = services::feed_after(&state, world.dm, None, 10);
    let first = &views[0];
    assert_eq!(first.id, FeedEntryId::new(1));
    assert_eq!(first.ts, UnixTime::from_seconds(FIXED_TIME));
    assert_eq!(first.clock_s, CampaignTime::from_seconds(0));
    assert_eq!(first.actor, Some(world.player));
    assert_eq!(first.entity, Some(world.pc.id()));
    assert_eq!(first.payload, feed[0].payload);
    let value = serde_json::to_value(first).expect("feed views serialize");
    assert!(
        value.get("audience").is_none(),
        "the view never carries the audience"
    );
    let keys: BTreeSet<&str> = value
        .as_object()
        .expect("a feed view is an object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        BTreeSet::from(["id", "ts", "clock_s", "actor", "entity", "payload"])
    );

    // Absent links are absent keys, not nulls: the third DM-visible entry has
    // no actor and no entity.
    let third = &views[2];
    assert_eq!(third.actor, None);
    assert_eq!(third.entity, None);
    let value = serde_json::to_value(third).expect("feed views serialize");
    assert!(value.get("actor").is_none() && value.get("entity").is_none());
    let keys: BTreeSet<&str> = value
        .as_object()
        .expect("a feed view is an object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, BTreeSet::from(["id", "ts", "clock_s", "payload"]));
}

// ---------------------------------------------------------------------------
// The execute_as pre-check
// ---------------------------------------------------------------------------

#[test]
fn clock_commands_are_dm_only() {
    let world = world();
    let entities = vec![world.pc.clone()];
    let mut store = seeded_store(&world.campaign, &entities, &[]);
    let mut executor = seeded_executor(&mut store);

    let result = executor.execute_as(world.player, &Command::AdvanceClock { by_seconds: 60 });
    assert!(matches!(
        result,
        Err(EngineError::Unauthorized { actor, entity: None }) if actor == world.player
    ));
    let result = executor.execute_as(
        world.player,
        &Command::SetClock {
            to: CampaignTime::from_seconds(999),
        },
    );
    assert!(matches!(result, Err(EngineError::Unauthorized { .. })));
    assert_eq!(
        executor.state().campaign().clock.now_s,
        CampaignTime::from_seconds(0),
        "a denied clock move changes nothing"
    );

    // The DM may move and set it.
    let executed = executor
        .execute_as(world.dm, &Command::AdvanceClock { by_seconds: 60 })
        .expect("the DM moves the clock");
    assert_eq!(executed.warnings, Vec::new());
    assert_eq!(executed.events, vec![Event::CampaignChanged]);
    assert_eq!(
        executor.state().campaign().clock.now_s,
        CampaignTime::from_seconds(60)
    );
    executor
        .execute_as(
            world.dm,
            &Command::SetClock {
                to: CampaignTime::from_seconds(999),
            },
        )
        .expect("the DM sets the clock");
    assert_eq!(
        executor.state().campaign().clock.now_s,
        CampaignTime::from_seconds(999)
    );
}

#[test]
fn write_commands_require_write_access() {
    let world = world();
    let entities = vec![world.pc.clone(), world.note.clone()];
    let mut store = seeded_store(&world.campaign, &entities, &[]);
    let mut executor = seeded_executor(&mut store);
    let damage = Command::ApplyDamage {
        entity: world.pc.id(),
        amount: 5,
        expected_rev: None,
    };

    // A member without a grant is denied; a read mask does not write.
    let result = executor.execute_as(world.stranger, &damage);
    assert!(matches!(
        result,
        Err(EngineError::Unauthorized { actor, entity: Some(id) })
            if actor == world.stranger && id == world.pc.id()
    ));
    let result = executor.execute_as(world.masked, &damage);
    assert!(matches!(result, Err(EngineError::Unauthorized { .. })));
    assert_eq!(
        statblock_of_state(executor.state(), world.pc.id())
            .body
            .hp
            .current,
        32,
        "the denials changed nothing"
    );

    // The write grant is enough, and so is being the owner.
    executor
        .execute_as(world.writer, &damage)
        .expect("the writer may damage");
    assert_eq!(
        statblock_of_state(executor.state(), world.pc.id())
            .body
            .hp
            .current,
        27
    );
    executor
        .execute_as(world.player, &damage)
        .expect("the owner may damage");
    assert_eq!(
        statblock_of_state(executor.state(), world.pc.id())
            .body
            .hp
            .current,
        22
    );

    // Authorization precedes the kind check: the note's write grant passes and
    // the command's own Malformed refuses; a reader never gets that far.
    let add = Command::AddStatus {
        entity: world.note.id(),
        status: spec("Note status"),
        expected_rev: None,
    };
    let result = executor.execute_as(world.stranger, &add);
    assert!(matches!(result, Err(EngineError::Unauthorized { .. })));
    let result = executor.execute_as(world.write_empty, &add);
    assert!(
        matches!(result, Err(EngineError::Malformed { .. })),
        "the write grant passed; the kind check then refuses"
    );
    let result = executor.execute_as(world.reader, &add);
    assert!(
        matches!(result, Err(EngineError::Unauthorized { .. })),
        "read is not write"
    );
}

#[test]
fn rolls_require_read_and_hit_dice_require_write() {
    let world = world();
    let mut aria = statblock(&world.pc).clone();
    let hit_die = ResourceId::new();
    aria.body.resources.push(Resource {
        id: hit_die,
        name: "Hit Dice (d6)".to_owned(),
        kind: ResourceKind::HitDie { sides: 6 },
        max: Formula::Const(5),
        current: 5,
        recharge: Recharge::Long,
    });
    let entities = vec![AnyEntity::from(aria.clone())];
    let mut store = seeded_store(&world.campaign, &entities, &[]);
    let mut executor = seeded_executor(&mut store);

    let check = Command::Roll {
        entity: Some(aria.id),
        spec: RollSpec::Check {
            stat: key("save.wis"),
            target_dc: None,
        },
        audience: None,
    };

    // No grant, and an empty mask, both deny the read the roll needs.
    let result = executor.execute_as(world.stranger, &check);
    assert!(matches!(
        result,
        Err(EngineError::Unauthorized { actor, entity: Some(id) })
            if actor == world.stranger && id == aria.id
    ));
    let result = executor.execute_as(world.empty, &check);
    assert!(matches!(result, Err(EngineError::Unauthorized { .. })));
    let result = executor.execute_as(world.reader, &check);
    assert!(
        matches!(result, Err(EngineError::Unauthorized { .. })),
        "a grant on another entity is not a grant here"
    );
    assert!(
        executor.state().feed().is_empty(),
        "a denied roll appends nothing to the feed"
    );

    // A non-empty mask authorizes the read; a write grant too. Both then die
    // on the default dice port: `Environment` proves the pre-check passed.
    let result = executor.execute_as(world.masked, &check);
    assert!(
        matches!(result, Err(EngineError::Environment { .. })),
        "the mask authorizes the roll; the environment cannot roll"
    );
    let result = executor.execute_as(world.writer, &check);
    assert!(matches!(result, Err(EngineError::Environment { .. })));

    // A free expression needs roster membership, not an entity grant.
    let free = Command::Roll {
        entity: None,
        spec: RollSpec::Expression {
            expr: DiceExpr::Const { value: 1 },
            target: None,
        },
        audience: None,
    };
    let result = executor.execute_as(world.stranger, &free);
    assert!(matches!(result, Err(EngineError::Environment { .. })));
    let result = executor.execute_as(world.unknown, &free);
    assert!(matches!(
        result,
        Err(EngineError::Unauthorized { actor, entity: None }) if actor == world.unknown
    ));

    // Spending a hit die is a write when it addresses an entity.
    let spend = Command::SpendHitDie {
        entity: aria.id,
        resource: hit_die,
        expected_rev: None,
    };
    let result = executor.execute_as(world.masked, &spend);
    assert!(matches!(result, Err(EngineError::Unauthorized { .. })));
    let result = executor.execute_as(world.writer, &spend);
    assert!(
        matches!(result, Err(EngineError::Environment { .. })),
        "the write check passed; only the dice port is missing"
    );
}

#[test]
fn the_trusted_path_performs_no_authorization() {
    let world = world();
    let entities = vec![world.pc.clone()];
    let mut store = seeded_store(&world.campaign, &entities, &[]);
    let mut executor = seeded_executor(&mut store);

    // The trusted system path carries no actor and is not authorized.
    let executed = executor
        .execute(&Command::ApplyDamage {
            entity: world.pc.id(),
            amount: 5,
            expected_rev: None,
        })
        .expect("the trusted path is unchecked");
    assert_eq!(executed.events.len(), 1);
    assert_eq!(
        statblock_of_state(executor.state(), world.pc.id())
            .body
            .hp
            .current,
        27
    );

    executor
        .execute(&Command::AdvanceClock { by_seconds: 60 })
        .expect("the trusted path moves the clock");
    assert_eq!(
        executor.state().campaign().clock.now_s,
        CampaignTime::from_seconds(60)
    );
}

#[test]
fn authorization_precedes_validation_and_cas() {
    let world = world();
    let entities = vec![world.pc.clone()];
    let mut store = seeded_store(&world.campaign, &entities, &[]);
    let mut executor = seeded_executor(&mut store);
    let spell = SpellEntryId::new();
    let cast = Command::CastSpell {
        entity: world.pc.id(),
        spell,
        slot: ResourceId::new(),
        upcast_slots: 0,
        concentrate: false,
        expected_rev: None,
    };

    // An unauthorized actor gets Unauthorized even when the request would also
    // be Malformed, stale (Conflict) or reference a missing element.
    let result = executor.execute_as(
        world.stranger,
        &Command::ApplyDamage {
            entity: world.pc.id(),
            amount: -1,
            expected_rev: None,
        },
    );
    assert!(matches!(result, Err(EngineError::Unauthorized { .. })));
    let result = executor.execute_as(
        world.stranger,
        &Command::ApplyDamage {
            entity: world.pc.id(),
            amount: 1,
            expected_rev: Some(999),
        },
    );
    assert!(matches!(result, Err(EngineError::Unauthorized { .. })));
    let result = executor.execute_as(world.stranger, &cast);
    assert!(matches!(result, Err(EngineError::Unauthorized { .. })));

    // The same requests from an authorized actor reach the deeper errors.
    let result = executor.execute_as(
        world.player,
        &Command::ApplyDamage {
            entity: world.pc.id(),
            amount: 1,
            expected_rev: Some(999),
        },
    );
    assert!(matches!(
        result,
        Err(EngineError::Conflict {
            expected: 999,
            actual: 0,
            ..
        })
    ));
    let result = executor.execute_as(
        world.player,
        &Command::ApplyDamage {
            entity: world.pc.id(),
            amount: -1,
            expected_rev: None,
        },
    );
    assert!(matches!(result, Err(EngineError::Malformed { .. })));
    let result = executor.execute_as(world.player, &cast);
    assert!(matches!(
        result,
        Err(EngineError::NotFound {
            entity,
            element: Some(ElementRef::Spell(id)),
        }) if entity == world.pc.id() && id == spell
    ));

    // A missing target is NotFound, not Unauthorized: there is nothing to
    // authorize, and the command's own error is the honest one.
    let ghost = EntityId::new();
    let result = executor.execute_as(
        world.stranger,
        &Command::ApplyDamage {
            entity: ghost,
            amount: 1,
            expected_rev: None,
        },
    );
    assert!(matches!(
        result,
        Err(EngineError::NotFound { entity, element: None }) if entity == ghost
    ));
    let result = executor.execute_as(
        world.dm,
        &Command::ApplyDamage {
            entity: ghost,
            amount: 1,
            expected_rev: None,
        },
    );
    assert!(matches!(result, Err(EngineError::NotFound { .. })));
}
