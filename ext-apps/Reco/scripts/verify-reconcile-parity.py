#!/usr/bin/env python3
"""Regression check: /api/reconcile's stats (native findings, primary
source since the 16 August 2026 cutover, plans/119-architecture-audit.md
§9) against a hand-derived answer key, over reco-now's real fresh sample
data (SAMPLE/*_aug2026.csv).

**Was a parity check between two independent paths** (reconciliation.py's
own tolerance/matching math vs. the native engine) **before the cutover.**
`/api/reconcile` now calls `native_findings.reconcile` directly, so the
two numbers below are the same data read two ways rather than two
independent computations — kept as two assertions anyway, because one
checks the aggregate buckets `overview()` exposes and the other checks
the underlying per-label finding counts, and a bug in the bucket-mapping
logic (`native_findings._STATUS_BY_LABEL`/`_STATUS_PRIORITY`) could make
the first wrong while the second still passes.

**Requires a live stack**, same as scripts/verify-pack-load.sh does for
packs/gst: a running graph-owl-server (open mode) with a *fresh* database
(old test uploads accumulate stale findings that make the comparison
meaningless — found the hard way earlier in this session) and a running
reco-now backend pointed at it. Not part of `pytest tests/` for the same
reason none of graph-owl's own `verify-*.sh` scripts are unit tests: this
is exercising the real, deployed system end to end, not a unit of logic.

    ./scripts/verify-reconcile-parity.py [--server http://127.0.0.1:8000]

Exit 0 if every assertion below holds; prints what it found either way.
"""

from __future__ import annotations

import argparse
import json
import sys
import urllib.request

# The answer key for SAMPLE/purchase_register_aug2026.csv +
# SAMPLE/gstr2b_aug2026.csv + SAMPLE/gstr2a_aug2026.csv, hand-derived by
# reading every finding query's WHERE clause against the fixture rows
# (plans/119-architecture-audit.md §8/§9) — not read off either system's
# output, so it catches both sides being wrong in the same way.
#
# 14 unique invoices across books+2b+gstr1 (books+2b alone would be 13 —
# reconciliation.py's old fallback-path number, still exercised if
# graph-owl is unreachable): 7 clean matches, 2 review (INV-AUG-103's
# amount+tax-head mismatch, INV-AUG-114's books-vs-GSTR-1 mismatch), 3
# only-books (INV-AUG-105, 109 not filed at all; INV-AUG-113 filed but not
# yet in 2B), 2 only-portal (INV-AUG-111 in 2B only, INV-AUG-112 in
# GSTR-1/2A only).
EXPECTED_RECO_STATS = {
    "total": 14, "matched": 7, "review": 2, "only_books": 3, "only_gstr2b": 2,
}
# The native engine's coverage once GSTR-1/2A evidence exists in the
# store — see the "known gap" note below before changing this.
#
# gst:PotentialMismatch (missing-in-gstr2b) is deliberately NOT here: its
# own query stands down globally the moment any gst:Gstr1Invoice exists
# anywhere in the store (see missing-in-gstr2b.sparql's "handover switch"
# comment) — INV-AUG-105 and INV-AUG-109 move to gst:SupplierNotFiled
# instead, which is the more specific answer GSTR-1 evidence unlocks.
EXPECTED_NATIVE_LABELS = {
    "gst:AmountMismatch": 1,       # INV-AUG-103 — claimed vs GSTR-2B taxable differ
    "gst:TaxHeadMismatch": 1,      # INV-AUG-103 — same invoice, cgst/sgst differ >₹1 too
    "gst:SupplierNotFiled": 2,     # INV-AUG-105, 109 — absent from GSTR-1 AND GSTR-2B
    "gst:Gstr1NotIn2b": 2,         # INV-AUG-113, 114 — supplier filed, absent from 2B
    "gst:MissingInBooks": 1,       # INV-AUG-112 — GSTR-1/2A only, never booked
    "gst:BooksGstr1Mismatch": 1,   # INV-AUG-114 — books 55000 vs GSTR-1 53000 (also
                                    # Gstr1NotIn2b, a real second distinction: absent
                                    # from 2B *and* mismatched against what was filed —
                                    # native_findings._STATUS_PRIORITY picks the review
                                    # bucket for this row, keeping both reasons)
}


def _get(url: str) -> dict:
    with urllib.request.urlopen(url) as r:
        return json.loads(r.read())


def _post(url: str, data: bytes | None = None, content_type: str | None = None) -> dict:
    req = urllib.request.Request(url, data=data or b"", method="POST")
    if content_type:
        req.add_header("content-type", content_type)
    with urllib.request.urlopen(req) as r:
        raw = r.read()
        return json.loads(raw) if raw else {}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--server", default="http://127.0.0.1:8000")
    args = parser.parse_args()
    base = args.server.rstrip("/")

    print("==> uploading fresh sample data")
    import pathlib
    import time
    import uuid

    sample_dir = pathlib.Path(__file__).resolve().parents[1] / "SAMPLE"
    boundary = uuid.uuid4().hex
    parts = []
    for fname in ("purchase_register_aug2026.csv", "gstr2b_aug2026.csv", "gstr2a_aug2026.csv"):
        content = (sample_dir / fname).read_bytes()
        header = (
            f'--{boundary}\r\nContent-Disposition: form-data; name="files"; '
            f'filename="{fname}"\r\nContent-Type: text/csv\r\n\r\n'
        ).encode()
        parts.append(header + content + b"\r\n")
    body = b"".join(parts) + f"--{boundary}--\r\n".encode()
    upload = _post(
        f"{base}/api/upload", data=body,
        content_type=f"multipart/form-data; boundary={boundary}",
    )
    assert upload.get("ok"), f"upload failed: {upload}"

    time.sleep(2)  # background ingestion

    print("==> running /api/reconcile (native findings, primary since the cutover)")
    reco = _post(f"{base}/api/reconcile")
    stats = {k: reco["stats"][k] for k in EXPECTED_RECO_STATS}
    print("    stats:", stats)
    ok = stats == EXPECTED_RECO_STATS
    print("    " + ("OK" if ok else f"MISMATCH — expected {EXPECTED_RECO_STATS}"))

    time.sleep(2)  # background native reconcile
    print("==> checking native graph-owl findings")
    native = _get(f"{base}/api/graphowl/reconcile")["reconcile"]
    counts: dict[str, int] = {}
    for f in native.get("findings", []):
        counts[f["label"]] = counts.get(f["label"], 0) + 1
    print("    findings:", counts)
    native_ok = counts == EXPECTED_NATIVE_LABELS
    print("    " + ("OK" if native_ok else f"MISMATCH — expected {EXPECTED_NATIVE_LABELS}"))

    print()
    print("Known, honest gap (not a bug): INV-AUG-111 is only_gstr2b with no")
    print("matching gst:Gstr1Invoice in this fixture's gstr2a_aug2026.csv —")
    print("packs/gst's MissingInBooks would catch it too, but only once GSTR-1")
    print("evidence for that specific invoice is loaded. Realistic partial 2A/2B")
    print("coverage, not a modeling gap; see plans/119-architecture-audit.md §8.")

    return 0 if (ok and native_ok) else 1


if __name__ == "__main__":
    sys.exit(main())
