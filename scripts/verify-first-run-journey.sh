#!/usr/bin/env bash
# Epic 39 Slice F, updated Plan 122a A11: the first-run journey, against a
# real server and a real, genuinely empty Postgres — the same reason
# `verify-generated-client.sh` exists for the generated API client: a UI test
# that never talks to the real backend cannot catch what only shows up when
# it does.
#
#   1. build the console (graph-owl-ui embeds graphowl-app/dist at Rust
#      compile time — see _archived/README.md's `ui/` entry for the A0-A11
#      migration this replaced)
#   2. start a real, empty Postgres
#   3. start a real server, open auth mode (no OIDC to drive through Playwright)
#   4. run the Playwright journey against it
set -euo pipefail

cd "$(dirname "$0")/.."

echo "==> building the console"
(cd graphowl-app && npm run build)

echo "==> starting Postgres"
docker rm -f graph-owl-journey-check >/dev/null 2>&1 || true
docker run -d --rm --name graph-owl-journey-check \
    -e POSTGRES_PASSWORD=postgres -e POSTGRES_DB=graphowl \
    -p 55498:5432 postgres:18-alpine >/dev/null
trap 'docker rm -f graph-owl-journey-check >/dev/null 2>&1 || true; kill "${SERVER_PID:-}" 2>/dev/null || true' EXIT

until docker exec graph-owl-journey-check pg_isready -U postgres >/dev/null 2>&1; do sleep 1; done

echo "==> starting the server (open mode, so the journey has no OIDC redirect to drive)"
DATABASE_URL=postgres://postgres:postgres@localhost:55498/graphowl \
    BIND_ADDR=127.0.0.1:8099 OIDC_ISSUER= GRAPH_OWL_JWT_SECRET= \
    cargo run -q -p graph-owl-server --bin graph-owl-server >/tmp/journey-check-server.log 2>&1 &
SERVER_PID=$!

until curl -sf http://127.0.0.1:8099/health >/dev/null 2>&1; do sleep 1; done

echo "==> running the first-run journey against a genuinely empty database"
(cd graphowl-app && GRAPH_OWL_BASE_URL=http://127.0.0.1:8099 npx playwright test)

echo "==> ok: empty-database first run through to the first ingested asset, keyboard-only, zero axe violations"
