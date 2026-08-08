#!/usr/bin/env bash
# Epic 36: prove the reference applications under examples/ work against a
# real service — mirrors verify-sdks.sh/verify-langchain.sh's own shape,
# and for the same reason: a unit test of an example tests its own opinion
# of the API, only a live round trip against the real thing tests the API.
#
# Two server phases, not one shared instance — found necessary, not
# designed in advance: agent-triage's filtered-view scenario needs JWT
# auth (a real, distinguishable principal a policy can be scoped to, which
# open mode has no way to express), but adapter-csv tests only convergence
# and per-item error reporting, neither of which is an authorization
# question. Running adapter-csv against the JWT-authed server would mean
# every one of its requests needs a bearer token and a viewer-role policy
# just to read back what it wrote — real setup cost for a property that
# has nothing to do with who is asking. Each app gets the simplest server
# its own acceptance criteria actually require.

set -euo pipefail

cd "$(dirname "$0")/.."

PORT="${GRAPH_OWL_EXAMPLES_PORT:-8099}"
PG_PORT="${GRAPH_OWL_EXAMPLES_PG_PORT:-55599}"
CONTAINER=graph-owl-examples-check
VENV="${TMPDIR:-/tmp}/graph-owl-examples-venv"
JWT_SECRET="graph-owl-example-fixture-secret-not-for-production"

echo "==> regenerating the contract from the code"
cargo run -q -p graph-owl-server --bin openapi > openapi.json

echo "==> generating the Python read client from the contract"
# Not committed — mirrors `sdk/typescript/src/generated/`
# (`verify-generated-client.sh`'s own reasoning: a committed copy is a
# second thing to keep in step with `openapi.json`). `uvx` runs it
# ephemerally, the same role `npx --yes openapi-typescript` plays for the
# TypeScript client.
uvx openapi-python-client generate --path openapi.json --meta none \
    --overwrite --output-path sdk/python/graph_owl_read_client/src/graph_owl_read_client

echo "==> surface purity"
python3 scripts/check-examples-purity.py

start_postgres() {
    docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
    docker run -d --rm --name "$CONTAINER" \
        -e POSTGRES_PASSWORD=postgres -e POSTGRES_DB=graphowl \
        -p "$PG_PORT":5432 postgres:18-alpine >/dev/null
    until docker exec "$CONTAINER" pg_isready -U postgres >/dev/null 2>&1; do sleep 1; done
}

stop_all() {
    docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
    kill "${SERVER_PID:-}" 2>/dev/null || true
}
trap stop_all EXIT

python3 -m venv "$VENV"
"$VENV/bin/pip" -q install pytest
"$VENV/bin/pip" -q install -e sdk/python
"$VENV/bin/pip" -q install -e sdk/python/graph_owl_read_client

echo "==> phase 1: open mode (adapter-csv, browse) "
start_postgres
DATABASE_URL="postgres://postgres:postgres@localhost:$PG_PORT/graphowl" \
    BIND_ADDR="127.0.0.1:$PORT" OIDC_ISSUER= GRAPH_OWL_JWT_SECRET= \
    cargo run -q -p graph-owl-server --bin graph-owl-server >/tmp/examples-check-server-open.log 2>&1 &
SERVER_PID=$!
until curl -sf "http://127.0.0.1:$PORT/health" >/dev/null 2>&1; do sleep 1; done

echo "==> adapter-csv"
GRAPH_OWL_TEST_ENDPOINT="http://127.0.0.1:$PORT" \
    "$VENV/bin/python" -m pytest -q sdk/python/tests/test_example_adapter_live.py

echo "==> browse"
GRAPH_OWL_TEST_ENDPOINT="http://127.0.0.1:$PORT" \
    "$VENV/bin/python" -m pytest -q examples/browse/test_browse.py

kill "$SERVER_PID" 2>/dev/null || true
wait "$SERVER_PID" 2>/dev/null || true

echo "==> phase 2: JWT auth enabled, admin=admin (agent-triage)"
start_postgres
DATABASE_URL="postgres://postgres:postgres@localhost:$PG_PORT/graphowl" \
    BIND_ADDR="127.0.0.1:$PORT" OIDC_ISSUER= \
    GRAPH_OWL_JWT_SECRET="$JWT_SECRET" GRAPH_OWL_ADMIN_SUBJECTS="admin" \
    cargo run -q -p graph-owl-server --bin graph-owl-server >/tmp/examples-check-server-jwt.log 2>&1 &
SERVER_PID=$!
until curl -sf "http://127.0.0.1:$PORT/health" >/dev/null 2>&1; do sleep 1; done

echo "==> agent-triage"
(
    cd examples/agent-triage
    GRAPH_OWL_TEST_ENDPOINT="http://127.0.0.1:$PORT" GRAPH_OWL_EXAMPLES_CONTAINER="$CONTAINER" \
        "$VENV/bin/python" -m pytest -q
)

echo "==> ok: reference applications round-trip against a live service"
