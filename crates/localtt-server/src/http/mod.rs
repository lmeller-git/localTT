//! The HTTP adapter over [`AppHandle`] (`docs/rfcs/server.md` 7).
//!
//! Every route requires a per-run bearer token, checks a browser `Origin` when
//! one is present, and caps request bodies at [`MAX_BODY_BYTES`]; an
//! origin-checked CORS preflight is the only unauthenticated request. REST and
//! the WebSocket upgrade share this module's state and auth/origin middleware.

use std::{io, net::SocketAddr, path::PathBuf};

use axum::{
    Json,
    http::{
        HeaderValue,
        StatusCode,
        header::{CACHE_CONTROL, X_CONTENT_TYPE_OPTIONS},
    },
    response::{IntoResponse, Response},
};
use localtt_core::types::PlayerId;
use localtt_protocol::{
    ErrorCode,
    ErrorResponse as ProtocolErrorResponse,
    HttpStatus,
    UnsupportedCommand,
};
use thiserror::Error;
use tokio::net::TcpListener;

use crate::{app::AppHandle, error::AppError};

mod auth;
mod routes;
mod ws;

#[cfg(test)]
mod tests;

pub use auth::ServerToken;
use auth::{AuthLimiter, OriginPolicy};

/// The maximum request body accepted by the HTTP adapter (1 MiB).
pub const MAX_BODY_BYTES: usize = 1024 * 1024;

/// The HTTP adapter's local error wrapper.
///
/// The wire shape lives in `localtt-protocol`; this newtype owns the axum
/// response implementation and the server-side conversions.
#[derive(Debug)]
pub(crate) struct ErrorResponse(ProtocolErrorResponse);

impl ErrorResponse {
    pub(crate) fn new(status: HttpStatus, code: ErrorCode) -> Self {
        Self(ProtocolErrorResponse::new(status, code))
    }

    pub(crate) fn malformed() -> Self {
        Self(ProtocolErrorResponse::malformed())
    }

    pub(crate) fn payload_too_large() -> Self {
        Self(ProtocolErrorResponse::payload_too_large())
    }

    pub(crate) fn unauthorized() -> Self {
        Self(ProtocolErrorResponse::unauthorized())
    }

    pub(crate) fn auth_rate_limited() -> Self {
        Self(ProtocolErrorResponse::auth_rate_limited())
    }

    pub(crate) fn origin_denied() -> Self {
        Self(ProtocolErrorResponse::origin_denied())
    }
}

impl From<ProtocolErrorResponse> for ErrorResponse {
    fn from(response: ProtocolErrorResponse) -> Self {
        Self(response)
    }
}

impl From<AppError> for ErrorResponse {
    fn from(error: AppError) -> Self {
        Self(ProtocolErrorResponse::from(error))
    }
}

impl From<UnsupportedCommand> for ErrorResponse {
    fn from(error: UnsupportedCommand) -> Self {
        Self(ProtocolErrorResponse::from(error))
    }
}

impl IntoResponse for ErrorResponse {
    #[inline]
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.0.status.as_u16())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let mut response = (status, Json(self.0.body)).into_response();
        response
            .headers_mut()
            .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
        response
            .headers_mut()
            .insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
        response
    }
}

/// Shared state for one HTTP listener.
#[derive(Clone)]
pub(crate) struct ApiState {
    pub(crate) app: AppHandle,
    pub(crate) actor: PlayerId,
    pub(crate) token: ServerToken,
    pub(crate) origins: OriginPolicy,
    pub(crate) auth: AuthLimiter,
}

/// How one HTTP listener is configured.
#[derive(Clone, Debug)]
pub struct ServerConfig {
    /// The address to bind.
    pub bind: SocketAddr,
    /// The per-run bearer token.
    pub token: ServerToken,
    /// Extra browser origins beyond the loopback defaults.
    pub origins: Vec<String>,
    /// Directory containing the built browser client; `None` serves the API only.
    pub web_dir: Option<PathBuf>,
}

impl ServerConfig {
    /// Builds a loopback-friendly config with no extra origins and no web
    /// assets.
    #[must_use]
    #[inline]
    pub fn new(bind: SocketAddr, token: ServerToken) -> Self {
        Self {
            bind,
            token,
            origins: Vec::new(),
            web_dir: None,
        }
    }

    /// Adds browser origins to the loopback defaults.
    #[must_use]
    #[inline]
    pub fn with_origins(mut self, origins: impl IntoIterator<Item = String>) -> Self {
        self.origins.extend(origins);
        self
    }

    /// Serves the built browser client from `web_dir`.
    #[must_use]
    #[inline]
    pub fn with_web_dir(mut self, web_dir: impl Into<PathBuf>) -> Self {
        self.web_dir = Some(web_dir.into());
        self
    }
}

/// A running HTTP listener.
///
/// Dropping the handle aborts the accept loop. Call [`Self::local_addr`] to
/// print the bound address (useful when binding port zero).
#[derive(Debug)]
pub struct ServerHandle {
    local_addr: SocketAddr,
    task: tokio::task::JoinHandle<Result<(), io::Error>>,
}

impl ServerHandle {
    /// The actual bound address.
    #[must_use]
    #[inline]
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Stops accepting requests and aborts the listener task.
    #[inline]
    pub fn stop(&self) {
        self.task.abort();
    }
}

impl Drop for ServerHandle {
    #[inline]
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Why the HTTP listener could not start.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ServeError {
    /// Binding or accepting failed.
    #[error(transparent)]
    Io(#[from] io::Error),
    /// `web_dir` does not contain a built browser client.
    #[error("web assets are missing: {0} has no index.html")]
    WebAssetsMissing(PathBuf),
}

/// Binds and starts the HTTP adapter.
///
/// The returned handle owns the listener task; dropping it aborts the task.
/// Loopback origins for the actual bound port are always allowed; `config`
/// origins are added on top.
///
/// # Errors
///
/// Returns [`ServeError::Io`] when the listener cannot bind or report its local
/// address, and [`ServeError::WebAssetsMissing`] when a configured `web_dir`
/// has no `index.html`.
///
/// # Panics
///
/// Panics when called outside a Tokio runtime.
#[inline]
pub async fn serve(
    app: AppHandle,
    actor: PlayerId,
    config: ServerConfig,
) -> Result<ServerHandle, ServeError> {
    if let Some(web_dir) = &config.web_dir
        && !web_dir.join("index.html").is_file()
    {
        return Err(ServeError::WebAssetsMissing(web_dir.clone()));
    }
    let listener = TcpListener::bind(config.bind).await?;
    let local_addr = listener.local_addr()?;
    let origins = OriginPolicy::loopback(local_addr).extended(config.origins);
    let state = ApiState {
        app,
        actor,
        token: config.token,
        origins,
        auth: AuthLimiter::default(),
    };
    let router = match config.web_dir {
        Some(web_dir) => routes::router_with_web(state, web_dir),
        None => routes::router(state),
    };
    let task = tokio::spawn(async move { axum::serve(listener, router).await });
    Ok(ServerHandle { local_addr, task })
}

/// The shared state builder for tests.
#[cfg(test)]
pub(crate) fn state_for(
    app: AppHandle,
    actor: PlayerId,
    token: ServerToken,
    origins: OriginPolicy,
) -> ApiState {
    ApiState {
        app,
        actor,
        token,
        origins,
        auth: AuthLimiter::default(),
    }
}
