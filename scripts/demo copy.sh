#!/usr/bin/env bash
# Launches the full demo: Postgres, the seeded bank estate, and the server with
# the console embedded. One command, one binary, one database.
#
#   ./scripts/demo.sh          light  — no auth, everything visible
#   ./scripts/demo.sh --secure        — JWT on, with the two-principal policy
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
case "${1:-}" in
  --stop) stop ;;
  --secure) SECURE=true ;;
  "") ;;
  *) die "unknown option: $1 (expected --secure or --stop)" ;;
esac

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
if [ "${SECURE}" = true ]; then
  DATABASE_URL="${APP_URL}" GRAPH_OWL_JWT_SECRET="${SECRET}" \
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

say "Cataloguing the estate"
# macOS ships bash 3.2, where "${ARRAY[@]}" on an empty array is an unbound
# variable under `set -u`. Two explicit calls rather than a clever expansion.
BODY="{\"connectionString\":\"${PG_URL}\",\"serviceName\":\"hdfc-core\",\"includeSchemas\":[\"core_banking\",\"payments\",\"lending\",\"risk\",\"regulatory\"]}"
if [ "${SECURE}" = true ]; then
  RUN=$(curl -fsS -X POST "http://localhost:${APP_PORT}/connectors/postgres/runs" \
    -H "authorization: Bearer $(token root)" -H 'content-type: application/json' -d "${BODY}")
else
  RUN=$(curl -fsS -X POST "http://localhost:${APP_PORT}/connectors/postgres/runs" \
    -H 'content-type: application/json' -d "${BODY}")
fi
echo "${RUN}" | python3 -c "import sys,json;d=json.load(sys.stdin);print(f\"  {d['created']} assets, {d['failed']} failed\")"

if [ "${SECURE}" = true ]; then
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

cat <<EOF

  ────────────────────────────────────────────────────────────
   Console   http://localhost:${APP_PORT}
   API      http://localhost:${APP_PORT}/assets/stats
   Postgres localhost:${PG_PORT}  (postgres/postgres)
   Logs     tail -f /tmp/graphowl.log
   Stop     ./scripts/demo.sh --stop
  ────────────────────────────────────────────────────────────

EOF
