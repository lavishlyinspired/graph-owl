#!/usr/bin/env bash
# Epic 1 Slice K: prove the contract *describes the service*, not merely that it
# is well-formed.
#
# A spec can be valid and wrong. The only test that catches that is generating a
# client from it and running that client against a real server — which is what
# this does:
#
#   1. regenerate openapi.json from the code
#   2. generate a TypeScript client from openapi.json
#   3. start a real server against a real Postgres
#   4. drive create -> read -> list -> patch -> delete through the client
#
# The client is **not committed**. It is derived from openapi.json, and a
# committed copy is a second thing to keep in step with the first.
set -euo pipefail

cd "$(dirname "$0")/.."

echo "==> regenerating the contract from the code"
cargo run -q -p graph-owl-server --bin openapi > openapi.json

echo "==> generating a TypeScript client from the contract"
(cd ui && npx --yes openapi-typescript ../openapi.json -o src/generated/api.d.ts)

echo "==> starting Postgres"
docker rm -f graph-owl-client-check >/dev/null 2>&1 || true
docker run -d --rm --name graph-owl-client-check \
    -e POSTGRES_PASSWORD=postgres -e POSTGRES_DB=graphowl \
    -p 55499:5432 postgres:18-alpine >/dev/null
trap 'docker rm -f graph-owl-client-check >/dev/null 2>&1 || true; kill "${SERVER_PID:-}" 2>/dev/null || true' EXIT

until docker exec graph-owl-client-check pg_isready -U postgres >/dev/null 2>&1; do sleep 1; done

echo "==> starting the server"
# Open mode: this checks the *contract*, and an auth flow would be testing the
# identity provider instead.
DATABASE_URL=postgres://postgres:postgres@localhost:55499/graphowl \
    BIND_ADDR=127.0.0.1:8098 OIDC_ISSUER= GRAPH_OWL_JWT_SECRET= \
    cargo run -q -p graph-owl-server --bin graph-owl-server >/tmp/client-check-server.log 2>&1 &
SERVER_PID=$!

until curl -sf http://127.0.0.1:8098/health >/dev/null 2>&1; do sleep 1; done

echo "==> round-tripping through the generated client"
(cd ui && GRAPH_OWL_BASE_URL=http://127.0.0.1:8098 npx vitest run src/generated/roundtrip.test.ts)

echo "==> ok: the generated client round-trips against a live service"
