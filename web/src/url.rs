//! URL construction for the browser client.

use std::fmt::Write as _;

use localtt_core::types::{EntityId, FeedEntryId, PlayerId, RefId};

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

/// The spell-only reference search path.
#[must_use]
pub fn spell_search_path(query: &str, limit: usize) -> String {
    format!(
        "/api/v1/reference/search?q={}&kind=spell&limit={limit}",
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

/// The roster administration path.
#[must_use]
pub fn roster_path() -> &'static str {
    "/api/v1/roster"
}

/// The invite path for one seat.
#[must_use]
pub fn invite_path(player: PlayerId) -> String {
    format!("/api/v1/roster/{player}/invite")
}

/// The removal path for one seat.
#[must_use]
pub fn roster_entry_path(player: PlayerId) -> String {
    format!("/api/v1/roster/{player}")
}

/// The bearer token carried by a login paste.
///
/// Accepts a raw token or a full join URL whose token sits in the fragment
/// (`#token=`) or the query string (`?token=`). A URL without a token is
/// rejected rather than sent as the token itself.
#[must_use]
pub fn token_from_input(input: &str) -> Option<&str> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    if let Some((_, fragment)) = input.split_once('#')
        && let Some(token) = parameter(fragment, "token")
    {
        return Some(token);
    }
    if let Some((_, query)) = input.split_once('?')
        && let Some(token) = parameter(query, "token")
    {
        return Some(token);
    }
    if input.contains("://") || input.contains('#') || input.contains('?') {
        return None;
    }
    Some(input)
}

/// Whether an invite URL points at the machine that minted it.
///
/// A loopback or unspecified host cannot be reached from another device, so
/// the roster panel warns instead of implying the link is shareable.
#[must_use]
pub fn invite_is_machine_local(invite_url: &str) -> bool {
    let Some((_, rest)) = invite_url.split_once("://") else {
        return false;
    };
    let authority = rest.split(['/', '#', '?']).next().unwrap_or(rest);
    let host = if let Some(rest) = authority.strip_prefix('[') {
        rest.split_once(']').map_or(rest, |(host, _)| host)
    } else {
        authority.split(':').next().unwrap_or(authority)
    };
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback() || ip.is_unspecified())
}

/// The token in a location fragment such as `#token=<value>`.
#[must_use]
pub fn fragment_token(fragment: &str) -> Option<&str> {
    parameter(fragment.strip_prefix('#').unwrap_or(fragment), "token")
}

fn parameter<'a>(value: &'a str, name: &str) -> Option<&'a str> {
    value.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == name && !value.is_empty()).then_some(value)
    })
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
        assert_eq!(
            spell_search_path("fire ball", 10),
            "/api/v1/reference/search?q=fire%20ball&kind=spell&limit=10"
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

    #[test]
    fn roster_paths_address_one_seat() {
        let player = PlayerId::new();
        assert_eq!(roster_path(), "/api/v1/roster");
        assert_eq!(
            invite_path(player),
            format!("/api/v1/roster/{player}/invite")
        );
        assert_eq!(
            roster_entry_path(player),
            format!("/api/v1/roster/{player}")
        );
    }

    #[test]
    fn machine_local_invites_are_recognised() {
        for url in [
            "http://127.0.0.1:8787/#token=abc",
            "http://localhost:8787/#token=abc",
            "http://[::1]:8787/#token=abc",
            "http://0.0.0.0:8787/#token=abc",
            "http://[::]:8787/#token=abc",
        ] {
            assert!(invite_is_machine_local(url), "{url} is machine-local");
        }
        for url in [
            "http://192.168.1.5:8787/#token=abc",
            "http://table.test:8787/#token=abc",
            "https://table.test/#token=abc",
            "not a url",
        ] {
            assert!(!invite_is_machine_local(url), "{url} is shareable");
        }
    }

    #[test]
    fn a_paste_can_be_a_raw_token_or_a_join_url() {
        assert_eq!(token_from_input("  abc123  "), Some("abc123"));
        assert_eq!(
            token_from_input("http://127.0.0.1:8787/#token=abc123"),
            Some("abc123")
        );
        assert_eq!(
            token_from_input("https://table.test:8787/?token=abc123"),
            Some("abc123")
        );
        assert_eq!(
            token_from_input("http://table.test/#tab&token=abc123&x=1"),
            Some("abc123")
        );
    }

    #[test]
    fn a_paste_without_a_token_is_refused() {
        assert_eq!(token_from_input(""), None);
        assert_eq!(token_from_input("   "), None);
        assert_eq!(token_from_input("http://127.0.0.1:8787/"), None);
        assert_eq!(token_from_input("http://table.test/?other=1#help"), None);
        assert_eq!(token_from_input("#token="), None);
    }

    #[test]
    fn the_fragment_parser_only_accepts_a_token_parameter() {
        assert_eq!(fragment_token("#token=abc"), Some("abc"));
        assert_eq!(fragment_token("token=abc"), Some("abc"));
        assert_eq!(fragment_token("#tab&token=abc"), Some("abc"));
        assert_eq!(fragment_token("#tab"), None);
        assert_eq!(fragment_token("#tokentwo=abc"), None);
    }
}
