//! The Leptos browser shell.

use futures_util::StreamExt;
use gloo_net::websocket::{Message, futures::WebSocket};
use gloo_timers::future::TimeoutFuture;
use leptos::{mount::mount_to_body, prelude::*};
use localtt_core::types::{EntityId, FeedEntryId, HandoutContent, InventoryItem, RefId};
use localtt_protocol::{
    CampaignView,
    ContentEntryDto,
    ContentSummaryDto,
    DerivedValueDto,
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
    format,
    http::{WebError, api_get},
    url,
};

const TOKEN_KEY: &str = "localtt.token";

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
            state.update(|state| state.error = Some("Enter the bearer token.".to_owned()));
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
        <main class="app">
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
        <section class="login">
            <h1>"local-tt"</h1>
            <p class="muted">
                "Paste the bearer token printed by the server, or read it from the token file."
            </p>
            <form on:submit=move |event| {
                event.prevent_default();
                connect.run(());
            }>
                <label for="token">"Bearer token"</label>
                <input
                    id="token"
                    type="password"
                    autocomplete="off"
                    spellcheck="false"
                    placeholder="64 hex characters"
                    prop:value=move || token_input.get()
                    on:input=move |event| token_input.set(event_target_value(&event))
                />
                <button type="submit">"Connect"</button>
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

    view! {
        <header class="topbar">
            <div>
                <h1>{campaign_name}</h1>
                <span class="clock">{clock}</span>
            </div>
            <div class="actions">
                <span class="stream">{stream_status}</span>
                <button type="button" on:click=move |_| refresh.run(())>"Refresh"</button>
                <button type="button" on:click=move |_| logout.run(())>"Sign out"</button>
            </div>
        </header>
        {move || {
            error()
                .map(|message| view! { <div class="error" role="alert">{message}</div> }.into_any())
                .unwrap_or_else(|| {
                    let status = status();
                    if status.is_empty() || status == "ready" {
                        view! { <div></div> }.into_any()
                    } else {
                        view! { <div class="status">{status}</div> }.into_any()
                    }
                })
        }}
        <div class="layout">
            <aside class="panel entities">
                <h2>"Characters"</h2>
                <EntityList state />
            </aside>
            <section class="panel detail">
                <EntityDetail state />
            </section>
            <aside class="panel side">
                <FeedPanel state />
                <ReferencePanel state />
            </aside>
        </div>
    }
}

#[component]
fn EntityList(state: RwSignal<AppState>) -> impl IntoView {
    let entries = move || state.get().entities.clone();
    view! {
        <div class="entity-list">
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
                view! { <p class="muted">"No visible entities."</p> }.into_any()
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
                <p class="empty">"Select a character to read its sheet."</p>
            }
            .into_any();
        };

        let name = format::entity_name(&entity);
        let kind = format::kind_label(entity.kind);
        let hidden = entity.hidden.len();
        let writable = entity.writable;
        let owner = snapshot
            .campaign
            .as_ref()
            .map(|campaign| format::player_name(entity.owner, &campaign.roster))
            .unwrap_or_else(|| entity.owner.to_string());
        let body = entity.body.clone();
        let sheet = snapshot.sheet.clone();

        view! {
            <article class="detail-body">
                <header class="detail-head">
                    <div>
                        <h2>{name}</h2>
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
                        view! { <StatblockPanel stats sheet /> }.into_any()
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
                                view! { <p>{format::item_name(&InventoryItem::Inline(item))}</p> }
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
fn StatblockPanel(stats: StatblockView, sheet: Option<SheetViewDto>) -> impl IntoView {
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
        <div class="stat-row">
            <div class="stat"><span>"HP"</span><strong>{hp}</strong></div>
            <div class="stat"><span>"Temp"</span><strong>{temp_hp}</strong></div>
            <div class="stat"><span>"AC"</span><strong>{ac}</strong></div>
            <div class="stat"><span>"Initiative"</span><strong>{initiative}</strong></div>
            <div class="stat"><span>"Speed"</span><strong>{speed}</strong></div>
            <div class="stat"><span>"Inspiration"</span><strong>{if stats.inspiration { "yes" } else { "no" }}</strong></div>
            <div class="stat"><span>"Exhaustion"</span><strong>{stats.exhaustion}</strong></div>
        </div>

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
            <section>
                <h3>"Inventory"</h3>
                <For
                    each=move || inventory.clone()
                    key=|entry| entry.id
                    children=move |entry| {
                        let name = format::item_name(&entry.item);
                        view! {
                            <div class="line">
                                <span>{name}</span>
                                <small>{format!("x{} / {:?}", entry.qty, entry.state)}</small>
                            </div>
                        }
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
        <section class="feed">
            <h2>"Feed"</h2>
            {move || {
                trimmed()
                    .map(|id| view! { <p class="muted">{format!("Earlier entries were trimmed before #{}.", id.get())}</p> }.into_any())
                    .unwrap_or_else(|| view! { <div></div> }.into_any())
            }}
            <For
                each=lines
                key=|(id, _)| *id
                children=move |(_, line)| view! { <p class="feed-line">{line}</p> }
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
        <section class="reference">
            <h2>"Reference"</h2>
            <form class="search" on:submit=search>
                <input
                    type="search"
                    placeholder="Search the SRD"
                    prop:value=move || query.get()
                    on:input=move |event| query.set(event_target_value(&event))
                />
                <button type="submit">"Go"</button>
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
                    state.error = Some(error.to_string());
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
                state.update(|state| state.error = Some(error.to_string()));
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
                state.update(|state| state.error = Some(error.to_string()));
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
                state.update(|state| state.error = Some(error.to_string()));
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
                    state.update(|state| state.error = Some(error.to_string()));
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
                state.update(|state| state.error = Some(error.to_string()));
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
                state.update(|state| state.error = Some(error.to_string()));
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
                        for entry in page.entries {
                            if !state.feed.iter().any(|known| known.id == entry.id) {
                                state.feed.push(entry);
                            }
                        }
                        state.feed.sort_by_key(|entry| entry.id.get());
                    }
                });
            }
        }
        Err(error) => {
            if active(state, &token) {
                state.update(|state| state.error = Some(error.to_string()));
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
                });
                TimeoutFuture::new(2_000).await;
                continue;
            }
        };
        state.update(|state| state.stream_status = "live".to_owned());
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
        state.update(|state| state.stream_status = "reconnecting".to_owned());
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
