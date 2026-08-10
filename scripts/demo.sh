#!/usr/bin/env bash
# Launches the full demo: Postgres, the seeded bank estate, and the server with
# the console embedded. One command, one binary, one database.
#
#   ./scripts/demo.sh          light  — no auth, everything visible
#   ./scripts/demo.sh --secure        — HS256 JWT, with the two-principal policy
#   ./scripts/demo.sh          OIDC auto-detected from .env (no flag needed)
#   ./scripts/demo.sh --gst           — plus the GST pack, loaded and reconciled
#   ./scripts/demo.sh --stop          — tear it all down
set -euo pipefail

CONTAINER=graphowl-demo
PG_PORT=55432
APP_PORT=8080
PG_URL="postgres://postgres:postgres@localhost:${PG_PORT}/postgres"
APP_URL="postgres://postgres:postgres@localhost:${PG_PORT}/graphowl"
SECRET="demo-secret-not-for-production"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

say() { printf "\n\033[1;36m▸ %s\033[0m\n" "$1"; }
die() { printf "\n\033[1;31m✗ %s\033[0m\n" "$1" >&2; exit 1; }

stop() {
  say "Stopping"
  lsof -ti:${APP_PORT} 2>/dev/null | xargs kill -9 2>/dev/null || true
  docker rm -f ${CONTAINER} >/dev/null 2>&1 || true
  echo "  server and database stopped"
  exit 0
}

SECURE=false
GST=false
case "${1:-}" in
  --stop) stop ;;
  --secure) SECURE=true ;;
  --gst) GST=true ;;
  "") ;;
  *) die "unknown option: $1 (expected --secure, --gst or --stop)" ;;
esac

# Auto-detect OIDC from .env (the server loads it the same way at startup).
# When OIDC is active the demo auto-fetches a token via client_credentials
# grant, falling back to $OIDC_TOKEN if set explicitly.
OIDC_MODE=false
OIDC_ACCESS_TOKEN=""
OIDC_ISSUER_URL=""
OIDC_AUDIENCE_URL=""
if [ "${SECURE}" = false ] && [ -f "${ROOT}/.env" ]; then
  _oidc=$(grep '^OIDC_ISSUER=' "${ROOT}/.env" 2>/dev/null | head -1 || true)
  _aud=$(grep '^OIDC_AUDIENCE=' "${ROOT}/.env" 2>/dev/null | head -1 || true)
  if [ -n "$_oidc" ]; then
    OIDC_MODE=true
    OIDC_ISSUER_URL="${_oidc#*=}"
    OIDC_AUDIENCE_URL="${_aud#*=}"
  fi
fi
# Environment beats .env
if [ -n "${OIDC_ISSUER:-}" ]; then OIDC_MODE=true; OIDC_ISSUER_URL="${OIDC_ISSUER}"; fi
if [ -n "${OIDC_AUDIENCE:-}" ]; then OIDC_AUDIENCE_URL="${OIDC_AUDIENCE}"; fi

# Acquire a token for OIDC-authenticated requests.
#
# This is auto-cataloguing's credential, not sign-in's — a human never sees it.
# It has to be a Machine-to-Machine application authorized in Auth0's dashboard
# for this API; the interactive SPA client the browser uses for PKCE login is a
# different application by design (Auth0 will not issue client_credentials
# tokens to it), so pointing OIDC_CLIENT_ID at the SPA client id fails here even
# though sign-in through that same client works fine.
#
# **Not fatal.** This step exists purely to save you clicking through the
# connector form once; failing it must not block the one thing that matters —
# reaching the console and signing in. A stale or missing M2M credential
# degrades to an empty catalog with a clear note, not a dead script.
OIDC_CATALOG_SKIPPED=false
if [ "${OIDC_MODE}" = true ]; then
  OIDC_ACCESS_TOKEN="${OIDC_TOKEN:-}"
  if [ -z "$OIDC_ACCESS_TOKEN" ]; then
    _cid=$(grep '^OIDC_CLIENT_ID=' "${ROOT}/.env" 2>/dev/null | head -1 | sed 's/^OIDC_CLIENT_ID=//' || true)
    _cs=$(grep '^OIDC_CLIENT_SECRET=' "${ROOT}/.env" 2>/dev/null | head -1 | sed 's/^OIDC_CLIENT_SECRET=//' || true)
    [ -z "$_cid" ] && _cid="${OIDC_CLIENT_ID:-}"
    [ -z "$_cs" ] && _cs="${OIDC_CLIENT_SECRET:-}"
    if [ -n "$_cid" ] && [ -n "$_cs" ]; then
      # `|| true` on the assignment, and a `try/except` inside the parser
      # rather than a bare `['access_token']` lookup: a 403 here must not take
      # the whole script down with it under `set -e`, and must not print a raw
      # traceback where a two-line warning would do.
      OIDC_ACCESS_TOKEN=$(curl -fsS -X POST "${OIDC_ISSUER_URL}oauth/token" \
        -H 'content-type: application/json' \
        -d "{\"client_id\":\"$_cid\",\"client_secret\":\"$_cs\",\"audience\":\"${OIDC_AUDIENCE_URL}\",\"grant_type\":\"client_credentials\"}" \
        2>/dev/null | python3 -c "
import sys, json
try:
    print(json.load(sys.stdin).get('access_token', ''))
except Exception:
    pass
" 2>/dev/null) || true
    fi
  fi
  if [ -z "$OIDC_ACCESS_TOKEN" ]; then
    echo
    echo "  ⚠ could not obtain an auto-catalogue token (client_credentials was" \
         "refused). The console will still start and Auth0 sign-in still" \
         "works — only auto-cataloguing the demo estate is skipped."
    echo "  Likely cause: OIDC_CLIENT_ID in .env names the browser's SPA" \
         "client, which Auth0 will not issue client_credentials tokens to." \
         "It needs its own Machine-to-Machine application, authorized for" \
         "${OIDC_AUDIENCE_URL} in the Auth0 dashboard."
    echo "  Or set OIDC_TOKEN yourself to skip acquisition entirely."
    OIDC_CATALOG_SKIPPED=true
  fi
fi

command -v docker >/dev/null || die "docker is required"
docker info >/dev/null 2>&1 || die "docker is not running — start Docker Desktop"

# ---------------------------------------------------------------- database
say "Starting Postgres on :${PG_PORT}"
docker rm -f ${CONTAINER} >/dev/null 2>&1 || true
docker run -d --name ${CONTAINER} \
  -e POSTGRES_PASSWORD=postgres -p ${PG_PORT}:5432 postgres:16-alpine >/dev/null
for _ in $(seq 1 60); do
  docker exec ${CONTAINER} pg_isready -U postgres >/dev/null 2>&1 && break
  sleep 1
done
docker exec ${CONTAINER} pg_isready -U postgres >/dev/null 2>&1 || die "Postgres did not become ready"
echo "  ready"

say "Seeding the source estate (Indian retail + corporate banking)"
docker exec -i ${CONTAINER} psql -U postgres -q < "${ROOT}/demo/seed-bank.sql"
docker exec ${CONTAINER} psql -U postgres -qtAc "CREATE DATABASE graphowl" >/dev/null
TABLES=$(docker exec ${CONTAINER} psql -U postgres -qtAc \
  "SELECT count(*) FROM information_schema.tables
   WHERE table_schema IN ('core_banking','payments','lending','risk','regulatory')")
echo "  ${TABLES} tables and views across 5 schemas"

# ---------------------------------------------------------------- frontend
say "Building the console"
if [ ! -d "${ROOT}/ui/node_modules" ]; then
  echo "  installing dependencies (first run only)"
  (cd "${ROOT}/ui" && npm install --silent)
fi
(cd "${ROOT}/ui" && npm run build >/dev/null)
echo "  $(du -h "${ROOT}/ui/dist/static/"*.js | awk '{print $1}' | head -1) bundle"

# ---------------------------------------------------------------- backend
say "Building the server"
(cd "${ROOT}" && cargo build --release -p graph-owl-server 2>&1 | tail -1)

say "Starting graph-owl on :${APP_PORT}"
lsof -ti:${APP_PORT} 2>/dev/null | xargs kill -9 2>/dev/null || true
# When --secure is passed, suppress OIDC_ISSUER so dotenvy in .env
# doesn't switch the server to OIDC mode (which would reject the HS256
# self-signed tokens the script generates). When not --secure, OIDC mode
# auto-detected from .env works.
if [ "${SECURE}" = true ]; then
  DATABASE_URL="${APP_URL}" GRAPH_OWL_JWT_SECRET="${SECRET}" \
    OIDC_ISSUER="" \
    nohup "${ROOT}/target/release/graph-owl-server" > /tmp/graphowl.log 2>&1 &
else
  DATABASE_URL="${APP_URL}" \
    nohup "${ROOT}/target/release/graph-owl-server" > /tmp/graphowl.log 2>&1 &
fi
for _ in $(seq 1 30); do
  curl -fsS "http://localhost:${APP_PORT}/health" >/dev/null 2>&1 && break
  sleep 1
done
curl -fsS "http://localhost:${APP_PORT}/health" >/dev/null 2>&1 || {
  tail -20 /tmp/graphowl.log; die "server did not start"
}
head -1 /tmp/graphowl.log | sed 's/^/  /'

# ---------------------------------------------------------------- catalogue
token() {
  python3 - "$1" <<'PY'
import base64, hmac, hashlib, json, sys
def seg(d): return base64.urlsafe_b64encode(json.dumps(d).encode()).rstrip(b"=")
msg = seg({"alg":"HS256","typ":"JWT"}) + b"." + seg({"sub":sys.argv[1],"name":sys.argv[1],"exp":4102444800})
sig = base64.urlsafe_b64encode(hmac.new(b"demo-secret-not-for-production", msg, hashlib.sha256).digest()).rstrip(b"=")
print((msg + b"." + sig).decode())
PY
}

if [ "${OIDC_CATALOG_SKIPPED}" = true ]; then
  say "Skipping auto-catalogue (no M2M token — see the warning above)"
else
  say "Cataloguing the estate"
  BODY="{\"connectionString\":\"${PG_URL}\",\"serviceName\":\"hdfc-core\",\"includeSchemas\":[\"core_banking\",\"payments\",\"lending\",\"risk\",\"regulatory\"]}"
  if [ "${OIDC_MODE}" = true ]; then
    RUN=$(curl -fsS -X POST "http://localhost:${APP_PORT}/connectors/postgres/runs" \
      -H "authorization: Bearer ${OIDC_ACCESS_TOKEN}" -H 'content-type: application/json' -d "${BODY}")
  elif [ "${SECURE}" = true ]; then
    RUN=$(curl -fsS -X POST "http://localhost:${APP_PORT}/connectors/postgres/runs" \
      -H "authorization: Bearer $(token root)" -H 'content-type: application/json' -d "${BODY}")
  else
    RUN=$(curl -fsS -X POST "http://localhost:${APP_PORT}/connectors/postgres/runs" \
      -H 'content-type: application/json' -d "${BODY}")
  fi
  echo "${RUN}" | python3 -c "import sys,json;d=json.load(sys.stdin);print(f\"  {d['created']} assets, {d['failed']} failed\")"
fi

if [ "${OIDC_MODE}" = true ]; then
  say "OIDC authentication active"
  echo "  Sign in through the console with your identity provider"
  echo "  Token subject is automatically provisioned on first request"
  echo "  Admin status from GRAPH_OWL_ADMIN_SUBJECTS in .env"
  if [ "${OIDC_CATALOG_SKIPPED}" = true ]; then
    echo "  Catalogue is empty — auto-cataloguing was skipped, see the warning above"
  fi
elif [ "${SECURE}" = true ]; then
  say "Configuring two principals"
  P() { docker exec ${CONTAINER} psql -U postgres -d graphowl -qtAc "$1" >/dev/null; }
  P "UPDATE users SET is_admin = TRUE WHERE id = 'root'"
  P "INSERT INTO roles (name) VALUES ('risk-analyst') ON CONFLICT DO NOTHING"
  P "INSERT INTO policies (name, rules) VALUES ('analyst-baseline', '[
       {\"name\":\"read-catalog\",\"effect\":\"allow\",\"operations\":[\"viewBasic\",\"viewDetails\"],\"resources\":{\"type\":\"all\"}},
       {\"name\":\"no-customer-pii\",\"effect\":\"deny\",\"operations\":[\"viewBasic\",\"viewDetails\"],\"resources\":{\"type\":\"fqnPrefix\",\"value\":\"hdfc-core.postgres.core_banking\"}}
     ]'::jsonb) ON CONFLICT DO NOTHING"
  P "INSERT INTO role_policies (role, policy) VALUES ('risk-analyst','analyst-baseline') ON CONFLICT DO NOTHING"
  curl -fsS -H "authorization: Bearer $(token asha)" \
    "http://localhost:${APP_PORT}/assets/stats" >/dev/null   # auto-provisions asha
  P "INSERT INTO user_roles (user_id, role) VALUES ('asha','risk-analyst') ON CONFLICT DO NOTHING"
  echo "  root  — admin, sees everything"
  echo "  asha  — risk analyst, denied core_banking (PAN, Aadhaar, CKYC)"
  echo
  echo "  Tokens:"
  echo "    root: $(token root)"
  echo "    asha: $(token asha)"
fi

# The GST pack — Epic 105. Deliberately the *last* step and deliberately
# additive: the bank estate above is untouched, the pack brings its own
# namespace, and nothing about the server changed to accept it. That is the
# demonstration, so it runs against the same binary that was already up.
if [ "${GST}" = true ]; then
  say "Loading the GST pack"
  if ! command -v python3 >/dev/null 2>&1; then
    die "python3 is needed to load a pack (the loader is stdlib-only, no install)"
  fi

  GST_TOKEN=""
  if [ "${SECURE}" = true ]; then
    GST_TOKEN="$(token root)"
  elif [ "${OIDC_MODE}" = true ]; then
    GST_TOKEN="${OIDC_ACCESS_TOKEN}"
  fi

  # Fixture mode is the *normal* path, not a degraded one. A live GSTR-2B
  # fetch needs a GSP account and credentials nobody has by default, and a
  # demo that cannot run without them is a demo nobody runs. Say so plainly
  # rather than failing or pretending the numbers are live.
  if [ -n "${GST_LIVE_SOURCE:-}" ]; then
    echo "  live source configured: ${GST_LIVE_SOURCE}"
  else
    echo "  fixture mode — the register and GSTR-2B come from packs/gst/fixtures/"
    echo "  (set GST_LIVE_SOURCE to point the connector at a real return)"
  fi

  PYTHONPATH="${ROOT}/connectors/python" python3 -m graph_owl_packs.cli \
    "${ROOT}/packs/gst" --server "http://localhost:${APP_PORT}" \
    ${GST_TOKEN:+--token "${GST_TOKEN}"} \
    | python3 -c 'import json,sys; d=json.load(sys.stdin); print("  namespace", d["namespaceCode"], "·", d["landed"], "subjects ·", len(d["rejected"]), "rejected")'

  say "Running the reconciliation"
  PYTHONPATH="${ROOT}/connectors/python" python3 -c '
import sys
from graph_owl_packs.cli import reconcile_main
sys.exit(reconcile_main(sys.argv[1:]))' \
    "${ROOT}/packs/gst" --server "http://localhost:${APP_PORT}" \
    ${GST_TOKEN:+--token "${GST_TOKEN}"} \
    | python3 -c 'import json,sys; d=json.load(sys.stdin); print(" ", d["rulesEvaluated"], "rules ·", d["opened"], "findings opened ·", d["alreadyOpen"], "already open")'

  echo
  echo "  Review them at http://localhost:${APP_PORT}/?section=review&kind=findings"
fi

cat <<EOF

  ────────────────────────────────────────────────────────────
   Console   http://localhost:${APP_PORT}
   API      http://localhost:${APP_PORT}/assets/stats
   Postgres localhost:${PG_PORT}  (postgres/postgres)
   Logs     tail -f /tmp/graphowl.log
   Stop     ./scripts/demo.sh --stop
  ────────────────────────────────────────────────────────────

EOF
