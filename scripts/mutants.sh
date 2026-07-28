#!/usr/bin/env bash
# Run cargo-mutants the fast way.
#
#   scripts/mutants.sh <crate> <file>          # whole file (new code)
#   scripts/mutants.sh <crate> --diff <file>   # only what the diff touches
#
# Both apply `--cargo-test-arg --lib`, which is what makes this affordable:
# without it, cargo-mutants runs this workspace's *integration* tests, each of
# which starts a Postgres container, once per mutant. Measured 29 Jul 2026 on a
# 42-mutant file: over an hour by default, 9 minutes with `--lib`.
#
# It also avoids the baseline abort. cargo-mutants invokes `cargo test` without
# `--test-threads=1`, so the container suite hits the documented PortNotExposed
# contention and the run dies with "cargo test failed in an unmutated tree".
#
# See CLAUDE.md for the measurements, including what was tried and rejected
# (`-j` is 55% slower; debug settings and `--baseline skip` are noise).
set -euo pipefail

crate=${1:?usage: mutants.sh <crate> [--diff] <file>}
shift

if [[ ${1:-} == "--diff" ]]; then
    file=${2:?usage: mutants.sh <crate> --diff <file>}
    diff=$(mktemp -t mutants-diff)
    trap 'rm -f "$diff"' EXIT
    git diff HEAD -- "$file" > "$diff"
    if [[ ! -s $diff ]]; then
        echo "no uncommitted change in $file; use the whole-file form" >&2
        exit 1
    fi
    echo "mutating only the lines changed in $file"
    exec cargo mutants -p "$crate" --in-diff "$diff" --cargo-test-arg --lib
fi

exec cargo mutants -p "$crate" --file "${1:?usage: mutants.sh <crate> [--diff] <file>}" \
    --cargo-test-arg --lib
