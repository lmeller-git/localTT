//! Minimal authenticated JSON client for the browser shell.

use gloo_net::http::{Request, Response};
use localtt_protocol::{ErrorBody, ErrorCode};
use serde::{Serialize, de::DeserializeOwned};

/// A request, status or decoding failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum WebError {
    /// The request could not be encoded.
    Encode(String),
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
            Self::Encode(message) => write!(formatter, "cannot encode the request: {message}"),
            Self::Transport(message) => write!(formatter, "cannot reach the server: {message}"),
            Self::Status { status, code } => {
                let code = code.map_or_else(|| "unknown".to_owned(), error_code_text);
                match *status {
                    401 => write!(formatter, "the token was rejected ({code})"),
                    403 => write!(formatter, "that request is not allowed ({code})"),
                    404 => write!(formatter, "the requested item was not found ({code})"),
                    409 => write!(
                        formatter,
                        "the item changed; refresh and try again ({code})"
                    ),
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
        .header("Authorization", &auth_header(token))
        .send()
        .await
        .map_err(|error| WebError::Transport(error.to_string()))?;
    decode(response).await
}

/// Performs an authenticated JSON POST and decodes a protocol DTO.
pub(crate) async fn api_post<T, B>(path: &str, token: &str, body: &B) -> Result<T, WebError>
where
    T: DeserializeOwned,
    B: Serialize + ?Sized,
{
    let body = serde_json::to_string(body).map_err(|error| WebError::Encode(error.to_string()))?;
    let response = Request::post(path)
        .header("Authorization", &auth_header(token))
        .header("Content-Type", "application/json")
        .body(body)
        .map_err(|error| WebError::Encode(error.to_string()))?
        .send()
        .await
        .map_err(|error| WebError::Transport(error.to_string()))?;
    decode(response).await
}

/// Performs an authenticated POST that expects an empty successful response.
pub(crate) async fn api_post_empty(path: &str, token: &str) -> Result<(), WebError> {
    let response = Request::post(path)
        .header("Authorization", &auth_header(token))
        .send()
        .await
        .map_err(|error| WebError::Transport(error.to_string()))?;
    let (status, text) = read(response).await?;
    if status >= 400 {
        return Err(status_error(status, &text));
    }
    Ok(())
}

async fn decode<T: DeserializeOwned>(response: Response) -> Result<T, WebError> {
    let (status, text) = read(response).await?;
    if status >= 400 {
        return Err(status_error(status, &text));
    }
    serde_json::from_str(&text).map_err(|error| WebError::Decode(error.to_string()))
}

async fn read(response: Response) -> Result<(u16, String), WebError> {
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|error| WebError::Transport(error.to_string()))?;
    Ok((status, text))
}

fn status_error(status: u16, text: &str) -> WebError {
    let code = serde_json::from_str::<ErrorBody>(text)
        .ok()
        .map(|body| body.code);
    WebError::Status { status, code }
}

fn auth_header(token: &str) -> String {
    format!("Bearer {token}")
}

fn error_code_text(code: ErrorCode) -> String {
    serde_json::to_value(code)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| format!("{code:?}").to_lowercase())
}
