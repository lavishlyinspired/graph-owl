#!/usr/bin/env bash
# Epic 16 Slice E: prove both SDKs work against a *real* service.
#
# The criterion is "an end-to-end test pushes through each SDK against a running
# service", and the reason it is worded that way is that unit tests of an SDK
# test the SDK's opinion of the contract. Only a live round-trip tests the
# contract.
#
#   1. regenerate openapi.json from the code
#   2. generate the TypeScript type layer from it
#   3. start a real server against a real Postgres
#   4. push through the Python SDK and the TypeScript SDK
set -euo pipefail

cd "$(dirname "$0")/.."

PORT="${GRAPH_OWL_SDK_PORT:-8097}"
PG_PORT="${GRAPH_OWL_SDK_PG_PORT:-55498}"
CONTAINER=graph-owl-sdk-check
# A dedicated, throwaway virtualenv. Never the caller's — an adapter author's
# environment is not ours to install into, and neither is a developer's.
VENV="${TMPDIR:-/tmp}/graph-owl-sdk-venv"

echo "==> regenerating the contract from the code"
cargo run -q -p graph-owl-server --bin openapi > openapi.json

echo "==> generating the TypeScript type layer"
(cd sdk/typescript && npm install --no-audit --no-fund --silent && npm run --silent generate)

echo "==> starting Postgres"
docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
docker run -d --rm --name "$CONTAINER" \
    -e POSTGRES_PASSWORD=postgres -e POSTGRES_DB=graphowl \
    -p "$PG_PORT":5432 postgres:18-alpine >/dev/null
trap 'docker rm -f '"$CONTAINER"' >/dev/null 2>&1 || true; kill "${SERVER_PID:-}" 2>/dev/null || true' EXIT

until docker exec "$CONTAINER" pg_isready -U postgres >/dev/null 2>&1; do sleep 1; done

echo "==> starting the server"
# Open mode: this checks the *SDKs*, and an auth flow would be testing the
# identity provider instead. The bot-principal path is documented in sdk/README.md.
DATABASE_URL="postgres://postgres:postgres@localhost:$PG_PORT/graphowl" \
    BIND_ADDR="127.0.0.1:$PORT" OIDC_ISSUER= GRAPH_OWL_JWT_SECRET= \
    cargo run -q -p graph-owl-server --bin graph-owl-server >/tmp/sdk-check-server.log 2>&1 &
SERVER_PID=$!

until curl -sf "http://127.0.0.1:$PORT/health" >/dev/null 2>&1; do sleep 1; done

echo "==> round-tripping the Python SDK"
python3 -m venv "$VENV"
"$VENV/bin/pip" -q install pytest
(cd sdk/python && PYTHONPATH=. GRAPH_OWL_BASE_URL="http://127.0.0.1:$PORT" "$VENV/bin/pytest" -q)

echo "==> round-tripping the TypeScript SDK"
(cd sdk/typescript && GRAPH_OWL_BASE_URL="http://127.0.0.1:$PORT" npx vitest run src)

echo "==> ok: both SDKs round-trip against a live service"
