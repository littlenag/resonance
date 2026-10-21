#!/usr/bin/env bash
# Line coverage for the Resonance workspace.
#
# Prints the per-file table (with a TOTAL row) by default. HTML and LCOV are
# written under target/llvm-cov/, which is gitignored with the rest of the
# build output.
#
# Not part of `just check`: it needs extra toolchain components and it
# re-runs the suite under instrumentation. See docs/testing.md.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

if ! rustup component list --installed | grep -q '^llvm-tools'; then
    echo "coverage: the llvm-tools component is not installed for this toolchain." >&2
    echo "  rustup component add llvm-tools-preview" >&2
    exit 1
fi

if ! cargo llvm-cov --version >/dev/null 2>&1; then
    echo "coverage: cargo-llvm-cov is not installed." >&2
    echo "  cargo install cargo-llvm-cov --locked" >&2
    exit 1
fi

# The mode word is optional and comes first. A leading dash means "summary"
# plus cargo-llvm-cov flags, so `scripts/coverage.sh -- --lib` works.
if [[ "${1:-}" == -* || -z "${1:-}" ]]; then
    mode=summary
else
    mode="$1"
    shift
fi

# Same suite as `just test`, then the CLI tests again in direct mode.
# `--no-report` keeps profdata (it also skips the default clean), so the
# second run adds to the first. `clean` first drops a previous run.
#
# Cargo rejects `--workspace` together with `-p`. A package filter replaces
# the workspace selection. The direct-mode run is always `-p resonance-cli`
# (plus whatever package filter was passed).
run_both() {
    local packaged=0 arg
    for arg in "$@"; do
        if [[ "$arg" == "-p" || "$arg" == "--package" || "$arg" == --package=* ]]; then
            packaged=1
        fi
    done
    if [[ "$packaged" -eq 0 ]]; then
        cargo llvm-cov --all-features --locked --no-report --workspace "$@"
    else
        cargo llvm-cov --all-features --locked --no-report "$@"
    fi
    RESONANCE_TEST_DIRECT=1 cargo llvm-cov --all-features --locked --no-report -p resonance-cli "$@"
}

cargo llvm-cov clean --workspace

case "$mode" in
    summary)
        run_both "$@"
        # Default report is llvm-cov's per-file table, ending in TOTAL.
        # `--summary-only` is only valid with --json, --lcov, or --cobertura.
        cargo llvm-cov report
        ;;
    html)
        run_both "$@"
        cargo llvm-cov report --html --output-dir target/llvm-cov/html
        echo "HTML report: target/llvm-cov/html/index.html"
        ;;
    lcov)
        run_both "$@"
        mkdir -p target/llvm-cov
        cargo llvm-cov report --lcov --output-path target/llvm-cov/lcov.info
        echo "LCOV: target/llvm-cov/lcov.info"
        ;;
    *)
        echo "usage: scripts/coverage.sh [summary|html|lcov] [cargo llvm-cov test args]" >&2
        echo "  example: scripts/coverage.sh summary -p resonance-cli --test git_ux" >&2
        exit 2
        ;;
esac
