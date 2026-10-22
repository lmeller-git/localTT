//! The WebSocket stream adapter (`docs/rfcs/server.md` 4.4 and 7).
//!
//! One connection maps to one [`EventStream`]. The handler subscribes before
//! completing the upgrade, then forwards projected [`StreamEvent`]s as
//! [`StreamEventDto`] JSON text. Events carry ids or refetch hints, never
//! documents, so the browser refetches through the authenticated REST routes
//! with its own viewer.

use axum::{
    extract::{
        State,
        WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::Response,
};
use tokio::select;

use super::ApiState;
use crate::{
    app::{EventStream, StreamEvent},
    protocol::{ErrorResponse, StreamEventDto},
};

/// Starts one authenticated WebSocket stream.
///
/// The subscription is registered before the upgrade response, so events
/// committed after the handshake completes are never lost between the
/// initial reads and the stream.
///
/// # Errors
///
/// Propagates [`crate::error::AppError`] when the viewer cannot subscribe
/// (not a campaign member, or the actor is gone).
pub(crate) async fn websocket(
    State(state): State<ApiState>,
    upgrade: WebSocketUpgrade,
) -> Result<Response, ErrorResponse> {
    let stream = state
        .app
        .subscribe(state.actor)
        .await
        .map_err(ErrorResponse::from)?;
    Ok(upgrade.on_upgrade(move |socket| stream_events(socket, stream)))
}

async fn stream_events(mut socket: WebSocket, mut stream: EventStream) {
    loop {
        select! {
            event = stream.recv() => {
                let Some(event) = event else {
                    break;
                };
                let Ok(text) = event_text(event) else {
                    break;
                };
                if socket.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
            message = socket.recv() => {
                match message {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(_)) => break,
                }
            }
        }
    }
}

fn event_text(event: StreamEvent) -> Result<String, serde_json::Error> {
    serde_json::to_string(&StreamEventDto::from(event))
}

#[cfg(test)]
mod tests {
    use super::event_text;
    use crate::app::StreamEvent;

    #[test]
    fn events_serialize_as_tagged_json_text() {
        assert_eq!(
            event_text(StreamEvent::CampaignChanged).expect("the event serializes"),
            r#"{"type":"campaign_changed"}"#
        );
        assert_eq!(
            event_text(StreamEvent::Lagged).expect("the event serializes"),
            r#"{"type":"lagged"}"#
        );
    }
}
