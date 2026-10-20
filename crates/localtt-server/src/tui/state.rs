//! The pure TUI reducer (`docs/rfcs/server.md` 3).
//!
//! `update` never performs I/O: it folds one message into the state and returns
//! the effects the task should run. The task feeds effect results back as
//! messages, so a slow save cannot freeze rendering.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use localtt_core::types::{CampaignTime, EntityId, FeedEntryId, PlayerId};
use localtt_engine::{
    authz::{CampaignView, EntityView, EntityViewBody, FeedEntryView},
    commands::Command,
};

use crate::{
    app::{CommandOutcome, FeedPage, StreamEvent},
    error::AppError,
    tui::command::{ActionTarget, PromptKind, parse},
};

/// How many feed entries the TUI keeps on screen.
const FEED_VISIBLE: usize = 200;

/// One message into the reducer.
#[derive(Debug)]
pub enum TuiMsg {
    /// A terminal key.
    Key(KeyEvent),
    /// The terminal changed size; the renderer reads the fresh size on draw.
    Resize,
    /// A campaign read completed.
    Campaign(Result<CampaignView, AppError>),
    /// An entity-list read completed.
    Entities(Result<Vec<EntityView>, AppError>),
    /// A feed read completed.
    Feed(Result<FeedPage, AppError>),
    /// A command completed.
    Executed(Result<CommandOutcome, AppError>),
    /// A save completed.
    Saved(Result<(), AppError>),
    /// A projected stream event.
    Stream(StreamEvent),
}

/// One side effect the task should run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Reload the campaign and roster.
    LoadCampaign,
    /// Reload the visible entity list.
    LoadEntities,
    /// Load one feed page after `cursor`.
    LoadFeed {
        /// The client's last seen feed id, if any.
        cursor: Option<FeedEntryId>,
    },
    /// Execute one command as the local actor.
    Execute(Command),
    /// Flush the store durability boundary.
    Save,
}

/// The current input mode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Normal navigation and action keys.
    Normal,
    /// A one-line prompt is open.
    Prompt(Prompt),
    /// The help overlay is open.
    Help,
}

/// One open prompt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prompt {
    /// The command grammar to parse on Enter.
    pub kind: PromptKind,
    /// The current buffer.
    pub buffer: String,
}

/// The whole TUI state.
pub struct TuiState {
    /// The local actor's player id.
    pub actor: PlayerId,
    /// The local actor's display name, when the roster has it.
    pub actor_name: String,
    /// The campaign header and roster.
    pub campaign: Option<CampaignView>,
    /// The entities the actor may read.
    pub entities: Vec<EntityView>,
    /// Selected index into `entities`; clamped on every list update.
    pub selected: usize,
    /// Visible feed entries, ascending id.
    pub feed: Vec<FeedEntryView>,
    /// The last feed id the client has applied.
    pub feed_cursor: Option<FeedEntryId>,
    /// One-line status/error message.
    pub status: String,
    /// Current input mode.
    pub mode: Mode,
    /// Set when the task should stop.
    pub should_quit: bool,
}

impl TuiState {
    /// Builds the initial state from the startup reads.
    #[must_use]
    #[inline]
    pub fn new(
        actor: PlayerId,
        campaign: CampaignView,
        mut entities: Vec<EntityView>,
        feed: FeedPage,
    ) -> Self {
        sort_entities(&mut entities);
        let actor_name = campaign
            .roster
            .iter()
            .find(|entry| entry.id == actor)
            .map_or_else(|| actor.to_string(), |entry| entry.display_name.clone());
        let trimmed = feed.trimmed_before;
        let mut state = Self {
            actor,
            actor_name,
            campaign: Some(campaign),
            entities,
            selected: 0,
            feed: Vec::new(),
            feed_cursor: None,
            status: "ready; ? for help".to_owned(),
            mode: Mode::Normal,
            should_quit: false,
        };
        state.merge_feed(feed.entries);
        if let Some(oldest) = trimmed {
            state.status = format!("feed gap: entries before {} were trimmed", oldest.get());
        }
        state
    }

    /// Folds one message into the state.
    #[inline]
    pub fn update(&mut self, message: TuiMsg) -> Vec<Effect> {
        match message {
            TuiMsg::Key(key) => self.key(key),
            TuiMsg::Resize => Vec::new(),
            TuiMsg::Campaign(result) => match result {
                Ok(campaign) => {
                    self.actor_name = campaign
                        .roster
                        .iter()
                        .find(|entry| entry.id == self.actor)
                        .map_or_else(
                            || self.actor.to_string(),
                            |entry| entry.display_name.clone(),
                        );
                    self.campaign = Some(campaign);
                    Vec::new()
                }
                Err(error) => {
                    self.status = error.to_string();
                    Vec::new()
                }
            },
            TuiMsg::Entities(result) => {
                match result {
                    Ok(entities) => self.set_entities(entities),
                    Err(error) => self.status = error.to_string(),
                }
                Vec::new()
            }
            TuiMsg::Feed(result) => {
                match result {
                    Ok(page) => {
                        let trimmed = page.trimmed_before;
                        self.merge_feed(page.entries);
                        if let Some(oldest) = trimmed {
                            self.status =
                                format!("feed gap: entries before {} were trimmed", oldest.get());
                        }
                    }
                    Err(error) => self.status = error.to_string(),
                }
                Vec::new()
            }
            TuiMsg::Executed(result) => match result {
                Ok(outcome) => self.apply_outcome(outcome),
                Err(error) => {
                    self.status = error.to_string();
                    Vec::new()
                }
            },
            TuiMsg::Saved(result) => {
                match result {
                    Ok(()) => self.status = "saved".to_owned(),
                    Err(error) => self.status = error.to_string(),
                }
                Vec::new()
            }
            TuiMsg::Stream(event) => self.stream(event),
        }
    }

    /// The selected entity id, if the list is non-empty.
    #[must_use]
    #[inline]
    pub fn selected_id(&self) -> Option<EntityId> {
        self.entities.get(self.selected).map(|entity| entity.id)
    }

    /// The selected entity view, if the list is non-empty.
    #[must_use]
    #[inline]
    pub fn selected(&self) -> Option<&EntityView> {
        self.entities.get(self.selected)
    }

    /// The selected entity's addressable ids, for prompt parsing.
    #[must_use]
    #[inline]
    pub fn target(&self) -> Option<ActionTarget> {
        let entity = self.selected()?;
        let mut target = ActionTarget {
            id: entity.id,
            statuses: Vec::new(),
            resources: Vec::new(),
            spells: Vec::new(),
        };
        if let Some(EntityViewBody::Statblock(body)) = &entity.body {
            if let Some(statuses) = &body.statuses {
                target.statuses = statuses.iter().map(|status| status.id).collect();
            }
            if let Some(resources) = &body.resources {
                target.resources = resources.iter().map(|resource| resource.id).collect();
            }
            if let Some(blocks) = &body.spellcasting {
                target.spells = blocks
                    .iter()
                    .flat_map(|block| block.spells.iter().map(|spell| spell.id))
                    .collect();
            }
        }
        Some(target)
    }

    /// The campaign clock, or the origin before the campaign read completes.
    #[must_use]
    #[inline]
    pub fn clock(&self) -> CampaignTime {
        self.campaign
            .as_ref()
            .map_or_else(|| CampaignTime::from_seconds(0), |campaign| campaign.clock)
    }

    /// A compact clock label for the header.
    #[must_use]
    #[inline]
    pub fn clock_text(&self) -> String {
        let clock = self.clock();
        format!("day {} {}", clock.days_since_start(), clock.time_of_day())
    }

    fn set_entities(&mut self, mut entities: Vec<EntityView>) {
        sort_entities(&mut entities);
        let selected = self.selected_id();
        self.entities = entities;
        self.selected = selected
            .and_then(|id| self.entities.iter().position(|entity| entity.id == id))
            .unwrap_or(0)
            .min(self.entities.len().saturating_sub(1));
    }

    fn merge_feed(&mut self, entries: Vec<FeedEntryView>) {
        for entry in entries {
            if !self.feed.iter().any(|known| known.id == entry.id) {
                self.feed.push(entry);
            }
        }
        self.feed.sort_by_key(|entry| entry.id);
        if self.feed.len() > FEED_VISIBLE {
            self.feed.drain(..self.feed.len() - FEED_VISIBLE);
        }
        self.feed_cursor = self.feed.last().map(|entry| entry.id);
    }

    fn apply_outcome(&mut self, outcome: CommandOutcome) -> Vec<Effect> {
        let mut effects = Vec::new();
        let selected = self.selected_id();
        for view in outcome.views {
            if let Some(existing) = self.entities.iter_mut().find(|entity| entity.id == view.id) {
                *existing = view;
            } else {
                self.entities.push(view);
            }
        }
        sort_entities(&mut self.entities);
        self.selected = selected
            .and_then(|id| self.entities.iter().position(|entity| entity.id == id))
            .unwrap_or(0)
            .min(self.entities.len().saturating_sub(1));
        if let Some(campaign) = outcome.campaign {
            self.actor_name = campaign
                .roster
                .iter()
                .find(|entry| entry.id == self.actor)
                .map_or_else(
                    || self.actor.to_string(),
                    |entry| entry.display_name.clone(),
                );
            self.campaign = Some(campaign);
        }
        self.merge_feed(outcome.feed);
        if outcome.executed.warnings.is_empty() {
            self.status = "ok".to_owned();
        } else {
            self.status = outcome
                .executed
                .warnings
                .iter()
                .map(|warning| warning.message.as_str())
                .collect::<Vec<_>>()
                .join("; ");
        }
        if outcome
            .executed
            .events
            .iter()
            .any(|event| matches!(event, localtt_engine::commands::Event::EntityRemoved { .. }))
        {
            effects.push(Effect::LoadEntities);
        }
        effects
    }

    fn stream(&mut self, event: StreamEvent) -> Vec<Effect> {
        match event {
            StreamEvent::EntityChanged(_) | StreamEvent::Forgotten(_) => {
                vec![Effect::LoadEntities]
            }
            StreamEvent::CampaignChanged => {
                vec![Effect::LoadCampaign, Effect::LoadEntities]
            }
            StreamEvent::FeedAppended(_) => vec![Effect::LoadFeed {
                cursor: self.feed_cursor,
            }],
            StreamEvent::Lagged => {
                self.status = "stream lagged; resyncing".to_owned();
                vec![
                    Effect::LoadCampaign,
                    Effect::LoadEntities,
                    Effect::LoadFeed {
                        cursor: self.feed_cursor,
                    },
                ]
            }
        }
    }

    fn key(&mut self, key: KeyEvent) -> Vec<Effect> {
        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c')) {
            self.should_quit = true;
            return Vec::new();
        }
        match self.mode.clone() {
            Mode::Help => {
                self.mode = Mode::Normal;
                Vec::new()
            }
            Mode::Prompt(mut prompt) => match key.code {
                KeyCode::Esc => {
                    self.mode = Mode::Normal;
                    self.status = "cancelled".to_owned();
                    Vec::new()
                }
                KeyCode::Enter => {
                    let input = prompt.buffer.trim().to_owned();
                    let target = self.target();
                    match parse(prompt.kind, &input, target.as_ref(), self.clock()) {
                        Ok(command) => {
                            self.mode = Mode::Normal;
                            self.status = format!("{} sent", prompt.kind.label());
                            vec![Effect::Execute(command)]
                        }
                        Err(message) => {
                            self.status = message;
                            self.mode = Mode::Prompt(prompt);
                            Vec::new()
                        }
                    }
                }
                KeyCode::Backspace => {
                    prompt.buffer.pop();
                    self.mode = Mode::Prompt(prompt);
                    Vec::new()
                }
                KeyCode::Char(character)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    prompt.buffer.push(character);
                    self.mode = Mode::Prompt(prompt);
                    Vec::new()
                }
                _ => {
                    self.mode = Mode::Prompt(prompt);
                    Vec::new()
                }
            },
            Mode::Normal => self.normal_key(key.code),
        }
    }

    fn normal_key(&mut self, code: KeyCode) -> Vec<Effect> {
        match code {
            KeyCode::Char('q') => {
                self.should_quit = true;
                Vec::new()
            }
            KeyCode::Char('?') => {
                self.mode = Mode::Help;
                Vec::new()
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if !self.entities.is_empty() {
                    self.selected = (self.selected + 1).min(self.entities.len() - 1);
                }
                Vec::new()
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
                Vec::new()
            }
            KeyCode::Char('g') => {
                self.selected = 0;
                Vec::new()
            }
            KeyCode::Char('G') => {
                self.selected = self.entities.len().saturating_sub(1);
                Vec::new()
            }
            KeyCode::Char('w') => vec![Effect::Save],
            KeyCode::Char('E') => {
                let Some(target) = self.target() else {
                    self.status = "select an entity first".to_owned();
                    return Vec::new();
                };
                vec![Effect::Execute(Command::EndConcentration {
                    entity: target.id,
                    expected_rev: None,
                })]
            }
            KeyCode::Char(character) => match prompt_for(character) {
                Some(kind) => {
                    self.mode = Mode::Prompt(Prompt {
                        kind,
                        buffer: String::new(),
                    });
                    Vec::new()
                }
                None => Vec::new(),
            },
            _ => Vec::new(),
        }
    }
}

fn prompt_for(character: char) -> Option<PromptKind> {
    match character {
        'd' => Some(PromptKind::Damage),
        'h' => Some(PromptKind::Heal),
        't' => Some(PromptKind::TempHp),
        's' => Some(PromptKind::DeathSaves),
        'r' => Some(PromptKind::Roll),
        'a' => Some(PromptKind::AddStatus),
        'x' => Some(PromptKind::RemoveStatus),
        'e' => Some(PromptKind::EditStatus),
        'p' => Some(PromptKind::Spend),
        'o' => Some(PromptKind::Restore),
        'R' => Some(PromptKind::ShortRest),
        'L' => Some(PromptKind::LongRest),
        'c' => Some(PromptKind::Cast),
        'C' => Some(PromptKind::BeginConcentration),
        'H' => Some(PromptKind::HitDie),
        'A' => Some(PromptKind::AdvanceClock),
        'S' => Some(PromptKind::SetClock),
        _ => None,
    }
}

fn sort_entities(entities: &mut [EntityView]) {
    entities.sort_by_key(|entity| (entity.name.clone(), entity.id));
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use localtt_core::types::{CampaignTime, FeedEntryId};
    use localtt_engine::{
        commands::{Command, Event},
        executor::Executed,
    };

    use super::{Effect, Mode, TuiMsg, TuiState};
    use crate::app::{CommandOutcome, FeedPage, StreamEvent};

    fn state() -> TuiState {
        let mut builder = localtt_core::builders::CampaignBuilder::new("Test");
        let dm = builder.dm("DM");
        let campaign = localtt_engine::authz::view_of_campaign(&builder.build(), dm)
            .expect("the DM is a member");
        TuiState::new(
            dm,
            campaign,
            Vec::new(),
            FeedPage {
                entries: Vec::new(),
                trimmed_before: None,
            },
        )
    }

    #[test]
    fn navigation_clamps_and_quit_sets_the_flag() {
        let mut state = state();
        let effects = state.update(TuiMsg::Key(KeyEvent::new(
            KeyCode::Char('j'),
            KeyModifiers::NONE,
        )));
        assert!(effects.is_empty());
        assert_eq!(state.selected, 0);
        state.update(TuiMsg::Key(KeyEvent::new(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
        )));
        assert!(state.should_quit);
    }

    #[test]
    fn a_prompt_sends_a_command_only_when_it_parses() {
        let mut state = state();
        state.update(TuiMsg::Key(KeyEvent::new(
            KeyCode::Char('A'),
            KeyModifiers::NONE,
        )));
        assert!(matches!(state.mode, Mode::Prompt(_)));
        for character in "60".chars() {
            state.update(TuiMsg::Key(KeyEvent::new(
                KeyCode::Char(character),
                KeyModifiers::NONE,
            )));
        }
        let effects = state.update(TuiMsg::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        )));
        assert_eq!(state.mode, Mode::Normal);
        assert_eq!(
            effects,
            vec![Effect::Execute(Command::AdvanceClock { by_seconds: 60 })]
        );
    }

    #[test]
    fn a_bad_prompt_keeps_the_prompt_open() {
        let mut state = state();
        state.update(TuiMsg::Key(KeyEvent::new(
            KeyCode::Char('d'),
            KeyModifiers::NONE,
        )));
        state.update(TuiMsg::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        )));
        assert!(matches!(state.mode, Mode::Prompt(_)));
        assert!(state.status.contains("select an entity"));
    }

    #[test]
    fn stream_events_request_the_matching_refetch() {
        let mut state = state();
        assert_eq!(
            state.update(TuiMsg::Stream(StreamEvent::CampaignChanged)),
            vec![Effect::LoadCampaign, Effect::LoadEntities]
        );
        assert_eq!(
            state.update(TuiMsg::Stream(StreamEvent::FeedAppended(FeedEntryId::new(
                1
            )))),
            vec![Effect::LoadFeed { cursor: None }]
        );
        assert!(
            state
                .update(TuiMsg::Stream(StreamEvent::Lagged))
                .contains(&Effect::LoadEntities)
        );
    }

    #[test]
    fn an_outcome_merges_views_and_surfaces_warnings() {
        let mut state = state();
        let effects = state.update(TuiMsg::Executed(Ok(CommandOutcome {
            executed: Executed {
                warnings: Vec::new(),
                events: vec![Event::EntityRemoved {
                    id: localtt_core::types::EntityId::new(),
                }],
            },
            views: Vec::new(),
            campaign: None,
            feed: Vec::new(),
        })));
        assert_eq!(effects, vec![Effect::LoadEntities]);
        assert_eq!(state.status, "ok");
    }

    #[test]
    fn help_closes_on_any_key() {
        let mut state = state();
        state.update(TuiMsg::Key(KeyEvent::new(
            KeyCode::Char('?'),
            KeyModifiers::NONE,
        )));
        assert_eq!(state.mode, Mode::Help);
        state.update(TuiMsg::Key(KeyEvent::new(
            KeyCode::Char('x'),
            KeyModifiers::NONE,
        )));
        assert_eq!(state.mode, Mode::Normal);
    }

    #[test]
    fn a_feed_page_updates_the_cursor() {
        let mut state = state();
        let entry = localtt_engine::authz::FeedEntryView {
            id: FeedEntryId::new(4),
            ts: localtt_core::types::UnixTime::from_seconds(0),
            clock_s: CampaignTime::from_seconds(0),
            actor: None,
            entity: None,
            payload: localtt_core::types::FeedPayload::Hint {
                text: "hello".to_owned(),
            },
        };
        state.update(TuiMsg::Feed(Ok(FeedPage {
            entries: vec![entry],
            trimmed_before: Some(FeedEntryId::new(3)),
        })));
        assert_eq!(state.feed_cursor, Some(FeedEntryId::new(4)));
        assert!(state.status.contains("feed gap"));
    }

    #[test]
    fn the_campaign_view_is_used_for_the_clock() {
        let mut builder = localtt_core::builders::CampaignBuilder::new("Clock");
        let dm = builder.dm("DM");
        let mut campaign = builder.build();
        campaign.clock.now_s = CampaignTime::from_seconds(90_000);
        let view = localtt_engine::authz::view_of_campaign(&campaign, dm).expect("member");
        let state = TuiState::new(
            dm,
            view,
            Vec::new(),
            FeedPage {
                entries: Vec::new(),
                trimmed_before: None,
            },
        );
        assert_eq!(state.clock_text(), "day 1 01:00:00");
        assert_eq!(state.actor, dm);
    }
}
