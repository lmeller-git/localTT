//! Minimal authenticated JSON client for the browser shell.

use gloo_net::http::{Request, Response};
use localtt_protocol::{ErrorBody, ErrorCode};
use serde::{Serialize, de::DeserializeOwned};

const REQUEST_ID_HEADER: &str = "x-request-id";

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
        /// Server correlation id, when the response carried one.
        request_id: Option<String>,
    },
    /// The body did not match the protocol DTO.
    Decode {
        /// The decoding failure.
        message: String,
        /// Server correlation id, when the response carried one.
        request_id: Option<String>,
    },
}

impl std::fmt::Display for WebError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Encode(message) => write!(formatter, "cannot encode the request: {message}"),
            Self::Transport(message) => write!(formatter, "cannot reach the server: {message}"),
            Self::Status {
                status,
                code,
                request_id,
            } => {
                let code = code.map_or_else(|| "unknown".to_owned(), error_code_text);
                match *status {
                    401 => write!(formatter, "the token was rejected ({code})")?,
                    403 => write!(formatter, "that request is not allowed ({code})")?,
                    404 => write!(formatter, "the requested item was not found ({code})")?,
                    409 => write!(
                        formatter,
                        "the item changed; refresh and try again ({code})"
                    )?,
                    _ => write!(formatter, "request failed with HTTP {status} ({code})")?,
                }
                write_request_id(formatter, request_id)?;
                Ok(())
            }
            Self::Decode {
                message,
                request_id,
            } => {
                write!(formatter, "the server response was unexpected: {message}")?;
                write_request_id(formatter, request_id)?;
                Ok(())
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
    let (status, text, request_id) = read(response).await?;
    if status >= 400 {
        return Err(status_error(status, &text, request_id));
    }
    Ok(())
}

async fn decode<T: DeserializeOwned>(response: Response) -> Result<T, WebError> {
    let (status, text, request_id) = read(response).await?;
    if status >= 400 {
        return Err(status_error(status, &text, request_id));
    }
    serde_json::from_str(&text).map_err(|error| WebError::Decode {
        message: error.to_string(),
        request_id,
    })
}

async fn read(response: Response) -> Result<(u16, String, Option<String>), WebError> {
    let status = response.status();
    let request_id = response.headers().get(REQUEST_ID_HEADER);
    let text = response
        .text()
        .await
        .map_err(|error| WebError::Transport(error.to_string()))?;
    Ok((status, text, request_id))
}

fn status_error(status: u16, text: &str, request_id: Option<String>) -> WebError {
    let code = serde_json::from_str::<ErrorBody>(text)
        .ok()
        .map(|body| body.code);
    WebError::Status {
        status,
        code,
        request_id,
    }
}

fn write_request_id(
    formatter: &mut std::fmt::Formatter<'_>,
    request_id: &Option<String>,
) -> std::fmt::Result {
    if let Some(request_id) = request_id {
        write!(formatter, " [request {request_id}]")?;
    }
    Ok(())
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
