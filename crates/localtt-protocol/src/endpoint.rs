//! The REST surface as types (`docs/architecture.md` section 6).
//!
//! Each route is one zero-sized marker type implementing [`Endpoint`]: its
//! method, its route template, the request body it takes and the response it
//! returns. The server mounts its router from [`Endpoint::ROUTE`], and clients
//! build requests from the same marker, so a renamed path or a changed
//! response type is a compile error on both sides instead of a runtime 404 or
//! a failed decode.
//!
//! Templates use the router's own syntax (`{id}` for one segment, `{*id}` for
//! a catch-all). The `path` builders on the parameterised markers produce the
//! concrete request path; a test pins every builder against its template.
//! Query-string parameters are not part of the template.

use std::fmt::Write as _;

use localtt_core::types::{EntityId, FeedEntryId, PlayerId, RefId};

use crate::{
    CampaignViewDto,
    CommandDto,
    CommandOutcomeDto,
    ContentEntryDto,
    ContentKindDto,
    ContentSummaryDto,
    CreateRosterEntryRequest,
    DiagnosticsDto,
    EntityViewDto,
    FeedPageDto,
    HealthDto,
    InviteDto,
    RosterAdminEntryDto,
    SheetViewDto,
    SourceInfoDto,
};

/// The WebSocket route; it takes the token as `?token=` because the browser
/// WebSocket API cannot set headers.
pub const WS_ROUTE: &str = "/ws";

/// An HTTP method of the REST surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    /// `GET`.
    Get,
    /// `POST`.
    Post,
    /// `DELETE`.
    Delete,
}

impl Method {
    /// The method's wire spelling.
    #[must_use]
    #[inline]
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Delete => "DELETE",
        }
    }
}

/// One REST route: where it lives and what it carries.
pub trait Endpoint {
    /// The HTTP method.
    const METHOD: Method;
    /// The route template, in the router's syntax.
    const ROUTE: &'static str;
    /// The JSON request body; `()` when the route takes none.
    type Request;
    /// The JSON response body; `()` when the route answers without one.
    type Response;
}

/// Percent-encodes a query value.
///
/// Uses the unreserved set from RFC 3986; everything else is UTF-8 percent
/// encoded.
#[must_use]
#[inline]
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

macro_rules! endpoint {
    ($(#[$meta:meta])* $name:ident, $method:ident, $route:literal, $request:ty => $response:ty) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug)]
        pub struct $name;

        impl Endpoint for $name {
            const METHOD: Method = Method::$method;
            const ROUTE: &'static str = $route;
            type Request = $request;
            type Response = $response;
        }
    };
}

endpoint!(
    /// `GET /api/v1/health`: server version, protocol version and the clock.
    Health, Get, "/api/v1/health", () => HealthDto
);
endpoint!(
    /// `GET /api/v1/diagnostics`: opt-in actor counters; DM only.
    Diagnostics, Get, "/api/v1/diagnostics", () => DiagnosticsDto
);
endpoint!(
    /// `GET /api/v1/campaign`: the campaign as the authenticated viewer sees it.
    Campaign, Get, "/api/v1/campaign", () => CampaignViewDto
);
endpoint!(
    /// `POST /api/v1/campaign/save`: flush the store; DM only.
    SaveCampaign, Post, "/api/v1/campaign/save", () => ()
);
endpoint!(
    /// `GET /api/v1/roster`: the roster with invite flags; DM only.
    RosterList, Get, "/api/v1/roster", () => Vec<RosterAdminEntryDto>
);
endpoint!(
    /// `POST /api/v1/roster`: create a seat and its first invite; DM only.
    RosterCreate, Post, "/api/v1/roster", CreateRosterEntryRequest => InviteDto
);
endpoint!(
    /// `POST /api/v1/roster/{id}/invite`: replace a seat's invite; DM only.
    RosterReissueInvite, Post, "/api/v1/roster/{id}/invite", () => InviteDto
);
endpoint!(
    /// `DELETE /api/v1/roster/{id}/invite`: revoke a seat's invite; DM only.
    RosterRevokeInvite, Delete, "/api/v1/roster/{id}/invite", () => ()
);
endpoint!(
    /// `DELETE /api/v1/roster/{id}`: remove a seat; DM only.
    RosterRemove, Delete, "/api/v1/roster/{id}", () => ()
);
endpoint!(
    /// `GET /api/v1/entities`: every entity the viewer may read.
    Entities, Get, "/api/v1/entities", () => Vec<EntityViewDto>
);
endpoint!(
    /// `GET /api/v1/entities/{id}`: one entity as the viewer sees it.
    EntityDetail, Get, "/api/v1/entities/{id}", () => EntityViewDto
);
endpoint!(
    /// `GET /api/v1/entities/{id}/sheet`: the derived sheet.
    EntitySheet, Get, "/api/v1/entities/{id}/sheet", () => SheetViewDto
);
endpoint!(
    /// `POST /api/v1/commands`: execute one command as the viewer.
    Commands, Post, "/api/v1/commands", CommandDto => CommandOutcomeDto
);
endpoint!(
    /// `GET /api/v1/feed`: one ascending page of visible feed entries.
    Feed, Get, "/api/v1/feed", () => FeedPageDto
);
endpoint!(
    /// `GET /api/v1/reference/search`: reference search hits.
    ReferenceSearch, Get, "/api/v1/reference/search", () => Vec<ContentSummaryDto>
);
endpoint!(
    /// `GET /api/v1/reference/sources`: the loaded content sources.
    ReferenceSources, Get, "/api/v1/reference/sources", () => Vec<SourceInfoDto>
);
endpoint!(
    /// `GET /api/v1/reference/{*id}`: one reference entry; the id contains `/`.
    ReferenceEntry, Get, "/api/v1/reference/{*id}", () => ContentEntryDto
);

impl RosterReissueInvite {
    /// The request path for one seat's invite.
    #[must_use]
    #[inline]
    pub fn path(player: PlayerId) -> String {
        format!("/api/v1/roster/{player}/invite")
    }
}

impl RosterRevokeInvite {
    /// The request path for one seat's invite.
    #[must_use]
    #[inline]
    pub fn path(player: PlayerId) -> String {
        RosterReissueInvite::path(player)
    }
}

impl RosterRemove {
    /// The request path for one seat.
    #[must_use]
    #[inline]
    pub fn path(player: PlayerId) -> String {
        format!("/api/v1/roster/{player}")
    }
}

impl EntityDetail {
    /// The request path for one entity.
    #[must_use]
    #[inline]
    pub fn path(id: EntityId) -> String {
        format!("/api/v1/entities/{id}")
    }
}

impl EntitySheet {
    /// The request path for one entity's derived sheet.
    #[must_use]
    #[inline]
    pub fn path(id: EntityId) -> String {
        format!("/api/v1/entities/{id}/sheet")
    }
}

impl Feed {
    /// The request path for one feed page, optionally after a cursor.
    #[must_use]
    #[inline]
    pub fn path(after: Option<FeedEntryId>, limit: usize) -> String {
        match after {
            Some(cursor) => format!("/api/v1/feed?after={}&limit={limit}", cursor.get()),
            None => format!("/api/v1/feed?limit={limit}"),
        }
    }
}

impl ReferenceSearch {
    /// The request path for a search, optionally limited to one kind.
    #[must_use]
    #[inline]
    pub fn path(query: &str, kind: Option<ContentKindDto>, limit: usize) -> String {
        let query = encode_query(query);
        match kind {
            Some(kind) => format!(
                "/api/v1/reference/search?q={query}&kind={}&limit={limit}",
                kind.as_str()
            ),
            None => format!("/api/v1/reference/search?q={query}&limit={limit}"),
        }
    }
}

impl ReferenceEntry {
    /// The request path for one reference entry.
    #[must_use]
    #[inline]
    pub fn path(id: &RefId) -> String {
        format!("/api/v1/reference/{}", id.as_str())
    }
}

/// The authenticated WebSocket URL for `host`.
#[must_use]
#[inline]
pub fn websocket_url(host: &str, secure: bool, token: &str) -> String {
    let scheme = if secure { "wss" } else { "ws" };
    format!("{scheme}://{host}{WS_ROUTE}?token={}", encode_query(token))
}

#[cfg(test)]
mod tests {
    use localtt_core::types::Id;
    use uuid::Uuid;

    use super::*;

    fn id<T>(value: u128) -> Id<T> {
        Id::from_uuid(Uuid::from_u128(value))
    }

    /// The path without its query string.
    fn bare(path: &str) -> &str {
        path.split('?').next().expect("split yields one piece")
    }

    #[test]
    fn every_path_builder_matches_its_route_template() {
        let player: PlayerId = id(2);
        let entity: EntityId = id(1);
        let reference = RefId::parse("srd2014:spell/fireball").expect("a valid reference");
        let cases = [
            (
                RosterReissueInvite::path(player),
                RosterReissueInvite::ROUTE,
                player.to_string(),
            ),
            (
                RosterRevokeInvite::path(player),
                RosterRevokeInvite::ROUTE,
                player.to_string(),
            ),
            (
                RosterRemove::path(player),
                RosterRemove::ROUTE,
                player.to_string(),
            ),
            (
                EntityDetail::path(entity),
                EntityDetail::ROUTE,
                entity.to_string(),
            ),
            (
                EntitySheet::path(entity),
                EntitySheet::ROUTE,
                entity.to_string(),
            ),
            (
                ReferenceEntry::path(&reference),
                ReferenceEntry::ROUTE,
                reference.as_str().to_owned(),
            ),
        ];
        for (path, route, value) in cases {
            let filled = route.replace("{*id}", &value).replace("{id}", &value);
            assert_eq!(path, filled, "{route}");
        }
    }

    #[test]
    fn parameterless_builders_stay_under_their_route() {
        assert_eq!(bare(&Feed::path(None, 100)), Feed::ROUTE);
        assert_eq!(bare(&Feed::path(Some(FeedEntryId::new(7)), 5)), Feed::ROUTE);
        assert_eq!(
            bare(&ReferenceSearch::path("fire", None, 20)),
            ReferenceSearch::ROUTE
        );
    }

    #[test]
    fn query_parameters_are_spelled_as_the_server_reads_them() {
        assert_eq!(Feed::path(None, 100), "/api/v1/feed?limit=100");
        assert_eq!(
            Feed::path(Some(FeedEntryId::new(7)), 5),
            "/api/v1/feed?after=7&limit=5"
        );
        assert_eq!(
            ReferenceSearch::path("magic missile", None, 20),
            "/api/v1/reference/search?q=magic%20missile&limit=20"
        );
        assert_eq!(
            ReferenceSearch::path("fire", Some(ContentKindDto::Spell), 20),
            "/api/v1/reference/search?q=fire&kind=spell&limit=20"
        );
    }

    #[test]
    fn query_values_are_percent_encoded() {
        assert_eq!(encode_query("fire ball"), "fire%20ball");
        assert_eq!(encode_query("a&b=c#d"), "a%26b%3Dc%23d");
        assert_eq!(encode_query("Über"), "%C3%9Cber");
        assert_eq!(encode_query("safe-_.~"), "safe-_.~");
    }

    #[test]
    fn reference_paths_keep_the_slashes_of_the_id() {
        let id = RefId::parse("srd2014:spell/fireball").expect("a reference");

        assert_eq!(
            ReferenceEntry::path(&id),
            "/api/v1/reference/srd2014:spell/fireball"
        );
        assert_eq!(
            ReferenceSearch::path("thieves tools", Some(ContentKindDto::Item), 20),
            "/api/v1/reference/search?q=thieves%20tools&kind=item&limit=20"
        );
    }

    #[test]
    fn methods_and_templates_are_pinned() {
        assert_eq!(Commands::METHOD.as_str(), "POST");
        assert_eq!(RosterRemove::METHOD.as_str(), "DELETE");
        assert_eq!(Health::METHOD.as_str(), "GET");
        assert_eq!(Commands::ROUTE, "/api/v1/commands");
        assert_eq!(WS_ROUTE, "/ws");
    }

    #[test]
    fn websocket_urls_encode_the_token() {
        assert_eq!(
            websocket_url("localhost:8787", false, "a b"),
            "ws://localhost:8787/ws?token=a%20b"
        );
        assert_eq!(
            websocket_url("tt.example", true, "tok"),
            "wss://tt.example/ws?token=tok"
        );
    }
}
