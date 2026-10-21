//! The HTTP adapter over [`AppHandle`] (`docs/rfcs/server.md` 7).
//!
//! Every route requires a per-run bearer token, checks a browser `Origin` when
//! one is present, and caps request bodies at [`MAX_BODY_BYTES`]; an
//! origin-checked CORS preflight is the only unauthenticated request. The
//! WebSocket route is added by M4b; this module owns the REST surface and the
//! shared state.

use std::{io, net::SocketAddr};

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
use thiserror::Error;
use tokio::net::TcpListener;

use crate::{app::AppHandle, protocol::ErrorResponse};

mod auth;
mod routes;

#[cfg(test)]
mod tests;

pub use auth::ServerToken;
use auth::{AuthLimiter, OriginPolicy};

/// The maximum request body accepted by the HTTP adapter (1 MiB).
pub const MAX_BODY_BYTES: usize = 1024 * 1024;

impl IntoResponse for ErrorResponse {
    #[inline]
    fn into_response(self) -> Response {
        let status =
            StatusCode::from_u16(self.status.as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let mut response = (status, Json(self.body)).into_response();
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
}

impl ServerConfig {
    /// Builds a loopback-friendly config with no extra origins.
    #[must_use]
    #[inline]
    pub fn new(bind: SocketAddr, token: ServerToken) -> Self {
        Self {
            bind,
            token,
            origins: Vec::new(),
        }
    }

    /// Adds browser origins to the loopback defaults.
    #[must_use]
    #[inline]
    pub fn with_origins(mut self, origins: impl IntoIterator<Item = String>) -> Self {
        self.origins.extend(origins);
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
/// address.
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
    let router = routes::router(state);
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
