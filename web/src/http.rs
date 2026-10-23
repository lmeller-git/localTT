//! Minimal authenticated JSON client for the browser shell.

use gloo_net::http::Request;
use localtt_protocol::{ErrorBody, ErrorCode};
use serde::de::DeserializeOwned;

/// A request, status or decoding failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum WebError {
    /// The request did not reach the server.
    Transport(String),
    /// The server rejected the request.
    Status {
        /// HTTP status.
        status: u16,
        /// Stable error code, when the body was parseable.
        code: Option<ErrorCode>,
    },
    /// The body did not match the protocol DTO.
    Decode(String),
}

impl std::fmt::Display for WebError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(message) => write!(formatter, "cannot reach the server: {message}"),
            Self::Status { status, code } => {
                let code = code
                    .map(|code| format!("{:?}", code).to_lowercase())
                    .unwrap_or_else(|| "unknown".to_owned());
                match *status {
                    401 => write!(formatter, "the token was rejected ({code})"),
                    403 => write!(formatter, "that request is not allowed ({code})"),
                    404 => write!(formatter, "the requested item was not found ({code})"),
                    _ => write!(formatter, "request failed with HTTP {status} ({code})"),
                }
            }
            Self::Decode(message) => {
                write!(formatter, "the server response was unexpected: {message}")
            }
        }
    }
}

/// Performs an authenticated GET and decodes a protocol DTO.
pub(crate) async fn api_get<T: DeserializeOwned>(path: &str, token: &str) -> Result<T, WebError> {
    let response = Request::get(path)
        .header("Authorization", &format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| WebError::Transport(error.to_string()))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|error| WebError::Transport(error.to_string()))?;

    if status >= 400 {
        let code = serde_json::from_str::<ErrorBody>(&text)
            .ok()
            .map(|body| body.code);
        return Err(WebError::Status { status, code });
    }

    serde_json::from_str(&text).map_err(|error| WebError::Decode(error.to_string()))
}
