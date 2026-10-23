//! `EntityView` and the `ViewField` section map (RFC `authz.md` 5.3, 5.4 and
//! section 6).

use std::collections::{BTreeMap, BTreeSet};

use localtt_core::types::{
    Abilities,
    Ability,
    AnyEntity,
    AttackEntry,
    Audience,
    Campaign,
    CampaignId,
    CampaignTime,
    ChallengeRating,
    ClassLevel,
    Coins,
    DeathSaves,
    EntityId,
    EntityKind,
    Ext,
    FeatureEntry,
    FeedEntryId,
    FeedPayload,
    HandoutContent,
    Hp,
    Identity,
    InventoryEntry,
    OverrideValue,
    PlayerId,
    Profile,
    Resource,
    Role,
    RulesetId,
    Size,
    Skill,
    SkillProficiency,
    Speed,
    SpellcastingBlock,
    StatKey,
    StatblockBody,
    Status,
    UnixTime,
    ViewField,
};
use serde::{Deserialize, Serialize};

use crate::{
    authz::access::{self, ALL},
    error::EngineError,
};

/// An engine-owned mirror of one entity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityView {
    /// Mirrored entity id.
    pub id: EntityId,
    /// Mirrored entity kind.
    pub kind: EntityKind,
    /// Mirrored owner.
    pub owner: PlayerId,
    /// Mirrored handout/companion parent.
    pub parent: Option<EntityId>,
    /// Mirrored document revision.
    pub rev: u64,
    /// Whether this viewer may write; `can_write` for the same arguments.
    pub writable: bool,
    /// Every section this viewer may not see; `ALL \ visible`.
    pub hidden: BTreeSet<ViewField>,
    /// Visible name; absent when `Name` is hidden.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Visible plugin bucket; absent when `Ext` is hidden.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ext: Option<Ext>,
    /// The visible body family; absent when no carrier section is visible.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<EntityViewBody>,
}

/// The three visible body families (core `BodyKind`), tagged like the core
/// document bodies.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[expect(
    clippy::large_enum_variant,
    reason = "the unboxed `Statblock(StatblockView)` shape is the pinned public contract (authz.md 4.1); one view is built per request, so the size is irrelevant"
)]
pub enum EntityViewBody {
    /// A statblock family entity.
    Statblock(StatblockView),
    /// A session note.
    Note(NoteView),
    /// A handout.
    Handout(HandoutView),
}

/// One statblock as a viewer sees it: one `Option` per carrier field.
///
/// Where the core field is already an `Option` (`cr`, `creature_type`), the
/// view keeps the core type; the visibility signal lives in `hidden`, so an
/// absent value and a hidden section stay distinguishable.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatblockView {
    /// Profile; gated by `ViewField::Identity`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<Profile>,
    /// Identity box; gated by `ViewField::Identity`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity: Option<Identity>,
    /// Class levels; gated by `ViewField::Identity`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub levels: Option<Vec<ClassLevel>>,
    /// Challenge rating; gated by `ViewField::Identity`, mirrors the core
    /// `Option`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cr: Option<ChallengeRating>,
    /// Creature type; gated by `ViewField::Identity`, mirrors the core
    /// `Option`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creature_type: Option<String>,
    /// Size; gated by `ViewField::Identity`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<Size>,
    /// Senses; gated by `ViewField::Identity`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub senses: Option<String>,
    /// Ability scores; gated by `ViewField::Abilities`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abilities: Option<Abilities>,
    /// Saves, skills, tools and languages; one gate each (section 5.3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proficiencies: Option<ProficienciesView>,
    /// Hit points; gated by `ViewField::Hp`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hp: Option<Hp>,
    /// Movement speeds; gated by `ViewField::Speed`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed: Option<Speed>,
    /// Death saves; gated by `ViewField::DeathSaves`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub death_saves: Option<DeathSaves>,
    /// Coins; gated by `ViewField::Coins`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coins: Option<Coins>,
    /// Derived-value overrides; gated by `ViewField::Overrides`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overrides: Option<BTreeMap<StatKey, OverrideValue>>,
    /// Resources; gated by `ViewField::Resources`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<Vec<Resource>>,
    /// Spellcasting blocks; gated by `ViewField::Spells`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spellcasting: Option<Vec<SpellcastingBlock>>,
    /// Inventory; gated by `ViewField::Inventory`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inventory: Option<Vec<InventoryEntry>>,
    /// Attack lines; gated by `ViewField::Attacks`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attacks: Option<Vec<AttackEntry>>,
    /// Features; gated by `ViewField::Features`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub features: Option<Vec<FeatureEntry>>,
    /// Statuses; gated by `ViewField::Statuses`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub statuses: Option<Vec<Status>>,
    /// Notes; gated by `ViewField::Notes`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// Exhaustion level; an always-visible rider (section 9, OQ-1).
    pub exhaustion: u8,
    /// Inspiration flag; an always-visible rider (section 9, OQ-1).
    pub inspiration: bool,
}

/// The four maskable parts of a statblock's proficiencies.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProficienciesView {
    /// Save proficiencies; gated by `ViewField::Saves`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saves: Option<BTreeSet<Ability>>,
    /// Skill proficiencies; gated by `ViewField::Skills`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skills: Option<BTreeMap<Skill, SkillProficiency>>,
    /// Tool lines; gated by `ViewField::Proficiencies`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<String>,
    /// Language lines; gated by `ViewField::Proficiencies`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub languages: Option<String>,
}

/// A session note as a viewer sees it; present iff `Notes` is visible.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteView {
    /// The note text.
    pub text: String,
}

/// A handout as a viewer sees it; present iff `Notes` is visible.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoutView {
    /// The handout payload.
    pub content: HandoutContent,
}

/// One feed entry as a viewer sees it: the entry minus its audience.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedEntryView {
    /// Feed position.
    pub id: FeedEntryId,
    /// Wall-clock stamp.
    pub ts: UnixTime,
    /// Campaign-clock stamp.
    pub clock_s: CampaignTime,
    /// Acting player, when the entry has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor: Option<PlayerId>,
    /// Related entity, when the entry has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity: Option<EntityId>,
    /// What happened.
    pub payload: FeedPayload,
}

/// One campaign participant as the roster exposes them.
///
/// A roster entry never carries the join-token hash or the last-seen stamp.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RosterEntryView {
    /// Player id.
    pub id: PlayerId,
    /// Display name.
    pub display_name: String,
    /// Campaign role.
    pub role: Role,
}

/// Campaign-wide settings every roster member may read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CampaignSettingsView {
    /// Default audience for player rolls.
    pub player_roll_audience_default: Audience,
    /// Feed ring-buffer capacity.
    pub feed_capacity: u32,
    /// Hour of day (0 to 23) at which Dawn recharges happen.
    pub dawn_hour: u8,
}

/// The campaign as one roster member sees it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CampaignView {
    /// The roster member this view was projected for.
    pub viewer: PlayerId,
    /// Campaign id.
    pub id: CampaignId,
    /// Display name.
    pub name: String,
    /// Default ruleset for new entities.
    pub default_ruleset: RulesetId,
    /// The campaign clock.
    pub clock: CampaignTime,
    /// Member-visible campaign settings.
    pub settings: CampaignSettingsView,
    /// The roster in stored order; no join-token material.
    pub roster: Vec<RosterEntryView>,
}

/// The viewer's campaign mirror; a non-member is unauthorized.
///
/// Membership, not entity access, gates the campaign document (RFC
/// `authz.md` 5.1). The roster is the same for every member; only the
/// entity/feed views vary by role and grants.
pub(super) fn view_of_campaign(
    campaign: &Campaign,
    viewer: PlayerId,
) -> Result<CampaignView, EngineError> {
    if !access::member(campaign, viewer) {
        return Err(EngineError::Unauthorized {
            actor: viewer,
            entity: None,
        });
    }
    Ok(CampaignView {
        viewer,
        id: campaign.id,
        name: campaign.name.clone(),
        default_ruleset: campaign.default_ruleset.clone(),
        clock: campaign.clock.now_s,
        settings: CampaignSettingsView {
            player_roll_audience_default: campaign.settings.player_roll_audience_default,
            feed_capacity: campaign.settings.feed_capacity,
            dawn_hour: campaign.settings.dawn_hour,
        },
        roster: campaign
            .players
            .iter()
            .map(|player| RosterEntryView {
                id: player.id,
                display_name: player.display_name.clone(),
                role: player.role,
            })
            .collect(),
    })
}

/// The viewer's mirror of one entity (RFC `authz.md` 5.3, 5.4): mirrored
/// headers, the visible sections, the hidden complement and the write flag.
pub(super) fn view_of(campaign: &Campaign, entity: &AnyEntity, viewer: PlayerId) -> EntityView {
    let (writable, visible) = match access::resolve(campaign, entity, viewer) {
        Some(access) => (access.write, access.visible),
        None => (false, BTreeSet::new()),
    };
    let hidden = ALL
        .iter()
        .copied()
        .filter(|field| !visible.contains(field))
        .collect();
    EntityView {
        id: entity.id(),
        kind: entity.kind(),
        owner: entity.owner(),
        parent: entity.parent(),
        rev: entity.rev(),
        writable,
        hidden,
        name: visible
            .contains(&ViewField::Name)
            .then(|| entity.name().to_owned()),
        ext: visible
            .contains(&ViewField::Ext)
            .then(|| entity.ext().clone()),
        body: body_of(entity, &visible),
    }
}

/// The carrier fields of a statblock body (5.3): every section except the
/// header-only `Name` and `Ext` and the carrier-less `Ac`.
const STATBLOCK_CARRIERS: [ViewField; 17] = [
    ViewField::Identity,
    ViewField::Abilities,
    ViewField::Saves,
    ViewField::Skills,
    ViewField::Proficiencies,
    ViewField::Hp,
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
];

/// The body gate (5.3): a statblock body appears with its first visible
/// carrier; a note or handout body rides `Notes` alone.
fn body_of(entity: &AnyEntity, visible: &BTreeSet<ViewField>) -> Option<EntityViewBody> {
    match entity {
        AnyEntity::Statblock(document) => STATBLOCK_CARRIERS
            .iter()
            .any(|field| visible.contains(field))
            .then(|| EntityViewBody::Statblock(statblock_view(&document.body, visible))),
        AnyEntity::Note(document) => visible.contains(&ViewField::Notes).then(|| {
            EntityViewBody::Note(NoteView {
                text: document.body.text.clone(),
            })
        }),
        AnyEntity::Handout(document) => visible.contains(&ViewField::Notes).then(|| {
            EntityViewBody::Handout(HandoutView {
                content: document.body.content.clone(),
            })
        }),
    }
}

/// One statblock body per the section map (5.3): each carrier appears iff
/// its `ViewField` is visible, and `cr`/`creature_type` mirror the core
/// `Option` behind the `Identity` gate.
fn statblock_view(body: &StatblockBody, visible: &BTreeSet<ViewField>) -> StatblockView {
    let identity = visible.contains(&ViewField::Identity);
    let saves = visible.contains(&ViewField::Saves);
    let skills = visible.contains(&ViewField::Skills);
    let proficiencies = visible.contains(&ViewField::Proficiencies);
    StatblockView {
        profile: identity.then_some(body.profile),
        identity: identity.then(|| body.identity.clone()),
        levels: identity.then(|| body.levels.clone()),
        cr: if identity { body.cr } else { None },
        creature_type: if identity {
            body.creature_type.clone()
        } else {
            None
        },
        size: identity.then_some(body.size),
        senses: identity.then(|| body.senses.clone()),
        abilities: visible
            .contains(&ViewField::Abilities)
            .then_some(body.abilities),
        proficiencies: (saves || skills || proficiencies).then(|| ProficienciesView {
            saves: saves.then(|| body.proficiencies.saves.clone()),
            skills: skills.then(|| body.proficiencies.skills.clone()),
            tools: proficiencies.then(|| body.proficiencies.tools.clone()),
            languages: proficiencies.then(|| body.proficiencies.languages.clone()),
        }),
        hp: visible.contains(&ViewField::Hp).then_some(body.hp),
        speed: visible.contains(&ViewField::Speed).then_some(body.speed),
        death_saves: visible
            .contains(&ViewField::DeathSaves)
            .then_some(body.death_saves),
        coins: visible.contains(&ViewField::Coins).then_some(body.coins),
        overrides: visible
            .contains(&ViewField::Overrides)
            .then(|| body.overrides.clone()),
        resources: visible
            .contains(&ViewField::Resources)
            .then(|| body.resources.clone()),
        spellcasting: visible
            .contains(&ViewField::Spells)
            .then(|| body.spellcasting.clone()),
        inventory: visible
            .contains(&ViewField::Inventory)
            .then(|| body.inventory.clone()),
        attacks: visible
            .contains(&ViewField::Attacks)
            .then(|| body.attacks.clone()),
        features: visible
            .contains(&ViewField::Features)
            .then(|| body.features.clone()),
        statuses: visible
            .contains(&ViewField::Statuses)
            .then(|| body.statuses.clone()),
        notes: visible
            .contains(&ViewField::Notes)
            .then(|| body.notes.clone()),
        exhaustion: body.exhaustion,
        inspiration: body.inspiration,
    }
}

#[cfg(test)]
mod tests {
    use localtt_core::{
        builders::{CampaignBuilder, StatblockBuilder},
        types::{
            AclEntry,
            AclLevel,
            ClassLevelId,
            Entity,
            FeatureCategory,
            FeatureEntryId,
            FieldMask,
            Formula,
            ItemKind,
            ItemState,
            NoteBody,
            Recharge,
            RefOrText,
            ResourceId,
            ResourceKind,
            SpellEntry,
            SpellEntryId,
            StatblockEntity,
            StatusDuration,
            StatusId,
            StatusKind,
            TokenHash,
        },
    };

    use super::*;

    /// A campaign with an owner and one unrelated member.
    fn campaign() -> (Campaign, PlayerId, PlayerId) {
        let mut builder = CampaignBuilder::new("Curse of the Vault");
        let owner = builder.player("Owner");
        let other = builder.player("Other");
        (builder.build(), owner, other)
    }

    /// A pc owned by `owner` and carrying `acl`.
    fn wizard(owner: PlayerId, acl: Vec<AclEntry>) -> AnyEntity {
        let mut builder = StatblockBuilder::character();
        builder
            .abilities(Abilities::all(10))
            .hp(32, 32)
            .notes("session notes")
            .exhaustion(2)
            .inspiration(true);
        let mut entity = builder
            .build_entity(EntityKind::Pc, EntityId::new(), "Aria", owner)
            .expect("abilities and hp are set");
        entity.acl = acl;
        AnyEntity::from(entity)
    }

    /// A note owned by `owner` and carrying `acl`.
    fn note(owner: PlayerId, acl: Vec<AclEntry>) -> AnyEntity {
        let mut entity = Entity::builder(
            EntityKind::Note,
            EntityId::new(),
            "A session note",
            owner,
            NoteBody {
                text: "the secret".to_owned(),
            },
        )
        .build()
        .expect("a note kind pairs with a note body");
        entity.acl = acl;
        AnyEntity::from(entity)
    }

    /// A read grant over exactly `fields`.
    fn read(player: PlayerId, fields: &[ViewField]) -> AclEntry {
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

    fn all_fields() -> BTreeSet<ViewField> {
        ALL.into_iter().collect()
    }

    #[test]
    fn carrierless_sections_open_no_body() {
        let (campaign, owner, other) = campaign();
        let entity = wizard(
            owner,
            vec![read(
                other,
                &[ViewField::Name, ViewField::Ac, ViewField::Ext],
            )],
        );

        let view = view_of(&campaign, &entity, other);
        assert_eq!(view.name.as_deref(), Some("Aria"));
        assert!(view.ext.is_some());
        assert!(view.body.is_none(), "Name, Ac and Ext carry no body");
        let mut hidden = all_fields();
        for field in [ViewField::Name, ViewField::Ac, ViewField::Ext] {
            hidden.remove(&field);
        }
        assert_eq!(view.hidden, hidden);
    }

    #[test]
    fn statblock_sections_gate_one_by_one() {
        let (campaign, owner, other) = campaign();
        let entity = wizard(owner, vec![read(other, &[ViewField::Hp, ViewField::Notes])]);

        let view = view_of(&campaign, &entity, other);
        let EntityViewBody::Statblock(body) = view.body.expect("Hp opens the body") else {
            panic!("the pc is a statblock");
        };
        assert_eq!(body.hp.map(|hp| (hp.max, hp.current)), Some((32, 32)));
        assert_eq!(body.notes.as_deref(), Some("session notes"));
        assert!(body.identity.is_none());
        assert!(body.abilities.is_none());
        assert!(body.proficiencies.is_none());
        assert_eq!(body.exhaustion, 2, "the riders are not masked");
        assert!(body.inspiration, "the riders are not masked");
    }

    #[test]
    fn a_note_body_rides_notes_alone() {
        let (campaign, owner, other) = campaign();

        let hidden_by_mask = note(owner, vec![read(other, &[ViewField::Hp])]);
        let view = view_of(&campaign, &hidden_by_mask, other);
        assert!(
            view.body.is_none(),
            "a mask without Notes hides the note body"
        );

        let visible = note(owner, vec![read(other, &[ViewField::Notes])]);
        let view = view_of(&campaign, &visible, other);
        assert_eq!(view.hidden, {
            let mut hidden = all_fields();
            hidden.remove(&ViewField::Notes);
            hidden
        });
        assert_eq!(
            view.body,
            Some(EntityViewBody::Note(NoteView {
                text: "the secret".to_owned(),
            }))
        );
    }

    /// A monster with every view section populated, for the mask sweep.
    fn marked_creature(owner: PlayerId) -> StatblockEntity {
        let mut builder = StatblockBuilder::creature(
            ChallengeRating::parse("1/4").expect("a literal challenge rating"),
        );
        builder
            .abilities(Abilities::all(10))
            .hp(40, 33)
            .size(Size::Small)
            .creature_type("TYPE")
            .senses("SENSES")
            .identity(Identity {
                race: Some(RefOrText::Text("RACE".to_owned())),
                background: Some(RefOrText::Text("BG".to_owned())),
                alignment: "ALIGNMENT".to_owned(),
            })
            .class_level_with_id(
                ClassLevelId::new(),
                RefOrText::Text("LEVEL".to_owned()),
                1,
                None,
            )
            .save_proficiency(Ability::Strength)
            .skill(Skill::Arcana, SkillProficiency::Proficient)
            .tools("TOOLS")
            .languages("LANGS")
            .speed(Speed {
                walk_ft: 40,
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
            .override_stat(
                StatKey::parse("ac").expect("a literal stat key"),
                OverrideValue::Int(99),
            )
            .notes("NOTES");
        builder.resource(Resource {
            id: ResourceId::new(),
            name: "RESOURCE".to_owned(),
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
                name: "SPELL".to_owned(),
                reference: None,
                prepared: true,
            }],
        );
        builder.item_inline("ITEM", ItemKind::Weapon, ItemState::Carried, false, false);
        builder.attack("ATTACK", Some(5), "1d6 + 3", None);
        builder.feature(FeatureEntry {
            id: FeatureEntryId::new(),
            category: FeatureCategory::Trait,
            name: "FEATURE".to_owned(),
            reference: None,
            text: None,
            resource_id: None,
            modifiers: Vec::new(),
            ext: Ext::default(),
        });
        builder.status(Status {
            id: StatusId::new(),
            kind: StatusKind::Effect,
            name: "STATUS".to_owned(),
            reference: None,
            text: None,
            started_at: CampaignTime::from_seconds(0),
            duration: StatusDuration::Indefinite,
            source: None,
            modifiers: Vec::new(),
        });

        let body = builder.build().expect("abilities and hp are set");
        Entity::builder(EntityKind::Monster, EntityId::new(), "NAME", owner, body)
            .build()
            .expect("a monster kind pairs with a statblock body")
    }

    /// The sections a view actually carries, read off the mirrored view.
    fn present(view: &EntityView) -> BTreeSet<ViewField> {
        let mut present = BTreeSet::new();
        if view.name.is_some() {
            present.insert(ViewField::Name);
        }
        if view.ext.is_some() {
            present.insert(ViewField::Ext);
        }
        let Some(EntityViewBody::Statblock(body)) = &view.body else {
            return present;
        };
        let prof = body.proficiencies.as_ref();
        let carriers = [
            (ViewField::Identity, body.profile.is_some()),
            (ViewField::Abilities, body.abilities.is_some()),
            (
                ViewField::Saves,
                prof.and_then(|p| p.saves.as_ref()).is_some(),
            ),
            (
                ViewField::Skills,
                prof.and_then(|p| p.skills.as_ref()).is_some(),
            ),
            (
                ViewField::Proficiencies,
                prof.is_some_and(|p| p.tools.is_some() && p.languages.is_some()),
            ),
            (ViewField::Hp, body.hp.is_some()),
            (ViewField::Speed, body.speed.is_some()),
            (ViewField::DeathSaves, body.death_saves.is_some()),
            (ViewField::Coins, body.coins.is_some()),
            (ViewField::Overrides, body.overrides.is_some()),
            (ViewField::Resources, body.resources.is_some()),
            (ViewField::Spells, body.spellcasting.is_some()),
            (ViewField::Inventory, body.inventory.is_some()),
            (ViewField::Attacks, body.attacks.is_some()),
            (ViewField::Features, body.features.is_some()),
            (ViewField::Statuses, body.statuses.is_some()),
            (ViewField::Notes, body.notes.is_some()),
        ];
        for (field, is_present) in carriers {
            if is_present {
                present.insert(field);
            }
        }
        present
    }

    #[test]
    fn campaign_view_is_member_only_and_never_carries_join_secrets() {
        let mut builder = CampaignBuilder::new("Curse of the Vault");
        let dm = builder.dm("Dungeon Master");
        let player = builder.player("Aria");
        let mut campaign = builder.build();
        campaign.players[0].token_hash = Some(TokenHash::parse("deadbeef").expect("lowercase hex"));
        campaign.players[0].last_seen = Some(UnixTime::from_seconds(1));

        let view = view_of_campaign(&campaign, player).expect("a member reads the campaign");
        assert_eq!(view.viewer, player);
        assert_eq!(view.id, campaign.id);
        assert_eq!(view.name, "Curse of the Vault");
        assert_eq!(view.default_ruleset, campaign.default_ruleset);
        assert_eq!(view.clock, campaign.clock.now_s);
        assert_eq!(
            view.roster,
            vec![
                RosterEntryView {
                    id: dm,
                    display_name: "Dungeon Master".to_owned(),
                    role: Role::Dm,
                },
                RosterEntryView {
                    id: player,
                    display_name: "Aria".to_owned(),
                    role: Role::Player,
                },
            ]
        );
        assert_eq!(
            view.settings,
            CampaignSettingsView {
                player_roll_audience_default: campaign.settings.player_roll_audience_default,
                feed_capacity: campaign.settings.feed_capacity,
                dawn_hour: campaign.settings.dawn_hour,
            }
        );

        let dm_view = view_of_campaign(&campaign, dm).expect("the DM is a member");
        assert_eq!(dm_view.viewer, dm);
        assert_eq!(dm_view.roster, view.roster);

        let json = serde_json::to_string(&view).expect("the campaign view is serializable");
        assert!(!json.contains("token_hash"));
        assert!(!json.contains("deadbeef"));
        assert!(!json.contains("last_seen"));

        let outsider = PlayerId::new();
        assert!(matches!(
            view_of_campaign(&campaign, outsider),
            Err(EngineError::Unauthorized {
                actor,
                entity: None
            }) if actor == outsider
        ));
    }

    #[test]
    fn every_single_field_mask_mirrors_exactly_that_section() {
        let (campaign, owner, other) = campaign();
        let source = marked_creature(owner);

        for field in ALL {
            let mut entity = source.clone();
            let mut mask = FieldMask::default();
            mask.insert(field);
            entity.acl = vec![AclEntry {
                player: other,
                level: AclLevel::Read,
                field_mask: Some(mask),
            }];
            let view = view_of(&campaign, &AnyEntity::from(entity), other);

            let mut hidden = all_fields();
            hidden.remove(&field);
            assert_eq!(view.hidden, hidden, "{field:?}");

            let mut expected = BTreeSet::new();
            if field != ViewField::Ac {
                expected.insert(field);
            }
            assert_eq!(present(&view), expected, "{field:?}");

            let has_body = !matches!(field, ViewField::Name | ViewField::Ac | ViewField::Ext);
            assert_eq!(view.body.is_some(), has_body, "{field:?}");
            if let Some(EntityViewBody::Statblock(body)) = &view.body {
                assert_eq!(body.exhaustion, 2, "the riders ride any body: {field:?}");
                assert!(body.inspiration, "the riders ride any body: {field:?}");
            }
        }
    }
}
