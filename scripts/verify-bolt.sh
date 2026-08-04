#!/usr/bin/env bash
# Epic 7d Slice F: prove the feature flag actually excludes Bolt, then prove
# a real driver works when it is compiled in.
#
#   1. off by default: graph-owl-bolt must not appear in the dependency tree
#      at all, asserted by `cargo tree`, not read off the source
#   2. start Postgres and a real graph-owl-server binary built *with*
#      `--features bolt`, BOLT_BIND_ADDR set
#   3. round-trip the official `neo4j` Python driver against it
#      (scripts/verify-bolt-driver.py)
set -euo pipefail

cd "$(dirname "$0")/.."

echo "==> the feature is off by default: graph-owl-bolt must not appear in the dependency tree"
if cargo tree -p graph-owl-server -i graph-owl-bolt >/dev/null 2>&1; then
    echo "FAIL: graph-owl-bolt is reachable from graph-owl-server without --features bolt" >&2
    exit 1
fi
echo "  confirmed: absent by default"

echo "==> with --features bolt, it is present"
cargo tree -p graph-owl-server --features bolt -i graph-owl-bolt >/dev/null

PORT="${GRAPH_OWL_BOLT_CHECK_HTTP_PORT:-8098}"
BOLT_PORT="${GRAPH_OWL_BOLT_CHECK_BOLT_PORT:-17687}"
PG_PORT="${GRAPH_OWL_BOLT_CHECK_PG_PORT:-55497}"
CONTAINER=graph-owl-bolt-check
VENV="${TMPDIR:-/tmp}/graph-owl-bolt-driver-venv"

echo "==> starting Postgres"
docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
docker run -d --rm --name "$CONTAINER" \
    -e POSTGRES_PASSWORD=postgres -e POSTGRES_DB=graphowl \
    -p "$PG_PORT":5432 postgres:18-alpine >/dev/null
trap 'docker rm -f '"$CONTAINER"' >/dev/null 2>&1 || true; kill "${SERVER_PID:-}" 2>/dev/null || true' EXIT

until docker exec "$CONTAINER" pg_isready -U postgres >/dev/null 2>&1; do sleep 1; done

echo "==> starting the server, built with --features bolt"
# Open mode, same reasoning as verify-sdks.sh: this checks the driver, and an
# auth flow would be testing the identity provider instead. HELLO's own
# credential handling — success, failure, identity-equivalence with HTTP — is
# covered exhaustively by crates/graph-owl-server/tests/bolt.rs.
DATABASE_URL="postgres://postgres:postgres@localhost:$PG_PORT/graphowl" \
    BIND_ADDR="127.0.0.1:$PORT" BOLT_BIND_ADDR="127.0.0.1:$BOLT_PORT" \
    OIDC_ISSUER= GRAPH_OWL_JWT_SECRET= \
    cargo run -q -p graph-owl-server --bin graph-owl-server --features bolt \
    >/tmp/bolt-check-server.log 2>&1 &
SERVER_PID=$!

until curl -sf "http://127.0.0.1:$PORT/health" >/dev/null 2>&1; do sleep 1; done

echo "==> round-tripping the official neo4j Python driver"
python3 -m venv "$VENV"
"$VENV/bin/pip" -q install neo4j
GRAPH_OWL_BASE_URL="http://127.0.0.1:$PORT" GRAPH_OWL_BOLT_URI="bolt://127.0.0.1:$BOLT_PORT" \
    "$VENV/bin/python3" scripts/verify-bolt-driver.py

echo "==> ok: feature-off excludes the crate, feature-on speaks to a real driver"
