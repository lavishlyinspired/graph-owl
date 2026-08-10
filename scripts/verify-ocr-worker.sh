#!/usr/bin/env bash
# Epic 21 follow-up, Slice 4: prove the OCR worker path end to end — no GPU,
# no real vision model, no network to a real endpoint.
#
# Same shape as verify-sdks.sh (real Postgres + a real graph-owl-server), with
# the model itself replaced by ocr-check-endpoint.py: a stdlib http.server
# double, the same "a real server, not a mock" discipline
# workers/python/tests/test_cli.py already uses for the CLI-level tests. What
# this proves that those tests cannot: a real committed PNG and a real
# committed textless PDF, run through the *installed* `graph-owl-worker`
# console script against a *live* server, land a real claim in
# `graph:extraction` — and that re-running is idempotent, because the whole
# point of the fingerprint is to skip the OCR pass on a document already seen.
set -euo pipefail

cd "$(dirname "$0")/.."

PORT="${GRAPH_OWL_OCR_CHECK_PORT:-8098}"
PG_PORT="${GRAPH_OWL_OCR_CHECK_PG_PORT:-55499}"
OCR_PORT="${GRAPH_OWL_OCR_CHECK_OCR_PORT:-8099}"
CONTAINER=graph-owl-ocr-check
# A dedicated, throwaway virtualenv — never the caller's, same reasoning as
# verify-sdks.sh.
VENV="${TMPDIR:-/tmp}/graph-owl-ocr-worker-venv"
# No '.': a root asset's `name` becomes its own fully-qualified name, and the
# server refuses a `.` there (it would make the FQN's segment boundary
# ambiguous) — found by running this script, not designed in advance.
FQN="ocr-check-$$"
WORKDIR="$(mktemp -d)"

cleanup() {
    docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
    kill "${SERVER_PID:-}" "${OCR_PID:-}" 2>/dev/null || true
    rm -rf "$WORKDIR"
}
trap cleanup EXIT

echo "==> starting Postgres"
docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
docker run -d --rm --name "$CONTAINER" \
    -e POSTGRES_PASSWORD=postgres -e POSTGRES_DB=graphowl \
    -p "$PG_PORT":5432 postgres:18-alpine >/dev/null

until docker exec "$CONTAINER" pg_isready -U postgres >/dev/null 2>&1; do sleep 1; done

echo "==> starting the server (open mode — this checks the worker, not auth)"
DATABASE_URL="postgres://postgres:postgres@localhost:$PG_PORT/graphowl" \
    BIND_ADDR="127.0.0.1:$PORT" OIDC_ISSUER= GRAPH_OWL_JWT_SECRET= \
    cargo run -q -p graph-owl-server --bin graph-owl-server >/tmp/ocr-check-server.log 2>&1 &
SERVER_PID=$!

until curl -sf "http://127.0.0.1:$PORT/health" >/dev/null 2>&1; do sleep 1; done

echo "==> starting the scripted OCR endpoint (no model, no GPU)"
python3 scripts/ocr-check-endpoint.py "$OCR_PORT" "$FQN" >/tmp/ocr-check-endpoint.log 2>&1 &
OCR_PID=$!
until curl -sf "http://127.0.0.1:$OCR_PORT/health" >/dev/null 2>&1; do sleep 1; done

echo "==> creating a subject asset for the scripted OCR text to mention"
curl -sf -X POST "http://127.0.0.1:$PORT/assets" \
    -H "content-type: application/json" \
    -d "{\"kind\": \"service\", \"name\": \"$FQN\"}" >/dev/null

echo "==> installing the worker into a throwaway venv"
python3 -m venv "$VENV"
"$VENV/bin/pip" -q install -e sdk/python -e "workers/python[ovis-ocr2]"

echo "==> running the worker over the fixture PNG and PDF (--ocr, no --pdf, no --token)"
"$VENV/bin/graph-owl-worker" \
    --server "http://127.0.0.1:$PORT" \
    --ocr --ocr-endpoint "http://127.0.0.1:$OCR_PORT" \
    workers/python/tests/fixtures | tee "$WORKDIR/run1.jsonl"

echo "==> checking a real claim landed in graph:extraction (the review queue)"
curl -sf "http://127.0.0.1:$PORT/extraction/queue" | python3 -c '
import json, sys
queue = json.load(sys.stdin)
assert isinstance(queue, list), f"expected the queue to be a JSON array, got: {queue!r}"
assert len(queue) >= 1, "expected at least one queued claim after the OCR run, got none"
print(f"ok: {len(queue)} claim(s) reached graph:extraction")
'

echo "==> re-running: must be idempotent (fingerprint pinning skips re-OCR)"
"$VENV/bin/graph-owl-worker" \
    --server "http://127.0.0.1:$PORT" \
    --ocr --ocr-endpoint "http://127.0.0.1:$OCR_PORT" \
    workers/python/tests/fixtures | tee "$WORKDIR/run2.jsonl"

python3 -c '
import json
with open("'"$WORKDIR"'/run2.jsonl") as f:
    outcomes = [json.loads(line) for line in f if line.strip()]
assert outcomes, "second run produced no output at all"
not_already = [o for o in outcomes if o["outcome"] != "alreadyExtracted"]
assert not not_already, f"re-run was not idempotent, expected every document already seen: {not_already}"
print(f"ok: all {len(outcomes)} document(s) reported alreadyExtracted on re-run")
'

echo "==> ok: OCR worker round-trips end to end, no GPU and no real model"
