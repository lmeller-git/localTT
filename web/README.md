# localtt web

The browser client for local-tt: a Leptos 0.8 CSR app built with Trunk. It is a
nested Cargo workspace and depends only on `localtt-protocol` and `localtt-core`.

## Prerequisites

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk --locked
cargo install wasm-bindgen-cli --version 0.2.129 --locked
# A Firefox/geckodriver pair for the headless WASM test.
```

`wasm-bindgen-cli` must match the `wasm-bindgen` version in `Cargo.lock`; Trunk
downloads the matching CLI for `trunk serve` automatically. The headless test
runs through `wasm-bindgen-test-runner` and Firefox, configured by
`web/.cargo/config.toml`.

## Build and serve

```sh
trunk build
localtt serve --web-dir web/dist
```

The shell is served at `/`; `/api/v1/*` and `/ws` still require the bearer token
printed by the server. `--web-dir` is optional, so the server can run API-only.

## Tests

```sh
just web-test
just e2e
just ui-shot
```

Native tests cover the pure command, formatting and URL helpers. The headless
WASM test mounts the login shell in Firefox. `just test` runs both layers with
the rest of the workspace. The browser E2E suite needs Firefox and
`geckodriver`; it logs in, creates a character and saves against a real
in-process server, and rebuilds the assets first. `just ui-shot` runs a
representative journey and writes desktop/narrow screenshots plus a semantic
report to `target/ui/latest`. See [`docs/testing.md`](../docs/testing.md).
