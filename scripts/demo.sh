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

AGENT_PORT=8899

stop() {
  say "Stopping"
  lsof -ti:${APP_PORT} 2>/dev/null | xargs kill -9 2>/dev/null || true
  lsof -ti:${AGENT_PORT} 2>/dev/null | xargs kill -9 2>/dev/null || true
  docker rm -f ${CONTAINER} >/dev/null 2>&1 || true
  echo "  server, agent service and database stopped"
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
# Environment beats .env, and "beats" has to include turning OIDC *off* —
# not just adding to it. `${VAR+x}` (parameter existence) rather than
# `-n "${VAR:-}"` (non-empty) is what makes that possible: the caller can
# be in one of three states — never mentioned OIDC_ISSUER (fall through to
# .env), set it to a real value (use that, skip .env), or explicitly set it
# to "" to force OIDC off for this run regardless of what .env says. The
# old `-n "${OIDC_ISSUER:-}"` check could only ever detect the second case;
# an explicit empty override was indistinguishable from never having set it
# at all, so `OIDC_ISSUER= OIDC_AUDIENCE= ./scripts/demo.sh` — this
# script's own documented escape hatch — silently did nothing.
if [ -n "${OIDC_ISSUER+x}" ]; then
  if [ -n "${OIDC_ISSUER}" ]; then
    OIDC_MODE=true
    OIDC_ISSUER_URL="${OIDC_ISSUER}"
    OIDC_AUDIENCE_URL="${OIDC_AUDIENCE:-}"
  fi
  # else: OIDC_ISSUER explicitly set empty — OIDC stays off, .env is not consulted.
elif [ "${SECURE}" = false ] && [ -f "${ROOT}/.env" ]; then
  _oidc=$(grep '^OIDC_ISSUER=' "${ROOT}/.env" 2>/dev/null | head -1 || true)
  _aud=$(grep '^OIDC_AUDIENCE=' "${ROOT}/.env" 2>/dev/null | head -1 || true)
  if [ -n "$_oidc" ]; then
    OIDC_MODE=true
    OIDC_ISSUER_URL="${_oidc#*=}"
    OIDC_AUDIENCE_URL="${_aud#*=}"
  fi
fi

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
if [ ! -d "${ROOT}/graphowl-app/node_modules" ]; then
  echo "  installing dependencies (first run only)"
  (cd "${ROOT}/graphowl-app" && npm install --silent)
fi
(cd "${ROOT}/graphowl-app" && npm run build >/dev/null)
echo "  $(du -h "${ROOT}/graphowl-app/dist/static/"*.js | awk '{print $1}' | head -1) bundle"

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

# ---------------------------------------------------------------- agent service
# The console's Agent tab is a consumer of a *second*, separate process —
# `integrations/langchain/agent_service` — never ported into graph-owl-server
# itself (`00j-language-boundaries.md`). Skipping this step is not a
# degraded demo, it's a broken one: the Agent tab has no fallback and fails
# every question with "Failed to fetch". So this is not "not fatal" the way
# auto-cataloguing above is — a failure here still gets a loud warning, on
# purpose, rather than a silent gap discovered only when someone clicks Agent.
say "Starting the reconciliation agent on :${AGENT_PORT}"
lsof -ti:${AGENT_PORT} 2>/dev/null | xargs kill -9 2>/dev/null || true
AGENT_DIR="${ROOT}/integrations/langchain"
AGENT_VENV="${AGENT_DIR}/.venv"
if [ ! -d "${AGENT_VENV}" ]; then
  echo "  creating venv and installing dependencies (first run only)"
  python3 -m venv "${AGENT_VENV}" 2>/dev/null || true
  (cd "${AGENT_DIR}" && "${AGENT_VENV}/bin/pip" install -q -e ".[langgraph]" langchain-openai fastapi uvicorn) \
    || echo "  ⚠ dependency install failed — see above"
fi
if [ -x "${AGENT_VENV}/bin/uvicorn" ] || [ -x "${AGENT_VENV}/bin/python3" ]; then
  # This service does no .env loading of its own (by design — it only sees
  # what launches it), so the three LLM_* values it needs are pulled the
  # same way OIDC_CLIENT_ID/SECRET already are above: grepped out of .env,
  # never sourced wholesale, so this can't shadow the OIDC override logic
  # earlier in the script.
  _llm_base=$(grep '^LLM_API_BASE_URL=' "${ROOT}/.env" 2>/dev/null | head -1 | sed 's/^LLM_API_BASE_URL=//' || true)
  _llm_model=$(grep '^LLM_MODEL=' "${ROOT}/.env" 2>/dev/null | head -1 | sed 's/^LLM_MODEL=//' || true)
  _llm_key=$(grep '^LLM_API_KEY=' "${ROOT}/.env" 2>/dev/null | head -1 | sed 's/^LLM_API_KEY=//' || true)
  # Only OIDC mode gives the browser a real per-request token to forward
  # (`getAccessToken()` in agentClient.ts). Light and --secure mode both
  # leave agent_service with nothing to check unless given this placeholder
  # — matching agent_service/README.md's own documented rule, not a guess.
  _agent_token=""
  [ "${OIDC_MODE}" = false ] && _agent_token="demo-placeholder-token"
  (cd "${AGENT_DIR}" && \
    GRAPH_OWL_SERVER="http://localhost:${APP_PORT}" \
    ${_agent_token:+GRAPH_OWL_TOKEN="${_agent_token}"} \
    ${_llm_base:+LLM_API_BASE_URL="${_llm_base}"} \
    ${_llm_model:+LLM_MODEL="${_llm_model}"} \
    ${_llm_key:+LLM_API_KEY="${_llm_key}"} \
    nohup "${AGENT_VENV}/bin/python3" -m uvicorn agent_service.server:app --port ${AGENT_PORT} \
    > /tmp/graphowl-agent.log 2>&1 &)
  for _ in $(seq 1 30); do
    curl -fsS "http://localhost:${AGENT_PORT}/providers" >/dev/null 2>&1 && break
    sleep 1
  done
  curl -fsS "http://localhost:${AGENT_PORT}/providers" >/dev/null 2>&1 || {
    echo "  ⚠ agent service did not come up — the Agent tab will show 'Failed to fetch'"
    tail -10 /tmp/graphowl-agent.log | sed 's/^/    /'
  }
  echo "  ready"
else
  echo "  ⚠ no working venv at ${AGENT_VENV} — the Agent tab will show 'Failed to fetch'"
  echo "    see integrations/langchain/agent_service/README.md to set one up"
fi

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

  # **Refuse early rather than 401 into a traceback.** When OIDC or --secure is
  # active the pack load needs a bearer token; without one every call returns
  # 401 and the JSON parse downstream fails with "Expecting value: line 1
  # column 1", which points at Python rather than at the missing token.
  if [ "${SECURE}" = false ] && [ "${OIDC_MODE}" = true ] && [ -z "${GST_TOKEN}" ]; then
    printf "\n\033[1;31m✗ %s\033[0m\n" "Cannot load the GST pack: OIDC is active but no token was obtained"
    echo "  The pack loader writes namespaces and predicates, which need a bearer token."
    echo
    echo "  Either set OIDC_CLIENT_ID/OIDC_CLIENT_SECRET in .env so the demo can fetch one,"
    echo "  or run the demo without OIDC:"
    echo "      OIDC_ISSUER= OIDC_AUDIENCE= ./scripts/demo.sh --gst"
    exit 1
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

  if ! _loaded=$(PYTHONPATH="${ROOT}/connectors/python" python3 -m graph_owl_packs.cli \
      "${ROOT}/packs/gst" --server "http://localhost:${APP_PORT}" \
      ${GST_TOKEN:+--token "${GST_TOKEN}"}); then
    die "the GST pack failed to load — see the error above"
  fi
  echo "${_loaded}" | python3 -c 'import json,sys; d=json.load(sys.stdin); print("  namespace", d["namespaceCode"], "·", d["landed"], "subjects ·", len(d["rejected"]), "rejected")'

  # Epic 105 P5b: the rule engine is native now — this triggers
  # `Catalog::reconcile_pack` over HTTP, it does not evaluate anything
  # itself. The pack id, not a directory: its rules were already registered
  # server-side as part of the load step above.
  say "Running the reconciliation"
  if ! _ran=$(PYTHONPATH="${ROOT}/connectors/python" python3 -c '
import sys
from graph_owl_packs.cli import reconcile_main
sys.exit(reconcile_main(sys.argv[1:]))' \
      gst --server "http://localhost:${APP_PORT}" \
      ${GST_TOKEN:+--token "${GST_TOKEN}"}); then
    die "the reconciliation failed — see the error above"
  fi
  echo "${_ran}" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(" ", d["rulesEvaluated"], "rules ·", d["opened"], "findings opened ·", d["alreadyOpen"], "already open")'

  echo
  echo "  Review them at http://localhost:${APP_PORT}/?section=review&kind=findings"
fi

cat <<EOF

  ────────────────────────────────────────────────────────────
   Console   http://localhost:${APP_PORT}
   API      http://localhost:${APP_PORT}/assets/stats
   Agent    http://localhost:${AGENT_PORT}  (console's Agent tab)
   Postgres localhost:${PG_PORT}  (postgres/postgres)
   Logs     tail -f /tmp/graphowl.log
            tail -f /tmp/graphowl-agent.log
   Stop     ./scripts/demo.sh --stop
  ────────────────────────────────────────────────────────────

EOF
