//! URL handling for the browser client: join links and tokens. Request paths
//! come from `localtt_protocol::endpoint`.

pub use localtt_protocol::endpoint::{encode_query, websocket_url};

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
