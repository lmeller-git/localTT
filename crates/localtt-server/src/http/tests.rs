//! HTTP adapter tests: auth, origin checks, body caps, command mapping and the
//! read-only API.

use std::{
    fs,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use axum::{
    body::{Body, to_bytes},
    http::{
        HeaderMap,
        Method,
        Request,
        StatusCode,
        header::{
            ACCESS_CONTROL_ALLOW_ORIGIN,
            ACCESS_CONTROL_REQUEST_HEADERS,
            ACCESS_CONTROL_REQUEST_METHOD,
            AUTHORIZATION,
            CACHE_CONTROL,
            CONTENT_SECURITY_POLICY,
            CONTENT_TYPE,
            ORIGIN,
            REFERRER_POLICY,
            WWW_AUTHENTICATE,
            X_CONTENT_TYPE_OPTIONS,
            X_FRAME_OPTIONS,
        },
    },
};
use futures_util::StreamExt;
use localtt_adapter::{reference::FixtureSource, rng::ChaChaRng, store::MemoryStore};
use localtt_core::{
    builders::{CampaignBuilder, StatblockBuilder},
    types::{Abilities, AnyEntity, Campaign, EntityId, EntityKind, PlayerId},
};
use localtt_engine::{
    commands::Command,
    content::ReferenceSource,
    ports::store::{
        AsyncCampaignStore,
        CampaignSnapshot,
        CampaignStore,
        StatePatch,
        StoreBackup,
        StoreFault,
    },
};
use serde_json::{Value, json};
use tokio::time::timeout;
use tokio_tungstenite::connect_async;
use tower::ServiceExt;

use super::{
    MAX_BODY_BYTES,
    ServeError,
    ServerConfig,
    ServerToken,
    auth::OriginPolicy,
    request_id::REQUEST_ID_HEADER,
    routes,
    serve,
    state_for,
};
use crate::{app::AppHandle, env::ServerEnv};

const TOKEN: &str = "test-token-0123456789abcdef";
const ALLOWED_ORIGIN: &str = "http://localhost:8787";

struct BackupSpy<S> {
    inner: S,
    syncs: Arc<AtomicUsize>,
}

#[async_trait]
impl<S: AsyncCampaignStore> AsyncCampaignStore for BackupSpy<S> {
    async fn load(&self) -> Result<CampaignSnapshot, StoreFault> {
        self.inner.load().await
    }

    async fn apply(&mut self, patch: &StatePatch<'_>) -> Result<(), StoreFault> {
        self.inner.apply(patch).await
    }
}

impl<S: AsyncCampaignStore> StoreBackup for BackupSpy<S> {
    fn backup(&mut self, _to: &Path) -> Result<(), StoreFault> {
        Err(StoreFault::Unavailable {
            message: "the test spy has no backup file".to_owned(),
        })
    }

    fn sync(&mut self) -> Result<(), StoreFault> {
        self.syncs.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

struct Fixture {
    app: AppHandle,
    actor: PlayerId,
    entity: EntityId,
    router: axum::Router,
    syncs: Arc<AtomicUsize>,
}

fn fixture() -> Fixture {
    let mut builder = CampaignBuilder::new("Curse of the Vault");
    let dm = builder.dm("Dungeon Master");
    builder.player("Owner");
    builder.player("Other");
    let campaign: Campaign = builder.build();
    let mut stats = StatblockBuilder::character();
    stats.abilities(Abilities::all(10)).hp(32, 32);
    let mut entity = stats
        .build_entity(EntityKind::Pc, EntityId::new(), "Aria", dm)
        .expect("abilities and hp are set");
    entity.acl.clear();
    let entity_id = entity.id;
    let entity = AnyEntity::from(entity);

    let mut store = MemoryStore::new();
    CampaignStore::apply(
        &mut store,
        &StatePatch {
            campaign: Some(&campaign),
            upserts: vec![&entity],
            removals: Vec::new(),
            feed_appends: Vec::new(),
            feed_keep_from: None,
        },
    )
    .expect("the fixture is a valid patch");
    let snapshot = CampaignStore::load(&store).expect("the fixture loaded");
    let syncs = Arc::new(AtomicUsize::new(0));
    let store = BackupSpy {
        inner: store,
        syncs: Arc::clone(&syncs),
    };
    let content: Arc<dyn ReferenceSource> = Arc::new(FixtureSource::new());
    let app = AppHandle::start(
        store,
        snapshot,
        Box::new(ServerEnv::with_rng(
            Arc::clone(&content),
            ChaChaRng::from_seed([11; 32]),
        )),
        content,
    )
    .expect("the actor starts");
    let state = state_for(
        app.clone(),
        dm,
        ServerToken::from_secret(TOKEN),
        OriginPolicy::new([ALLOWED_ORIGIN.to_owned()]),
        false,
    );
    let router = routes::router(state);
    Fixture {
        app,
        actor: dm,
        entity: entity_id,
        router,
        syncs,
    }
}

fn auth_request(method: Method, uri: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(AUTHORIZATION, format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .expect("the request builds")
}

fn auth_json(uri: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(AUTHORIZATION, format!("Bearer {TOKEN}"))
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .expect("the request builds")
}

async fn send(router: &axum::Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(request)
        .await
        .expect("the router responds");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .expect("the body collects");
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("the response is JSON")
    };
    (status, body)
}

#[tokio::test]
async fn health_campaign_save_and_auth_are_wired() {
    let fixture = fixture();

    let (status, body) = send(
        &fixture.router,
        Request::builder()
            .uri("/api/v1/health")
            .body(Body::empty())
            .expect("the request builds"),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["code"], "unauthorized");

    let (status, body) = send(&fixture.router, auth_request(Method::GET, "/api/v1/health")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["campaign_name"], "Curse of the Vault");
    assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
    assert!(body["campaign_id"].is_string());

    let (status, body) = send(
        &fixture.router,
        Request::builder()
            .uri("/ws?token=wrong")
            .body(Body::empty())
            .expect("the request builds"),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["code"], "unauthorized");

    let (status, body) = send(
        &fixture.router,
        auth_request(Method::GET, "/api/v1/campaign"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "Curse of the Vault");
    assert_eq!(body["viewer"], json!(fixture.actor));
    assert_eq!(body["roster"].as_array().expect("a roster").len(), 3);

    let (status, _) = send(
        &fixture.router,
        auth_request(Method::POST, "/api/v1/campaign/save"),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(fixture.syncs.load(Ordering::SeqCst), 1);

    fixture.app.shutdown().await.expect("the actor stops");
}

#[tokio::test]
async fn commands_update_the_entity_and_project_the_outcome() {
    let fixture = fixture();

    let (status, body) = send(
        &fixture.router,
        auth_json(
            "/api/v1/commands",
            json!({"type": "apply_damage", "entity": fixture.entity, "amount": 12}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["views"].as_array().expect("views").len(), 1);
    assert_eq!(body["executed"]["events"][0]["type"], "entity_changed");
    assert_eq!(body["executed"]["warnings"], json!([]));

    let (status, body) = send(
        &fixture.router,
        auth_request(Method::GET, &format!("/api/v1/entities/{}", fixture.entity)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], json!(fixture.entity));
    assert_eq!(body["rev"], 1);

    let (status, body) = send(
        &fixture.router,
        auth_request(
            Method::GET,
            &format!("/api/v1/entities/{}/sheet", fixture.entity),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["entity"], json!(fixture.entity));
    assert_eq!(body["rev"], 1);
    assert!(body["sheet"]["values"]["ac"].is_object());

    let (status, body) = send(
        &fixture.router,
        auth_request(Method::GET, "/api/v1/entities?kind=pc"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().expect("entities").len(), 1);

    let (status, body) = send(
        &fixture.router,
        auth_request(Method::GET, "/api/v1/entities?kind=monster"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!([]));

    let (status, body) = send(
        &fixture.router,
        auth_request(Method::GET, "/api/v1/entities?kind=dragon"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "malformed");

    let (status, body) = send(&fixture.router, auth_request(Method::GET, "/api/v1/feed")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["entries"], json!([]));
    assert_eq!(body["trimmed_before"], Value::Null);

    fixture.app.shutdown().await.expect("the actor stops");
}

#[tokio::test]
async fn malformed_and_unknown_commands_use_the_safe_error_shape() {
    let fixture = fixture();

    let (status, body) = send(
        &fixture.router,
        auth_json("/api/v1/commands", json!({"type": "apply_damage"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "malformed");

    let (status, body) = send(
        &fixture.router,
        auth_json("/api/v1/commands", json!({"type": "future_command"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "unsupported_command");

    fixture.app.shutdown().await.expect("the actor stops");
}

#[tokio::test]
async fn origin_and_rate_limits_are_enforced() {
    let fixture = fixture();

    let (status, body) = send(
        &fixture.router,
        Request::builder()
            .uri("/api/v1/health")
            .header(AUTHORIZATION, format!("Bearer {TOKEN}"))
            .header(ORIGIN, "http://evil.example")
            .body(Body::empty())
            .expect("the request builds"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], "origin_denied");

    let (status, _) = send(
        &fixture.router,
        Request::builder()
            .uri("/api/v1/health")
            .header(AUTHORIZATION, format!("Bearer {TOKEN}"))
            .header(ORIGIN, ALLOWED_ORIGIN)
            .body(Body::empty())
            .expect("the request builds"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    for attempt in 0..8 {
        let (status, _) = send(
            &fixture.router,
            Request::builder()
                .uri("/api/v1/health")
                .header(AUTHORIZATION, "Bearer wrong")
                .body(Body::empty())
                .expect("the request builds"),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "attempt {attempt}");
    }
    let (status, body) = send(&fixture.router, auth_request(Method::GET, "/api/v1/health")).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["code"], "auth_rate_limited");

    fixture.app.shutdown().await.expect("the actor stops");
}

#[tokio::test]
async fn oversized_bodies_return_the_transport_error() {
    let fixture = fixture();
    let payload = " ".repeat(MAX_BODY_BYTES + 1);
    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/v1/commands")
        .header(AUTHORIZATION, format!("Bearer {TOKEN}"))
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(payload))
        .expect("the request builds");

    let (status, body) = send(&fixture.router, request).await;

    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(body["code"], "payload_too_large");
    fixture.app.shutdown().await.expect("the actor stops");
}

#[tokio::test]
async fn reference_search_entry_and_sources_are_available() {
    let fixture = fixture();

    let (status, body) = send(
        &fixture.router,
        auth_request(
            Method::GET,
            "/api/v1/reference/search?q=fire&kind=spell&limit=5",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().expect("hits").len(), 1);
    assert_eq!(body[0]["id"], "fixture:spell/fire-bolt");

    let (status, body) = send(
        &fixture.router,
        auth_request(Method::GET, "/api/v1/reference/fixture:spell/fire-bolt"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "Fire Bolt");

    let (status, body) = send(
        &fixture.router,
        auth_request(Method::GET, "/api/v1/reference/sources"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body.as_array().expect("sources").is_empty());

    let (status, body) = send(
        &fixture.router,
        auth_request(Method::GET, "/api/v1/reference/fixture:spell/missing"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "not_found");

    let (status, body) = send(
        &fixture.router,
        auth_request(Method::GET, "/api/v1/reference/search?kind=dragon"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "malformed");

    fixture.app.shutdown().await.expect("the actor stops");
}

#[tokio::test]
async fn cors_preflight_and_security_headers_are_set() {
    let fixture = fixture();

    let response = fixture
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::OPTIONS)
                .uri("/api/v1/commands")
                .header(ORIGIN, ALLOWED_ORIGIN)
                .header(ACCESS_CONTROL_REQUEST_METHOD, "POST")
                .header(ACCESS_CONTROL_REQUEST_HEADERS, "content-type,authorization")
                .body(Body::empty())
                .expect("the request builds"),
        )
        .await
        .expect("the router responds");
    assert!(
        response.status().is_success(),
        "preflight status: {}",
        response.status()
    );
    assert_eq!(
        response
            .headers()
            .get(ACCESS_CONTROL_ALLOW_ORIGIN)
            .expect("an allow-origin header"),
        ALLOWED_ORIGIN
    );
    assert!(
        response
            .headers()
            .get("access-control-allow-methods")
            .expect("an allow-methods header")
            .to_str()
            .expect("a visible header")
            .contains("POST")
    );

    let response = fixture
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/health")
                .header(ORIGIN, ALLOWED_ORIGIN)
                .body(Body::empty())
                .expect("the request builds"),
        )
        .await
        .expect("the router responds");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        response
            .headers()
            .get(ACCESS_CONTROL_ALLOW_ORIGIN)
            .expect("an allow-origin header on the error"),
        ALLOWED_ORIGIN
    );
    assert_eq!(
        response
            .headers()
            .get(WWW_AUTHENTICATE)
            .expect("a challenge header"),
        "Bearer"
    );
    assert_eq!(
        response
            .headers()
            .get(CACHE_CONTROL)
            .expect("a cache header"),
        "no-store"
    );

    let mut request = auth_request(Method::GET, "/api/v1/health");
    request
        .headers_mut()
        .insert(ORIGIN, ALLOWED_ORIGIN.parse().expect("a header value"));
    let response = fixture
        .router
        .clone()
        .oneshot(request)
        .await
        .expect("the router responds");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(ACCESS_CONTROL_ALLOW_ORIGIN)
            .expect("an allow-origin header"),
        ALLOWED_ORIGIN
    );
    assert_eq!(
        response
            .headers()
            .get(CACHE_CONTROL)
            .expect("a cache header"),
        "no-store"
    );
    assert_eq!(
        response
            .headers()
            .get(X_CONTENT_TYPE_OPTIONS)
            .expect("a nosniff header"),
        "nosniff"
    );

    fixture.app.shutdown().await.expect("the actor stops");
}

#[tokio::test]
async fn a_bound_server_reports_its_address() {
    let fixture = fixture();
    let token = ServerToken::from_secret(TOKEN);
    let server = serve(
        fixture.app.clone(),
        PlayerId::new(),
        ServerConfig::new("127.0.0.1:0".parse().expect("an address"), token),
    )
    .await
    .expect("the listener binds");
    assert_ne!(server.local_addr().port(), 0);

    server.stop();
    fixture.app.shutdown().await.expect("the actor stops");
}

#[tokio::test]
async fn the_websocket_streams_projected_events_with_a_query_token() {
    let fixture = fixture();
    let server = serve(
        fixture.app.clone(),
        fixture.actor,
        ServerConfig::new(
            "127.0.0.1:0".parse().expect("an address"),
            ServerToken::from_secret(TOKEN),
        ),
    )
    .await
    .expect("the listener binds");

    let url = format!("ws://{}/ws?token={TOKEN}", server.local_addr());
    let (mut socket, response) = timeout(Duration::from_secs(5), connect_async(url))
        .await
        .expect("the handshake does not time out")
        .expect("the handshake succeeds");
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);

    fixture
        .app
        .execute(fixture.actor, Command::AdvanceClock { by_seconds: 60 })
        .await
        .expect("the DM advances the clock");

    let message = timeout(Duration::from_secs(5), socket.next())
        .await
        .expect("an event arrives")
        .expect("the socket is open")
        .expect("the message reads");
    let text = message.into_text().expect("the event is text");
    let event: Value = serde_json::from_str(text.as_str()).expect("the event is JSON");
    assert_eq!(event, json!({"type": "campaign_changed"}));

    socket.close(None).await.expect("the client closes");
    server.stop();
    fixture.app.shutdown().await.expect("the actor stops");
}

async fn send_raw(
    router: &axum::Router,
    request: Request<Body>,
) -> (StatusCode, HeaderMap, Vec<u8>) {
    let response = router
        .clone()
        .oneshot(request)
        .await
        .expect("the router responds");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .expect("the body collects");
    (status, headers, bytes.to_vec())
}

#[tokio::test]
async fn browser_assets_are_public_but_api_stays_guarded() {
    let fixture = fixture();
    let web = tempfile::tempdir().expect("the web fixture directory");
    fs::write(
        web.path().join("index.html"),
        "<!doctype html><title>localtt web fixture</title>",
    )
    .expect("the index writes");
    fs::write(web.path().join("app.js"), "console.log('fixture');").expect("the asset writes");

    let state = state_for(
        fixture.app.clone(),
        fixture.actor,
        ServerToken::from_secret(TOKEN),
        OriginPolicy::new([ALLOWED_ORIGIN.to_owned()]),
        false,
    );
    let router = routes::router_with_web(state, web.path().to_path_buf());

    let (status, headers, body) = send_raw(
        &router,
        Request::builder()
            .uri("/")
            .body(Body::empty())
            .expect("the request builds"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[CONTENT_TYPE], "text/html");
    assert_eq!(headers[CACHE_CONTROL], "no-store");
    assert_eq!(headers[X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert_eq!(headers[REFERRER_POLICY], "no-referrer");
    assert_eq!(headers[X_FRAME_OPTIONS], "DENY");
    assert!(
        headers[CONTENT_SECURITY_POLICY]
            .to_str()
            .expect("the CSP is text")
            .contains("wasm-unsafe-eval")
    );
    assert!(
        headers[CONTENT_SECURITY_POLICY]
            .to_str()
            .expect("the CSP is text")
            .contains("unsafe-inline")
    );
    assert!(
        String::from_utf8(body)
            .expect("the body is UTF-8")
            .contains("localtt web fixture")
    );

    let (status, headers, _) = send_raw(
        &router,
        Request::builder()
            .uri("/app.js")
            .body(Body::empty())
            .expect("the request builds"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[CONTENT_TYPE], "text/javascript");

    let (status, _, body) = send_raw(
        &router,
        Request::builder()
            .uri("/characters/one")
            .body(Body::empty())
            .expect("the request builds"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        String::from_utf8(body)
            .expect("the body is UTF-8")
            .contains("localtt web fixture")
    );

    let (status, _, _) = send_raw(
        &router,
        Request::builder()
            .uri("/api/v1/health")
            .body(Body::empty())
            .expect("the request builds"),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, body) = send(&router, auth_request(Method::GET, "/api/v1/unknown")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "not_found");
}

#[tokio::test]
async fn every_api_response_carries_a_distinct_request_id() {
    let fixture = fixture();

    let (status, headers, _) =
        send_raw(&fixture.router, auth_request(Method::GET, "/api/v1/health")).await;
    assert_eq!(status, StatusCode::OK);
    let first = headers
        .get(REQUEST_ID_HEADER)
        .expect("the response has a request id")
        .to_str()
        .expect("the request id is text")
        .to_owned();

    let (status, headers, _) =
        send_raw(&fixture.router, auth_request(Method::GET, "/api/v1/health")).await;
    assert_eq!(status, StatusCode::OK);
    let second = headers
        .get(REQUEST_ID_HEADER)
        .expect("the response has a request id")
        .to_str()
        .expect("the request id is text")
        .to_owned();
    assert_ne!(first, second);

    let (status, headers, _) = send_raw(
        &fixture.router,
        Request::builder()
            .uri("/api/v1/health")
            .body(Body::empty())
            .expect("the request builds"),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(headers.contains_key(REQUEST_ID_HEADER));

    fixture.app.shutdown().await.expect("the actor stops");
}

#[tokio::test]
async fn diagnostics_are_gated_and_report_actor_counters() {
    let fixture = fixture();

    let (status, body) = send(
        &fixture.router,
        auth_request(Method::GET, "/api/v1/diagnostics"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "not_found");

    let state = state_for(
        fixture.app.clone(),
        fixture.actor,
        ServerToken::from_secret(TOKEN),
        OriginPolicy::new([ALLOWED_ORIGIN.to_owned()]),
        true,
    );
    let router = routes::router(state);
    let (status, body) = send(&router, auth_request(Method::GET, "/api/v1/diagnostics")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["campaign_name"], "Curse of the Vault");
    assert_eq!(body["actor"]["queue_capacity"], 64);
    assert_eq!(body["actor"]["queue_depth"], 0);
    assert_eq!(body["actor"]["subscribers"], 0);
    assert!(body["actor"]["requests"].as_u64().expect("a count") >= 2);
    assert!(body["actor"]["reads"].as_u64().expect("a count") >= 1);

    fixture.app.shutdown().await.expect("the actor stops");
}

#[tokio::test]
async fn a_configured_web_dir_must_contain_an_index() {
    let fixture = fixture();
    let root = tempfile::tempdir().expect("the fixture directory");
    let config = ServerConfig::new(
        "127.0.0.1:0".parse().expect("the bind address"),
        ServerToken::from_secret(TOKEN),
    )
    .with_web_dir(root.path().join("missing"));
    let error = serve(fixture.app, fixture.actor, config)
        .await
        .expect_err("missing web assets are refused");
    assert!(matches!(error, ServeError::WebAssetsMissing(_)));
}
