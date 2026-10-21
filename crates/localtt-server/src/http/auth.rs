//! Bearer-token, origin and auth-rate-limit middleware.
//!
//! The token grants the local DM actor in v0. Every API route is authenticated
//! except an origin-checked CORS preflight; the TUI bypasses HTTP. Browser
//! WebSocket clients may pass the token in `?token=` because the WebSocket API
//! cannot set headers; all other requests use `Authorization: Bearer`.

use std::{
    fmt::{self, Write as _},
    net::SocketAddr,
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};

use axum::{
    extract::State,
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
            WWW_AUTHENTICATE,
            X_CONTENT_TYPE_OPTIONS,
        },
    },
    middleware::Next,
    response::{IntoResponse, Response},
};

use super::ApiState;
use crate::protocol::ErrorResponse;

/// Failed authentication attempts allowed in one window before 429 responses.
const FAILURE_LIMIT: u32 = 8;

/// The auth-failure accounting window.
const FAILURE_WINDOW: Duration = Duration::from_secs(60);

/// The per-run bearer token.
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
    /// Tests and embedders use this; production callers should use
    /// [`Self::generate`] so the token has 256 bits of entropy.
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

    fn matches(&self, candidate: &str) -> bool {
        constant_time_eq(self.secret.as_bytes(), candidate.as_bytes())
    }
}

impl fmt::Debug for ServerToken {
    #[inline]
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ServerToken(<redacted>)")
    }
}

/// Compares two byte strings without an early exit on the first mismatch.
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0_u8;
    for (left, right) in left.iter().zip(right) {
        difference |= left ^ right;
    }
    difference == 0
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

/// A small fixed-window counter for failed authentication attempts.
#[derive(Clone, Debug, Default)]
pub(crate) struct AuthLimiter {
    state: Arc<Mutex<Failures>>,
}

#[derive(Debug, Default)]
struct Failures {
    count: u32,
    since: Option<Instant>,
}

impl AuthLimiter {
    fn lock(&self) -> MutexGuard<'_, Failures> {
        self.state
            .lock()
            .expect("the auth limiter mutex is not poisoned")
    }

    /// Whether the current window has reached the failure limit.
    pub(crate) fn is_limited(&self) -> bool {
        let mut state = self.lock();
        prune(&mut state);
        state.count >= FAILURE_LIMIT
    }

    /// Records one failed authentication attempt.
    pub(crate) fn record_failure(&self) {
        let mut state = self.lock();
        prune(&mut state);
        if state.since.is_none() {
            state.since = Some(Instant::now());
        }
        state.count = state.count.saturating_add(1);
    }

    /// Clears the window after a successful authentication.
    pub(crate) fn reset(&self) {
        let mut state = self.lock();
        state.count = 0;
        state.since = None;
    }
}

fn prune(state: &mut Failures) {
    if state
        .since
        .is_some_and(|since| since.elapsed() >= FAILURE_WINDOW)
    {
        state.count = 0;
        state.since = None;
    }
}

/// Checks origin, authentication and the failure window, then runs the route.
pub(crate) async fn authorize(
    State(state): State<ApiState>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
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

    if state.auth.is_limited() {
        return ErrorResponse::auth_rate_limited().into_response();
    }

    let token = bearer_token(request.headers()).or_else(|| {
        (request.uri().path() == "/ws")
            .then(|| query_token(request.uri()))
            .flatten()
    });
    let Some(token) = token else {
        state.auth.record_failure();
        return unauthorized();
    };
    if !state.token.matches(token) {
        state.auth.record_failure();
        return unauthorized();
    }

    state.auth.reset();
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
    use super::{AuthLimiter, OriginPolicy, ServerToken, constant_time_eq};

    #[test]
    fn the_generated_token_is_256_bits_of_hex() {
        let token = ServerToken::generate();
        assert_eq!(token.expose().len(), 64);
        assert!(token.expose().bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_eq!(format!("{token:?}"), "ServerToken(<redacted>)");
    }

    #[test]
    fn constant_time_comparison_accepts_only_identical_bytes() {
        assert!(constant_time_eq(b"secret", b"secret"));
        assert!(!constant_time_eq(b"secret", b"secreT"));
        assert!(!constant_time_eq(b"secret", b"secret "));
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
        let limiter = AuthLimiter::default();
        for _ in 0..8 {
            assert!(!limiter.is_limited());
            limiter.record_failure();
        }
        assert!(limiter.is_limited());
        limiter.reset();
        assert!(!limiter.is_limited());
    }
}
