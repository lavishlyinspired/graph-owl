#!/usr/bin/env bash
# Why is the test suite slow today?
#
# Run this *before* blaming the code. Every slowdown this project has had was
# environmental, and each time it cost an hour of looking in the wrong place:
#
#   - 146 leaked containers (a `static OnceCell` never drops, so testcontainers
#     never cleaned up). Same binary: 7.9s clean, 25.9s with the leftovers.
#   - 197 stale databases in the shared container, because the sweep that was
#     supposed to drop them was defined in `graph-owl-server`'s test harness and
#     never called. One binary: 4.0s with them, 2.2s without.
#
# Both were invisible in the code and obvious in thirty seconds of looking at
# Docker. This script is that thirty seconds, so it does not have to be
# rediscovered.
set -euo pipefail

CONTAINER=graph-owl-tests

# **The one that has cost the most.** Two `cargo test` runs at once contend on
# the shared Postgres container and on cargo's build lock, and the symptom is
# 30-second stalls before a binary's first test — which looks exactly like a
# fixture bug and is not. Measured: `cargo test -p graph-owl-server` is 74s
# alone and 228s alongside a second run.
# **Match the executable, not the command line.** `pgrep -f <anything>` matches
# every process whose *full command line* contains the pattern — including the
# shell running the pgrep, and including any wait-loop that mentions it. Two
# separate attempts at a "wait for the suite" loop in this project waited for
# themselves forever, and both then showed up in `ps` as extra concurrent
# suites, which is a wrong conclusion stacked on a wrong measurement.
#
# `pgrep -x cargo` matches the process *name* only, which is what was meant.
# `|| true` because **`pgrep` exits 1 when nothing matches**, which under
# `set -euo pipefail` killed this script silently — and it exits 1 precisely when
# no cargo is running, i.e. in the healthy case. So the diagnostic worked only
# when the environment was already broken, which is the one time you do not need
# it to tell you anything. Found 30 July 2026 after it printed nothing at all.
concurrent=$(pgrep -x cargo 2>/dev/null | wc -l | tr -d ' ' || true)
if [ "$concurrent" -gt 1 ]; then
    echo "!! ${concurrent} 'cargo test' processes are running."
    echo "   Two suites racing is the single biggest slowdown this project has had."
    echo "   pkill -f 'cargo test'  and start one."
    echo
fi

running=$(docker ps -q 2>/dev/null | wc -l | tr -d ' ' || echo 0)
total=$(docker ps -aq 2>/dev/null | wc -l | tr -d ' ' || echo 0)
echo "containers: ${running} running, ${total} total"
if [ "$running" -gt 10 ]; then
    echo "  ^ that is a lot. Leaked test containers degrade Docker badly."
fi
stopped=$(( total - running ))
if [ "$stopped" -gt 10 ]; then
    echo "  ^ ${stopped} stopped containers. Testcontainers looks the container up"
    echo "    on every binary's first use, and Docker Desktop degrades badly with"
    echo "    dead state — 28 of them once held 4.8GB. Fix: docker container prune -f"
fi

if ! docker ps --format '{{.Names}}' | grep -qx "$CONTAINER"; then
    echo "shared container '$CONTAINER' is not running — the next run starts it"
    exit 0
fi

port=$(docker port "$CONTAINER" 5432 2>/dev/null | cut -d: -f2 || true)
stale=$(PGPASSWORD=postgres psql -h localhost -p "$port" -U postgres -tAc \
    "SELECT count(*) FROM pg_database WHERE datname LIKE 't%';" 2>/dev/null || echo "?")
echo "test databases in $CONTAINER: ${stale}"
if [ "$stale" != "?" ] && [ "$stale" -gt 60 ]; then
    echo "  ^ the per-process sweep is not keeping up, or is not wired into a"
    echo "    test harness. Check every tests/common/mod.rs calls"
    echo "    sweep_stale_databases — it has silently gone missing from one before."
fi

shm=$(docker exec "$CONTAINER" df -h /dev/shm 2>/dev/null | awk 'NR==2 {print $2" total, "$5" used"}' || echo "unknown")
echo "/dev/shm: ${shm}"

echo
echo "Clean baselines, measured 30 July 2026 on 964 tests:"
echo "  one server test binary        ~1.8s"
echo "  cargo test -p graph-owl-server  ~74s  (69s of it real test execution)"
echo "  cargo test --workspace         ~241s (97s of it real test execution)"
echo
echo "If those numbers hold, the slowness is a rebuild rather than the"
echo "environment: time 'cargo build --workspace --tests' separately first."
echo
echo "And if a suite is running: do not compile anything until it finishes."
echo "cargo build, cargo clippy and cargo test -p <crate> all take the same"
echo "build lock and relink crates the running suite links against. That, not"
echo "the environment, is the usual reason a clean-looking workspace run takes"
echo "half an hour."
