//! The Leptos browser shell.

use futures_util::StreamExt;
use gloo_net::websocket::{Message, futures::WebSocket};
use gloo_timers::future::TimeoutFuture;
use leptos::{mount::mount_to_body, prelude::*};
use localtt_core::types::{
    EntityId,
    FeedEntryId,
    HandoutContent,
    InventoryEntry,
    InventoryItem,
    PlayerId,
    RefId,
    RulesetId,
};
use localtt_protocol::{
    CampaignView,
    CommandDto,
    CommandOutcomeDto,
    ContentEntryDto,
    ContentSummaryDto,
    DerivedValueDto,
    DiagnosticsDto,
    EntityView,
    EntityViewBody,
    FeedEntryView,
    FeedPageDto,
    HealthDto,
    SheetViewDto,
    SourceInfoDto,
    StatblockView,
    StreamEventDto,
};
use wasm_bindgen_futures::spawn_local;
use web_sys::window;

use crate::{
    commands::{self, CharacterDraft, ClockUnit, HandoutDraft, HandoutKind, ItemDraft},
    format,
    http::{WebError, api_get, api_post, api_post_empty},
    url,
};

const TOKEN_KEY: &str = "localtt.token";
const DEBUG_LIMIT: usize = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DebugLevel {
    Info,
    Error,
}

impl DebugLevel {
    fn label(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Error => "error",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DebugEntry {
    seq: u64,
    level: DebugLevel,
    message: String,
}

#[derive(Clone, Default)]
struct AppState {
    token: Option<String>,
    health: Option<HealthDto>,
    campaign: Option<CampaignView>,
    entities: Vec<EntityView>,
    selected: Option<EntityId>,
    selected_view: Option<EntityView>,
    sheet: Option<SheetViewDto>,
    feed: Vec<FeedEntryView>,
    feed_trimmed_before: Option<FeedEntryId>,
    sources: Vec<SourceInfoDto>,
    search_results: Vec<ContentSummaryDto>,
    content: Option<ContentEntryDto>,
    status: String,
    stream_status: String,
    error: Option<String>,
    busy: bool,
    diagnostics: Option<DiagnosticsDto>,
    debug: Vec<DebugEntry>,
    debug_seq: u64,
    show_debug: bool,
}

impl AppState {
    fn record_debug(&mut self, level: DebugLevel, message: impl Into<String>) {
        self.debug_seq = self.debug_seq.saturating_add(1);
        if self.debug.len() >= DEBUG_LIMIT {
            self.debug.remove(0);
        }
        self.debug.push(DebugEntry {
            seq: self.debug_seq,
            level,
            message: message.into(),
        });
    }

    fn note(&mut self, message: impl Into<String>) {
        self.record_debug(DebugLevel::Info, message);
    }

    fn fail(&mut self, message: impl Into<String>) {
        let message = message.into();
        self.error = Some(message.clone());
        self.record_debug(DebugLevel::Error, message);
    }
}

/// Mounts the browser app.
pub fn start() {
    console_error_panic_hook::set_once();
    mount_to_body(App);
}

#[component]
fn App() -> impl IntoView {
    let state = RwSignal::new(AppState::default());
    let token_input = RwSignal::new(String::new());

    Effect::new(move |_| {
        if let Some(token) = stored_token() {
            start_session(state, token);
        }
    });

    let connect = Callback::new(move |()| {
        let token = token_input.get().trim().to_owned();
        if token.is_empty() {
            state.update(|state| state.fail("Enter the bearer token."));
        } else {
            start_session(state, token);
        }
    });
    let refresh = Callback::new(move |()| {
        if let Some(token) = state.get().token.clone() {
            spawn_local(load_initial(state, token));
        }
    });
    let logout = Callback::new(move |()| {
        clear_token();
        if let Some(window) = window() {
            let _ = window.location().reload();
        }
    });

    view! {
        <main class="app" data-testid="app">
            {move || {
                if state.get().token.is_some() {
                    view! { <Shell state refresh logout /> }.into_any()
                } else {
                    view! { <Login token_input connect /> }.into_any()
                }
            }}
        </main>
    }
}

#[component]
fn Login(token_input: RwSignal<String>, connect: Callback<()>) -> impl IntoView {
    view! {
        <section class="login" data-testid="login">
            <h1>"local-tt"</h1>
            <p class="muted">
                "Paste the bearer token printed by the server, or read it from the token file."
            </p>
            <form
                data-testid="login-form"
                on:submit=move |event| {
                    event.prevent_default();
                    connect.run(());
                }
            >
                <label for="token">"Bearer token"</label>
                <input
                    id="token"
                    data-testid="token-input"
                    type="password"
                    autocomplete="off"
                    spellcheck="false"
                    placeholder="64 hex characters"
                    prop:value=move || token_input.get()
                    on:input=move |event| token_input.set(event_target_value(&event))
                />
                <button type="submit" data-testid="connect-button">"Connect"</button>
            </form>
        </section>
    }
}

#[component]
fn Shell(state: RwSignal<AppState>, refresh: Callback<()>, logout: Callback<()>) -> impl IntoView {
    let campaign_name = move || {
        state
            .get()
            .campaign
            .as_ref()
            .map(|campaign| campaign.name.clone())
            .unwrap_or_else(|| "local-tt".to_owned())
    };
    let clock = move || {
        state
            .get()
            .health
            .as_ref()
            .map(|health| format::clock(health.clock))
            .unwrap_or_default()
    };
    let status = move || state.get().status.clone();
    let stream_status = move || state.get().stream_status.clone();
    let error = move || state.get().error.clone();
    let loaded = move || state.get().campaign.is_some();
    let shell_state = move || if loaded() { "ready" } else { "loading" };
    let busy = move || state.get().busy.to_string();
    let ws_state = move || state.get().stream_status.clone();
    let has_error = move || state.get().error.is_some().to_string();
    let toggle_debug = Callback::new(move |()| {
        let opening = !state.get().show_debug;
        state.update(|state| state.show_debug = opening);
        if opening && let Some(token) = state.get().token.clone() {
            spawn_local(load_diagnostics(state, token));
        }
    });

    view! {
        <header
            class="topbar"
            data-testid="shell"
            data-state=shell_state
            data-busy=busy
            data-ws-state=ws_state
            data-error=has_error
        >
            <div>
                <h1 data-testid="campaign-name">{campaign_name}</h1>
                <span class="clock" data-testid="clock">{clock}</span>
            </div>
            <div class="actions">
                <span class="stream" data-testid="stream-status">{stream_status}</span>
                <button type="button" data-testid="refresh-button" on:click=move |_| refresh.run(())>
                    "Refresh"
                </button>
                <button
                    type="button"
                    data-testid="save-button"
                    disabled=move || state.get().busy
                    on:click=move |_| save_campaign(state)
                >
                    "Save"
                </button>
                <button
                    type="button"
                    data-testid="debug-button"
                    aria-expanded=move || state.get().show_debug.to_string()
                    on:click=move |_| toggle_debug.run(())
                >
                    "Debug"
                </button>
                <button type="button" data-testid="sign-out-button" on:click=move |_| logout.run(())>
                    "Sign out"
                </button>
            </div>
        </header>
        {move || {
            error()
                .map(|message| {
                    view! { <div class="error" role="alert" data-testid="error">{message}</div> }
                        .into_any()
                })
                .unwrap_or_else(|| {
                    let status = status();
                    if status.is_empty() || status == "ready" {
                        view! { <div></div> }.into_any()
                    } else {
                        view! { <div class="status" role="status" data-testid="status">{status}</div> }
                            .into_any()
                    }
                })
        }}
        {move || state.get().show_debug.then(|| view! { <DebugPanel state /> })}
        <div class="layout">
            <aside class="panel entities" data-testid="entities-panel">
                <h2>"Characters"</h2>
                <EntityList state />
                <CreatePanel state />
            </aside>
            <section class="panel detail" data-testid="detail-panel">
                <EntityDetail state />
            </section>
            <aside class="panel side" data-testid="side-panel">
                <CommandPanel state />
                <FeedPanel state />
                <ReferencePanel state />
            </aside>
        </div>
    }
}

#[component]
fn DebugPanel(state: RwSignal<AppState>) -> impl IntoView {
    let refresh = Callback::new(move |()| {
        if let Some(token) = state.get().token.clone() {
            spawn_local(load_diagnostics(state, token));
        }
    });
    let clear = Callback::new(move |()| state.update(|state| state.debug.clear()));
    let entries = move || {
        let mut entries = state.get().debug;
        entries.reverse();
        entries
    };

    view! {
        <section class="debug" data-testid="debug-panel">
            <header class="debug-head">
                <div>
                    <h2>"Debug"</h2>
                    <p class="muted">"Server counters and the latest browser events."</p>
                </div>
                <div class="actions">
                    <button
                        type="button"
                        data-testid="diagnostics-refresh"
                        on:click=move |_| refresh.run(())
                    >
                        "Refresh diagnostics"
                    </button>
                    <button
                        type="button"
                        data-testid="debug-clear"
                        on:click=move |_| clear.run(())
                    >
                        "Clear log"
                    </button>
                </div>
            </header>
            {move || match state.get().diagnostics {
                Some(diagnostics) => {
                    view! {
                        <dl class="diagnostics" data-testid="diagnostics">
                            <div><dt>"Campaign"</dt><dd>{diagnostics.campaign_name}</dd></div>
                            <div><dt>"Clock"</dt><dd>{format::clock(diagnostics.clock)}</dd></div>
                            <div><dt>"Uptime s"</dt><dd>{diagnostics.uptime_s}</dd></div>
                            <div>
                                <dt>"Requests"</dt>
                                <dd data-testid="diagnostics-requests">{diagnostics.actor.requests}</dd>
                            </div>
                            <div><dt>"Errors"</dt><dd>{diagnostics.actor.errors}</dd></div>
                            <div><dt>"Commands"</dt><dd>{diagnostics.actor.commands}</dd></div>
                            <div><dt>"Reads"</dt><dd>{diagnostics.actor.reads}</dd></div>
                            <div><dt>"Saves"</dt><dd>{diagnostics.actor.saves}</dd></div>
                            <div><dt>"Subscribers"</dt><dd>{diagnostics.actor.subscribers}</dd></div>
                            <div>
                                <dt>"Queue"</dt>
                                <dd>{diagnostics.actor.queue_depth} " / " {diagnostics.actor.queue_capacity}</dd>
                            </div>
                        </dl>
                    }
                    .into_any()
                }
                None => {
                    view! {
                        <p class="muted" data-testid="diagnostics-empty">
                            "Diagnostics are not loaded. Start the server with --diagnostics and refresh."
                        </p>
                    }
                    .into_any()
                }
            }}
            <ol class="debug-ring" data-testid="debug-ring">
                <For
                    each=entries
                    key=|entry| entry.seq
                    children=move |entry| {
                        view! {
                            <li data-testid="debug-entry" data-level=entry.level.label()>
                                {format!("#{} [{}] {}", entry.seq, entry.level.label(), entry.message)}
                            </li>
                        }
                    }
                />
            </ol>
            {move || {
                state
                    .get()
                    .debug
                    .is_empty()
                    .then(|| view! { <p class="muted" data-testid="debug-empty">"No events yet."</p> })
            }}
        </section>
    }
}

#[component]
fn EntityList(state: RwSignal<AppState>) -> impl IntoView {
    let entries = move || state.get().entities.clone();
    view! {
        <div class="entity-list" data-testid="entity-list">
            <For
                each=entries
                key=|entity| entity.id
                children=move |entity| {
                    let id = entity.id;
                    let name = format::entity_name(&entity);
                    let kind = format::kind_label(entity.kind);
                    let selected = move || state.get().selected == Some(id);
                    view! {
                        <button
                            type="button"
                            class="entity-row"
                            data-testid="entity-row"
                            data-entity-id=id.to_string()
                            class:selected=selected
                            on:click=move |_| select_entity(state, id)
                        >
                            <span class="entity-name">{name}</span>
                            <span class="entity-kind">{kind}</span>
                        </button>
                    }
                }
            />
        </div>
        {move || {
            if state.get().entities.is_empty() {
                view! { <p class="muted" data-testid="entities-empty">"No visible entities."</p> }.into_any()
            } else {
                view! { <div></div> }.into_any()
            }
        }}
    }
}

#[component]
fn EntityDetail(state: RwSignal<AppState>) -> impl IntoView {
    move || {
        let snapshot = state.get();
        let Some(entity) = snapshot.selected_view.clone() else {
            return view! {
                <p class="empty" data-testid="detail-empty">
                    "Select a character to read its sheet."
                </p>
            }
            .into_any();
        };

        let name = format::entity_name(&entity);
        let kind = format::kind_label(entity.kind);
        let hidden = entity.hidden.len();
        let writable = entity.writable;
        let entity_id = entity.id;
        let owner = snapshot
            .campaign
            .as_ref()
            .map(|campaign| format::player_name(entity.owner, &campaign.roster))
            .unwrap_or_else(|| entity.owner.to_string());
        let body = entity.body.clone();
        let sheet = snapshot.sheet.clone();

        view! {
            <article
                class="detail-body"
                data-testid="entity-detail"
                data-entity-id=entity_id.to_string()
                data-writable=writable.to_string()
            >
                <header class="detail-head">
                    <div>
                        <h2 data-testid="entity-name">{name}</h2>
                        <p class="muted">{kind} " / " {owner}</p>
                    </div>
                    <div class="badges">
                        <span class="badge">"rev " {entity.rev}</span>
                        <span class="badge">{if writable { "writable" } else { "read-only" }}</span>
                        {if hidden > 0 {
                            view! { <span class="badge warning">{hidden} " hidden"</span> }.into_any()
                        } else {
                            view! { <span></span> }.into_any()
                        }}
                    </div>
                </header>
                {match body {
                    Some(EntityViewBody::Statblock(stats)) => {
                        view! {
                            <StatblockPanel state entity_id writable stats sheet />
                        }
                        .into_any()
                    }
                    Some(EntityViewBody::Note(note)) => {
                        view! { <pre class="content-text">{note.text}</pre> }.into_any()
                    }
                    Some(EntityViewBody::Handout(handout)) => {
                        match handout.content {
                            HandoutContent::Text { text } => {
                                view! { <pre class="content-text">{text}</pre> }.into_any()
                            }
                            HandoutContent::Item { item } => {
                                view! {
                                    <p>{format::item_name(&InventoryItem::Inline(item))}</p>
                                    <GiveHandoutForm state handout=entity_id />
                                }
                                .into_any()
                            }
                        }
                    }
                    None => view! { <p class="muted">"The body is not visible."</p> }.into_any(),
                }}
            </article>
        }
        .into_any()
    }
}

#[component]
fn StatblockPanel(
    state: RwSignal<AppState>,
    entity_id: EntityId,
    writable: bool,
    stats: StatblockView,
    sheet: Option<SheetViewDto>,
) -> impl IntoView {
    let hp = stats
        .hp
        .as_ref()
        .map(|hp| format!("{}/{}", hp.current, hp.max))
        .unwrap_or_else(|| "hidden".to_owned());
    let temp_hp = stats.hp.as_ref().map(|hp| hp.temp).unwrap_or(0);
    let ac = sheet
        .as_ref()
        .map(|sheet| format::stat_value(sheet, "ac"))
        .unwrap_or_else(|| "-".to_owned());
    let initiative = sheet
        .as_ref()
        .map(|sheet| format::stat_value(sheet, "initiative"))
        .unwrap_or_else(|| "-".to_owned());
    let speed = stats
        .speed
        .as_ref()
        .map(|speed| format!("{} ft", speed.walk_ft))
        .unwrap_or_else(|| "-".to_owned());
    let abilities = stats.abilities;
    let inventory = stats.inventory.unwrap_or_default();
    let features = stats.features.unwrap_or_default();
    let statuses = stats.statuses.unwrap_or_default();
    let resources = stats.resources.unwrap_or_default();
    let notes = stats.notes;
    let values = sheet
        .as_ref()
        .map(|sheet| {
            sheet
                .sheet
                .values
                .iter()
                .map(|(key, value)| (key.to_string(), value.clone()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let warnings = sheet
        .as_ref()
        .map(|sheet| sheet.sheet.warnings.clone())
        .unwrap_or_default();
    let slots = sheet
        .as_ref()
        .map(|sheet| {
            sheet
                .sheet
                .slots
                .spell
                .iter()
                .map(|(level, count)| (*level, *count))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let hit_dice = sheet
        .as_ref()
        .map(|sheet| {
            sheet
                .sheet
                .hit_dice
                .iter()
                .map(|(sides, count)| (*sides, *count))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let resources_sheet = sheet.clone();

    view! {
        <div class="stat-row" data-testid="stat-row">
            <div class="stat"><span>"HP"</span><strong data-testid="hp-value">{hp}</strong></div>
            <div class="stat"><span>"Temp"</span><strong data-testid="temp-hp-value">{temp_hp}</strong></div>
            <div class="stat"><span>"AC"</span><strong data-testid="ac-value">{ac}</strong></div>
            <div class="stat"><span>"Initiative"</span><strong>{initiative}</strong></div>
            <div class="stat"><span>"Speed"</span><strong>{speed}</strong></div>
            <div class="stat"><span>"Inspiration"</span><strong>{if stats.inspiration { "yes" } else { "no" }}</strong></div>
            <div class="stat"><span>"Exhaustion"</span><strong>{stats.exhaustion}</strong></div>
        </div>

        {if writable {
            view! { <ActionPanel state entity_id /> }.into_any()
        } else {
            view! { <div></div> }.into_any()
        }}

        {abilities.map(|abilities| {
            view! {
                <div class="abilities">
                    <div class="ability"><span>"STR"</span><strong>{abilities.strength}</strong><small>{sheet.as_ref().map(|sheet| format::stat_value(sheet, "ability_mod.str")).unwrap_or_else(|| "-".to_owned())}</small></div>
                    <div class="ability"><span>"DEX"</span><strong>{abilities.dexterity}</strong><small>{sheet.as_ref().map(|sheet| format::stat_value(sheet, "ability_mod.dex")).unwrap_or_else(|| "-".to_owned())}</small></div>
                    <div class="ability"><span>"CON"</span><strong>{abilities.constitution}</strong><small>{sheet.as_ref().map(|sheet| format::stat_value(sheet, "ability_mod.con")).unwrap_or_else(|| "-".to_owned())}</small></div>
                    <div class="ability"><span>"INT"</span><strong>{abilities.intelligence}</strong><small>{sheet.as_ref().map(|sheet| format::stat_value(sheet, "ability_mod.int")).unwrap_or_else(|| "-".to_owned())}</small></div>
                    <div class="ability"><span>"WIS"</span><strong>{abilities.wisdom}</strong><small>{sheet.as_ref().map(|sheet| format::stat_value(sheet, "ability_mod.wis")).unwrap_or_else(|| "-".to_owned())}</small></div>
                    <div class="ability"><span>"CHA"</span><strong>{abilities.charisma}</strong><small>{sheet.as_ref().map(|sheet| format::stat_value(sheet, "ability_mod.cha")).unwrap_or_else(|| "-".to_owned())}</small></div>
                </div>
            }
            .into_any()
        }).unwrap_or_else(|| view! { <p class="muted">"Abilities are hidden."</p> }.into_any())}

        <div class="two-col">
            <section data-testid="inventory-section">
                <h3>"Inventory"</h3>
                {if writable {
                    view! { <InventoryAddForm state entity_id /> }.into_any()
                } else {
                    view! { <div></div> }.into_any()
                }}
                <For
                    each=move || inventory.clone()
                    key=|entry| entry.id
                    children=move |entry| {
                        view! { <InventoryRow state entity_id writable entry /> }
                    }
                />
            </section>
            <section>
                <h3>"Resources"</h3>
                <For
                    each=move || resources.clone()
                    key=|resource| resource.id
                    children=move |resource| {
                        let maximum = resources_sheet
                            .as_ref()
                            .and_then(|sheet| sheet.sheet.resources.get(&resource.id))
                            .and_then(|resource| resource.max)
                            .map(|maximum| maximum.to_string())
                            .unwrap_or_else(|| "?".to_owned());
                        view! {
                            <div class="line">
                                <span>{resource.name}</span>
                                <small>{format!("{} / {}", resource.current, maximum)}</small>
                            </div>
                        }
                    }
                />
            </section>
        </div>

        <div class="two-col">
            <section>
                <h3>"Features"</h3>
                <For
                    each=move || features.clone()
                    key=|feature| feature.id
                    children=move |feature| view! { <div class="line">{feature.name}</div> }
                />
            </section>
            <section>
                <h3>"Statuses"</h3>
                <For
                    each=move || statuses.clone()
                    key=|status| status.id
                    children=move |status| view! { <div class="line">{status.name}</div> }
                />
            </section>
        </div>

        <div class="two-col">
            <section>
                <h3>"Spell slots"</h3>
                <For
                    each=move || slots.clone()
                    key=|(level, _)| *level
                    children=move |(level, count)| view! { <div class="line">{format!("Level {level}")}<small>{count}</small></div> }
                />
            </section>
            <section>
                <h3>"Hit dice"</h3>
                <For
                    each=move || hit_dice.clone()
                    key=|(sides, _)| *sides
                    children=move |(sides, count)| view! { <div class="line">{format!("d{sides}")}<small>{count}</small></div> }
                />
            </section>
        </div>

        {notes.map(|notes| view! { <section><h3>"Notes"</h3><pre class="content-text">{notes}</pre></section> }.into_any()).unwrap_or_else(|| view! { <div></div> }.into_any())}

        {if warnings.is_empty() {
            view! { <div></div> }.into_any()
        } else {
            view! {
                <section class="warnings">
                    <h3>"Warnings"</h3>
                    <For
                        each=move || {
                            warnings.clone().into_iter().enumerate().collect::<Vec<_>>()
                        }
                        key=|(index, _)| *index
                        children=move |(_, warning)| view! { <p>{warning.message}</p> }
                    />
                </section>
            }
            .into_any()
        }}

        <details class="derived">
            <summary>"Derived values"</summary>
            <For
                each=move || values.clone()
                key=|(key, _)| key.clone()
                children=move |(key, value)| view! { <ValueRow key value /> }
            />
        </details>
    }
}

#[component]
fn ValueRow(key: String, value: DerivedValueDto) -> impl IntoView {
    let effective = format::value_text(value.effective.as_ref());
    let computed = value
        .computed
        .map(|computed| computed.to_string())
        .unwrap_or_else(|| "-".to_owned());
    let breakdown = value
        .breakdown
        .iter()
        .map(format::contribution_text)
        .enumerate()
        .collect::<Vec<_>>();
    view! {
        <details class="value-row">
            <summary>
                <span>{key}</span>
                <strong>{effective}</strong>
                <small>{format!("computed {computed}")}</small>
            </summary>
            <For
                each=move || breakdown.clone()
                key=|(index, _)| *index
                children=move |(_, line)| view! { <div class="line muted">{line}</div> }
            />
        </details>
    }
}

#[component]
fn CreatePanel(state: RwSignal<AppState>) -> impl IntoView {
    view! {
        <section class="create" data-testid="create-panel">
            <h2>"Create"</h2>
            <details data-testid="character-create">
                <summary>"Character"</summary>
                <CharacterForm state />
            </details>
            <details data-testid="handout-create">
                <summary>"Handout"</summary>
                <HandoutForm state />
            </details>
        </section>
    }
}

#[component]
fn CharacterForm(state: RwSignal<AppState>) -> impl IntoView {
    let draft = RwSignal::new(CharacterDraft::default());
    let submit = move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        let Some((actor, ruleset)) = session_context(state) else {
            state.update(|state| state.fail("the campaign is not loaded"));
            return;
        };
        match draft.get().command(actor, &ruleset) {
            Ok(command) => execute_command(state, command),
            Err(message) => state.update(|state| state.fail(message)),
        }
    };

    view! {
        <form class="command-form" data-testid="character-form" on:submit=submit>
            <label>
                <span>"Name"</span>
                <input
                    type="text"
                    name="name"
                    data-testid="character-name"
                    prop:value=move || draft.get().name
                    on:input=move |event| draft.update(|draft| draft.name = event_target_value(&event))
                />
            </label>
            <label>
                <span>"Class"</span>
                <input
                    type="text"
                    name="class"
                    data-testid="character-class"
                    prop:value=move || draft.get().class
                    on:input=move |event| draft.update(|draft| draft.class = event_target_value(&event))
                />
            </label>
            <div class="field-row">
                <label>
                    <span>"Level"</span>
                    <input
                        type="number"
                        name="level"
                        data-testid="character-level"
                        min="1"
                        max="20"
                        prop:value=move || draft.get().level
                        on:input=move |event| draft.update(|draft| draft.level = event_target_value(&event))
                    />
                </label>
                <label>
                    <span>"Max HP"</span>
                    <input
                        type="number"
                        name="max_hp"
                        data-testid="character-max-hp"
                        min="1"
                        prop:value=move || draft.get().max_hp
                        on:input=move |event| draft.update(|draft| draft.max_hp = event_target_value(&event))
                    />
                </label>
            </div>
            <label>
                <span>"Abilities"</span>
                <input
                    type="text"
                    name="abilities"
                    data-testid="character-abilities"
                    prop:value=move || draft.get().abilities
                    on:input=move |event| draft.update(|draft| draft.abilities = event_target_value(&event))
                />
            </label>
            <p class="muted">"Use standard or six scores, e.g. 15,14,13,12,10,8."</p>
            <button
                type="submit"
                data-testid="create-character-button"
                disabled=move || state.get().busy
            >
                "Create character"
            </button>
        </form>
    }
}

#[component]
fn HandoutForm(state: RwSignal<AppState>) -> impl IntoView {
    let draft = RwSignal::new(HandoutDraft::default());
    let submit = move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        let Some((actor, ruleset)) = session_context(state) else {
            state.update(|state| state.fail("the campaign is not loaded"));
            return;
        };
        match draft.get().command(actor, &ruleset) {
            Ok(command) => execute_command(state, command),
            Err(message) => state.update(|state| state.fail(message)),
        }
    };

    view! {
        <form class="command-form" data-testid="handout-form" on:submit=submit>
            <label>
                <span>"Kind"</span>
                <select
                    name="kind"
                    data-testid="handout-kind"
                    prop:value=move || draft.get().kind.value()
                    on:change=move |event| {
                        if let Some(kind) = HandoutKind::parse(&event_target_value(&event)) {
                            draft.update(|draft| draft.kind = kind);
                        }
                    }
                >
                    <option value="text">"Text"</option>
                    <option value="item">"Item"</option>
                </select>
            </label>
            <label>
                <span>"Name"</span>
                <input
                    type="text"
                    name="name"
                    data-testid="handout-name"
                    prop:value=move || draft.get().name
                    on:input=move |event| draft.update(|draft| draft.name = event_target_value(&event))
                />
            </label>
            <label>
                <span>{move || if draft.get().kind == HandoutKind::Item { "Item name" } else { "Text" }}</span>
                <textarea
                    name="content"
                    data-testid="handout-content"
                    rows="3"
                    prop:value=move || draft.get().content
                    on:input=move |event| draft.update(|draft| draft.content = event_target_value(&event))
                ></textarea>
            </label>
            <button
                type="submit"
                data-testid="create-handout-button"
                disabled=move || state.get().busy
            >
                "Create handout"
            </button>
        </form>
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HpAction {
    Damage,
    Heal,
    Temp,
}

#[component]
fn ActionPanel(state: RwSignal<AppState>, entity_id: EntityId) -> impl IntoView {
    let amount = RwSignal::new("1".to_owned());
    let apply = Callback::new(move |action: HpAction| {
        let value = match commands::parse_amount(&amount.get()) {
            Ok(value) => value,
            Err(message) => {
                state.update(|state| state.fail(message));
                return;
            }
        };
        let command = match action {
            HpAction::Damage => CommandDto::ApplyDamage {
                entity: entity_id,
                amount: value,
                expected_rev: None,
            },
            HpAction::Heal => CommandDto::Heal {
                entity: entity_id,
                amount: value,
                expected_rev: None,
            },
            HpAction::Temp => CommandDto::SetTempHp {
                entity: entity_id,
                amount: value,
                expected_rev: None,
            },
        };
        execute_command(state, command);
    });

    view! {
        <div class="command-bar" data-testid="action-panel">
            <label class="amount">
                <span>"Amount"</span>
                <input
                    type="number"
                    name="hp_amount"
                    data-testid="hp-amount"
                    min="0"
                    prop:value=move || amount.get()
                    on:input=move |event| amount.set(event_target_value(&event))
                />
            </label>
            <button
                type="button"
                data-testid="damage-button"
                disabled=move || state.get().busy
                on:click=move |_| apply.run(HpAction::Damage)
            >
                "Damage"
            </button>
            <button
                type="button"
                data-testid="heal-button"
                disabled=move || state.get().busy
                on:click=move |_| apply.run(HpAction::Heal)
            >
                "Heal"
            </button>
            <button
                type="button"
                data-testid="temp-hp-button"
                disabled=move || state.get().busy
                on:click=move |_| apply.run(HpAction::Temp)
            >
                "Temp HP"
            </button>
            <span class="divider"></span>
            <button
                type="button"
                data-testid="short-rest-button"
                disabled=move || state.get().busy
                on:click=move |_| execute_command(
                    state,
                    CommandDto::ShortRest {
                        entity: entity_id,
                        advance_seconds: None,
                        expected_rev: None,
                    },
                )
            >
                "Short rest"
            </button>
            <button
                type="button"
                data-testid="long-rest-button"
                disabled=move || state.get().busy
                on:click=move |_| execute_command(
                    state,
                    CommandDto::LongRest {
                        entity: entity_id,
                        advance_seconds: None,
                        expected_rev: None,
                    },
                )
            >
                "Long rest"
            </button>
        </div>
    }
}

#[component]
fn InventoryAddForm(state: RwSignal<AppState>, entity_id: EntityId) -> impl IntoView {
    let draft = RwSignal::new(ItemDraft::default());
    let submit = move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        match draft.get().add_command(entity_id) {
            Ok(command) => execute_command(state, command),
            Err(message) => state.update(|state| state.fail(message)),
        }
    };

    view! {
        <form class="command-form inventory-add" data-testid="inventory-add-form" on:submit=submit>
            <label class="grow">
                <span>"Item"</span>
                <input
                    type="text"
                    name="item"
                    data-testid="inventory-item-name"
                    placeholder="Item name"
                    prop:value=move || draft.get().name
                    on:input=move |event| draft.update(|draft| draft.name = event_target_value(&event))
                />
            </label>
            <label class="narrow">
                <span>"Qty"</span>
                <input
                    type="number"
                    name="qty"
                    data-testid="inventory-item-qty"
                    min="0"
                    prop:value=move || draft.get().qty
                    on:input=move |event| draft.update(|draft| draft.qty = event_target_value(&event))
                />
            </label>
            <label>
                <span>"State"</span>
                <select
                    name="state"
                    data-testid="inventory-item-state"
                    prop:value=move || format::item_state_label(draft.get().state)
                    on:change=move |event| {
                        if let Some(item_state) = commands::parse_item_state(&event_target_value(&event)) {
                            draft.update(|draft| draft.state = item_state);
                        }
                    }
                >
                    <option value="carried">"Carried"</option>
                    <option value="equipped">"Equipped"</option>
                    <option value="stowed">"Stowed"</option>
                </select>
            </label>
            <button type="submit" data-testid="inventory-add-button" disabled=move || state.get().busy>
                "Add"
            </button>
        </form>
    }
}

#[component]
fn InventoryRow(
    state: RwSignal<AppState>,
    entity_id: EntityId,
    writable: bool,
    entry: InventoryEntry,
) -> impl IntoView {
    let entry_id = entry.id;
    let name = format::item_name(&entry.item);
    let qty = entry.qty;
    let state_label = format::item_state_label(entry.state);
    let notes = entry.notes.clone();
    if !writable {
        return view! {
            <div
                class="line"
                data-testid="inventory-row"
                data-entry-id=entry_id.to_string()
            >
                <span>{name}</span>
                <small>{format!("x{qty} / {state_label}")}</small>
            </div>
        }
        .into_any();
    }

    let qty_input = RwSignal::new(qty.to_string());
    let item_state = RwSignal::new(entry.state);
    let attuned = RwSignal::new(entry.attuned);
    let notes_input = RwSignal::new(notes);
    let save = move |_| {
        let qty = match commands::parse_quantity(&qty_input.get()) {
            Ok(qty) => qty,
            Err(message) => {
                state.update(|state| state.fail(message));
                return;
            }
        };
        execute_command(
            state,
            commands::edit_item_command(
                entity_id,
                entry_id,
                qty,
                item_state.get(),
                attuned.get(),
                notes_input.get(),
            ),
        );
    };
    let remove = move |_| {
        execute_command(state, commands::remove_item_command(entity_id, entry_id));
    };

    view! {
        <div
            class="inventory-row"
            data-testid="inventory-row"
            data-entry-id=entry_id.to_string()
        >
            <div class="line">
                <span>{name}</span>
                <small>{format!("x{qty} / {state_label}")}</small>
            </div>
            <details class="row-edit" data-testid="inventory-edit">
                <summary>"Edit"</summary>
                <div class="command-form inventory-edit">
                    <label class="narrow">
                        <span>"Qty"</span>
                        <input
                            type="number"
                            name="qty"
                            data-testid="inventory-edit-qty"
                            min="0"
                            prop:value=move || qty_input.get()
                            on:input=move |event| qty_input.set(event_target_value(&event))
                        />
                    </label>
                    <label>
                        <span>"State"</span>
                        <select
                            name="state"
                            data-testid="inventory-edit-state"
                            prop:value=move || format::item_state_label(item_state.get())
                            on:change=move |event| {
                                if let Some(parsed) = commands::parse_item_state(&event_target_value(&event)) {
                                    item_state.set(parsed);
                                }
                            }
                        >
                            <option value="carried">"Carried"</option>
                            <option value="equipped">"Equipped"</option>
                            <option value="stowed">"Stowed"</option>
                        </select>
                    </label>
                    <label class="check">
                        <input
                            type="checkbox"
                            name="attuned"
                            data-testid="inventory-edit-attuned"
                            prop:checked=move || attuned.get()
                            on:change=move |event| attuned.set(event_target_checked(&event))
                        />
                        <span>"Attuned"</span>
                    </label>
                    <label class="grow">
                        <span>"Notes"</span>
                        <input
                            type="text"
                            name="notes"
                            data-testid="inventory-edit-notes"
                            prop:value=move || notes_input.get()
                            on:input=move |event| notes_input.set(event_target_value(&event))
                        />
                    </label>
                    <button
                        type="button"
                        data-testid="inventory-edit-apply"
                        disabled=move || state.get().busy
                        on:click=save
                    >
                        "Apply"
                    </button>
                    <button
                        type="button"
                        class="danger"
                        data-testid="inventory-edit-remove"
                        disabled=move || state.get().busy
                        on:click=remove
                    >
                        "Remove"
                    </button>
                </div>
            </details>
        </div>
    }
    .into_any()
}

#[component]
fn GiveHandoutForm(state: RwSignal<AppState>, handout: EntityId) -> impl IntoView {
    let recipient = RwSignal::new(0usize);
    let recipients = move || {
        let snapshot = state.get();
        snapshot
            .entities
            .iter()
            .filter(|entity| {
                entity.id != handout && matches!(entity.body, Some(EntityViewBody::Statblock(_)))
            })
            .map(|entity| (entity.id, format::entity_name(entity)))
            .collect::<Vec<_>>()
    };
    let submit = move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        let list = recipients();
        let Some((recipient_id, _)) = list.get(recipient.get()).or_else(|| list.first()) else {
            state.update(|state| state.fail("there is no statblock to receive the item"));
            return;
        };
        execute_command(
            state,
            CommandDto::GiveHandout {
                handout,
                recipient: *recipient_id,
                expected_rev: None,
            },
        );
    };

    view! {
        <section class="give" data-testid="give-panel">
            <h3>"Give"</h3>
            {move || {
                let list = recipients();
                if list.is_empty() {
                    view! { <p class="muted" data-testid="give-empty">"No visible statblock can receive this item."</p> }.into_any()
                } else {
                    view! {
                        <form class="command-form inline" data-testid="give-form" on:submit=submit>
                            <select
                                name="recipient"
                                data-testid="give-recipient"
                                prop:value=move || recipient.get().to_string()
                                on:change=move |event| {
                                    if let Ok(index) = event_target_value(&event).parse() {
                                        recipient.set(index);
                                    }
                                }
                            >
                                {move || {
                                    recipients()
                                        .into_iter()
                                        .enumerate()
                                        .map(|(index, (_, name))| {
                                            view! {
                                                <option value=index.to_string()>{name}</option>
                                            }
                                        })
                                        .collect_view()
                                }}
                            </select>
                            <button
                                type="submit"
                                data-testid="give-button"
                                disabled=move || state.get().busy
                            >
                                "Give item"
                            </button>
                        </form>
                    }
                    .into_any()
                }
            }}
        </section>
    }
}

#[component]
fn CommandPanel(state: RwSignal<AppState>) -> impl IntoView {
    let expression = RwSignal::new("1d20".to_owned());
    let clock_amount = RwSignal::new("10".to_owned());
    let clock_unit = RwSignal::new(ClockUnit::Minutes);
    let roll = move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        match commands::roll_command(&expression.get(), state.get().selected) {
            Ok(command) => execute_command(state, command),
            Err(message) => state.update(|state| state.fail(message)),
        }
    };
    let advance_clock = move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        match commands::advance_clock_command(&clock_amount.get(), clock_unit.get()) {
            Ok(command) => execute_command(state, command),
            Err(message) => state.update(|state| state.fail(message)),
        }
    };
    let target = move || {
        let snapshot = state.get();
        snapshot
            .selected
            .and_then(|id| {
                snapshot
                    .entities
                    .iter()
                    .find(|entity| entity.id == id)
                    .map(format::entity_name)
            })
            .unwrap_or_else(|| "free roll".to_owned())
    };

    view! {
        <section class="commands" data-testid="command-panel">
            <h2>"Table"</h2>
            <form class="command-form" data-testid="roll-form" on:submit=roll>
                <label>
                    <span>"Roll"</span>
                    <input
                        type="text"
                        name="expression"
                        data-testid="roll-expression"
                        prop:value=move || expression.get()
                        on:input=move |event| expression.set(event_target_value(&event))
                    />
                </label>
                <p class="muted">{move || format!("for {}", target())}</p>
                <button type="submit" data-testid="roll-button" disabled=move || state.get().busy>
                    "Roll dice"
                </button>
            </form>
            <form class="command-form" data-testid="clock-form" on:submit=advance_clock>
                <span class="label">"Advance clock"</span>
                <div class="field-row">
                    <input
                        type="number"
                        name="clock_amount"
                        data-testid="clock-amount"
                        prop:value=move || clock_amount.get()
                        on:input=move |event| clock_amount.set(event_target_value(&event))
                    />
                    <select
                        name="clock_unit"
                        data-testid="clock-unit"
                        prop:value=move || clock_unit.get().value()
                        on:change=move |event| {
                            if let Some(unit) = ClockUnit::parse(&event_target_value(&event)) {
                                clock_unit.set(unit);
                            }
                        }
                    >
                        <option value="seconds">"Seconds"</option>
                        <option value="minutes">"Minutes"</option>
                        <option value="hours">"Hours"</option>
                        <option value="days">"Days"</option>
                    </select>
                    <button type="submit" data-testid="clock-advance-button" disabled=move || state.get().busy>
                        "Advance"
                    </button>
                </div>
            </form>
        </section>
    }
}

#[component]
fn FeedPanel(state: RwSignal<AppState>) -> impl IntoView {
    let lines = move || {
        let snapshot = state.get();
        let roster = snapshot
            .campaign
            .as_ref()
            .map(|campaign| campaign.roster.clone())
            .unwrap_or_default();
        snapshot
            .feed
            .iter()
            .map(|entry| (entry.id, format::feed_line(entry, &roster)))
            .collect::<Vec<_>>()
    };
    let trimmed = move || state.get().feed_trimmed_before;
    view! {
        <section class="feed" data-testid="feed-panel">
            <h2>"Feed"</h2>
            {move || {
                trimmed()
                    .map(|id| view! { <p class="muted">{format!("Earlier entries were trimmed before #{}.", id.get())}</p> }.into_any())
                    .unwrap_or_else(|| view! { <div></div> }.into_any())
            }}
            <For
                each=lines
                key=|(id, _)| *id
                children=move |(id, line)| {
                    view! { <p class="feed-line" data-feed-id=id.get().to_string()>{line}</p> }
                }
            />
        </section>
    }
}

#[component]
fn ReferencePanel(state: RwSignal<AppState>) -> impl IntoView {
    let query = RwSignal::new(String::new());
    let search = move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        let text = query.get();
        if let Some(token) = state.get().token.clone() {
            spawn_local(search_content(state, token, text));
        }
    };
    let results = move || state.get().search_results.clone();
    let content = move || state.get().content.clone();
    let sources = move || state.get().sources.clone();

    view! {
        <section class="reference" data-testid="reference-panel">
            <h2>"Reference"</h2>
            <form class="search" data-testid="reference-search-form" on:submit=search>
                <input
                    type="search"
                    name="query"
                    data-testid="reference-search"
                    placeholder="Search the SRD"
                    prop:value=move || query.get()
                    on:input=move |event| query.set(event_target_value(&event))
                />
                <button type="submit" data-testid="reference-search-button">"Go"</button>
            </form>
            <For
                each=results
                key=|hit| hit.id.clone()
                children=move |hit| {
                    let id = hit.id.clone();
                    let name = hit.name.clone();
                    view! {
                        <button
                            type="button"
                            class="hit"
                            data-testid="reference-hit"
                            on:click=move |_| open_content(state, id.clone())
                        >
                            {name}
                        </button>
                    }
                }
            />
            {move || {
                content()
                    .map(|entry| {
                        let title = entry.name.clone();
                        let kind = format!("{:?}", entry.kind).to_lowercase();
                        let description = entry.description.clone();
                        let source = entry.provenance.source.clone();
                        let attribution = entry.provenance.attribution.clone();
                        let review = entry.needs_review;
                        view! {
                            <article class="entry">
                                <h3>{title}</h3>
                                <p class="muted">{kind}</p>
                                {if review {
                                    view! { <p class="badge warning">"needs review"</p> }.into_any()
                                } else {
                                    view! { <span></span> }.into_any()
                                }}
                                <pre class="content-text">{description}</pre>
                                <p class="muted">{source} " / " {attribution}</p>
                            </article>
                        }
                        .into_any()
                    })
                    .unwrap_or_else(|| view! { <div></div> }.into_any())
            }}
            <details class="sources">
                <summary>"Sources"</summary>
                <For
                    each=sources
                    key=|source| source.namespace.to_string()
                    children=move |source| view! { <p class="muted">{source.source} " / " {source.license}</p> }
                />
            </details>
        </section>
    }
}

fn session_context(state: RwSignal<AppState>) -> Option<(PlayerId, RulesetId)> {
    state
        .get()
        .campaign
        .map(|campaign| (campaign.viewer, campaign.default_ruleset))
}

fn execute_command(state: RwSignal<AppState>, command: CommandDto) {
    if state.get().busy {
        return;
    }
    let Some(token) = state.get().token.clone() else {
        state.update(|state| state.fail("the session is not connected"));
        return;
    };
    state.update(|state| {
        state.busy = true;
        state.status = "working".to_owned();
        state.error = None;
    });
    spawn_local(async move {
        match api_post::<CommandOutcomeDto, _>("/api/v1/commands", &token, &command).await {
            Ok(outcome) => {
                if !active(state, &token) {
                    return;
                }
                let events = outcome.executed.events.len();
                let selected = apply_outcome(state, outcome);
                state.update(|state| {
                    state.busy = false;
                    state.note(format!("command applied ({events} events)"));
                });
                if let Some(id) = selected {
                    spawn_local(load_detail(state, token, id));
                }
            }
            Err(error) => {
                if active(state, &token) {
                    state.update(|state| {
                        state.busy = false;
                        state.status = "ready".to_owned();
                        state.fail(error.to_string());
                    });
                }
            }
        }
    });
}

fn apply_outcome(state: RwSignal<AppState>, outcome: CommandOutcomeDto) -> Option<EntityId> {
    let CommandOutcomeDto {
        executed,
        views,
        campaign,
        feed,
    } = outcome;
    let warnings = executed
        .warnings
        .iter()
        .map(|warning| warning.message.clone())
        .collect::<Vec<_>>();
    let mut created = Vec::new();
    state.update(|state| {
        for view in views {
            let id = view.id;
            if let Some(existing) = state.entities.iter_mut().find(|entity| entity.id == id) {
                *existing = view.clone();
            } else {
                state.entities.push(view.clone());
                created.push(id);
            }
            if state.selected == Some(id) {
                state.selected_view = Some(view);
            }
        }
        if let Some(campaign) = campaign {
            if let Some(health) = state.health.as_mut() {
                health.clock = campaign.clock;
            }
            state.campaign = Some(campaign);
        }
        merge_feed(state, feed);
        state.error = None;
        state.status = if warnings.is_empty() {
            "updated".to_owned()
        } else {
            format!("warning: {}", warnings.join(" / "))
        };
    });

    if let Some(id) = created.first().copied() {
        state.update(|state| {
            state.selected = Some(id);
            state.selected_view = None;
            state.sheet = None;
        });
        Some(id)
    } else {
        state.get().selected
    }
}

fn merge_feed(state: &mut AppState, entries: Vec<FeedEntryView>) {
    for entry in entries {
        if !state.feed.iter().any(|known| known.id == entry.id) {
            state.feed.push(entry);
        }
    }
    state.feed.sort_by_key(|entry| entry.id.get());
}

fn save_campaign(state: RwSignal<AppState>) {
    if state.get().busy {
        return;
    }
    let Some(token) = state.get().token.clone() else {
        return;
    };
    state.update(|state| {
        state.busy = true;
        state.status = "saving".to_owned();
        state.error = None;
    });
    spawn_local(async move {
        match api_post_empty("/api/v1/campaign/save", &token).await {
            Ok(()) => {
                if active(state, &token) {
                    state.update(|state| {
                        state.busy = false;
                        state.status = "saved".to_owned();
                        state.note("campaign saved");
                    });
                }
            }
            Err(error) => {
                if active(state, &token) {
                    state.update(|state| {
                        state.busy = false;
                        state.status = "ready".to_owned();
                        state.fail(error.to_string());
                    });
                }
            }
        }
    });
}

fn stored_token() -> Option<String> {
    window()
        .and_then(|window| window.local_storage().ok().flatten())
        .and_then(|storage| storage.get_item(TOKEN_KEY).ok().flatten())
}

fn store_token(token: &str) {
    if let Some(storage) = window().and_then(|window| window.local_storage().ok().flatten()) {
        let _ = storage.set_item(TOKEN_KEY, token);
    }
}

fn clear_token() {
    if let Some(storage) = window().and_then(|window| window.local_storage().ok().flatten()) {
        let _ = storage.remove_item(TOKEN_KEY);
    }
}

fn start_session(state: RwSignal<AppState>, token: String) {
    state.update(|state| {
        state.token = Some(token.clone());
        state.error = None;
        state.status = "connecting".to_owned();
        state.stream_status = "connecting".to_owned();
        state.busy = false;
        state.diagnostics = None;
        state.debug.clear();
        state.debug_seq = 0;
        state.show_debug = false;
        state.note("session starting");
    });
    store_token(&token);
    spawn_local(load_initial(state, token.clone()));
    spawn_local(watch_stream(state, token));
}

fn active(state: RwSignal<AppState>, token: &str) -> bool {
    state.get().token.as_deref() == Some(token)
}

async fn load_initial(state: RwSignal<AppState>, token: String) {
    if !active(state, &token) {
        return;
    }
    state.update(|state| state.status = "loading".to_owned());
    let result = async {
        let health = api_get::<HealthDto>("/api/v1/health", &token).await?;
        let campaign = api_get::<CampaignView>("/api/v1/campaign", &token).await?;
        let entities = api_get::<Vec<EntityView>>("/api/v1/entities", &token).await?;
        let feed = api_get::<FeedPageDto>(&url::feed_path(None, 100), &token).await?;
        Ok::<_, WebError>((health, campaign, entities, feed))
    }
    .await;

    match result {
        Ok((health, campaign, entities, feed)) => {
            if !active(state, &token) {
                return;
            }
            let previous = state.get().selected;
            let selected = previous
                .filter(|id| entities.iter().any(|entity| entity.id == *id))
                .or_else(|| entities.first().map(|entity| entity.id));
            let campaign_name = health.campaign_name.clone();
            state.update(|state| {
                state.health = Some(health);
                state.campaign = Some(campaign);
                state.entities = entities;
                state.feed = feed.entries;
                state.feed_trimmed_before = feed.trimmed_before;
                state.selected = selected;
                state.selected_view = None;
                state.sheet = None;
                state.status = "ready".to_owned();
                state.error = None;
                state.note(format!("campaign loaded: {campaign_name}"));
            });
            if let Some(id) = selected {
                load_detail(state, token.clone(), id).await;
            }
            if let Ok(sources) =
                api_get::<Vec<SourceInfoDto>>("/api/v1/reference/sources", &token).await
                && active(state, &token)
            {
                state.update(|state| state.sources = sources);
            }
        }
        Err(error) => {
            if active(state, &token) {
                state.update(|state| {
                    state.status = "offline".to_owned();
                    state.fail(error.to_string());
                });
            }
        }
    }
}

async fn load_detail(state: RwSignal<AppState>, token: String, id: EntityId) {
    let view = match api_get::<EntityView>(&url::entity_path(id), &token).await {
        Ok(view) => view,
        Err(error) => {
            if active(state, &token) {
                state.update(|state| state.fail(error.to_string()));
            }
            return;
        }
    };
    if !active(state, &token) || state.get().selected != Some(id) {
        return;
    }
    let is_statblock = matches!(view.body, Some(EntityViewBody::Statblock(_)));
    state.update(|state| {
        state.selected_view = Some(view);
        state.sheet = None;
    });
    if !is_statblock {
        return;
    }
    match api_get::<SheetViewDto>(&url::sheet_path(id), &token).await {
        Ok(sheet) => {
            if active(state, &token) && state.get().selected == Some(id) {
                state.update(|state| state.sheet = Some(sheet));
            }
        }
        Err(error) => {
            if active(state, &token) {
                state.update(|state| state.fail(error.to_string()));
            }
        }
    }
}

fn select_entity(state: RwSignal<AppState>, id: EntityId) {
    let Some(token) = state.get().token.clone() else {
        return;
    };
    state.update(|state| {
        state.selected = Some(id);
        state.selected_view = None;
        state.sheet = None;
        state.error = None;
    });
    spawn_local(load_detail(state, token, id));
}

async fn search_content(state: RwSignal<AppState>, token: String, query: String) {
    match api_get::<Vec<ContentSummaryDto>>(&url::search_path(&query, 20), &token).await {
        Ok(results) => {
            if active(state, &token) {
                state.update(|state| state.search_results = results);
            }
        }
        Err(error) => {
            if active(state, &token) {
                state.update(|state| state.fail(error.to_string()));
            }
        }
    }
}

fn open_content(state: RwSignal<AppState>, id: RefId) {
    let Some(token) = state.get().token.clone() else {
        return;
    };
    spawn_local(async move {
        match api_get::<ContentEntryDto>(&url::content_path(&id), &token).await {
            Ok(entry) => {
                if active(state, &token) {
                    state.update(|state| state.content = Some(entry));
                }
            }
            Err(error) => {
                if active(state, &token) {
                    state.update(|state| state.fail(error.to_string()));
                }
            }
        }
    });
}

async fn refresh_entities(state: RwSignal<AppState>, token: String) {
    match api_get::<Vec<EntityView>>("/api/v1/entities", &token).await {
        Ok(entities) => {
            if !active(state, &token) {
                return;
            }
            let selected = state.get().selected;
            state.update(|state| {
                state.entities = entities;
                if selected.is_some_and(|id| !state.entities.iter().any(|entity| entity.id == id)) {
                    state.selected = None;
                    state.selected_view = None;
                    state.sheet = None;
                }
            });
        }
        Err(error) => {
            if active(state, &token) {
                state.update(|state| state.fail(error.to_string()));
            }
        }
    }
}

async fn refresh_campaign(state: RwSignal<AppState>, token: String) {
    let result = async {
        let health = api_get::<HealthDto>("/api/v1/health", &token).await?;
        let campaign = api_get::<CampaignView>("/api/v1/campaign", &token).await?;
        Ok::<_, WebError>((health, campaign))
    }
    .await;
    match result {
        Ok((health, campaign)) => {
            if active(state, &token) {
                state.update(|state| {
                    state.health = Some(health);
                    state.campaign = Some(campaign);
                });
            }
        }
        Err(error) => {
            if active(state, &token) {
                state.update(|state| state.fail(error.to_string()));
            }
        }
    }
}

async fn load_diagnostics(state: RwSignal<AppState>, token: String) {
    match api_get::<DiagnosticsDto>("/api/v1/diagnostics", &token).await {
        Ok(diagnostics) => {
            if active(state, &token) {
                state.update(|state| {
                    state.diagnostics = Some(diagnostics);
                    state.note("diagnostics refreshed");
                });
            }
        }
        Err(error) => {
            if active(state, &token) {
                state.update(|state| {
                    state.diagnostics = None;
                    state.record_debug(
                        DebugLevel::Error,
                        format!("diagnostics unavailable: {error}"),
                    );
                });
            }
        }
    }
}

async fn refresh_feed(state: RwSignal<AppState>, token: String) {
    let after = state.get().feed.last().map(|entry| entry.id);
    match api_get::<FeedPageDto>(&url::feed_path(after, 100), &token).await {
        Ok(page) => {
            if active(state, &token) {
                state.update(|state| {
                    state.feed_trimmed_before = page.trimmed_before;
                    if after.is_none() {
                        state.feed = page.entries;
                    } else {
                        merge_feed(state, page.entries);
                    }
                });
            }
        }
        Err(error) => {
            if active(state, &token) {
                state.update(|state| state.fail(error.to_string()));
            }
        }
    }
}

fn handle_stream_event(state: RwSignal<AppState>, token: String, event: StreamEventDto) {
    match event {
        StreamEventDto::EntityChanged { id } => {
            spawn_local(refresh_entities(state, token.clone()));
            if state.get().selected == Some(id) {
                spawn_local(load_detail(state, token, id));
            }
        }
        StreamEventDto::Forgotten { id } => {
            spawn_local(refresh_entities(state, token.clone()));
            if state.get().selected == Some(id) {
                state.update(|state| {
                    state.selected = None;
                    state.selected_view = None;
                    state.sheet = None;
                });
            }
        }
        StreamEventDto::CampaignChanged => {
            spawn_local(refresh_campaign(state, token));
        }
        StreamEventDto::FeedAppended { .. } => {
            spawn_local(refresh_feed(state, token));
        }
        StreamEventDto::Lagged => {
            spawn_local(load_initial(state, token));
        }
        // A newer server may stream events this build does not project yet.
        _ => {}
    }
}

async fn watch_stream(state: RwSignal<AppState>, token: String) {
    loop {
        if !active(state, &token) {
            break;
        }
        let Some(window) = window() else {
            break;
        };
        let location = window.location();
        let host = location.host().unwrap_or_default();
        let secure = location
            .protocol()
            .map(|scheme| scheme == "https:")
            .unwrap_or(false);
        let url = url::websocket_url(&host, secure, &token);
        let socket = match WebSocket::open(&url) {
            Ok(socket) => socket,
            Err(error) => {
                state.update(|state| {
                    state.stream_status = format!("stream offline: {error}");
                    state.record_debug(
                        DebugLevel::Error,
                        format!("stream connection failed: {error}"),
                    );
                });
                TimeoutFuture::new(2_000).await;
                continue;
            }
        };
        state.update(|state| {
            state.stream_status = "live".to_owned();
            state.note("stream live");
        });
        let (sink, mut stream) = socket.split();
        while let Some(message) = stream.next().await {
            match message {
                Ok(Message::Text(text)) => {
                    if let Ok(event) = serde_json::from_str::<StreamEventDto>(&text) {
                        handle_stream_event(state, token.clone(), event);
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        drop(sink);
        if !active(state, &token) {
            break;
        }
        state.update(|state| {
            state.stream_status = "reconnecting".to_owned();
            state.note("stream reconnecting");
        });
        TimeoutFuture::new(2_000).await;
    }
}

#[cfg(test)]
mod tests {
    use leptos::mount::mount_to;
    use wasm_bindgen::JsCast;
    use wasm_bindgen_test::wasm_bindgen_test;

    use super::*;

    #[wasm_bindgen_test]
    fn the_debug_ring_keeps_the_latest_entries() {
        let mut state = AppState::default();
        for index in 0..DEBUG_LIMIT + 2 {
            state.note(index.to_string());
        }

        assert_eq!(state.debug.len(), DEBUG_LIMIT);
        assert_eq!(
            state.debug.first().expect("the oldest retained entry").seq,
            3
        );
        assert_eq!(
            state.debug.last().expect("the newest entry").seq,
            u64::try_from(DEBUG_LIMIT + 2).expect("the test count fits")
        );
    }

    #[wasm_bindgen_test]
    fn the_login_shell_mounts() {
        console_error_panic_hook::set_once();
        let document = window()
            .and_then(|window| window.document())
            .expect("the test needs a document");
        let body = document.body().expect("the test needs a body");
        let root = document.create_element("div").expect("a mount point");
        body.append_child(&root).expect("the mount point attaches");
        let root: web_sys::HtmlElement = root.unchecked_into();
        let _app = mount_to(root.clone(), App);
        let text = root.text_content().unwrap_or_default();
        assert!(text.contains("local-tt"));
        assert!(text.contains("Bearer token"));
        assert!(text.contains("Connect"));
    }
}
