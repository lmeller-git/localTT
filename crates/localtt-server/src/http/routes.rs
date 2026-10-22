//! REST handlers and the WebSocket upgrade route (`docs/rfcs/server.md` 7).

use std::time::Duration;

use axum::{
    Json,
    Router,
    extract::{
        DefaultBodyLimit,
        Path,
        Query,
        State,
        rejection::{JsonRejection, PathRejection, QueryRejection},
    },
    http::{
        Method,
        StatusCode,
        header::{AUTHORIZATION, CONTENT_TYPE},
    },
    middleware,
    routing::{get, post},
};
use localtt_core::types::{EntityId, EntityKind, FeedEntryId, RefId};
use localtt_engine::{
    authz::{CampaignView, EntityView},
    content::ContentKind,
};
use serde::Deserialize;
use tower_http::cors::{AllowOrigin, CorsLayer};
use uuid::Uuid;

use super::{ApiState, ErrorResponse, MAX_BODY_BYTES, auth::authorize, ws};
use crate::{
    app::ContentQuery,
    protocol::{
        CommandDto,
        CommandOutcomeDto,
        ContentEntryDto,
        ContentSummaryDto,
        ErrorCode,
        FeedPageDto,
        HealthDto,
        HttpStatus,
        SheetViewDto,
        SourceInfoDto,
    },
};

/// Builds the REST router; the listener and tests share this exact shape.
pub(crate) fn router(state: ApiState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(state.origins.header_values()))
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([AUTHORIZATION, CONTENT_TYPE])
        .max_age(Duration::from_secs(600));

    Router::new()
        .route("/api/v1/health", get(health).options(preflight))
        .route("/api/v1/campaign", get(campaign).options(preflight))
        .route("/api/v1/campaign/save", post(save).options(preflight))
        .route("/api/v1/entities", get(entities).options(preflight))
        .route("/api/v1/entities/{id}", get(entity).options(preflight))
        .route(
            "/api/v1/entities/{id}/sheet",
            get(entity_sheet).options(preflight),
        )
        .route("/api/v1/commands", post(command).options(preflight))
        .route("/api/v1/feed", get(feed).options(preflight))
        .route(
            "/api/v1/reference/search",
            get(reference_search).options(preflight),
        )
        .route(
            "/api/v1/reference/sources",
            get(reference_sources).options(preflight),
        )
        .route(
            "/api/v1/reference/{*id}",
            get(reference_entry).options(preflight),
        )
        .route("/ws", get(ws::websocket))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .route_layer(middleware::from_fn_with_state(state.clone(), authorize))
        .layer(cors)
        .with_state(state)
}

async fn health(State(state): State<ApiState>) -> Result<Json<HealthDto>, ErrorResponse> {
    let campaign = state
        .app
        .campaign(state.actor)
        .await
        .map_err(ErrorResponse::from)?;
    Ok(Json(HealthDto::new(&campaign)))
}

async fn campaign(State(state): State<ApiState>) -> Result<Json<CampaignView>, ErrorResponse> {
    let campaign = state
        .app
        .campaign(state.actor)
        .await
        .map_err(ErrorResponse::from)?;
    Ok(Json(campaign))
}

async fn save(State(state): State<ApiState>) -> Result<StatusCode, ErrorResponse> {
    state.app.save().await.map_err(ErrorResponse::from)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn preflight() -> StatusCode {
    StatusCode::NO_CONTENT
}

#[derive(Debug, Deserialize)]
struct EntityQuery {
    kind: Option<String>,
}

async fn entities(
    State(state): State<ApiState>,
    query: Result<Query<EntityQuery>, QueryRejection>,
) -> Result<Json<Vec<EntityView>>, ErrorResponse> {
    let Query(query) = query.map_err(|_| ErrorResponse::malformed())?;
    let kind = query.kind.as_deref().map(parse_entity_kind).transpose()?;
    let entities = state
        .app
        .list_entities(state.actor)
        .await
        .map_err(ErrorResponse::from)?;
    let entities = match kind {
        Some(kind) => entities
            .into_iter()
            .filter(|entity| entity.kind == kind)
            .collect(),
        None => entities,
    };
    Ok(Json(entities))
}

async fn entity(
    State(state): State<ApiState>,
    path: Result<Path<String>, PathRejection>,
) -> Result<Json<EntityView>, ErrorResponse> {
    let Path(raw) = path.map_err(|_| ErrorResponse::malformed())?;
    let id = parse_entity_id(&raw)?;
    let view = state
        .app
        .get_entity(state.actor, id)
        .await
        .map_err(ErrorResponse::from)?;
    Ok(Json(view))
}

async fn entity_sheet(
    State(state): State<ApiState>,
    path: Result<Path<String>, PathRejection>,
) -> Result<Json<SheetViewDto>, ErrorResponse> {
    let Path(raw) = path.map_err(|_| ErrorResponse::malformed())?;
    let id = parse_entity_id(&raw)?;
    let sheet = state
        .app
        .sheet(state.actor, id)
        .await
        .map_err(ErrorResponse::from)?;
    Ok(Json(SheetViewDto::from(sheet)))
}

async fn command(
    State(state): State<ApiState>,
    payload: Result<Json<CommandDto>, JsonRejection>,
) -> Result<Json<CommandOutcomeDto>, ErrorResponse> {
    let Json(dto) = payload.map_err(json_rejection)?;
    let command = localtt_engine::commands::Command::try_from(dto).map_err(ErrorResponse::from)?;
    let outcome = state
        .app
        .execute(state.actor, command)
        .await
        .map_err(ErrorResponse::from)?;
    Ok(Json(outcome.into()))
}

#[derive(Debug, Deserialize)]
struct FeedQuery {
    after: Option<String>,
    limit: Option<usize>,
}

async fn feed(
    State(state): State<ApiState>,
    query: Result<Query<FeedQuery>, QueryRejection>,
) -> Result<Json<FeedPageDto>, ErrorResponse> {
    let Query(query) = query.map_err(|_| ErrorResponse::malformed())?;
    let after = query
        .after
        .as_deref()
        .map(parse_feed_entry_id)
        .transpose()?;
    let limit = query.limit.unwrap_or(100).min(500);
    let page = state
        .app
        .feed_after(state.actor, after, limit)
        .await
        .map_err(ErrorResponse::from)?;
    Ok(Json(FeedPageDto::from(page)))
}

#[derive(Debug, Deserialize)]
struct ReferenceSearchQuery {
    q: Option<String>,
    kind: Option<String>,
    limit: Option<usize>,
}

async fn reference_search(
    State(state): State<ApiState>,
    query: Result<Query<ReferenceSearchQuery>, QueryRejection>,
) -> Result<Json<Vec<ContentSummaryDto>>, ErrorResponse> {
    let Query(query) = query.map_err(|_| ErrorResponse::malformed())?;
    let kinds = parse_content_kinds(query.kind.as_deref())?;
    let summaries = state
        .app
        .search_content(ContentQuery {
            text: query.q.unwrap_or_default(),
            kinds,
            limit: query.limit.unwrap_or(20),
        })
        .await
        .map_err(ErrorResponse::from)?;
    Ok(Json(
        summaries.into_iter().map(ContentSummaryDto::from).collect(),
    ))
}

async fn reference_sources(
    State(state): State<ApiState>,
) -> Result<Json<Vec<SourceInfoDto>>, ErrorResponse> {
    let sources = state
        .app
        .content_sources()
        .await
        .map_err(ErrorResponse::from)?;
    Ok(Json(sources.into_iter().map(SourceInfoDto::from).collect()))
}

async fn reference_entry(
    State(state): State<ApiState>,
    path: Result<Path<String>, PathRejection>,
) -> Result<Json<ContentEntryDto>, ErrorResponse> {
    let Path(raw) = path.map_err(|_| ErrorResponse::malformed())?;
    let id = RefId::parse(&raw).map_err(|_| ErrorResponse::malformed())?;
    let entry = state
        .app
        .content_entry(&id)
        .await
        .map_err(ErrorResponse::from)?;
    let entry =
        entry.ok_or_else(|| ErrorResponse::new(HttpStatus::NotFound, ErrorCode::NotFound))?;
    Ok(Json(ContentEntryDto::from(entry)))
}

fn parse_entity_id(raw: &str) -> Result<EntityId, ErrorResponse> {
    Uuid::parse_str(raw)
        .map(EntityId::from_uuid)
        .map_err(|_| ErrorResponse::malformed())
}

fn parse_feed_entry_id(raw: &str) -> Result<FeedEntryId, ErrorResponse> {
    raw.parse::<u64>()
        .map(FeedEntryId::new)
        .map_err(|_| ErrorResponse::malformed())
}

fn parse_entity_kind(raw: &str) -> Result<EntityKind, ErrorResponse> {
    let kind = match raw {
        "pc" => EntityKind::Pc,
        "companion" => EntityKind::Companion,
        "summon" => EntityKind::Summon,
        "familiar" => EntityKind::Familiar,
        "npc" => EntityKind::Npc,
        "monster" => EntityKind::Monster,
        "note" => EntityKind::Note,
        "handout" => EntityKind::Handout,
        _ => return Err(ErrorResponse::malformed()),
    };
    Ok(kind)
}

fn parse_content_kinds(raw: Option<&str>) -> Result<Vec<ContentKind>, ErrorResponse> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    raw.split(',')
        .map(str::trim)
        .filter(|kind| !kind.is_empty())
        .map(|kind| {
            ContentKind::ALL
                .iter()
                .copied()
                .find(|candidate| candidate.as_str() == kind)
                .ok_or_else(ErrorResponse::malformed)
        })
        .collect()
}

fn json_rejection(rejection: JsonRejection) -> ErrorResponse {
    if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
        ErrorResponse::payload_too_large()
    } else {
        ErrorResponse::malformed()
    }
}
