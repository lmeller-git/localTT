# Browser testing

The browser client has four test layers:

| Layer | Command | What it covers |
|---|---|---|
| Native | `just web-test` | Pure command builders, formatting and URL helpers. |
| WASM smoke | `just web-test` | Mounts the login shell in headless Firefox through `wasm-bindgen-test`. |
| Browser E2E | `just e2e` | Real HTTP and WebSocket server, built Trunk assets, and a headless Firefox session through geckodriver. |
| UI snapshots | `just ui-shot` | A representative browser journey captured at desktop and narrow viewports with a semantic report. |

`just test` runs the native and WASM layers. Browser tests are ignored by
default so `just test` stays free of Firefox and geckodriver requirements.

## Prerequisites

- Firefox and `geckodriver` on `PATH`; set `GECKODRIVER` to use a different
  binary.
- Trunk and the `wasm32-unknown-unknown` target, as described in
  [`web/README.md`](../web/README.md).

`just e2e` and `just ui-shot` rebuild `web/dist` before running.

## Harness

`crates/localtt-integration/tests/browser_e2e.rs` is the E2E suite entry point.
`crates/localtt-integration/tests/ui_snapshots.rs` is the snapshot-loop entry
point. Both share `tests/support/server.rs`, which starts a real server
in-process on an ephemeral loopback port with:

- a temporary SQLite campaign seeded through `session::start`;
- the shipped `FixtureSource` for reference data;
- `ServerEnv::with_rng` with a fixed seed, so dice are deterministic;
- `web/dist` from the repository, served through `--web-dir`.

`tests/support/browser.rs` starts geckodriver on a free port, opens headless
Firefox, logs in with the test token and exposes small wait, input, scroll and
screenshot helpers. `tests/support/journey.rs` owns one server and one browser
per test, captures a failure artifact and always tears both down in order.
`tests/support/flows.rs` holds the steps more than one journey repeats, such as
`Character` and `create_character`.

`TestServer::restart` reopens the same store and rebinds the same loopback port,
so the browser keeps its origin and token. Commands passed to it are applied
after the actor loads the campaign and before the listener binds again, which
means a reconnecting client can only learn them by resyncing.

Only one geckodriver session should run at a time, so both recipes use
`--test-threads=1`.

## Journeys

`just e2e` runs five journeys, each against its own fresh campaign:

| Test | Covers |
|---|---|
| `browser_login_create_and_save` | login, empty roster, character creation, campaign save, diagnostics panel, debug ring and a rejected roll |
| `browser_hp_temp_and_long_rest` | damage, temporary hit points absorbing damage first, healing and a long rest restoring the maximum |
| `browser_handout_give` | item handout creation, giving it to the only statblock and the recipient inventory |
| `browser_reconnects_and_resyncs` | a server restart on the same origin, a command applied while no client is subscribed, and the reconnect resyncing the sheet over REST |
| `browser_restart_keeps_the_campaign` | save, restart on the same data directory, page reload with the stored token, and the campaign, character and hit points intact |

Each journey asserts the visible sheet, not the wire format, and ends by
checking that the shell reports no error. A failure writes its screenshot, page
source and URL under `target/ui/failure/` with the journey name.

## UI snapshot loop

`just ui-shot` is the manual review loop for visual and responsive changes. It
runs one scenario against a fresh campaign:

1. capture the login screen at desktop and narrow widths;
2. log in, create a character and select it;
3. add two inventory items, roll dice and advance the clock;
4. capture the populated desktop sheet;
5. capture the narrow layout from the top and again with the detail pane
   scrolled into view;
6. open the debug panel and capture the diagnostics snapshot on desktop.

The run clears and rewrites `target/ui/latest` (under the Cargo target
directory):

- `report.md` embeds each screenshot and the semantic UI state read beside it:
  campaign, clock, stream, busy/error flags, entity count, selected name, HP,
  temp HP, AC, inventory rows, feed lines and reference hits.
- `manifest.json` is the same data in machine-readable form.
- `NN-label-viewport.png` are the screenshots.

The report records both the requested viewport and the CSS-pixel viewport
Firefox actually provided. Headless Firefox clamps the outer window width and
includes browser chrome in the height, so the narrow capture is currently
`500x844` rather than the requested `390x930`.

`tests/support/scenario.rs` owns viewport resizing, scrolling, screenshot
writing and report generation. Add journey steps and `Scenario::capture` calls
in `ui_snapshots.rs`; keep each capture label short and stable because it is
part of the artifact filename.

## Selectors and state

The browser UI exposes stable hooks for tests:

- `data-testid` on panels, forms, inputs, buttons, feed lines and inventory rows.
- `data-state="ready"` on the top bar once the campaign view has loaded.
- `data-busy` on the top bar while a command or save is in flight.
- `data-error` on the top bar when the current error banner is populated.
- `data-ws-state` on the top bar with the projected stream status (`live` when
  connected).
- `data-entity-id`, `data-writable` and `data-entry-id` on rendered rows and
  detail views.

When adding or renaming a control that a browser test drives, keep its
`data-testid` stable. Prefer asserting the visible outcome (entity name, HP,
status) over internal state.

The sheet is rebuilt after every command, so the HP amount field comes back with
its default value. Re-enter the amount after the previous update has been
observed; filling it while the sheet is still replacing the input loses the
value.

The test server enables `--diagnostics`, so the smoke opens the debug panel,
waits for the diagnostics snapshot and checks that the local ring records both a
save event and a rejected command. `data-testid='diagnostics-requests'` exposes
the actor request count; `data-testid='debug-entry'` carries
`data-level="info"` or `data-level="error"`.

## Request correlation and telemetry

`localtt` initialises `tracing-subscriber` from `RUST_LOG` and defaults to
`localtt_server=info`. API requests are logged with method, path, status and
latency, and every API response carries a generated `x-request-id` header. The
browser HTTP client attaches the id to status and decode errors, so a failure in
the debug ring can be matched to the server log line.

The WebSocket client reconnects after a drop and performs a full REST resync
before reading new events, so anything committed while it was offline is
reflected. A `lagged` stream event takes the same path. The debug ring records
`stream live`, `stream resynced` and `stream reconnecting`.

`--diagnostics` gates `GET /api/v1/diagnostics`. The route stays authenticated
and returns 404 after auth when the flag is absent. Its `ActorStatsDto` reports
request/error/command/read/save counts, live subscribers and actor queue
depth/capacity; it never includes command payloads, tokens or filesystem paths.
Native router tests cover the gate, counters, request-id uniqueness and the
header on auth failures.

## Failure artifacts

On failure the E2E harness writes a screenshot, page source and URL to
`target/ui/failure/` under the Cargo target directory. CI uploads that directory
as the `browser-smoke-failure` artifact.

A failed `just ui-shot` run writes the same `ui-snapshot.png`, `.html` and
`.url` files. If the scenario had started, `target/ui/latest/report.md` is still
written with the captures completed so far and a run-error section.

## Scope

The E2E journeys cover login, campaign load, character creation, campaign save,
hit points with temporary hit points and rests, item handouts, reconnect resync,
restart persistence, the diagnostics panel, the debug ring and a rejected
command. The snapshot scenario exercises inventory add, dice, clock advance and
the debug panel on the same observed path. Broader error paths, accessibility
and deeper responsive assertions are follow-ups.
