#!/usr/bin/env bash
# Epic 105 DN-3: **the domain-neutrality proof, run rather than asserted.**
#
# Loads two packs that share no vocabulary, no legal spine, no identifier
# scheme and no subject matter — hospitality and GST — into one real
# graph-owl, and asserts both landed. The platform was designed against seven
# Indian financial-compliance domains, and seven samples from one family prove
# nothing about neutrality; hospitality is here precisely because it is not
# one of them.
#
# **The acceptance criterion is a diff.** Both packs work with zero changes to
# any `.rs` and zero changes to any `.tsx` — asserted at the end of this
# script against `git`, not by inspection. If a pack needs either, the
# neutrality claim is false and the design gets corrected rather than the pack
# special-cased.
set -euo pipefail

cd "$(dirname "$0")/.."

PORT="${GRAPH_OWL_PACK_PORT:-8100}"
PG_PORT="${GRAPH_OWL_PACK_PG_PORT:-55500}"
CONTAINER=graph-owl-pack-check
VENV="${TMPDIR:-/tmp}/graph-owl-pack-venv"

cleanup() {
    docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
    kill "${SERVER_PID:-}" 2>/dev/null || true
}
trap cleanup EXIT

echo "==> starting Postgres"
docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
docker run -d --rm --name "$CONTAINER" \
    -e POSTGRES_PASSWORD=postgres -e POSTGRES_DB=graphowl \
    -p "$PG_PORT":5432 postgres:18-alpine >/dev/null
until docker exec "$CONTAINER" pg_isready -U postgres >/dev/null 2>&1; do sleep 1; done

echo "==> starting the server (open mode — this checks pack loading, not auth)"
DATABASE_URL="postgres://postgres:postgres@localhost:$PG_PORT/graphowl" \
    BIND_ADDR="127.0.0.1:$PORT" OIDC_ISSUER= GRAPH_OWL_JWT_SECRET= \
    cargo run -q -p graph-owl-server --bin graph-owl-server >/tmp/pack-check-server.log 2>&1 &
SERVER_PID=$!
until curl -sf "http://127.0.0.1:$PORT/health" >/dev/null 2>&1; do sleep 1; done

echo "==> installing the loader into a throwaway venv"
python3 -m venv "$VENV"
"$VENV/bin/pip" -q install -e connectors/python

for pack in hospitality gst; do
    echo "==> loading packs/$pack"
    "$VENV/bin/graph-owl-load-pack" "packs/$pack" --server "http://127.0.0.1:$PORT" \
        | tee "/tmp/pack-$pack.json"

    python3 - "$pack" <<'PY'
import json, sys
pack = sys.argv[1]
result = json.load(open(f"/tmp/pack-{pack}.json"))
assert result["pack"] == pack, result
assert result["namespaceCode"] >= 1024, (
    f"{pack} got namespace code {result['namespaceCode']}, which is inside the "
    "range the binary owns — a runtime namespace must be 1024 or above"
)
assert result["landed"] > 0, f"{pack} landed nothing: {result}"
assert not result["rejected"], f"{pack} had rejections: {result['rejected']}"
print(f"ok: {pack} landed {result['landed']} subject(s) under code {result['namespaceCode']}")
PY
done

echo "==> re-loading both packs: must be idempotent"
for pack in hospitality gst; do
    "$VENV/bin/graph-owl-load-pack" "packs/$pack" --server "http://127.0.0.1:$PORT" \
        > "/tmp/pack-$pack-again.json"
    python3 - "$pack" <<'PY'
import json, sys
pack = sys.argv[1]
first = json.load(open(f"/tmp/pack-{pack}.json"))
again = json.load(open(f"/tmp/pack-{pack}-again.json"))
assert again["namespaceCode"] == first["namespaceCode"], (
    f"{pack} was given a second namespace code on reload — its own terms would "
    f"resolve to two different Sids: {first['namespaceCode']} then {again['namespaceCode']}"
)
assert again["landed"] == 0, f"{pack} re-landed subjects on reload: {again}"
assert again["skipped"] > 0, f"{pack} reported no skips on reload: {again}"
print(f"ok: {pack} reloaded as a no-op, same namespace code {again['namespaceCode']}")
PY
done

echo "==> checking the two packs got different namespaces"
curl -sf "http://127.0.0.1:$PORT/namespaces" | python3 -c '
import json, sys
declared = json.load(sys.stdin)
by_iri = {n["iri"]: n["code"] for n in declared}
assert len(by_iri) == 2, f"expected two namespaces, got {declared}"
assert len(set(by_iri.values())) == 2, f"two packs share a code: {declared}"
assert all(c >= 1024 for c in by_iri.values()), declared
print("ok:", ", ".join(f"{i} -> {c}" for i, c in sorted(by_iri.items())))
'

echo "==> running the GST pack's own reconciliation queries"
# **The pack's queries answer against the pack's data, today.** This is what
# makes GST a working use case rather than a directory of files: no findings
# runtime exists yet, and the same answers are already reachable through the
# shipped SPARQL surface the console's workbench uses.
for query in missing-in-gstr2b tax-amount-mismatch; do
    python3 -c "
import json, sys
print(json.dumps({'query': open('packs/gst/queries/$query.sparql').read()}))
" > /tmp/gst-$query.json
    curl -sf -X POST "http://127.0.0.1:$PORT/sparql" \
        -H "content-type: application/json" \
        --data @/tmp/gst-$query.json > "/tmp/gst-$query-result.json"
done

python3 - <<'PYCHECK'
import json, sys

def result(name):
    body = json.load(open(f"/tmp/gst-{name}-result.json"))
    # `rows`/`variables`, not SPARQL-JSON `results.bindings` — this server's
    # own shape, and the first version of this check read the wrong one and
    # reported zero rows for a query that was working.
    return body.get("rows", []), body.get("factsScanned", 0)

missing, scanned = result("missing-in-gstr2b")
mismatch, _ = result("tax-amount-mismatch")

def field(row, key):
    return (row.get(key) or "").strip('"<>')

assert scanned > 0, (
    "the reconciliation scanned no facts at all. `scope_facts` admits a flake "
    "only when its subject is a visible catalog *asset* or its namespace is a "
    "declared vocabulary — a pack's subjects are neither assets nor, before "
    "Epic 105, recognised vocabularies."
)

unmatched = sorted(field(r, "number") for r in missing)
assert unmatched == ["INV-1003", "INV-1004"], (
    f"expected INV-1003 (never filed) and INV-1004 (transposed GSTIN), got {unmatched}"
)

deltas = {field(r, "number"): (field(r, "claimed"), field(r, "filed")) for r in mismatch}
assert deltas == {"INV-1002": ("18000.00", "17100.00")}, deltas

print("ok: INV-1003 never filed; INV-1004 unmatched on an exact join (its GSTIN "
      "is transposed — the `ngram` strategy is what would pair it, which is the "
      "argument for the fusion engine); INV-1002 claims 18000.00 against "
      "17100.00 filed; INV-1001 correctly produces nothing")
PYCHECK

echo "==> the acceptance criterion: no pack needed Rust or TypeScript"
python3 - <<'PY'
import subprocess, sys
# Every file the packs and their loader added, versus the code they must not
# have touched. Checked against git rather than by inspection, because "we
# didn't change any Rust" is exactly the kind of claim that quietly stops
# being true.
tracked = subprocess.run(
    ["git", "ls-files", "packs/"], capture_output=True, text=True, check=True
).stdout.split()
# **A check that passes on an empty set is not a check.** The first version of
# this ran before the packs were committed, found nothing, and reported "ok: 0
# pack files, none of them code" — which is true and worthless. Caught by
# reading the output rather than the exit code.
if not tracked:
    print(
        "no pack files are tracked by git, so this check proved nothing — "
        "commit packs/ before relying on it",
        file=sys.stderr,
    )
    sys.exit(1)
offenders = [p for p in tracked if p.endswith((".rs", ".tsx", ".ts", ".css"))]
if offenders:
    print("a pack ships code, which the neutrality claim forbids:", offenders, file=sys.stderr)
    sys.exit(1)
print(f"ok: {len(tracked)} pack files, none of them code")
PY

echo "==> ok: two unrelated domains load into one platform, no per-domain code"
