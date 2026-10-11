# Deterministic, repeatable project tasks. Run `just` to list them.
#
# Cargo commands use --locked so that builds use exactly the committed
# Cargo.lock; the toolchain is pinned by rust-toolchain.toml.

set shell := ["bash", "-euo", "pipefail", "-c"]

# List available recipes.
default:
    @just --list --unsorted

# Everything CI should run: formatting, lints, tests (through daemons and in direct mode), store boundary, spec checks.
check: fmt-check lint test test-direct boundary spec-check

# Build every crate and target.
build:
    cargo build --workspace --all-targets --locked

# Run all tests, including the store conformance suite.
test:
    cargo test --workspace --all-features --locked

# Run the CLI acceptance tests in direct mode (--no-daemon), as well as through daemons.
test-direct:
    RESONANCE_TEST_DIRECT=1 cargo test -p resonance-cli --locked

# Format all Rust code in place.
fmt:
    cargo fmt --all

# Fail if any Rust code is not formatted.
fmt-check:
    cargo fmt --all --check

# Clippy with warnings treated as errors.
lint:
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings

# resonance-core must depend on neither SQLite nor a source-code parser (FR-110, FR-161).
boundary:
    #!/usr/bin/env bash
    set -euo pipefail
    deps="$(cargo tree -p resonance-core --edges normal,build --prefix none --locked)"
    if grep -Eq '^(rusqlite|libsqlite3-sys|resonance-store-sqlite|tree-sitter)' <<<"$deps"; then
        echo "boundary: resonance-core depends on a forbidden crate:" >&2
        grep -E '^(rusqlite|libsqlite3-sys|resonance-store-sqlite|tree-sitter)' <<<"$deps" >&2
        exit 1
    fi
    echo "boundary: ok"

# Spec consistency: requirement IDs, MVP/backlog coverage, section references, US spelling.
spec-check *checks:
    python3 -I scripts/spec_check.py . {{ if checks == "" { "" } else { "--only " + checks } }}

# Run the resonance binary from source, for example `just run status`.
run *args:
    cargo run --quiet --locked -p resonance-cli -- {{args}}

# Install the resonance binary from source; the MVP's distribution channel (FR-261).
install:
    cargo install --locked --path crates/resonance-cli

# Create a fresh test repository from the starter fixtures, for field tests (never this repo).
fixture-repo dir="":
    scripts/fixture_repo.sh {{dir}}

# Remove build output.
clean:
    cargo clean
