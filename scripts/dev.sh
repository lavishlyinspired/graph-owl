#!/usr/bin/env bash
# Frontend development with hot reload.
#
# Runs the Rust server on :8080 and Vite on :5173. Vite proxies /api to the
# server, so the console reloads on save without rebuilding the binary — the
# embedded-asset path (rust-embed) requires a full rebuild and is what
# ./scripts/demo.sh uses.
#
#   ./scripts/dev.sh        assumes ./scripts/demo.sh has already seeded Postgres
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_PORT=8080
PG_PORT=55432

say() { printf "\n\033[1;36m▸ %s\033[0m\n" "$1"; }

docker exec graphowl-demo pg_isready -U postgres >/dev/null 2>&1 || {
  printf "\033[1;31m✗ Postgres is not running. Start it with ./scripts/demo.sh first.\033[0m\n" >&2
  exit 1
}

cleanup() {
  say "Stopping dev servers"
  [ -n "${SERVER_PID:-}" ] && kill "${SERVER_PID}" 2>/dev/null || true
  [ -n "${VITE_PID:-}" ] && kill "${VITE_PID}" 2>/dev/null || true
}
trap cleanup EXIT INT TERM

say "Starting the API on :${APP_PORT}"
lsof -ti:${APP_PORT} 2>/dev/null | xargs kill -9 2>/dev/null || true
(cd "${ROOT}" && cargo build -p graph-owl-server 2>&1 | tail -1)
DATABASE_URL="postgres://postgres:postgres@localhost:${PG_PORT}/graphowl" \
  "${ROOT}/target/debug/graph-owl-server" > /tmp/graphowl-dev.log 2>&1 &
SERVER_PID=$!
sleep 3
curl -fsS "http://localhost:${APP_PORT}/health" >/dev/null || {
  tail -20 /tmp/graphowl-dev.log; exit 1
}
echo "  api ready (logs: /tmp/graphowl-dev.log)"

say "Starting Vite with hot reload"
cd "${ROOT}/ui"
[ -d node_modules ] || npm install --silent
npm run dev &
VITE_PID=$!

cat <<EOF

  ────────────────────────────────────────────────────────────
   Console (hot reload)  http://localhost:5173
   API                   http://localhost:${APP_PORT}
   Ctrl-C stops both
  ────────────────────────────────────────────────────────────

EOF
wait
