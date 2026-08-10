#!/usr/bin/env bash
# The GST pack's six finding rules, against a real graph-owl and a real
# Postgres — Epic 105 P5/P6.
#
# **Every rule is checked in both directions.** A reconciliation that fires on
# everything is not a reconciliation, so for each rule this asserts the case
# that must produce a finding *and* the case that must not. The negatives are
# the load-bearing half: the 2020 invoice with a 5% delta must stay silent,
# because the cap in force then was 10% — if it fires, the percentage has
# leaked into the query and the whole "law is data" claim is false.
#
# Nothing here is GST-specific machinery. The pack is files; the loader, the
# reconciliation runtime and the review queue are the same ones the
# hospitality pack uses.
#
# Usage: scripts/verify-gst-reconciliation.sh [port]

set -euo pipefail

PORT="${1:-8107}"
PG_PORT=$((PORT + 40000))
CONTAINER="graph-owl-gst-verify"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PASS=0
FAIL=0

cleanup() {
  [[ -n "${SERVER_PID:-}" ]] && kill "$SERVER_PID" 2>/dev/null || true
  [[ -n "${LIVE_PID:-}" ]] && kill "$LIVE_PID" 2>/dev/null || true
  docker rm -f "$CONTAINER" "$CONTAINER-live" >/dev/null 2>&1 || true
}
trap cleanup EXIT

ok()   { printf '  \033[32mok\033[0m   %s\n' "$1"; PASS=$((PASS + 1)); }
bad()  { printf '  \033[31mFAIL\033[0m %s\n' "$1"; FAIL=$((FAIL + 1)); }
check() { if [[ "$2" == "$3" ]]; then ok "$1"; else bad "$1 — expected [$3], got [$2]"; fi; }

# How many findings carry this label, optionally about this subject.
count() {
  curl -sf "http://127.0.0.1:$PORT/findings?pack=gst" \
    | python3 -c "
import json,sys
label,subject = sys.argv[1], (sys.argv[2] if len(sys.argv) > 2 else None)
rows = json.load(sys.stdin)
print(sum(1 for r in rows
          if r['label'] == label
          and (subject is None or r['subject'].endswith(subject))))" "$@"
}

# One evidence value off a finding, so the citation can be asserted rather
# than eyeballed — the difference between "a number appeared" and "the number
# came from the provision in force".
evidence() {
  curl -sf "http://127.0.0.1:$PORT/findings?pack=gst" \
    | python3 -c "
import json,sys
label,subject,predicate = sys.argv[1:4]
for r in json.load(sys.stdin):
    if r['label'] == label and r['subject'].endswith(subject):
        for e in r['evidence']:
            if e['predicate'].endswith(predicate):
                print(e['value']); sys.exit()
print('(absent)')" "$@"
}

echo "==> a real Postgres and a real graph-owl"
docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
docker run -d --rm --name "$CONTAINER" \
  -e POSTGRES_PASSWORD=postgres -e POSTGRES_DB=graphowl \
  -p "$PG_PORT:5432" postgres:18-alpine >/dev/null
until docker exec "$CONTAINER" pg_isready -U postgres >/dev/null 2>&1; do sleep 1; done

DATABASE_URL="postgres://postgres:postgres@localhost:$PG_PORT/graphowl" \
BIND_ADDR="127.0.0.1:$PORT" OIDC_ISSUER= GRAPH_OWL_JWT_SECRET= \
  cargo run -q -p graph-owl-server --bin graph-owl-server >/tmp/gst-verify.log 2>&1 &
SERVER_PID=$!
until curl -sf "http://127.0.0.1:$PORT/health" >/dev/null 2>&1; do sleep 1; done

export PYTHONPATH="$ROOT/connectors/python"
load()      { python3 -m graph_owl_packs.cli "$@"; }
reconcile() { python3 -c "
import sys
from graph_owl_packs.cli import reconcile_main
sys.exit(reconcile_main(sys.argv[1:]))" "$@"; }

echo "==> loading the pack (no Rust, no TypeScript, no restart)"
LOADED=$(load "$ROOT/packs/gst" --server "http://127.0.0.1:$PORT")
check "the pack's vocabulary is a runtime namespace" \
  "$(echo "$LOADED" | python3 -c 'import json,sys; print(json.load(sys.stdin)["namespaceCode"] >= 1024)')" "True"
check "nothing was rejected" \
  "$(echo "$LOADED" | python3 -c 'import json,sys; print(len(json.load(sys.stdin)["rejected"]))')" "0"

echo "==> running the six rules"
RUN=$(reconcile "$ROOT/packs/gst" --server "http://127.0.0.1:$PORT")
check "all six rules were evaluated" \
  "$(echo "$RUN" | python3 -c 'import json,sys; print(json.load(sys.stdin)["rulesEvaluated"])')" "6"

echo
echo "==> Section 16(2)(aa): claimed but never filed"
check "INV-1003, absent from GSTR-2B, is a finding" "$(count gst:PotentialMismatch INV-1003)" "1"
check "INV-1001, which matches exactly, is not"     "$(count gst:PotentialMismatch INV-1001)" "0"

echo
echo "==> Rule 36(4): the cap is read from the law, not written in the query"
check "INV-1002 (2026, 5% delta, nil cap) is a finding"        "$(count gst:AmountMismatch INV-1002)" "1"
check "  …and cites the notification then in force"           "$(evidence gst:AmountMismatch INV-1002 citation)" "Notification 40/2021-CT"
check "INV-2002 (2020, 20% delta, 10% cap) is a finding"       "$(count gst:AmountMismatch INV-2002)" "1"
check "  …and cites the *different* notification in force then" "$(evidence gst:AmountMismatch INV-2002 citation)" "Notification 75/2019-CT"
# The single most important assertion in this file. A hardcoded cap of any
# value makes this fire: at 0 it is a false accusation, at 20 the 2026 case
# stops working. Only reading the provision in force on the invoice date gives
# both answers at once.
check "INV-2001 (2020, 5% delta, 10% cap) is NOT a finding"    "$(count gst:AmountMismatch INV-2001)" "0"

echo
echo "==> Section 17(5) and reverse charge: matched invoices that still carry no credit"
check "INV-1005, credit reported unavailable, is a finding" "$(count gst:ITCNotAvailable INV-1005)" "1"
check "INV-1001, credit available, is not"                  "$(count gst:ITCNotAvailable INV-1001)" "0"
check "INV-1006, flagged reverse-charge, is a finding"      "$(count gst:Reversed INV-1006)" "1"
check "INV-1001 is not"                                     "$(count gst:Reversed INV-1001)" "0"

echo
echo "==> matching policy: a transposed GSTIN is surfaced, never merged"
check "INV-1004 (…1MZ against …1ZM) is a finding" "$(count gst:GstinTransposition INV-1004)" "1"
# The band's upper bound. Every correctly matched invoice scores 1.0 on the
# similarity, and without `at_most` all of them would be reported as typos.
check "no correctly matched invoice is called a typo" "$(count gst:GstinTransposition)" "1"

echo
echo "==> Section 16(2)(d): the 180-day span between two events"
check "INV-1003, paid after 240 days, is a finding" "$(count gst:PaymentOverdue INV-1003)" "1"
# The case a "days to pay" column cannot express: no payment row, so no delta.
check "INV-2002, unpaid for six years, is a finding" "$(count gst:PaymentOverdue INV-2002)" "1"
check "INV-1001, paid after 20 days, is not"        "$(count gst:PaymentOverdue INV-1001)" "0"
# And the correction that "unpaid" is not the same as "overdue": flagging an
# invoice that is simply not due yet is a false accusation, and is what
# `when_missing = "finding"` used to do to every one of them.
check "INV-1006, unpaid but only six days old, is NOT" "$(count gst:PaymentOverdue INV-1006)" "0"

echo
echo "==> a re-run over unchanged data opens nothing"
AGAIN=$(reconcile "$ROOT/packs/gst" --server "http://127.0.0.1:$PORT")
check "opened"      "$(echo "$AGAIN" | python3 -c 'import json,sys; print(json.load(sys.stdin)["opened"])')" "0"
check "alreadyOpen" "$(echo "$AGAIN" | python3 -c 'import json,sys; print(json.load(sys.stdin)["alreadyOpen"])')" "9"

echo
echo "==> a dismissal survives the next run"
ID=$(curl -sf "http://127.0.0.1:$PORT/findings?pack=gst&status=pending" \
     | python3 -c 'import json,sys; print(json.load(sys.stdin)[0]["id"])')
check "dismissing without a reason is refused" \
  "$(curl -s -o /dev/null -w '%{http_code}' -X POST "http://127.0.0.1:$PORT/findings/$ID/decision" \
     -H 'content-type: application/json' -d '{"status":"rejected"}')" "400"
curl -s -o /dev/null -X POST "http://127.0.0.1:$PORT/findings/$ID/decision" \
  -H 'content-type: application/json' \
  -d '{"status":"rejected","reason":"supplier filed in the next period"}'
THIRD=$(reconcile "$ROOT/packs/gst" --server "http://127.0.0.1:$PORT")
check "the dismissed finding does not come back" \
  "$(echo "$THIRD" | python3 -c 'import json,sys; print(json.load(sys.stdin)["opened"])')" "0"

echo
echo "==> the rules cannot tell live-shaped data from a hand-written fixture"
# **The claim the whole connector rests on.** The API-shaped response carries
# DD-MM-YYYY dates and splits one invoice's tax into CGST/SGST — neither of
# which the hand-written fixture does — and after normalization the six rules
# must reach exactly the same conclusions. If they do not, the "develop
# against fixtures, deploy against a GSP" split is a fiction.
BEFORE=$(curl -sf "http://127.0.0.1:$PORT/findings?pack=gst" \
  | python3 -c 'import json,sys; print(sorted((r["label"], r["subject"]) for r in json.load(sys.stdin)))')

LIVE=$(mktemp -d)
cp -r "$ROOT/packs/gst/." "$LIVE/"
python3 -c "
import sys
from graph_owl_packs.cli import gstr2b_main
sys.exit(gstr2b_main(sys.argv[1:]))" \
  --from-file "$ROOT/packs/gst/fixtures/gstr2b-api-response.json" \
  --out "$LIVE/fixtures/gstr2b.ttl" >/dev/null

# A second server, so the comparison is against a clean graph rather than one
# already holding the fixture's triples.
docker rm -f "$CONTAINER-live" >/dev/null 2>&1 || true
docker run -d --rm --name "$CONTAINER-live" \
  -e POSTGRES_PASSWORD=postgres -e POSTGRES_DB=graphowl \
  -p "$((PG_PORT + 1)):5432" postgres:18-alpine >/dev/null
until docker exec "$CONTAINER-live" pg_isready -U postgres >/dev/null 2>&1; do sleep 1; done
DATABASE_URL="postgres://postgres:postgres@localhost:$((PG_PORT + 1))/graphowl" \
BIND_ADDR="127.0.0.1:$((PORT + 1))" OIDC_ISSUER= GRAPH_OWL_JWT_SECRET= \
  cargo run -q -p graph-owl-server --bin graph-owl-server >/tmp/gst-verify-live.log 2>&1 &
LIVE_PID=$!
until curl -sf "http://127.0.0.1:$((PORT + 1))/health" >/dev/null 2>&1; do sleep 1; done

python3 -m graph_owl_packs.cli "$LIVE" --server "http://127.0.0.1:$((PORT + 1))" >/dev/null
python3 -c "
import sys
from graph_owl_packs.cli import reconcile_main
sys.exit(reconcile_main(sys.argv[1:]))" "$LIVE" --server "http://127.0.0.1:$((PORT + 1))" >/dev/null
AFTER=$(curl -sf "http://127.0.0.1:$((PORT + 1))/findings?pack=gst" \
  | python3 -c 'import json,sys; print(sorted((r["label"], r["subject"]) for r in json.load(sys.stdin)))')
kill "$LIVE_PID" 2>/dev/null || true
docker rm -f "$CONTAINER-live" >/dev/null 2>&1 || true
rm -rf "$LIVE"

check "normalized GSTR-2B produces the identical finding set" "$AFTER" "$BEFORE"

echo
echo "==> the pack is files, not code"
CODE=$(find "$ROOT/packs/gst" -type f \( -name '*.py' -o -name '*.rs' -o -name '*.ts' -o -name '*.sh' \) | wc -l | tr -d ' ')
FILES=$(find "$ROOT/packs/gst" -type f | wc -l | tr -d ' ')
check "no executable code in the pack" "$CODE" "0"
# Guards against the check passing because it found nothing at all.
check "and there is a pack there to check" "$([[ $FILES -ge 10 ]] && echo yes || echo no)" "yes"

echo
if [[ $FAIL -eq 0 ]]; then
  printf '\033[32m%d checks passed.\033[0m\n' "$PASS"
else
  printf '\033[31m%d passed, %d FAILED.\033[0m\n' "$PASS" "$FAIL"
  exit 1
fi
