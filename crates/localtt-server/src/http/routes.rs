//! REST handlers and the WebSocket upgrade route (`docs/rfcs/server.md` 7).

use std::{
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    time::Duration,
};

use axum::{
    Json,
    Router,
    extract::{
        DefaultBodyLimit,
        Extension,
        Path,
        Query,
        State,
        rejection::{JsonRejection, PathRejection, QueryRejection},
    },
    http::{
        HeaderMap,
        HeaderName,
        HeaderValue,
        Method,
        StatusCode,
        header::{
            AUTHORIZATION,
            CACHE_CONTROL,
            CONTENT_SECURITY_POLICY,
            CONTENT_TYPE,
            HOST,
            ORIGIN,
            REFERRER_POLICY,
            X_CONTENT_TYPE_OPTIONS,
            X_FRAME_OPTIONS,
        },
    },
    middleware,
    routing::{any, delete, get, post},
};
use localtt_core::types::{EntityId, EntityKind, FeedEntryId, PlayerId, RefId, Role};
use localtt_engine::{
    authz::{CampaignView, EntityView},
    commands::Command,
    content::ContentKind,
};
use serde::Deserialize;
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    services::{ServeDir, ServeFile},
    set_header::SetResponseHeaderLayer,
};
use uuid::Uuid;

use super::{
    ApiState,
    ErrorResponse,
    MAX_BODY_BYTES,
    ServerToken,
    auth::authorize,
    request_id::{REQUEST_ID_HEADER, correlate},
    url_host,
    ws,
};
use crate::{
    app::ContentQuery,
    identity::Viewer,
    protocol::{
        CommandDto,
        CommandOutcomeDto,
        ContentEntryDto,
        ContentSummaryDto,
        CreateRosterEntryRequest,
        DiagnosticsDto,
        ErrorCode,
        FeedPageDto,
        HealthDto,
        HttpStatus,
        InviteDto,
        RosterAdminEntry,
        SheetViewDto,
        SourceInfoDto,
    },
};

/// Builds the authenticated API router; the listener and tests share this shape.
pub(crate) fn router(state: ApiState) -> Router {
    api_router(state)
}

/// Builds the API router plus the unauthenticated browser shell and assets.
///
/// API and WebSocket routes stay behind the auth middleware. Unknown `/api`
/// paths are answered as JSON before the SPA fallback can return `index.html`.
pub(crate) fn router_with_web(state: ApiState, web_dir: PathBuf) -> Router {
    let index = web_dir.join("index.html");
    let assets = ServeDir::new(web_dir).fallback(ServeFile::new(index));
    let csp = content_security_policy(&state);

    api_router(state)
        .fallback_service(assets)
        .layer(SetResponseHeaderLayer::overriding(
            CACHE_CONTROL,
            HeaderValue::from_static("no-store"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            REFERRER_POLICY,
            HeaderValue::from_static("no-referrer"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            CONTENT_SECURITY_POLICY,
            csp,
        ))
}

/// The static-asset CSP with every enabled WebSocket origin.
pub(super) fn content_security_policy(state: &ApiState) -> HeaderValue {
    let mut connect = String::from("'self' ws://127.0.0.1:* ws://localhost:* ws://[::1]:*");
    for ip in &state.lan_ips {
        connect.push_str(" ws://");
        connect.push_str(&url_host(*ip));
        connect.push_str(":*");
    }
    if let Some(host) = &state.lan_host {
        let host = host
            .rsplit_once(':')
            .map_or(host.as_str(), |(host, _)| host);
        connect.push_str(" ws://");
        connect.push_str(host);
        connect.push_str(":*");
    }
    HeaderValue::from_str(&format!(
        "default-src 'self'; script-src 'self' 'unsafe-inline' \
         'wasm-unsafe-eval'; style-src 'self'; img-src 'self' data:; \
         font-src 'self'; connect-src {connect}; base-uri 'none'; \
         form-action 'self'; frame-ancestors 'none'"
    ))
    .expect("the generated CSP is a valid header value")
}

fn api_router(state: ApiState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(state.origins.header_values()))
        .allow_methods([Method::GET, Method::POST, Method::DELETE, Method::OPTIONS])
        .allow_headers([AUTHORIZATION, CONTENT_TYPE])
        .expose_headers([HeaderName::from_static(REQUEST_ID_HEADER)])
        .max_age(Duration::from_secs(600));

    Router::new()
        .route("/api/v1/health", get(health).options(preflight))
        .route("/api/v1/diagnostics", get(diagnostics).options(preflight))
        .route("/api/v1/campaign", get(campaign).options(preflight))
        .route("/api/v1/campaign/save", post(save).options(preflight))
        .route(
            "/api/v1/roster",
            get(roster).post(create_roster_entry).options(preflight),
        )
        .route(
            "/api/v1/roster/{id}/invite",
            post(reissue_invite)
                .delete(revoke_invite)
                .options(preflight),
        )
        .route(
            "/api/v1/roster/{id}",
            delete(remove_roster_entry).options(preflight),
        )
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
        .route("/api", any(api_not_found))
        .route("/api/{*rest}", any(api_not_found))
        .route("/ws", get(ws::websocket))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .route_layer(middleware::from_fn_with_state(state.clone(), authorize))
        .layer(cors)
        .layer(middleware::from_fn(correlate))
        .with_state(state)
}

async fn api_not_found() -> ErrorResponse {
    ErrorResponse::new(HttpStatus::NotFound, ErrorCode::NotFound)
}

async fn health(
    State(state): State<ApiState>,
    Extension(viewer): Extension<Viewer>,
) -> Result<Json<HealthDto>, ErrorResponse> {
    let campaign = state
        .app
        .campaign(viewer.id)
        .await
        .map_err(ErrorResponse::from)?;
    Ok(Json(HealthDto::new(&campaign)))
}

async fn diagnostics(
    State(state): State<ApiState>,
    Extension(viewer): Extension<Viewer>,
) -> Result<Json<DiagnosticsDto>, ErrorResponse> {
    require_dm(viewer)?;
    if !state.diagnostics {
        return Err(ErrorResponse::new(
            HttpStatus::NotFound,
            ErrorCode::NotFound,
        ));
    }
    let campaign = state
        .app
        .campaign(viewer.id)
        .await
        .map_err(ErrorResponse::from)?;
    let stats = state.app.stats().await.map_err(ErrorResponse::from)?;
    Ok(Json(DiagnosticsDto::new(
        &campaign,
        state.app.uptime().as_secs(),
        stats,
    )))
}

async fn campaign(
    State(state): State<ApiState>,
    Extension(viewer): Extension<Viewer>,
) -> Result<Json<CampaignView>, ErrorResponse> {
    let campaign = state
        .app
        .campaign(viewer.id)
        .await
        .map_err(ErrorResponse::from)?;
    Ok(Json(campaign))
}

async fn save(
    State(state): State<ApiState>,
    Extension(viewer): Extension<Viewer>,
) -> Result<StatusCode, ErrorResponse> {
    require_dm(viewer)?;
    state.app.save().await.map_err(ErrorResponse::from)?;
    Ok(StatusCode::NO_CONTENT)
}

fn require_dm(viewer: Viewer) -> Result<(), ErrorResponse> {
    if viewer.role == Role::Dm {
        Ok(())
    } else {
        Err(ErrorResponse::forbidden())
    }
}

async fn roster(
    State(state): State<ApiState>,
    Extension(viewer): Extension<Viewer>,
) -> Result<Json<Vec<RosterAdminEntry>>, ErrorResponse> {
    require_dm(viewer)?;
    let entries = state
        .app
        .roster_admin(viewer.id)
        .await
        .map_err(ErrorResponse::from)?;
    Ok(Json(entries))
}

async fn create_roster_entry(
    State(state): State<ApiState>,
    Extension(viewer): Extension<Viewer>,
    headers: HeaderMap,
    payload: Result<Json<CreateRosterEntryRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<InviteDto>), ErrorResponse> {
    require_dm(viewer)?;
    let Json(request) = payload.map_err(json_rejection)?;
    let player = PlayerId::new();
    let token = ServerToken::generate();
    state
        .app
        .execute(
            viewer.id,
            Command::AddPlayer {
                id: player,
                display_name: request.display_name,
                token_hash: Some(token.hash()),
            },
        )
        .await
        .map_err(ErrorResponse::from)?;
    Ok((
        StatusCode::CREATED,
        Json(InviteDto {
            player,
            invite_url: invite_url(&state, &headers, token.expose()),
        }),
    ))
}

async fn reissue_invite(
    State(state): State<ApiState>,
    Extension(viewer): Extension<Viewer>,
    headers: HeaderMap,
    path: Result<Path<String>, PathRejection>,
) -> Result<Json<InviteDto>, ErrorResponse> {
    require_dm(viewer)?;
    let Path(raw) = path.map_err(|_| ErrorResponse::malformed())?;
    let player = parse_player_id(&raw)?;
    require_other_seat(&viewer, player)?;
    let token = ServerToken::generate();
    state
        .app
        .execute(
            viewer.id,
            Command::SetPlayerToken {
                player,
                token_hash: Some(token.hash()),
            },
        )
        .await
        .map_err(ErrorResponse::from)?;
    Ok(Json(InviteDto {
        player,
        invite_url: invite_url(&state, &headers, token.expose()),
    }))
}

async fn revoke_invite(
    State(state): State<ApiState>,
    Extension(viewer): Extension<Viewer>,
    path: Result<Path<String>, PathRejection>,
) -> Result<StatusCode, ErrorResponse> {
    require_dm(viewer)?;
    let Path(raw) = path.map_err(|_| ErrorResponse::malformed())?;
    let player = parse_player_id(&raw)?;
    require_other_seat(&viewer, player)?;
    state
        .app
        .execute(
            viewer.id,
            Command::SetPlayerToken {
                player,
                token_hash: None,
            },
        )
        .await
        .map_err(ErrorResponse::from)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn remove_roster_entry(
    State(state): State<ApiState>,
    Extension(viewer): Extension<Viewer>,
    path: Result<Path<String>, PathRejection>,
) -> Result<StatusCode, ErrorResponse> {
    require_dm(viewer)?;
    let Path(raw) = path.map_err(|_| ErrorResponse::malformed())?;
    let player = parse_player_id(&raw)?;
    state
        .app
        .execute(
            viewer.id,
            Command::RemovePlayer {
                player,
                reassign_to: viewer.id,
            },
        )
        .await
        .map_err(ErrorResponse::from)?;
    Ok(StatusCode::NO_CONTENT)
}

fn require_other_seat(viewer: &Viewer, player: PlayerId) -> Result<(), ErrorResponse> {
    if viewer.id == player {
        Err(ErrorResponse::forbidden())
    } else {
        Ok(())
    }
}

fn parse_player_id(raw: &str) -> Result<PlayerId, ErrorResponse> {
    Uuid::parse_str(raw)
        .map(PlayerId::from_uuid)
        .map_err(|_| ErrorResponse::malformed())
}

fn invite_url(state: &ApiState, headers: &HeaderMap, token: &str) -> String {
    let base = state
        .lan_host
        .as_deref()
        .map(|host| format!("http://{host}"))
        .or_else(|| {
            headers
                .get(ORIGIN)
                .and_then(|value| value.to_str().ok())
                .filter(|origin| state.origins.allows(origin))
                .and_then(origin_base)
        })
        .or_else(|| {
            headers
                .get(HOST)
                .and_then(|value| value.to_str().ok())
                .and_then(host_base)
        })
        .or_else(|| {
            state
                .lan_ips
                .first()
                .map(|ip| format!("http://{}", SocketAddr::new(*ip, state.bound.port())))
        })
        .unwrap_or_else(|| loopback_base(state.bound));
    format!("{base}/#token={token}")
}

fn origin_base(origin: &str) -> Option<String> {
    let rest = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))?;
    let host = rest.split('/').next()?;
    (!is_loopback_host(host)).then(|| origin.trim_end_matches('/').to_owned())
}

fn host_base(host: &str) -> Option<String> {
    let host = host.trim();
    if host.is_empty() || host.chars().any(char::is_whitespace) || is_loopback_host(host) {
        return None;
    }
    Some(format!("http://{host}"))
}

fn is_loopback_host(host: &str) -> bool {
    if let Some(ip) = host_ip(host) {
        return ip.is_loopback() || ip.is_unspecified();
    }
    let name = host.split(':').next().unwrap_or(host);
    name.eq_ignore_ascii_case("localhost")
}

fn host_ip(host: &str) -> Option<IpAddr> {
    if let Some(rest) = host.strip_prefix('[') {
        let (ip, _) = rest.split_once(']')?;
        return ip.parse().ok();
    }
    host.parse()
        .ok()
        .or_else(|| host.rsplit_once(':').and_then(|(ip, _)| ip.parse().ok()))
}

fn loopback_base(bound: SocketAddr) -> String {
    let address = if bound.ip().is_unspecified() {
        if bound.is_ipv6() {
            format!("[::1]:{}", bound.port())
        } else {
            format!("127.0.0.1:{}", bound.port())
        }
    } else {
        bound.to_string()
    };
    format!("http://{address}")
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
    Extension(viewer): Extension<Viewer>,
    query: Result<Query<EntityQuery>, QueryRejection>,
) -> Result<Json<Vec<EntityView>>, ErrorResponse> {
    let Query(query) = query.map_err(|_| ErrorResponse::malformed())?;
    let kind = query.kind.as_deref().map(parse_entity_kind).transpose()?;
    let entities = state
        .app
        .list_entities(viewer.id)
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
    Extension(viewer): Extension<Viewer>,
    path: Result<Path<String>, PathRejection>,
) -> Result<Json<EntityView>, ErrorResponse> {
    let Path(raw) = path.map_err(|_| ErrorResponse::malformed())?;
    let id = parse_entity_id(&raw)?;
    let view = state
        .app
        .get_entity(viewer.id, id)
        .await
        .map_err(ErrorResponse::from)?;
    Ok(Json(view))
}

async fn entity_sheet(
    State(state): State<ApiState>,
    Extension(viewer): Extension<Viewer>,
    path: Result<Path<String>, PathRejection>,
) -> Result<Json<SheetViewDto>, ErrorResponse> {
    let Path(raw) = path.map_err(|_| ErrorResponse::malformed())?;
    let id = parse_entity_id(&raw)?;
    let sheet = state
        .app
        .sheet(viewer.id, id)
        .await
        .map_err(ErrorResponse::from)?;
    Ok(Json(SheetViewDto::from(sheet)))
}

async fn command(
    State(state): State<ApiState>,
    Extension(viewer): Extension<Viewer>,
    payload: Result<Json<CommandDto>, JsonRejection>,
) -> Result<Json<CommandOutcomeDto>, ErrorResponse> {
    let Json(dto) = payload.map_err(json_rejection)?;
    let command = Command::try_from(dto).map_err(ErrorResponse::from)?;
    let outcome = state
        .app
        .execute(viewer.id, command)
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
    Extension(viewer): Extension<Viewer>,
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
        .feed_after(viewer.id, after, limit)
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
