default:
    @just --list

test-rs:
    cargo test --locked --all-features --all-targets
    cargo test --locked --all-features --doc
    cargo test --locked --no-default-features --all-targets
    cargo test --locked --no-default-features --doc
    # Miri cannot execute rusqlite's SQLite FFI, so it covers the pure crates
    # only; add a new pure crate to this -p list when one lands.
    # Dont realy need miri for now
    # cargo +nightly miri test --locked --all-features -p localtt-core -p localtt-engine
    # Loom models the pure concurrency primitives. The server's axum/tokio-net
    # stack does not build under cfg loom, so keep this gate to the pure crates.
    LOOM_MAX_PREEMPTIONS=2 RUSTFLAGS="--cfg loom" cargo test --locked --lib --all-features --release -p localtt-core -p localtt-engine
    RUSTFLAGS="--cfg shuttle" cargo test --locked --lib --all-features

web-build:
    cd web && trunk build --release

web-test:
    cd web && cargo test --locked --all-targets
    cd web && WASM_BINDGEN_USE_BROWSER=1 WASM_BINDGEN_TEST_TIMEOUT=120 cargo test --locked --target wasm32-unknown-unknown --lib

e2e: web-build
    cargo test --locked -p localtt-integration --test browser_e2e -- --ignored --nocapture --test-threads=1

ui-shot: web-build
    cargo test --locked -p localtt-integration --test ui_snapshots -- --ignored --nocapture --test-threads=1

lint-rs:
    cargo +nightly fmt --all -- --check
    cd web && cargo +nightly fmt --all -- --check
    cargo clippy --all-targets --all-features -- -D warnings
    cd web && cargo clippy --all-targets -- -D warnings
    cd web && cargo clippy --all-targets --target wasm32-unknown-unknown -- -D warnings

check-rs:
    cargo doc --workspace --no-deps
    cargo hack --feature-powerset check
    cd web && cargo check --all-targets
    cd web && cargo check --all-targets --target wasm32-unknown-unknown

fmt:
    cargo +nightly fmt --all
    cd web && cargo +nightly fmt --all

test: test-rs web-test

lint: lint-rs

check: check-rs

bench:
    cargo bench --all-features

