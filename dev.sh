#!/usr/bin/env bash
# Start the full dev stack for both apps in one shot:
#
#   graph-owl-server  : 8080   Rust backend (graphowl-app's vite proxy targets this)
#   reco-now backend  : 8000   FastAPI (reconow frontend's vite proxies /api here)
#   reco-now frontend : 5173   vite default (RecoNow/frontend/vite.config.ts)
#   graphowl frontend : 5174   graphowl-app/vite.config.ts server.port
#
# Port allocation and env defaults mirror ext-apps/RecoNow/run-dev.sh — see
# that file for why 8080 vs 8000 matters (both backends default to 8080, and
# a collision makes Reco Now call itself instead of graph-owl).
#
# Everything runs detached with logs under $LOGDIR; re-run to restart.
# Stop with: scripts/dev-all.sh stop
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PGHOST="${PGHOST:-127.0.0.1}"
PGPORT="${PGPORT:-55000}"
PGUSER="${PGUSER:-postgres}"
PGPASSWORD="${PGPASSWORD:-postgres}"
GRAPHOWL_DATABASE_URL="${GRAPHOWL_DATABASE_URL:-postgresql://${PGUSER}:${PGPASSWORD}@${PGHOST}:${PGPORT}/graphowl_reconow}"
RECONOW_DATABASE_URL="${RECONOW_DATABASE_URL:-postgresql://${PGUSER}:${PGPASSWORD}@${PGHOST}:${PGPORT}/reconow}"
LOGDIR="${LOGDIR:-/tmp/graphowl-reconow-run}"
mkdir -p "$LOGDIR"

kill_port() { lsof -ti:"$1" | xargs kill 2>/dev/null || true; }

stop_all() {
  echo "==> stopping"
  for port in 5174 5173 8080 8000; do kill_port "$port"; done
}

if [ "${1:-}" = "stop" ]; then stop_all; exit 0; fi

if [ ! -x "$ROOT/target/debug/graph-owl-server" ]; then
  echo "==> building graph-owl-server (not built yet)"
  cargo build -p graph-owl-server
fi

for app_dir in "$ROOT/graphowl-app" "$ROOT/ext-apps/RecoNow/frontend"; do
  if [ ! -d "$app_dir/node_modules" ]; then
    echo "==> npm install in ${app_dir#$ROOT/}"
    (cd "$app_dir" && npm install)
  fi
done

echo "==> graph-owl-server on :8080"
kill_port 8080
sleep 1
(cd "$ROOT" && DATABASE_URL="$GRAPHOWL_DATABASE_URL" \
BIND_ADDR="0.0.0.0:8080" OIDC_ISSUER= GRAPH_OWL_JWT_SECRET= \
  nohup "$ROOT/target/debug/graph-owl-server" > "$LOGDIR/graphowl.log" 2>&1 < /dev/null &)

echo "==> reco-now backend on :8000"
kill_port 8000
sleep 1
(cd "$ROOT/ext-apps/RecoNow/backend" && DATABASE_URL="$RECONOW_DATABASE_URL" \
GRAPH_OWL_SERVER="http://127.0.0.1:8080" \
  nohup .venv/bin/uvicorn app.main:app --host 127.0.0.1 --port 8000 \
  > "$LOGDIR/reconow.log" 2>&1 < /dev/null &)

echo "==> reco-now frontend on :5173"
kill_port 5173
(cd "$ROOT/ext-apps/RecoNow/frontend" && nohup npm run dev > "$LOGDIR/reconow-ui.log" 2>&1 < /dev/null &)

echo "==> graphowl frontend on :5174"
kill_port 5174
(cd "$ROOT/graphowl-app" && nohup npm run dev > "$LOGDIR/graphowl-ui.log" 2>&1 < /dev/null &)

sleep 8
echo "==> health"
check() { curl -sf -m 5 "$1" >/dev/null && echo "  ok    $2" || echo "  FAILED $2"; }
check http://127.0.0.1:8080/health      "graphowl backend  http://localhost:8080"
check http://127.0.0.1:8000/api/health  "reconow backend   http://localhost:8000"
check http://127.0.0.1:5173/            "reconow frontend  http://localhost:5173"
check http://127.0.0.1:5174/            "graphowl frontend http://localhost:5174"

echo
echo "Open:"
echo "  graphowl console  http://localhost:5174"
echo "  reconow UI        http://localhost:5173"
echo "Logs: $LOGDIR/{graphowl,reconow,reconow-ui,graphowl-ui}.log"
echo "Stop: $0 stop"
