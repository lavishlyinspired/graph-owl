#!/usr/bin/env bash
# Start the full Reco Now stack locally, with the port allocation written down
# rather than left to whoever types the uvicorn command.
#
# Why this script exists: graph-owl-server and the Reco Now backend both
# default to :8080. Starting Reco Now on 8080 breaks two things at once and
# neither says so — the Vite proxy (which targets :8000) 404s every screen,
# and GRAPH_OWL_SERVER's default of localhost:8080 makes Reco Now call
# *itself* instead of graph-owl, so the GST pack never loads and no finding
# is ever produced. The UI still renders, which is what makes it dangerous.
#
#   graph-owl-server : 8080   (its own default; frontend/vite proxies here too)
#   reco-now backend : 8000   (what frontend/vite.config.ts proxies /api to)
#   reco-now frontend: 5173   (vite default)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PGHOST="${PGHOST:-127.0.0.1}"
PGPORT="${PGPORT:-55000}"
LOGDIR="${LOGDIR:-/tmp/reconow-run}"
mkdir -p "$LOGDIR"

# OIDC_ISSUER/GRAPH_OWL_JWT_SECRET are blanked deliberately to select
# AuthMode::Open for local dev — the same thing scripts/verify-*.sh do.
# .env ships an Auth0 issuer, which would otherwise 401 every call Reco Now
# makes, since Reco Now has no token to present.
echo "==> graph-owl-server on :8080"
lsof -ti:8080 | xargs kill 2>/dev/null || true
sleep 1
DATABASE_URL="postgresql://postgres:postgres@${PGHOST}:${PGPORT}/graphowl_reconow" \
BIND_ADDR="0.0.0.0:8080" OIDC_ISSUER= GRAPH_OWL_JWT_SECRET= \
  nohup "$ROOT/target/debug/graph-owl-server" > "$LOGDIR/graphowl.log" 2>&1 &

echo "==> reco-now backend on :8000"
lsof -ti:8000 | xargs kill 2>/dev/null || true
sleep 1
cd "$ROOT/ext-apps/RecoNow/backend"
DATABASE_URL="postgresql://postgres:postgres@${PGHOST}:${PGPORT}/reconow" \
GRAPH_OWL_SERVER="http://127.0.0.1:8080" \
  nohup .venv/bin/uvicorn app.main:app --host 127.0.0.1 --port 8000 \
  > "$LOGDIR/reconow.log" 2>&1 &

sleep 8
echo "==> health"
curl -sf -m 5 http://127.0.0.1:8080/health  >/dev/null && echo "  graph-owl  :8080 ok" || echo "  graph-owl  :8080 FAILED"
curl -sf -m 5 http://127.0.0.1:8000/api/health >/dev/null && echo "  reco-now   :8000 ok" || echo "  reco-now   :8000 FAILED"
# The check that would have caught the original bug: Reco Now must report a
# graph-owl server that is not itself.
curl -sf -m 8 http://127.0.0.1:8000/api/graphowl/status \
  | python3 -c 'import json,sys; s=json.load(sys.stdin)["server"]; print(f"  reco-now -> graph-owl at {s}" + ("  *** SELF-REFERENCE ***" if "8000" in s else ""))'
echo
echo "Frontend:  cd $ROOT/ext-apps/RecoNow/frontend && npm run dev   # :5173"
echo "Logs:      $LOGDIR/{graphowl,reconow}.log"
