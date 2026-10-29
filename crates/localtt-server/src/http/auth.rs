//! Bearer-token, origin and auth-rate-limit middleware.
//!
//! A bearer token resolves to one roster seat through the actor-owned auth
//! index. Every API route is authenticated except an origin-checked CORS
//! preflight; the TUI bypasses HTTP. Browser WebSocket clients may pass the
//! token in `?token=` because the WebSocket API cannot set headers; all other
//! requests use `Authorization: Bearer`.

use std::{
    collections::BTreeMap,
    fmt::{self, Write as _},
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};

use axum::{
    extract::{ConnectInfo, State},
    http::{
        HeaderMap,
        HeaderValue,
        Method,
        Request,
        Uri,
        header::{
            ACCESS_CONTROL_REQUEST_METHOD,
            AUTHORIZATION,
            CACHE_CONTROL,
            ORIGIN,
            RETRY_AFTER,
            WWW_AUTHENTICATE,
            X_CONTENT_TYPE_OPTIONS,
        },
    },
    middleware::Next,
    response::{IntoResponse, Response},
};
use localtt_core::types::TokenHash;

use super::{ApiState, ErrorResponse};

/// Failed authentication attempts allowed in one window before 429 responses.
const FAILURE_LIMIT: u32 = 8;

/// The auth-failure accounting window.
const FAILURE_WINDOW: Duration = Duration::from_secs(60);

/// The most client addresses tracked at once.
///
/// A flood from many addresses evicts the oldest window, so memory stays
/// bounded without weakening the limit for a single address.
const MAX_TRACKED_IPS: usize = 1024;

/// The limiter key used by router tests and embedders without `ConnectInfo`.
const UNKNOWN_CLIENT: IpAddr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);

/// A bearer token, usually the DM key persisted on disk.
///
/// `Debug` is redacted; use [`Self::expose`] only where the token is
/// intentionally shown or written.
#[derive(Clone)]
pub struct ServerToken {
    secret: Arc<str>,
}

impl ServerToken {
    /// Generates a 256-bit token from the OS entropy source.
    ///
    /// # Panics
    ///
    /// Panics when the OS entropy source is unavailable; there is no meaningful
    /// fallback secret.
    #[must_use]
    #[inline]
    pub fn generate() -> Self {
        let mut bytes = [0_u8; 32];
        getrandom::fill(&mut bytes).expect("the OS entropy source is available");
        let mut secret = String::with_capacity(64);
        for byte in bytes {
            write!(&mut secret, "{byte:02x}").expect("writing to a String cannot fail");
        }
        Self {
            secret: Arc::from(secret),
        }
    }

    /// Wraps an externally supplied secret.
    ///
    /// Tests, key-file loading and embedders use this; production callers mint a
    /// new key with [`Self::generate`] so the token has 256 bits of entropy.
    #[must_use]
    #[inline]
    pub fn from_secret(secret: impl Into<String>) -> Self {
        Self {
            secret: Arc::from(secret.into()),
        }
    }

    /// The token value, for the one-time startup print and token file.
    #[must_use]
    #[inline]
    pub fn expose(&self) -> &str {
        &self.secret
    }

    /// The SHA-256 digest persisted on the roster.
    #[must_use]
    #[inline]
    pub fn hash(&self) -> TokenHash {
        crate::identity::hash_token(self.expose())
    }
}

impl fmt::Debug for ServerToken {
    #[inline]
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ServerToken(<redacted>)")
    }
}

/// Browser origins allowed to call the API.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct OriginPolicy {
    allowed: Vec<String>,
}

impl OriginPolicy {
    /// Builds the policy from explicit origins, trimming trailing slashes.
    pub(crate) fn new(origins: impl IntoIterator<Item = String>) -> Self {
        let mut allowed: Vec<String> = origins
            .into_iter()
            .map(|origin| origin.trim_end_matches('/').to_owned())
            .filter(|origin| !origin.is_empty())
            .collect();
        allowed.sort();
        allowed.dedup();
        Self { allowed }
    }

    /// The loopback origins for one bound listener.
    pub(crate) fn loopback(address: SocketAddr) -> Self {
        let port = address.port();
        Self::new([
            format!("http://127.0.0.1:{port}"),
            format!("http://localhost:{port}"),
            format!("http://[::1]:{port}"),
        ])
    }

    /// The allowlist as CORS header values.
    pub(crate) fn header_values(&self) -> Vec<HeaderValue> {
        self.allowed
            .iter()
            .filter_map(|origin| HeaderValue::from_str(origin).ok())
            .collect()
    }

    /// Adds more origins to the policy.
    pub(crate) fn extended(self, extra: impl IntoIterator<Item = String>) -> Self {
        let mut allowed = self.allowed;
        allowed.extend(extra);
        Self::new(allowed)
    }

    /// Whether `origin` is in the allowlist.
    pub(crate) fn allows(&self, origin: &str) -> bool {
        self.allowed.iter().any(|allowed| allowed == origin)
    }
}

/// A fixed-window counter per client address for failed authentication.
#[derive(Clone, Debug, Default)]
pub(crate) struct AuthLimiter {
    state: Arc<Mutex<BTreeMap<IpAddr, Failures>>>,
}

#[derive(Debug, Default)]
struct Failures {
    count: u32,
    since: Option<Instant>,
}

impl AuthLimiter {
    fn lock(&self) -> MutexGuard<'_, BTreeMap<IpAddr, Failures>> {
        self.state
            .lock()
            .expect("the auth limiter mutex is not poisoned")
    }

    /// Whether the address reached the failure limit in the current window.
    pub(crate) fn is_limited(&self, client: IpAddr) -> bool {
        let mut state = self.lock();
        prune_expired(&mut state);
        state
            .get(&client)
            .is_some_and(|failures| failures.count >= FAILURE_LIMIT)
    }

    /// Records one failed authentication attempt for the address.
    pub(crate) fn record_failure(&self, client: IpAddr) {
        let mut state = self.lock();
        prune_expired(&mut state);
        if !state.contains_key(&client) && state.len() >= MAX_TRACKED_IPS {
            evict_oldest(&mut state);
        }
        let failures = state.entry(client).or_default();
        if failures.since.is_none() {
            failures.since = Some(Instant::now());
        }
        failures.count = failures.count.saturating_add(1);
    }

    /// Clears the address's window after a successful authentication.
    pub(crate) fn reset(&self, client: IpAddr) {
        let mut state = self.lock();
        state.remove(&client);
    }
}

fn prune_expired(state: &mut BTreeMap<IpAddr, Failures>) {
    state.retain(|_, failures| {
        failures
            .since
            .is_some_and(|since| since.elapsed() < FAILURE_WINDOW)
    });
}

fn evict_oldest(state: &mut BTreeMap<IpAddr, Failures>) {
    if let Some(oldest) = state
        .iter()
        .min_by_key(|(_, failures)| failures.since)
        .map(|(client, _)| *client)
    {
        state.remove(&oldest);
    }
}

/// Checks origin, authentication and the per-address failure window, then runs
/// the route with the resolved [`crate::identity::Viewer`] extension.
pub(crate) async fn authorize(
    State(state): State<ApiState>,
    mut request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let client = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map_or(UNKNOWN_CLIENT, |ConnectInfo(address)| address.ip());

    if let Some(origin) = request.headers().get(ORIGIN) {
        let allowed = origin
            .to_str()
            .is_ok_and(|origin| state.origins.allows(origin));
        if !allowed {
            return ErrorResponse::origin_denied().into_response();
        }
    }

    if is_cors_preflight(request.method(), request.headers()) {
        return next.run(request).await;
    }

    if state.auth.is_limited(client) {
        return rate_limited();
    }

    let token = bearer_token(request.headers()).or_else(|| {
        (request.uri().path() == "/ws")
            .then(|| query_token(request.uri()))
            .flatten()
    });
    let Some(token) = token else {
        state.auth.record_failure(client);
        return unauthorized();
    };
    let Some(viewer) = state.app.auth_index().authenticate(token) else {
        state.auth.record_failure(client);
        return unauthorized();
    };

    state.auth.reset(client);
    request.extensions_mut().insert(viewer);
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
        .headers_mut()
        .insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    response
}

fn unauthorized() -> Response {
    let mut response = ErrorResponse::unauthorized().into_response();
    response
        .headers_mut()
        .insert(WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
    response
}

fn rate_limited() -> Response {
    let mut response = ErrorResponse::auth_rate_limited().into_response();
    response
        .headers_mut()
        .insert(RETRY_AFTER, HeaderValue::from(FAILURE_WINDOW.as_secs()));
    response
}

fn is_cors_preflight(method: &Method, headers: &HeaderMap) -> bool {
    method == Method::OPTIONS
        && headers.contains_key(ORIGIN)
        && headers.contains_key(ACCESS_CONTROL_REQUEST_METHOD)
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    let token = token.trim();
    (scheme.eq_ignore_ascii_case("bearer") && !token.is_empty()).then_some(token)
}

fn query_token(uri: &Uri) -> Option<&str> {
    uri.query()?.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == "token" && !value.is_empty()).then_some(value)
    })
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    use super::{AuthLimiter, MAX_TRACKED_IPS, OriginPolicy, ServerToken};

    #[test]
    fn the_generated_token_is_256_bits_of_hex() {
        let token = ServerToken::generate();
        assert_eq!(token.expose().len(), 64);
        assert!(token.expose().bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_eq!(format!("{token:?}"), "ServerToken(<redacted>)");
    }

    #[test]
    fn the_origin_policy_trims_slashes_and_deduplicates() {
        let policy = OriginPolicy::new([
            "http://localhost:8787/".to_owned(),
            "http://localhost:8787".to_owned(),
            String::new(),
        ]);
        assert!(policy.allows("http://localhost:8787"));
        assert!(!policy.allows("http://localhost:8787/"));
        assert!(!policy.allows("http://evil.example"));
    }

    #[test]
    fn the_loopback_policy_covers_localhost_and_both_ip_families() {
        let policy = OriginPolicy::loopback("127.0.0.1:8787".parse().expect("a socket address"));
        assert!(policy.allows("http://127.0.0.1:8787"));
        assert!(policy.allows("http://localhost:8787"));
        assert!(policy.allows("http://[::1]:8787"));
    }

    #[test]
    fn the_auth_limiter_opens_after_eight_failures_and_resets() {
        let client: IpAddr = "127.0.0.1".parse().expect("an IP address");
        let limiter = AuthLimiter::default();
        for _ in 0..8 {
            assert!(!limiter.is_limited(client));
            limiter.record_failure(client);
        }
        assert!(limiter.is_limited(client));
        limiter.reset(client);
        assert!(!limiter.is_limited(client));
    }

    #[test]
    fn auth_failures_are_counted_per_address() {
        // Arrange
        let blocked: IpAddr = "127.0.0.1".parse().expect("an IP address");
        let other: IpAddr = "127.0.0.2".parse().expect("an IP address");
        let limiter = AuthLimiter::default();

        // Act
        for _ in 0..8 {
            limiter.record_failure(blocked);
        }

        // Assert
        assert!(limiter.is_limited(blocked));
        assert!(
            !limiter.is_limited(other),
            "one client cannot lock out another"
        );
    }

    #[test]
    fn the_limiter_keeps_its_tracking_map_bounded() {
        // Arrange
        let limiter = AuthLimiter::default();

        // Act
        for index in 0..=MAX_TRACKED_IPS {
            let client: IpAddr = format!("10.0.{}.{}", index / 250, index % 250 + 1)
                .parse()
                .expect("a valid IPv4 address");
            limiter.record_failure(client);
        }

        // Assert
        let tracked = limiter
            .state
            .lock()
            .expect("the limiter mutex is not poisoned")
            .len();
        assert_eq!(tracked, MAX_TRACKED_IPS);
    }
}
