//! Conversions between the engine's `view` types and their DTOs.

use localtt_engine::authz::{
    CampaignSettingsView,
    CampaignView,
    EntityView,
    EntityViewBody,
    FeedEntryView,
    HandoutView,
    InitiativeEntryView,
    InitiativeView,
    NoteView,
    ProficienciesView,
    RosterAdminEntry,
    RosterEntryView,
    StatblockView,
};

use crate::{
    CampaignSettingsViewDto,
    CampaignViewDto,
    EntityViewBodyDto,
    EntityViewDto,
    FeedEntryViewDto,
    HandoutViewDto,
    InitiativeEntryViewDto,
    InitiativeViewDto,
    NoteViewDto,
    ProficienciesViewDto,
    RosterAdminEntryDto,
    RosterEntryViewDto,
    StatblockViewDto,
};

impl From<EntityView> for EntityViewDto {
    #[inline]
    fn from(view: EntityView) -> Self {
        Self {
            id: view.id,
            kind: view.kind,
            owner: view.owner,
            parent: view.parent,
            rev: view.rev,
            writable: view.writable,
            hidden: view.hidden,
            name: view.name,
            ext: view.ext,
            body: view.body.map(EntityViewBodyDto::from),
        }
    }
}

impl From<EntityViewBody> for EntityViewBodyDto {
    #[inline]
    fn from(body: EntityViewBody) -> Self {
        match body {
            EntityViewBody::Statblock(view) => Self::Statblock(view.into()),
            EntityViewBody::Note(view) => Self::Note(view.into()),
            EntityViewBody::Handout(view) => Self::Handout(view.into()),
        }
    }
}

impl From<StatblockView> for StatblockViewDto {
    #[inline]
    fn from(view: StatblockView) -> Self {
        Self {
            profile: view.profile,
            identity: view.identity,
            levels: view.levels,
            cr: view.cr,
            creature_type: view.creature_type,
            size: view.size,
            senses: view.senses,
            abilities: view.abilities,
            proficiencies: view.proficiencies.map(ProficienciesViewDto::from),
            hp: view.hp,
            speed: view.speed,
            death_saves: view.death_saves,
            coins: view.coins,
            overrides: view.overrides,
            resources: view.resources,
            spellcasting: view.spellcasting,
            inventory: view.inventory,
            attacks: view.attacks,
            features: view.features,
            statuses: view.statuses,
            notes: view.notes,
            exhaustion: view.exhaustion,
            inspiration: view.inspiration,
        }
    }
}

impl From<ProficienciesView> for ProficienciesViewDto {
    #[inline]
    fn from(view: ProficienciesView) -> Self {
        Self {
            saves: view.saves,
            skills: view.skills,
            tools: view.tools,
            languages: view.languages,
        }
    }
}

impl From<NoteView> for NoteViewDto {
    #[inline]
    fn from(view: NoteView) -> Self {
        Self { text: view.text }
    }
}

impl From<HandoutView> for HandoutViewDto {
    #[inline]
    fn from(view: HandoutView) -> Self {
        Self {
            content: view.content,
        }
    }
}

impl From<FeedEntryView> for FeedEntryViewDto {
    #[inline]
    fn from(view: FeedEntryView) -> Self {
        Self {
            id: view.id,
            ts: view.ts,
            clock_s: view.clock_s,
            actor: view.actor,
            entity: view.entity,
            payload: view.payload,
        }
    }
}

impl From<RosterEntryView> for RosterEntryViewDto {
    #[inline]
    fn from(view: RosterEntryView) -> Self {
        Self {
            id: view.id,
            display_name: view.display_name,
            role: view.role,
        }
    }
}

impl From<RosterAdminEntry> for RosterAdminEntryDto {
    #[inline]
    fn from(entry: RosterAdminEntry) -> Self {
        Self {
            id: entry.id,
            display_name: entry.display_name,
            role: entry.role,
            has_invite: entry.has_invite,
        }
    }
}

impl From<CampaignSettingsView> for CampaignSettingsViewDto {
    #[inline]
    fn from(view: CampaignSettingsView) -> Self {
        Self {
            player_roll_audience_default: view.player_roll_audience_default,
            feed_capacity: view.feed_capacity,
            dawn_hour: view.dawn_hour,
        }
    }
}

impl From<InitiativeEntryView> for InitiativeEntryViewDto {
    #[inline]
    fn from(view: InitiativeEntryView) -> Self {
        Self {
            entity: view.entity,
            name: view.name,
            owner: view.owner,
            value: view.value,
        }
    }
}

impl From<InitiativeView> for InitiativeViewDto {
    #[inline]
    fn from(view: InitiativeView) -> Self {
        Self {
            entries: view
                .entries
                .into_iter()
                .map(InitiativeEntryViewDto::from)
                .collect(),
            current: view.current,
            round: view.round,
        }
    }
}

impl From<CampaignView> for CampaignViewDto {
    #[inline]
    fn from(view: CampaignView) -> Self {
        Self {
            viewer: view.viewer,
            id: view.id,
            name: view.name,
            default_ruleset: view.default_ruleset,
            clock: view.clock,
            settings: view.settings.into(),
            initiative: view.initiative.into(),
            roster: view
                .roster
                .into_iter()
                .map(RosterEntryViewDto::from)
                .collect(),
        }
    }
}
