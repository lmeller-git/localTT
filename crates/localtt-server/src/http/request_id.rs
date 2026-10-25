//! Per-request correlation IDs and HTTP completion traces.
//!
//! The middleware runs outside authentication, so rejected and not-found
//! requests still carry an id and a span.

use std::time::Instant;

use axum::{
    extract::Request,
    http::{HeaderName, HeaderValue},
    middleware::Next,
    response::Response,
};
use tracing::Instrument;
use uuid::Uuid;

/// The request and response header carrying the correlation id.
pub(crate) const REQUEST_ID_HEADER: &str = "x-request-id";

/// Adds a generated request id to every API response and traces the completion.
pub(crate) async fn correlate(request: Request, next: Next) -> Response {
    let request_id = Uuid::new_v4().to_string();
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let span = tracing::info_span!(
        "http.request",
        request_id = %request_id,
        method = %method,
        path = %path,
    );

    async move {
        let started = Instant::now();
        let mut response = next.run(request).await;
        let status = response.status().as_u16();
        let elapsed_ms = started.elapsed().as_millis();
        if let Ok(value) = HeaderValue::from_str(&request_id) {
            response
                .headers_mut()
                .insert(HeaderName::from_static(REQUEST_ID_HEADER), value);
        }

        if status >= 500 {
            tracing::warn!(status, elapsed_ms, "request completed");
        } else {
            tracing::info!(status, elapsed_ms, "request completed");
        }
        response
    }
    .instrument(span)
    .await
}
