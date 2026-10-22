//! URL construction for the browser client.

use std::fmt::Write as _;

use localtt_core::types::{EntityId, FeedEntryId, RefId};

/// Percent-encodes a query value.
///
/// Uses the unreserved set from RFC 3986; everything else is UTF-8 percent
/// encoded.
#[must_use]
pub fn encode_query(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

/// The feed page path, optionally after a cursor.
#[must_use]
pub fn feed_path(after: Option<FeedEntryId>, limit: usize) -> String {
    match after {
        Some(cursor) => format!("/api/v1/feed?after={}&limit={limit}", cursor.get()),
        None => format!("/api/v1/feed?limit={limit}"),
    }
}

/// The entity detail path.
#[must_use]
pub fn entity_path(id: EntityId) -> String {
    format!("/api/v1/entities/{id}")
}

/// The derived-sheet path.
#[must_use]
pub fn sheet_path(id: EntityId) -> String {
    format!("/api/v1/entities/{id}/sheet")
}

/// The reference search path.
#[must_use]
pub fn search_path(query: &str, limit: usize) -> String {
    format!(
        "/api/v1/reference/search?q={}&limit={limit}",
        encode_query(query)
    )
}

/// The reference entry path.
#[must_use]
pub fn content_path(id: &RefId) -> String {
    format!("/api/v1/reference/{}", id.as_str())
}

/// The authenticated WebSocket URL.
#[must_use]
pub fn websocket_url(host: &str, secure: bool, token: &str) -> String {
    let scheme = if secure { "wss" } else { "ws" };
    format!("{scheme}://{host}/ws?token={}", encode_query(token))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_values_are_percent_encoded() {
        assert_eq!(encode_query("fire ball"), "fire%20ball");
        assert_eq!(encode_query("a&b=c#d"), "a%26b%3Dc%23d");
        assert_eq!(encode_query("Über"), "%C3%9Cber");
        assert_eq!(encode_query("safe-_.~"), "safe-_.~");
    }

    #[test]
    fn paths_keep_ref_slashes_and_escape_queries() {
        let id = RefId::parse("srd2014:spell/fireball").expect("a reference");
        assert_eq!(
            content_path(&id),
            "/api/v1/reference/srd2014:spell/fireball"
        );
        assert_eq!(
            search_path("fire ball", 10),
            "/api/v1/reference/search?q=fire%20ball&limit=10"
        );
    }

    #[test]
    fn feed_cursors_are_explicit() {
        assert_eq!(feed_path(None, 50), "/api/v1/feed?limit=50");
        assert_eq!(
            feed_path(Some(FeedEntryId::new(7)), 50),
            "/api/v1/feed?after=7&limit=50"
        );
    }

    #[test]
    fn websocket_url_uses_the_page_scheme_and_query_token() {
        assert_eq!(
            websocket_url("127.0.0.1:8787", false, "abc"),
            "ws://127.0.0.1:8787/ws?token=abc"
        );
        assert_eq!(
            websocket_url("example.test", true, "a b"),
            "wss://example.test/ws?token=a%20b"
        );
    }
}
