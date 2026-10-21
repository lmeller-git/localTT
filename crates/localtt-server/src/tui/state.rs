//! The pure TUI reducer (`docs/rfcs/server.md` 3).
//!
//! `update` never performs I/O: it folds one message into the state and returns
//! the effects the task should run. The task feeds effect results back as
//! messages, so a slow save cannot freeze rendering.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use localtt_core::{
    builders::StatblockBuilder,
    types::{
        Abilities,
        AnyEntity,
        CampaignTime,
        EntityId,
        EntityKind,
        FeedEntryId,
        PlayerId,
        RefId,
        RefOrText,
        RulesetId,
    },
};
use localtt_engine::{
    authz::{CampaignView, EntityView, EntityViewBody, FeedEntryView},
    commands::Command,
    content::{ContentEntry, ContentSummary},
    services::SheetView,
};

use crate::{
    app::{CommandOutcome, ContentQuery, FeedPage, StreamEvent},
    error::AppError,
    tui::command::{ActionTarget, PromptKind, handout_command, parse},
};

/// How many feed entries the TUI keeps on screen.
const FEED_VISIBLE: usize = 200;

/// How many reference hits one TUI search asks for.
const CONTENT_SEARCH_LIMIT: usize = 50;

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
    /// A derived-sheet read completed for `id`.
    Sheet {
        /// The entity the read asked about.
        id: EntityId,
        /// The derived sheet or the refusal.
        result: Result<SheetView, AppError>,
    },
    /// A content search completed for `query`.
    ContentResults {
        /// The query that produced the hits.
        query: String,
        /// Ranked hits or the failure.
        result: Result<Vec<ContentSummary>, AppError>,
    },
    /// A content-entry read completed for `id`.
    ContentEntry {
        /// The reference the read asked about.
        id: RefId,
        /// The entry, a miss, or the failure.
        result: Result<Option<Box<ContentEntry>>, AppError>,
    },
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
    /// Derive the selected entity's sheet.
    LoadSheet {
        /// The entity to derive.
        id: EntityId,
    },
    /// Search reference content.
    SearchContent {
        /// The owned query to run.
        query: ContentQuery,
    },
    /// Load one reference entry.
    LoadContent {
        /// The entry to resolve.
        id: RefId,
    },
    /// Flush the store durability boundary.
    Save,
}

/// The small new-character form.
///
/// Every field is a single line and the cursor stays at the end of the focused
/// field; the toy aims for quick creation, not a full text editor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateForm {
    /// Field values in display order: name, class, level, max HP, abilities.
    pub fields: [String; 5],
    /// Focused field index.
    pub focus: usize,
}

/// Field positions in [`CreateForm::fields`].
const NAME_FIELD: usize = 0;
const CLASS_FIELD: usize = 1;
const LEVEL_FIELD: usize = 2;
const HP_FIELD: usize = 3;
const ABILITIES_FIELD: usize = 4;
const CREATE_FIELDS: usize = 5;

impl Default for CreateForm {
    #[inline]
    fn default() -> Self {
        Self {
            fields: [
                String::new(),
                "Fighter".to_owned(),
                "1".to_owned(),
                "10".to_owned(),
                "standard".to_owned(),
            ],
            focus: NAME_FIELD,
        }
    }
}

impl CreateForm {
    /// The human-readable label for `index`.
    #[must_use]
    #[inline]
    pub fn label(index: usize) -> &'static str {
        match index {
            NAME_FIELD => "name",
            CLASS_FIELD => "class",
            LEVEL_FIELD => "level",
            HP_FIELD => "max hp",
            ABILITIES_FIELD => "abilities",
            _ => "?",
        }
    }

    /// Builds the creation command from the current field values.
    ///
    /// # Errors
    ///
    /// A human-readable validation message; the reducer shows it in the status
    /// line and keeps the form open.
    #[inline]
    pub fn command(&self, actor: PlayerId, ruleset: &RulesetId) -> Result<Command, String> {
        let name = self.fields[NAME_FIELD].trim();
        if name.is_empty() {
            return Err("name is required".to_owned());
        }
        if name.chars().count() > 120 {
            return Err("name must be at most 120 characters".to_owned());
        }
        let class = self.fields[CLASS_FIELD].trim();
        if class.is_empty() {
            return Err("class is required".to_owned());
        }
        if class.chars().count() > 80 {
            return Err("class must be at most 80 characters".to_owned());
        }
        let level = parse_number(&self.fields[LEVEL_FIELD], "level", 1, 20)?;
        let hp = parse_number(&self.fields[HP_FIELD], "max hp", 1, 100_000)?;
        let abilities = parse_abilities(&self.fields[ABILITIES_FIELD])?;

        let mut builder = StatblockBuilder::character();
        builder.abilities(abilities).hp(hp, hp);
        builder.class_level(RefOrText::Text(class.to_owned()), level, None);
        let mut entity = builder
            .build_entity(EntityKind::Pc, EntityId::new(), name, actor)
            .map_err(|error| format!("could not build character: {error}"))?;
        entity.ruleset = ruleset.clone();
        Ok(Command::CreateEntity {
            entity: Box::new(AnyEntity::from(entity)),
        })
    }
}

fn parse_number(raw: &str, label: &str, min: i32, max: i32) -> Result<i32, String> {
    let value = raw
        .trim()
        .parse::<i32>()
        .map_err(|_| format!("{label} must be a whole number"))?;
    if !(min..=max).contains(&value) {
        return Err(format!("{label} must be between {min} and {max}"));
    }
    Ok(value)
}

fn parse_abilities(raw: &str) -> Result<Abilities, String> {
    let values = if raw.trim().eq_ignore_ascii_case("standard") {
        vec![15, 14, 13, 12, 10, 8]
    } else {
        raw.split(|character: char| character == ',' || character.is_whitespace())
            .filter(|part| !part.is_empty())
            .map(|part| {
                part.parse::<i32>()
                    .map_err(|_| "abilities must be six whole numbers".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    if values.len() != 6 {
        return Err("abilities must be six scores, for example 15,14,13,12,10,8".to_owned());
    }
    if values.iter().any(|value| !(1..=30).contains(value)) {
        return Err("ability scores must be between 1 and 30".to_owned());
    }
    Ok(Abilities {
        strength: values[0],
        dexterity: values[1],
        constitution: values[2],
        intelligence: values[3],
        wisdom: values[4],
        charisma: values[5],
    })
}

/// The small handout form: kind, document name, text or item name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandoutForm {
    /// Field values in display order: kind, name, content.
    pub fields: [String; 3],
    /// Focused field index.
    pub focus: usize,
}

/// Field positions in [`HandoutForm::fields`].
const HANDOUT_KIND_FIELD: usize = 0;
const HANDOUT_NAME_FIELD: usize = 1;
const HANDOUT_CONTENT_FIELD: usize = 2;
const HANDOUT_FIELDS: usize = 3;

impl Default for HandoutForm {
    #[inline]
    fn default() -> Self {
        Self {
            fields: ["text".to_owned(), String::new(), String::new()],
            focus: HANDOUT_NAME_FIELD,
        }
    }
}

impl HandoutForm {
    /// The human-readable label for `index`.
    #[must_use]
    #[inline]
    pub fn label(index: usize) -> &'static str {
        match index {
            HANDOUT_KIND_FIELD => "kind",
            HANDOUT_NAME_FIELD => "name",
            HANDOUT_CONTENT_FIELD => "text or item",
            _ => "?",
        }
    }

    /// Builds the creation command for the current values.
    ///
    /// # Errors
    ///
    /// A short message when a field is missing or the kind is unknown.
    #[inline]
    pub fn command(&self, actor: PlayerId, ruleset: &RulesetId) -> Result<Command, String> {
        handout_command(
            &self.fields[HANDOUT_KIND_FIELD],
            &self.fields[HANDOUT_NAME_FIELD],
            &self.fields[HANDOUT_CONTENT_FIELD],
            actor,
            ruleset,
        )
    }
}

/// The reference browser: a search line, ranked hits and one open entry.
///
/// The entry itself lives on [`TuiState::content_entry`] so the mode stays
/// cheap to clone on every key event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentBrowser {
    /// The query text; it is what the last or pending search used.
    pub query: String,
    /// True while the search line accepts characters.
    pub editing: bool,
    /// Hits in source ranking order.
    pub results: Vec<ContentSummary>,
    /// Selected hit index; clamped when results arrive.
    pub selected: usize,
    /// Detail scroll offset in lines.
    pub scroll: u16,
}

impl Default for ContentBrowser {
    #[inline]
    fn default() -> Self {
        Self {
            query: String::new(),
            editing: true,
            results: Vec::new(),
            selected: 0,
            scroll: 0,
        }
    }
}

/// The current input mode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Normal navigation and action keys.
    Normal,
    /// A one-line prompt is open.
    Prompt(Prompt),
    /// The new-character form is open.
    Create(CreateForm),
    /// The new-handout form is open.
    Handout(HandoutForm),
    /// The reference browser is open.
    Content,
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
    /// The selected statblock's derived sheet, once loaded at its revision.
    pub sheet: Option<SheetView>,
    /// The derived-sheet read currently in flight.
    pending_sheet: Option<(EntityId, u64)>,
    /// The open reference browser, when [`Mode::Content`] is active.
    pub content: Option<ContentBrowser>,
    /// The currently opened reference entry.
    pub content_entry: Option<ContentEntry>,
    /// The content-entry read currently in flight.
    pending_content: Option<RefId>,
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
            sheet: None,
            pending_sheet: None,
            content: None,
            content_entry: None,
            pending_content: None,
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
            TuiMsg::Entities(result) => match result {
                Ok(entities) => {
                    self.set_entities(entities);
                    self.sheet_effects()
                }
                Err(error) => {
                    self.status = error.to_string();
                    Vec::new()
                }
            },
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
            TuiMsg::Sheet { id, result } => self.sheet_read(id, result),
            TuiMsg::ContentResults { query, result } => self.content_results(query, result),
            TuiMsg::ContentEntry { id, result } => self.content_entry_read(id, result),
        }
    }

    /// Effects the task runs after the startup reads.
    #[must_use]
    #[inline]
    pub fn initial_effects(&mut self) -> Vec<Effect> {
        self.sheet_effects()
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
            items: Vec::new(),
            recipients: self
                .entities
                .iter()
                .filter(|view| is_statblock(view.kind))
                .map(|view| {
                    (
                        view.id,
                        view.name.clone().unwrap_or_else(|| view.id.to_string()),
                    )
                })
                .collect(),
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
            if let Some(inventory) = &body.inventory {
                target.items = inventory.iter().map(|entry| entry.id).collect();
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
        let mut created = None;
        for view in outcome.views {
            if let Some(existing) = self.entities.iter_mut().find(|entity| entity.id == view.id) {
                *existing = view;
            } else {
                created = Some(view.id);
                self.entities.push(view);
            }
        }
        sort_entities(&mut self.entities);
        self.selected = created
            .or(selected)
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
        effects.extend(self.sheet_effects());
        effects
    }

    fn sheet_read(&mut self, id: EntityId, result: Result<SheetView, AppError>) -> Vec<Effect> {
        if self.pending_sheet.is_some_and(|(pending, _)| pending == id) {
            self.pending_sheet = None;
        }
        let Some(selected) = self.selected() else {
            return Vec::new();
        };
        if selected.id != id {
            return Vec::new();
        }
        let rev = selected.rev;
        match result {
            Ok(sheet) if sheet.rev == rev => {
                self.sheet = Some(sheet);
                Vec::new()
            }
            Ok(_) => self.sheet_effects(),
            Err(error) => {
                self.sheet = None;
                self.status = error.to_string();
                Vec::new()
            }
        }
    }

    fn sheet_effects(&mut self) -> Vec<Effect> {
        let Some(entity) = self.selected() else {
            self.sheet = None;
            return Vec::new();
        };
        if !is_statblock(entity.kind) {
            self.sheet = None;
            return Vec::new();
        }
        let id = entity.id;
        let rev = entity.rev;
        if self
            .sheet
            .as_ref()
            .is_some_and(|sheet| sheet.entity == id && sheet.rev == rev)
            || self.pending_sheet == Some((id, rev))
        {
            return Vec::new();
        }
        self.sheet = None;
        self.pending_sheet = Some((id, rev));
        vec![Effect::LoadSheet { id }]
    }

    fn content_results(
        &mut self,
        query: String,
        result: Result<Vec<ContentSummary>, AppError>,
    ) -> Vec<Effect> {
        let Some(browser) = self.content.as_mut() else {
            return Vec::new();
        };
        if browser.editing || browser.query != query {
            return Vec::new();
        }
        match result {
            Ok(results) => {
                let count = results.len();
                browser.results = results;
                browser.selected = 0;
                browser.scroll = 0;
                self.content_entry = None;
                self.status = if count == 0 {
                    format!("no content matches for {query}")
                } else {
                    let noun = if count == 1 { "match" } else { "matches" };
                    format!("{count} content {noun} for {query}")
                };
            }
            Err(error) => {
                browser.results.clear();
                browser.selected = 0;
                browser.scroll = 0;
                self.content_entry = None;
                self.status = error.to_string();
            }
        }
        Vec::new()
    }

    fn content_entry_read(
        &mut self,
        id: RefId,
        result: Result<Option<Box<ContentEntry>>, AppError>,
    ) -> Vec<Effect> {
        if self.pending_content.as_ref() == Some(&id) {
            self.pending_content = None;
        }
        let Some(browser) = self.content.as_ref() else {
            return Vec::new();
        };
        if browser.editing
            || browser
                .results
                .get(browser.selected)
                .is_none_or(|hit| hit.id != id)
        {
            return Vec::new();
        }
        match result {
            Ok(Some(entry)) if entry.id == id => {
                self.status = format!("content: {}", entry.name);
                self.content_entry = Some(*entry);
                if let Some(browser) = self.content.as_mut() {
                    browser.scroll = 0;
                }
            }
            Ok(Some(_)) => self.status = "content entry mismatch".to_owned(),
            Ok(None) => self.status = format!("content entry {id} not found"),
            Err(error) => self.status = error.to_string(),
        }
        Vec::new()
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
            Mode::Create(form) => self.create_key(form, key),
            Mode::Handout(form) => self.handout_key(form, key),
            Mode::Content => self.content_key(key),
            Mode::Normal => self.normal_key(key.code),
        }
    }

    fn create_key(&mut self, mut form: CreateForm, key: KeyEvent) -> Vec<Effect> {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.status = "cancelled".to_owned();
                Vec::new()
            }
            KeyCode::Enter => {
                let Some(campaign) = self.campaign.as_ref() else {
                    self.status = "campaign not loaded yet".to_owned();
                    self.mode = Mode::Create(form);
                    return Vec::new();
                };
                match form.command(self.actor, &campaign.default_ruleset) {
                    Ok(command) => {
                        let name = form.fields[NAME_FIELD].trim().to_owned();
                        self.mode = Mode::Normal;
                        self.status = format!("creating {name}");
                        vec![Effect::Execute(command)]
                    }
                    Err(message) => {
                        self.status = message;
                        self.mode = Mode::Create(form);
                        Vec::new()
                    }
                }
            }
            KeyCode::Tab | KeyCode::Down => {
                form.focus = (form.focus + 1) % CREATE_FIELDS;
                self.mode = Mode::Create(form);
                Vec::new()
            }
            KeyCode::BackTab | KeyCode::Up => {
                form.focus = (form.focus + CREATE_FIELDS - 1) % CREATE_FIELDS;
                self.mode = Mode::Create(form);
                Vec::new()
            }
            KeyCode::Backspace => {
                form.fields[form.focus].pop();
                self.mode = Mode::Create(form);
                Vec::new()
            }
            KeyCode::Char(character)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                form.fields[form.focus].push(character);
                self.mode = Mode::Create(form);
                Vec::new()
            }
            _ => {
                self.mode = Mode::Create(form);
                Vec::new()
            }
        }
    }

    fn prompt(&mut self, kind: PromptKind) -> Vec<Effect> {
        self.mode = Mode::Prompt(Prompt {
            kind,
            buffer: String::new(),
        });
        Vec::new()
    }

    fn handout_key(&mut self, mut form: HandoutForm, key: KeyEvent) -> Vec<Effect> {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.status = "cancelled".to_owned();
                Vec::new()
            }
            KeyCode::Enter => {
                let Some(campaign) = self.campaign.as_ref() else {
                    self.status = "campaign not loaded yet".to_owned();
                    self.mode = Mode::Handout(form);
                    return Vec::new();
                };
                match form.command(self.actor, &campaign.default_ruleset) {
                    Ok(command) => {
                        let name = form.fields[HANDOUT_NAME_FIELD].trim().to_owned();
                        self.mode = Mode::Normal;
                        self.status = format!("creating handout {name}");
                        vec![Effect::Execute(command)]
                    }
                    Err(message) => {
                        self.status = message;
                        self.mode = Mode::Handout(form);
                        Vec::new()
                    }
                }
            }
            KeyCode::Tab | KeyCode::Down => {
                form.focus = (form.focus + 1) % HANDOUT_FIELDS;
                self.mode = Mode::Handout(form);
                Vec::new()
            }
            KeyCode::BackTab | KeyCode::Up => {
                form.focus = (form.focus + HANDOUT_FIELDS - 1) % HANDOUT_FIELDS;
                self.mode = Mode::Handout(form);
                Vec::new()
            }
            KeyCode::Backspace => {
                form.fields[form.focus].pop();
                self.mode = Mode::Handout(form);
                Vec::new()
            }
            KeyCode::Char(character)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                form.fields[form.focus].push(character);
                self.mode = Mode::Handout(form);
                Vec::new()
            }
            _ => {
                self.mode = Mode::Handout(form);
                Vec::new()
            }
        }
    }

    fn content_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        let Some(mut browser) = self.content.take() else {
            self.mode = Mode::Normal;
            return Vec::new();
        };
        let effects = match key.code {
            KeyCode::Esc => {
                if browser.editing || self.content_entry.is_none() {
                    self.mode = Mode::Normal;
                    self.content_entry = None;
                    self.pending_content = None;
                    self.status = "content closed".to_owned();
                } else {
                    self.content_entry = None;
                    self.pending_content = None;
                    self.status = "content results".to_owned();
                }
                Vec::new()
            }
            KeyCode::Enter if browser.editing => {
                let text = browser.query.trim().to_owned();
                if text.is_empty() {
                    self.status = "type a content name first".to_owned();
                    Vec::new()
                } else {
                    browser.query = text.clone();
                    browser.editing = false;
                    browser.results.clear();
                    browser.selected = 0;
                    browser.scroll = 0;
                    self.content_entry = None;
                    self.pending_content = None;
                    self.status = format!("searching content for {text}");
                    vec![Effect::SearchContent {
                        query: ContentQuery {
                            text,
                            kinds: Vec::new(),
                            limit: CONTENT_SEARCH_LIMIT,
                        },
                    }]
                }
            }
            KeyCode::Enter if self.content_entry.is_none() => {
                if let Some(hit) = browser.results.get(browser.selected) {
                    let id = hit.id.clone();
                    if self.pending_content.as_ref() != Some(&id) {
                        self.pending_content = Some(id.clone());
                        self.status = format!("loading {id}");
                        vec![Effect::LoadContent { id }]
                    } else {
                        Vec::new()
                    }
                } else {
                    self.status = "no content hit selected".to_owned();
                    Vec::new()
                }
            }
            KeyCode::Backspace if browser.editing => {
                browser.query.pop();
                Vec::new()
            }
            KeyCode::Char(character)
                if browser.editing
                    && !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                browser.query.push(character);
                Vec::new()
            }
            KeyCode::Char('r') if !browser.editing => {
                browser.editing = true;
                browser.results.clear();
                browser.selected = 0;
                browser.scroll = 0;
                self.content_entry = None;
                self.pending_content = None;
                self.status = "edit the query; Enter searches".to_owned();
                Vec::new()
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if self.content_entry.is_some() {
                    browser.scroll = browser.scroll.saturating_add(1);
                } else {
                    browser.selected =
                        (browser.selected + 1).min(browser.results.len().saturating_sub(1));
                }
                Vec::new()
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if self.content_entry.is_some() {
                    browser.scroll = browser.scroll.saturating_sub(1);
                } else {
                    browser.selected = browser.selected.saturating_sub(1);
                }
                Vec::new()
            }
            KeyCode::PageDown => {
                if let Some(entry) = &self.content_entry {
                    let last = u16::try_from(entry.description.lines().count().saturating_sub(1))
                        .unwrap_or(u16::MAX);
                    browser.scroll = browser.scroll.saturating_add(10).min(last);
                } else {
                    browser.selected =
                        (browser.selected + 10).min(browser.results.len().saturating_sub(1));
                }
                Vec::new()
            }
            KeyCode::PageUp => {
                if self.content_entry.is_some() {
                    browser.scroll = browser.scroll.saturating_sub(10);
                } else {
                    browser.selected = browser.selected.saturating_sub(10);
                }
                Vec::new()
            }
            KeyCode::Home => {
                if self.content_entry.is_some() {
                    browser.scroll = 0;
                } else {
                    browser.selected = 0;
                }
                Vec::new()
            }
            _ => Vec::new(),
        };
        if self.mode == Mode::Content {
            self.content = Some(browser);
        }
        effects
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
            KeyCode::Char('/') => {
                self.mode = Mode::Content;
                self.content = Some(ContentBrowser::default());
                self.content_entry = None;
                self.pending_content = None;
                self.status = "content search: type a name, Enter searches, Esc closes".to_owned();
                Vec::new()
            }
            KeyCode::Char('n') => {
                self.mode = Mode::Create(CreateForm::default());
                self.status = "new character: name, class, level, hp, abilities".to_owned();
                Vec::new()
            }
            KeyCode::Char('N') => {
                self.mode = Mode::Handout(HandoutForm::default());
                self.status = "new handout: kind text|item, name, content".to_owned();
                Vec::new()
            }
            KeyCode::Char('i') => self.prompt(PromptKind::AddItem),
            KeyCode::Char('u') => self.prompt(PromptKind::EditItem),
            KeyCode::Char('y') => self.prompt(PromptKind::RemoveItem),
            KeyCode::Char('f') => self.prompt(PromptKind::Give),
            KeyCode::Down | KeyCode::Char('j') => {
                if !self.entities.is_empty() {
                    self.selected = (self.selected + 1).min(self.entities.len() - 1);
                }
                self.sheet_effects()
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
                self.sheet_effects()
            }
            KeyCode::Char('g') => {
                self.selected = 0;
                self.sheet_effects()
            }
            KeyCode::Char('G') => {
                self.selected = self.entities.len().saturating_sub(1);
                self.sheet_effects()
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
                Some(kind) => self.prompt(kind),
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

fn is_statblock(kind: EntityKind) -> bool {
    matches!(
        kind,
        EntityKind::Pc
            | EntityKind::Companion
            | EntityKind::Summon
            | EntityKind::Familiar
            | EntityKind::Npc
            | EntityKind::Monster
    )
}

fn sort_entities(entities: &mut [EntityView]) {
    entities.sort_by_key(|entity| (entity.name.clone(), entity.id));
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use localtt_core::types::{CampaignTime, FeedEntryId, RefId};
    use localtt_engine::{
        commands::{Command, Event},
        executor::Executed,
    };

    use super::{CreateForm, Effect, Mode, TuiMsg, TuiState};
    use crate::app::{CommandOutcome, FeedPage, StreamEvent};

    fn state() -> TuiState {
        let (mut state, _id) = statblock_state();
        state.entities.clear();
        state
    }

    fn statblock_state() -> (TuiState, localtt_core::types::EntityId) {
        let mut builder = localtt_core::builders::CampaignBuilder::new("Test");
        let dm = builder.dm("DM");
        let campaign = builder.build();
        let mut character = localtt_core::builders::StatblockBuilder::character();
        character.abilities(localtt_core::types::Abilities {
            strength: 15,
            dexterity: 14,
            constitution: 13,
            intelligence: 12,
            wisdom: 10,
            charisma: 8,
        });
        character.hp(10, 10);
        let id = localtt_core::types::EntityId::new();
        let entity = localtt_core::types::AnyEntity::from(
            character
                .build_entity(localtt_core::types::EntityKind::Pc, id, "Aria", dm)
                .expect("the fixture builds"),
        );
        let view = localtt_engine::authz::view_of(&campaign, &entity, dm);
        let campaign =
            localtt_engine::authz::view_of_campaign(&campaign, dm).expect("the DM is a member");
        (
            TuiState::new(
                dm,
                campaign,
                vec![view],
                FeedPage {
                    entries: Vec::new(),
                    trimmed_before: None,
                },
            ),
            id,
        )
    }

    fn empty_sheet(id: localtt_core::types::EntityId) -> localtt_engine::services::SheetView {
        localtt_engine::services::SheetView {
            entity: id,
            rev: 0,
            sheet: localtt_engine::rules::DerivedSheet {
                values: std::collections::BTreeMap::new(),
                resources: std::collections::BTreeMap::new(),
                slots: localtt_engine::rules::SlotMaxima {
                    spell: std::collections::BTreeMap::new(),
                    pact: None,
                },
                hit_dice: std::collections::BTreeMap::new(),
                warnings: Vec::new(),
            },
        }
    }

    #[test]
    fn the_selected_statblock_sheet_loads_once_and_merges() {
        let (mut state, id) = statblock_state();
        assert_eq!(state.initial_effects(), vec![Effect::LoadSheet { id }]);
        assert_eq!(state.initial_effects(), Vec::new());

        let effects = state.update(TuiMsg::Sheet {
            id,
            result: Ok(empty_sheet(id)),
        });
        assert!(effects.is_empty());
        assert_eq!(state.sheet.as_ref().map(|sheet| sheet.entity), Some(id));
        assert_eq!(state.initial_effects(), Vec::new());
    }

    #[test]
    fn a_stale_or_foreign_sheet_read_does_not_replace_the_state() {
        let (mut state, id) = statblock_state();
        let _ = state.initial_effects();
        let foreign = localtt_core::types::EntityId::new();
        state.update(TuiMsg::Sheet {
            id: foreign,
            result: Ok(empty_sheet(foreign)),
        });
        assert!(state.sheet.is_none());

        let mut stale = empty_sheet(id);
        stale.rev = 7;
        let effects = state.update(TuiMsg::Sheet {
            id,
            result: Ok(stale),
        });
        assert!(state.sheet.is_none());
        assert_eq!(effects, vec![Effect::LoadSheet { id }]);

        state.update(TuiMsg::Sheet {
            id,
            result: Err(crate::error::AppError::ActorGone),
        });
        assert!(state.sheet.is_none());
        assert!(state.status.contains("actor"));
    }

    #[test]
    fn a_selection_move_without_a_sheet_requests_the_new_one() {
        let (mut state, _id) = statblock_state();
        let mut second = localtt_core::builders::StatblockBuilder::character();
        second.abilities(localtt_core::types::Abilities {
            strength: 10,
            dexterity: 10,
            constitution: 10,
            intelligence: 10,
            wisdom: 10,
            charisma: 10,
        });
        second.hp(10, 10);
        let second_id = localtt_core::types::EntityId::new();
        let second = localtt_core::types::AnyEntity::from(
            second
                .build_entity(
                    localtt_core::types::EntityKind::Pc,
                    second_id,
                    "Borin",
                    state.actor,
                )
                .expect("the fixture builds"),
        );
        state.entities.push(localtt_engine::authz::view_of(
            &localtt_core::builders::CampaignBuilder::new("Test").build(),
            &second,
            state.actor,
        ));

        let effects = state.update(TuiMsg::Key(key(KeyCode::Char('j'))));
        assert_eq!(effects, vec![Effect::LoadSheet { id: second_id }]);
        assert_eq!(state.selected, 1);
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

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn the_create_form_builds_an_owned_player_character() {
        let state = state();
        let mut form = CreateForm::default();
        form.fields[0] = "Aria".to_owned();
        let campaign = state.campaign.as_ref().expect("the state has a campaign");

        let command = form
            .command(state.actor, &campaign.default_ruleset)
            .expect("the default form is valid after the name");

        let Command::CreateEntity { entity } = command else {
            panic!("the form must create an entity");
        };
        let localtt_core::types::AnyEntity::Statblock(document) = *entity else {
            panic!("a player character is a statblock");
        };
        assert_eq!(document.kind(), localtt_core::types::EntityKind::Pc);
        assert_eq!(document.name, "Aria");
        assert_eq!(document.owner, state.actor);
        assert_eq!(document.rev, 0);
        assert_eq!(document.body.abilities.strength, 15);
        assert_eq!(document.body.abilities.charisma, 8);
        assert_eq!(document.body.hp.max, 10);
        assert_eq!(document.body.levels.len(), 1);
        assert_eq!(document.body.levels[0].level, 1);
    }

    #[test]
    fn the_create_form_validates_its_fields() {
        let state = state();
        let campaign = state.campaign.as_ref().expect("the state has a campaign");
        let mut form = CreateForm::default();
        assert!(
            form.command(state.actor, &campaign.default_ruleset)
                .is_err()
        );

        form.fields[0] = "Aria".to_owned();
        form.fields[4] = "15,14,13".to_owned();
        assert!(
            form.command(state.actor, &campaign.default_ruleset)
                .is_err()
        );
        form.fields[4] = "31,14,13,12,10,8".to_owned();
        assert!(
            form.command(state.actor, &campaign.default_ruleset)
                .is_err()
        );
        form.fields[4] = "15 14 13 12 10 8".to_owned();
        assert!(form.command(state.actor, &campaign.default_ruleset).is_ok());
    }

    #[test]
    fn n_opens_the_form_and_enter_emits_a_create_command() {
        let mut state = state();
        state.update(TuiMsg::Key(key(KeyCode::Char('n'))));
        assert!(matches!(state.mode, Mode::Create(_)));

        for character in "Aria".chars() {
            state.update(TuiMsg::Key(key(KeyCode::Char(character))));
        }
        let effects = state.update(TuiMsg::Key(key(KeyCode::Enter)));

        assert_eq!(state.mode, Mode::Normal);
        assert_eq!(effects.len(), 1);
        assert!(matches!(
            &effects[0],
            Effect::Execute(Command::CreateEntity { .. })
        ));
    }

    #[test]
    fn shift_n_opens_the_handout_form_and_enter_creates_one() {
        let mut state = state();
        state.update(TuiMsg::Key(key(KeyCode::Char('N'))));
        assert!(matches!(state.mode, Mode::Handout(_)));

        for character in "Loot".chars() {
            state.update(TuiMsg::Key(key(KeyCode::Char(character))));
        }
        state.update(TuiMsg::Key(key(KeyCode::Tab)));
        for character in "A map".chars() {
            state.update(TuiMsg::Key(key(KeyCode::Char(character))));
        }
        let effects = state.update(TuiMsg::Key(key(KeyCode::Enter)));

        assert_eq!(state.mode, Mode::Normal);
        assert_eq!(effects.len(), 1);
        let Effect::Execute(Command::CreateEntity { entity }) = &effects[0] else {
            panic!("the form creates an entity");
        };
        let localtt_core::types::AnyEntity::Handout(handout) = entity.as_ref() else {
            panic!("the draft is a handout");
        };
        assert_eq!(handout.name, "Loot");
        assert_eq!(
            handout.body.content,
            localtt_core::types::HandoutContent::Text {
                text: "A map".to_owned()
            }
        );
    }

    #[test]
    fn item_and_give_keys_open_their_prompts() {
        let mut state = statblock_state().0;
        for character in ['i', 'u', 'y', 'f'] {
            state.update(TuiMsg::Key(key(KeyCode::Char(character))));
            assert!(matches!(state.mode, Mode::Prompt(_)));
            state.update(TuiMsg::Key(key(KeyCode::Esc)));
        }
    }

    #[test]
    fn an_invalid_form_stays_open_and_escape_cancels() {
        let mut state = state();
        state.update(TuiMsg::Key(key(KeyCode::Char('n'))));
        state.update(TuiMsg::Key(key(KeyCode::Enter)));
        assert!(matches!(state.mode, Mode::Create(_)));
        assert!(state.status.contains("name"));

        state.update(TuiMsg::Key(key(KeyCode::Tab)));
        match &state.mode {
            Mode::Create(form) => assert_eq!(form.focus, 1),
            other => panic!("the form stays open, got {other:?}"),
        }
        state.update(TuiMsg::Key(key(KeyCode::Esc)));
        assert_eq!(state.mode, Mode::Normal);
    }

    #[test]
    fn the_reference_browser_searches_opens_and_closes() {
        let mut state = state();
        state.update(TuiMsg::Key(key(KeyCode::Char('/'))));
        assert_eq!(state.mode, Mode::Content);
        assert!(state.content.as_ref().expect("the browser opens").editing);

        for character in "fire".chars() {
            state.update(TuiMsg::Key(key(KeyCode::Char(character))));
        }
        let effects = state.update(TuiMsg::Key(key(KeyCode::Enter)));
        let Effect::SearchContent { query } = &effects[0] else {
            panic!("Enter starts a content search");
        };
        assert_eq!(query.text, "fire");
        assert_eq!(query.kinds, Vec::new());
        assert_eq!(query.limit, super::CONTENT_SEARCH_LIMIT);
        assert!(!state.content.as_ref().expect("the browser stays").editing);

        let hit = localtt_engine::content::ContentSummary {
            id: RefId::parse("fixture:spell/fire-bolt").expect("a literal reference"),
            kind: localtt_engine::content::ContentKind::Spell,
            name: "Fire Bolt".to_owned(),
        };
        state.update(TuiMsg::ContentResults {
            query: "fire".to_owned(),
            result: Ok(vec![hit.clone()]),
        });
        assert_eq!(
            state.content.as_ref().expect("the browser").results.len(),
            1
        );
        assert!(state.status.contains("1 content match"));

        let effects = state.update(TuiMsg::Key(key(KeyCode::Enter)));
        assert_eq!(effects, vec![Effect::LoadContent { id: hit.id.clone() }]);
        let entry = localtt_adapter::reference::FixtureSource::catalog()
            .into_iter()
            .find(|entry| entry.name == "Fire Bolt")
            .expect("the fixture has Fire Bolt");
        state.update(TuiMsg::ContentEntry {
            id: hit.id.clone(),
            result: Ok(Some(Box::new(entry))),
        });
        assert_eq!(
            state
                .content_entry
                .as_ref()
                .map(|entry| entry.name.as_str()),
            Some("Fire Bolt")
        );

        state.update(TuiMsg::Key(key(KeyCode::Esc)));
        assert!(state.content_entry.is_none());
        assert_eq!(state.mode, Mode::Content);
        state.update(TuiMsg::Key(key(KeyCode::Esc)));
        assert_eq!(state.mode, Mode::Normal);
        assert!(state.content.is_none());
    }

    #[test]
    fn stale_content_reads_do_not_replace_the_browser() {
        let mut state = state();
        state.update(TuiMsg::Key(key(KeyCode::Char('/'))));
        for character in "fire".chars() {
            state.update(TuiMsg::Key(key(KeyCode::Char(character))));
        }
        state.update(TuiMsg::Key(key(KeyCode::Enter)));
        let hit = localtt_engine::content::ContentSummary {
            id: RefId::parse("fixture:spell/fire-bolt").expect("a literal reference"),
            kind: localtt_engine::content::ContentKind::Spell,
            name: "Fire Bolt".to_owned(),
        };

        state.update(TuiMsg::ContentResults {
            query: "shield".to_owned(),
            result: Ok(vec![hit.clone()]),
        });
        assert!(
            state
                .content
                .as_ref()
                .expect("the browser")
                .results
                .is_empty()
        );

        state.update(TuiMsg::ContentResults {
            query: "fire".to_owned(),
            result: Ok(vec![hit.clone()]),
        });
        state.update(TuiMsg::Key(key(KeyCode::Enter)));
        let foreign = RefId::parse("fixture:spell/shield").expect("a literal reference");
        state.update(TuiMsg::ContentEntry {
            id: foreign,
            result: Ok(None),
        });
        assert!(state.content_entry.is_none());
    }

    #[test]
    fn an_empty_content_query_stays_on_the_search_line() {
        let mut state = state();
        state.update(TuiMsg::Key(key(KeyCode::Char('/'))));
        let effects = state.update(TuiMsg::Key(key(KeyCode::Enter)));
        assert!(effects.is_empty());
        assert!(state.content.as_ref().expect("the browser stays").editing);
        assert!(state.status.contains("type a content name"));
    }

    #[test]
    fn r_returns_to_the_query_and_keeps_the_text() {
        let mut state = state();
        state.update(TuiMsg::Key(key(KeyCode::Char('/'))));
        for character in "fire".chars() {
            state.update(TuiMsg::Key(key(KeyCode::Char(character))));
        }
        state.update(TuiMsg::Key(key(KeyCode::Enter)));
        state.update(TuiMsg::Key(key(KeyCode::Char('r'))));
        let browser = state.content.as_ref().expect("the browser stays");
        assert!(browser.editing);
        assert_eq!(browser.query, "fire");
        assert!(browser.results.is_empty());
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
