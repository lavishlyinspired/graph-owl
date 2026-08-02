#!/usr/bin/env bash
# The verification gate — run ONCE per epic, never per slice.
#
# While writing code, use the cheap tier instead:
#
#     CARGO_TARGET_DIR=/tmp/check cargo check -p <crate> --all-targets
#
# which takes no workspace build lock and costs seconds. This script is what
# you run when the whole epic is written.
#
# Ordering is deliberate and was got wrong repeatedly before it was written
# down: fmt and clippy CHANGE THE CODE, so running them after the suite means
# running the suite twice.
set -euo pipefail

cd "$(dirname "$0")/.."

# --- 0. Environment, before believing any timing -------------------------
# Every "slow build" investigation in this project's history ended in an
# environmental cause, and each cost an hour of looking at the wrong thing.
containers=$(docker ps -q 2>/dev/null | wc -l | tr -d ' ')
if [[ ${containers:-0} -gt 4 ]]; then
    echo "WARNING: $containers docker containers running." >&2
    echo "  Leaked test containers make everything slower (see CLAUDE.md)." >&2
    echo "  docker ps --format '{{.Names}}\t{{.Image}}'" >&2
fi
for proc in cargo claude rustc; do
    n=$(pgrep -x "$proc" 2>/dev/null | wc -l | tr -d ' ')
    # `cargo` counts this script's own invocation once it starts.
    if [[ ${n:-0} -gt 1 ]]; then
        echo "WARNING: $n '$proc' processes — something else is competing." >&2
    fi
done

# --- 1. Format (changes code) --------------------------------------------
echo "==> fmt"
cargo fmt --all

# --- 2. Lint (changes nothing, but reads everything) ----------------------
echo "==> clippy"
cargo clippy --workspace --all-targets

# --- 3. Compile everything -----------------------------------------------
# The tier that earns its place: every cross-crate breakage in this
# project's history has been a compile error, not a test failure.
echo "==> build"
cargo build --workspace --tests

# --- 4. Behaviour --------------------------------------------------------
# `nextest` over `cargo test`: it never builds doc-tests (84 minutes, zero
# behavioural coverage) and schedules across binaries instead of serialising
# behind the slowest one. `.config/nextest.toml` puts the container-backed
# binaries in a serial group, so those stay safe while everything else runs
# wide — which `cargo test -- --test-threads=1` could not express.
echo "==> test"
if command -v cargo-nextest >/dev/null 2>&1; then
    cargo nextest run --workspace
else
    echo "  (cargo-nextest not installed; falling back to cargo test)" >&2
    cargo test --workspace --lib --tests -- --test-threads=1
fi

echo
echo "Gate passed. Doc-tests are NOT part of this gate — run them before"
echo "pushing only:  cargo test --workspace --doc"
