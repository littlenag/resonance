# Deterministic, repeatable project tasks. Run `just` to list them.
#
# Cargo commands use --locked so that builds use exactly the committed
# Cargo.lock; the toolchain is pinned by rust-toolchain.toml.

set shell := ["bash", "-euo", "pipefail", "-c"]

# List available recipes.
default:
    @just --list --unsorted

# Everything CI should run: formatting, lints, tests (through daemons and in direct mode), store boundary, spec checks, the web UI.
check: fmt-check lint test test-direct boundary spec-check ui-check

# Build every crate and target, with the web UI embedded.
build: ui-build
    cargo build --workspace --all-targets --locked

# Build the web UI into web/dist, which the binary embeds (MVP-043). Needs Node and npm.
ui-build: ui-deps
    cd web && npm run build

# The web UI's type checks, lint and component tests.
ui-check: ui-deps
    cd web && npm run typecheck && npm run lint && npm test

# Install the web UI's dependencies from its lockfile, when they changed.
ui-deps:
    #!/usr/bin/env bash
    set -euo pipefail
    cd web
    if [ ! -d node_modules ] || [ package-lock.json -nt node_modules/.package-lock.json ]; then
        npm ci
    fi

# Work on the web UI with live reload, against a running daemon: `just ui-dev http://127.0.0.1:<port>`.
ui-dev daemon:
    cd web && RESONANCE_UI={{daemon}} npm run dev

# Run all tests, including the store conformance suite.
test:
    cargo test --workspace --all-features --locked

# Run the CLI acceptance tests in direct mode (--no-daemon), as well as through daemons.
test-direct:
    RESONANCE_TEST_DIRECT=1 cargo test -p resonance-cli --locked

# Line coverage (docs/testing.md). Not part of `check`. Args: summary (default), html, or lcov.
coverage *args:
    bash scripts/coverage.sh {{args}}

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

# Install the resonance binary from source, with the web UI; the MVP's distribution channel (FR-261).
install: ui-build
    cargo install --locked --path crates/resonance-cli

# Create a fresh test repository from the starter fixtures, for field tests (never this repo).
fixture-repo dir="":
    scripts/fixture_repo.sh {{dir}}

# Remove build output.
clean:
    cargo clean
