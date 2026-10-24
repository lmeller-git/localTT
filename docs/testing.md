# Browser testing

The browser client has three test layers:

| Layer | Command | What it covers |
|---|---|---|
| Native | `just web-test` | Pure command builders, formatting and URL helpers. |
| WASM smoke | `just web-test` | Mounts the login shell in headless Firefox through `wasm-bindgen-test`. |
| Browser E2E | `just e2e` | Real HTTP and WebSocket server, built Trunk assets, and a headless Firefox session through geckodriver. |

`just test` runs the native and WASM layers. Browser E2E tests are ignored by
default so `just test` stays free of Firefox and geckodriver requirements.

## Prerequisites

- Firefox and `geckodriver` on `PATH`; set `GECKODRIVER` to use a different
  binary.
- Trunk and the `wasm32-unknown-unknown` target, as described in
  [`web/README.md`](../web/README.md).

`just e2e` rebuilds `web/dist` before running the suite.

## Harness

`crates/localtt-integration/tests/browser_e2e.rs` is the suite entry point.
`tests/support/server.rs` starts a real server in-process on an ephemeral
loopback port with:

- a temporary SQLite campaign seeded through `session::start`;
- the shipped `FixtureSource` for reference data;
- `ServerEnv::with_rng` with a fixed seed, so dice are deterministic;
- `web/dist` from the repository, served through `--web-dir`.

`tests/support/browser.rs` starts geckodriver on a free port, opens headless
Firefox, logs in with the test token and exposes small wait helpers. The server
and browser are shut down explicitly at the end of each test.

Only one geckodriver session should run at a time, so the recipe uses
`--test-threads=1`.

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

## Failure artifacts

On failure the harness writes a screenshot, page source and URL to
`target/ui/failure/` under the Cargo target directory. CI uploads that directory
as the `browser-smoke-failure` artifact.

## Scope

The first browser E2E test covers login, campaign load, character creation and
campaign save. HP/rest, inventory, handouts, dice/clock, WebSocket resync,
restart persistence, error paths, accessibility and responsive layout are
follow-ups. The UI snapshot loop (`just ui-shot`) is not implemented yet.
