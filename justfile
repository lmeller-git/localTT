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

lint-rs:
    cargo +nightly fmt --all -- --check
    cargo clippy --all-targets --all-features -- -D warnings

check-rs:
    cargo doc --workspace --no-deps
    cargo hack --feature-powerset check

fmt:
    cargo +nightly fmt --all

test: test-rs

lint: lint-rs

check: check-rs

bench:
    cargo bench --all-features

