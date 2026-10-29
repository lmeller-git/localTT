# M5 identity plan

Status: approved 2026-10-27 after adversarial design review. Slice 1 (engine
roster commands), slice 2 (roster auth index, middleware cutover, DM key
startup) and slice 3 (DM-only roster admin routes) are built. `--lan`,
WebSocket revocation and the browser roster panel remain.

Scope: F2 roster/invite, join tokens, DM key, `--lan`.

## Identity model

- A bearer token is 256 random bits, lowercase hex. The server persists only
  `SHA-256(token)` in `Player.token_hash`; raw player tokens exist only in the
  invite response.
- Only 64-hex SHA-256 hashes are indexed. A short or legacy hash is ignored and
  can never authenticate; tokens are never compared by prefix.
- `PlayerId` and token hashes are unique across the roster. The auth index
  rejects duplicates at startup and is rebuilt after every commit.
- A request resolves a token to exactly one `Viewer { id, role }`. Zero matches
  is 401; more than one is a startup failure, never first-match-wins.
- The DM key is a bearer token backed by `<campaign file>.dm.key` (for example
  `default.sqlite.dm.key`) and the selected DM's `Player.token_hash`.
- Player invites are minted server-side, persisted as hashes, and shown once.
  Reissue replaces the hash; revoke clears it; removal deletes the seat.
- Roster mutations are DM-only engine commands. Save and diagnostics are
  DM-only. Engine authz remains the final check, not the route layer.

## Review resolutions

The design review found five blockers; this plan resolves them as follows.

### B1 - token ambiguity

`AddPlayer`, `SetPlayerToken` and `RotateDmKey` reject a duplicate token hash.
`AddPlayer` rejects a duplicate `PlayerId`. Startup validation rejects duplicate
ids and duplicate hashes. `AuthIndex` is a map from a full 64-hex hash to one
seat; a duplicate hash is a startup error. A short stored hash is skipped with a
warning and cannot match a SHA-256 digest. The engine also rejects any
`TokenHash` that is not 64 lowercase hex at the command boundary, so a short
legacy value cannot re-enter through a roster command.

### B2 - DM key atomicity

The key file is per campaign, not per data root. Startup reads and hashes it,
compares it with the selected DM seat, and fails closed on a mismatch. A missing
file with an existing hash also fails closed. `--rotate-dm-key` is the only
silent-rotation path. The new key is written to a same-directory temporary file
with `0600`, the hash is committed through `RotateDmKey`, then the temporary file
is fsynced and renamed. The listener binds only after both succeed. A failed
rename after the commit is reported and leaves an explicit recovery instruction.
`<data>/server.token` is adopted once as the first M5 DM key when the seat has no
hash, then renamed to `server.token.migrated`.

### B3 - `--lan`

`--lan` is the only way to bind a non-loopback address. The CLI rejects a
non-loopback `--bind` without `--lan` and a loopback `--bind` with `--lan`.
Detected non-loopback interface addresses enter the origin allowlist; link-local
IPv6 is skipped. `--lan-host` pins the invite base and adds its origin; `--origin`
remains the escape hatch. CORS allows `DELETE`. The static CSP is generated at
startup with `ws://` entries for loopback and every LAN host, replacing the
loopback-only static header. Browser URLs are printed only when `--web-dir` is
set; otherwise the banner says the API is headless.

### B4 - WebSocket revocation

Subscribers store the token hash used at connect. On `CampaignChanged`, the actor
compares it with the current auth index; a stale subscriber receives
`StreamEvent::Revoked` and is dropped. The socket closes with policy code 1008.
The browser clears the token, signs out with a notice, and stops retrying. REST
401/429 also ends the session; reconnect uses capped backoff.

### B5 - removal

`RemovePlayer` transfers every entity owned by the removed player to the
`reassign_to` DM, removes ACL entries naming the removed player from every
entity, stamps each changed entity, and emits `EntityChanged` for each plus one
`CampaignChanged`. DM seats cannot be removed. Feed entries keep the historical
actor id as audit data.

## Engine commands

```rust
Command::AddPlayer {
    id: PlayerId,
    display_name: String,
    token_hash: Option<TokenHash>,
}
Command::SetPlayerToken {
    player: PlayerId,
    token_hash: Option<TokenHash>,
}
Command::RotateDmKey {
    player: PlayerId,
    token_hash: TokenHash,
}
Command::RemovePlayer {
    player: PlayerId,
    reassign_to: PlayerId,
}
```

- `AddPlayer`: fresh id, trimmed name, always `Role::Player`,
  `created_at = env.now()`, `last_seen = None`; rejects duplicate id/hash.
- `SetPlayerToken`: players only; sets or clears the hash; rejects duplicate hash
  and DM rows (`PlayerProtected`).
- `RotateDmKey`: target must be a DM; replaces that seat's hash; rejects
  duplicate hashes. The server passes the selected DM.
- `RemovePlayer`: target must be a Player; `reassign_to` must be a DM; transfers
  ownership and scrubs ACL entries.
- Limits: `MAX_PLAYERS` and `MAX_DISPLAY_NAME`; over-limit is
  `EngineError::TooLarge`.
- New errors: `PlayerNotFound`, `PlayerExists`, `PlayerProtected`,
  `PlayerTokenInUse`.
- All classify as `Requirement::Dm`.
- Events: entity changes in ascending id, then `CampaignChanged`; no feed change
  unless a future command adds one.

## Protocol

- `RosterEntryView` stays id/display name/role; ENG-30 is unchanged.
- New DM-only `RosterAdminEntry { id, display_name, role, has_invite }` and
  `GET /api/v1/roster`. `has_invite` means a hash is present, not that the player
  connected.
- `CreateRosterEntryRequest { display_name }`, `InviteDto { player, invite_url }`.
- Roster commands stay out of `CommandDto`. Amend ARCH-13 to cover every
  client-executable engine command and add a test for the excluded set.
- New engine errors map to stable codes and statuses. Authentication failures are
  401 in middleware; 403 is only a valid non-DM on a DM route.

## Server

### Auth index and middleware

- The actor owns `Arc<RwLock<Arc<AuthIndex>>>` and rebuilds it after every commit.
  Middleware reads it without an actor round-trip.
- `ApiState` loses `actor` and `token`; `ServerConfig` loses `token`; `serve`
  loses the actor parameter. Handlers use a `Viewer` extension.
- Middleware: origin check, preflight, bounded per-IP limiter keyed by
  `ConnectInfo`, hash, index lookup, 401 on failure, insert viewer. `429` carries
  `Retry-After`.
- `save` and `diagnostics` require `Role::Dm`.

### DM key and startup

- `--actor` is required with multiple DMs, and the selected actor must be a DM.
- Reuse a matching key file; abort on mismatch or a missing file with an existing
  hash; rotate only with `--rotate-dm-key`.
- `--no-dm-key-file` regenerates each start and documents the lost bookmark.
- Atomic writer: same-directory temporary file, `0600`, write, fsync, rename,
  fsync directory. On Windows the mode is best effort and documented.
- Startup banner: DM link, every candidate LAN player URL, cleartext warning when
  non-loopback, and an API-only warning without `--web-dir`.

### Roster routes

| Method and path | Purpose |
|---|---|
| `GET /api/v1/roster` | admin roster |
| `POST /api/v1/roster` | create seat plus first invite |
| `POST /api/v1/roster/{id}/invite` | reissue |
| `DELETE /api/v1/roster/{id}/invite` | revoke |
| `DELETE /api/v1/roster/{id}` | remove and transfer |

- Invite URL uses `#token=`; base order is `--lan-host`, a non-loopback request
  origin/host, the first LAN address, then loopback.
- The local DM cannot reissue or revoke its own token through this route; use
  `--rotate-dm-key`.

### `--lan`

- `bind: Option<SocketAddr>`; `--lan` alone means `0.0.0.0:8787`; explicit
  non-loopback `--bind` requires `--lan`; `--lan` plus explicit loopback is an
  error.
- Enumerate non-loopback IPv4 and global IPv6 into the origin allowlist and CSP.
- CORS methods add `DELETE`.
- Print all candidate URLs on multi-homed hosts.

### WebSocket and browser

- `subscribe(viewer, token_hash)`; revalidate on `CampaignChanged`; send
  `Revoked` and close 1008.
- Join link: auto-consume `#token=` only when no stored token; otherwise show a
  "sign out to use this link" confirmation. Never read `location.search` on
  load. Strip the fragment with `replaceState`.
- Pasted input accepts a full URL or raw token. Clipboard copy falls back to a
  readonly selectable field.
- DM roster panel: admin roster, create, one-time link, reissue, revoke, remove
  with two-step confirmation. Non-DM sees no admin controls and no save or
  diagnostics controls.
- E2E: two Firefox sessions if the driver permits; otherwise one browser plus the
  real HTTP/WS harness.

## Tests

- Engine: duplicate id/hash, short hash cannot authenticate, DM protection,
  removal with PC/note/handout and ACL grants, revision/event shape, limits,
  non-DM refusal, last DM protected.
- Auth: DM/player/unknown/revoked/removed tokens, duplicate-hash startup failure,
  short hash, per-IP isolation, 401 versus 403, index rebuilds.
- DM key: reuse, rotate, missing, mismatch, empty, read-only directory,
  commit failure, M4 `server.token` migration, multiple DMs,
  `--no-dm-key-file`, permissions, restart continuity.
- Routes: admin roster, invite URL bases, one-time exposure, reissue invalidation,
  revoke, remove, DM self-route refusal.
- `--lan`: bind/URL/origin helpers, CORS `DELETE` preflight, CSP LAN `ws://`,
  no-interface warning.
- WebSocket: revoked on rotation/removal, no events after revocation.
- Browser: join-link confirmation and stripping, pasted URL normalization,
  roster panel, two-browser join if available.
- Recipes: `just test`, `just lint`, `just check`, `just e2e`.

## Deferred

- Co-DM role changes and multiple live DM keys.
- Invite expiry and single-use invites.
- QR codes (recorded as deferred; copy/paste link in M5).
- TUI roster/invite UI; browser is the invite surface in M5.
- `last_seen` and invite "claimed" state.
- TLS and reverse proxy.
- Hash algorithm tags; M5 is SHA-256 only.

## Docs to update

- `docs/rfcs/server.md`: M5 built, identity, routes, D1/D11.
- `docs/decisions.md`: new ENG/DATA/ARCH entries; amend ARCH-13 (client-executable
  command set), ARCH-14 (auth index, per-IP limiter), ARCH-15 (WS revocation),
  ARCH-17/20 (join link, no silent switch). ENG-30 stays.
- `docs/architecture.md`, `RFC-0001` 5.8/9.3.3, `docs/testing.md`, core and
  integration `DESIGN.md`, `TODO.md`.
