#!/usr/bin/env bash
# Epic 43 Slice F: prove graph-owl-langchain works against a *real* service.
#
# Mirrors scripts/verify-sdks.sh's own shape for the same reason stated
# there: a unit test of this package tests its own opinion of the MCP
# contract. Only a live round trip tests the contract itself — and this is
# exactly the check that already found three real findings (an isError
# shape correction, an RFC 9457 vs JSON-RPC error-body correction, and the
# checkpointer's asset/capability prerequisites) that no mock would have
# caught, recorded in plans/43-framework-integrations.md.
#
#   1. start a real server against a real Postgres, open mode
#   2. run the package's full test suite against it, including the
#      contract-drift and live-service tests that skip without one
set -euo pipefail

cd "$(dirname "$0")/.."

PORT="${GRAPH_OWL_LANGCHAIN_PORT:-8098}"
PG_PORT="${GRAPH_OWL_LANGCHAIN_PG_PORT:-55499}"
CONTAINER=graph-owl-langchain-check
VENV="${TMPDIR:-/tmp}/graph-owl-langchain-venv"

echo "==> starting Postgres"
docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
docker run -d --rm --name "$CONTAINER" \
    -e POSTGRES_PASSWORD=postgres -e POSTGRES_DB=graphowl \
    -p "$PG_PORT":5432 postgres:18-alpine >/dev/null
trap 'docker rm -f '"$CONTAINER"' >/dev/null 2>&1 || true; kill "${SERVER_PID:-}" 2>/dev/null || true' EXIT

until docker exec "$CONTAINER" pg_isready -U postgres >/dev/null 2>&1; do sleep 1; done

echo "==> starting the server"
# Open mode, same reasoning as verify-sdks.sh: this checks the *adapter*,
# not an identity provider. record_memory's own capability gate (Epic 32,
# admin-only, human-only) still applies even in open mode — the
# contract-drift suite's search/list checks do not need it; a checkpointer
# round-trip against a live service would additionally need a granted
# `recordMemory` capability, which this script does not attempt to
# automate for the reason plans/43-framework-integrations.md gives: that
# gate exists on purpose, and working around it here would defeat it.
DATABASE_URL="postgres://postgres:postgres@localhost:$PG_PORT/graphowl" \
    BIND_ADDR="127.0.0.1:$PORT" OIDC_ISSUER= GRAPH_OWL_JWT_SECRET= \
    cargo run -q -p graph-owl-server --bin graph-owl-server >/tmp/langchain-check-server.log 2>&1 &
SERVER_PID=$!

until curl -sf "http://127.0.0.1:$PORT/health" >/dev/null 2>&1; do sleep 1; done

echo "==> running graph-owl-langchain's suite against the live service"
python3 -m venv "$VENV"
"$VENV/bin/pip" -q install -e "integrations/langchain[dev]"
(
    cd integrations/langchain
    GRAPH_OWL_TEST_ENDPOINT="http://127.0.0.1:$PORT" \
        GRAPH_OWL_STRUCTURAL_CHECK_BASE="${GRAPH_OWL_STRUCTURAL_CHECK_BASE:-}" \
        "$VENV/bin/pytest" -q
)

echo "==> ok: graph-owl-langchain round-trips against a live service"
