#!/usr/bin/env bash
# The verification gate — run ONCE per epic, never per slice.
#
#     scripts/gate.sh                      # scoped to crates the epic touched
#     scripts/gate.sh --full               # whole workspace (before a push)
#     scripts/gate.sh -p graph-owl-cli     # explicit crates
#
# While writing code, use the cheap tier instead:
#
#     CARGO_TARGET_DIR=/tmp/check cargo check -p <crate> --all-targets
#
# which takes no workspace build lock and costs seconds. This script is what
# you run when the whole epic is written.
#
# **Scoped by default, and that is the setting that matters.** Measured 2 Aug
# 2026: the full workspace run costs 1118s, a single-crate run 185s — a 6x
# saving, larger than batching several epics into one gate would ever give,
# and without the cost that makes batching a bad trade (N epics of failures
# arriving together, in unfamiliar code, with no way to attribute them).
# `.github/workflows/ci.yml` already runs the full workspace suite plus
# doc-tests, so the exhaustive pass has an owner; running it locally on every
# epic duplicates CI while blocking the person waiting.
#
# Ordering is deliberate and was got wrong repeatedly before it was written
# down: fmt and clippy CHANGE THE CODE, so running them after the suite means
# running the suite twice.
set -euo pipefail

cd "$(dirname "$0")/.."

# --- 0. Environment, before believing any timing -------------------------
# Every "slow build" investigation in this project's history ended in an
# environmental cause, and each cost an hour of looking at the wrong thing.
# `|| true` on every one of these: with `set -o pipefail`, a `pgrep` or
# `docker` that legitimately matches nothing exits 1, the assignment inherits
# it, and `set -e` kills the script — **silently, before running any of the
# gate**. That happened: a run reported nothing and was read as green when it
# had in fact done nothing at all. A gate that can pass without executing is
# worse than no gate.
containers=$(docker ps -q 2>/dev/null | wc -l | tr -d ' ' || true)
if [[ ${containers:-0} -gt 4 ]]; then
    echo "WARNING: $containers docker containers running." >&2
    echo "  Leaked test containers make everything slower (see CLAUDE.md)." >&2
    echo "  docker ps --format '{{.Names}}\t{{.Image}}'" >&2
fi
for proc in cargo claude rustc; do
    n=$(pgrep -x "$proc" 2>/dev/null | wc -l | tr -d ' ' || true)
    # `cargo` counts this script's own invocation once it starts.
    if [[ ${n:-0} -gt 1 ]]; then
        echo "WARNING: $n '$proc' processes — something else is competing." >&2
    fi
done

# --- 0b. Decide scope ----------------------------------------------------
# Crates with uncommitted changes, unless told otherwise. A gate that tests
# what was not touched is spending minutes to re-prove what CI already knows.
FULL=0
SCOPE=()
if [[ ${1:-} == "--full" ]]; then
    FULL=1
elif [[ $# -gt 0 ]]; then
    SCOPE=("$@")
else
    # Uncommitted changes first; if the tree is clean, fall back to whatever
    # is committed-but-unpushed. **The clean-tree case is the common one** —
    # it is exactly the state right after committing an epic, which is when
    # a gate before pushing is most wanted, and scoping it to "nothing" would
    # silently promote every such run to a full workspace pass.
    CHANGED=$(git status --porcelain | awk '{print $NF}' || true)
    if [[ -z $CHANGED ]] && git rev-parse --abbrev-ref '@{upstream}' >/dev/null 2>&1; then
        CHANGED=$(git diff --name-only '@{upstream}'...HEAD || true)
    fi
    while IFS= read -r crate; do
        [[ -n $crate ]] && SCOPE+=(-p "$crate")
    done < <(printf '%s\n' "$CHANGED" \
        | grep -oE 'crates/[^/]+' | sort -u | cut -d/ -f2 || true)
fi

if [[ $FULL -eq 1 || ${#SCOPE[@]} -eq 0 ]]; then
    SCOPE=(--workspace)
    echo "==> scope: whole workspace"
else
    echo "==> scope: ${SCOPE[*]}"
fi

# --- 1. Format (changes code) --------------------------------------------
echo "==> fmt"
cargo fmt --all

# --- 2. Lint (changes nothing, but reads everything) ----------------------
echo "==> clippy"
cargo clippy "${SCOPE[@]}" --all-targets

# --- 3. Compile everything -----------------------------------------------
# The tier that earns its place: every cross-crate breakage in this
# project's history has been a compile error, not a test failure.
echo "==> build"
cargo build "${SCOPE[@]}" --tests

# --- 4. Behaviour --------------------------------------------------------
# `nextest` over `cargo test`: it never builds doc-tests (84 minutes, zero
# behavioural coverage) and schedules across binaries instead of serialising
# behind the slowest one. `.config/nextest.toml` puts the container-backed
# binaries in a serial group, so those stay safe while everything else runs
# wide — which `cargo test -- --test-threads=1` could not express.
echo "==> test"
if command -v cargo-nextest >/dev/null 2>&1; then
    cargo nextest run "${SCOPE[@]}"
else
    echo "  (cargo-nextest not installed; falling back to cargo test)" >&2
    cargo test "${SCOPE[@]}" --lib --tests -- --test-threads=1
fi

echo
echo "Gate passed — and it actually ran: $(date -u +%H:%M:%SZ)."
echo
echo "This was a SCOPED run unless you passed --full. The whole-workspace"
echo "suite and the doc-tests are CI's job (.github/workflows/ci.yml) — that"
echo "is where the exhaustive pass belongs, not in front of the person"
echo "waiting to commit. Before a push, either let CI answer or run:"
echo "    scripts/gate.sh --full && cargo test --workspace --doc"
