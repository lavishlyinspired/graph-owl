#!/usr/bin/env python3
"""Parity check: reconciliation.py's classification vs. the native
graph-owl reconcile engine, over reco-now's real fresh sample data
(SAMPLE/*_aug2026.csv) — plans/119-architecture-audit.md §5b step 5.

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
# SAMPLE/gstr2b_aug2026.csv, hand-derived when the fixture was authored
# (11 unique invoices: 7 clean matches, 1 amount mismatch, 2 books-only,
# 1 portal-only) — not read off either system's output, so it catches
# both sides being wrong in the same way.
EXPECTED_RECO_STATS = {
    "total": 11, "matched": 7, "review": 1, "only_books": 2, "only_gstr2b": 1,
}
# The native engine's honest coverage of that same answer key — see the
# "known gap" note below before changing this.
EXPECTED_NATIVE_LABELS = {
    "gst:PotentialMismatch": 2,   # == only_books
    "gst:AmountMismatch": 1,      # == review
    "gst:TaxHeadMismatch": 1,     # same invoice as AmountMismatch — a real,
                                   # additional distinction (cgst/sgst genuinely
                                   # differ by >₹1, proportional to the taxable
                                   # value delta), not double-counting a bug.
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
    for fname in ("purchase_register_aug2026.csv", "gstr2b_aug2026.csv"):
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

    print("==> running reconciliation.py")
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
    print("Known, honest gap (not a bug): the native engine has no wired")
    print("finding for reconciliation.py's only_gstr2b case ('in GSTR-2B,")
    print("not in books') without GSTR-1 data — packs/gst's MissingInBooks")
    print("requires it. Full parity does not hold; see")
    print("plans/119-architecture-audit.md §5b/§6 for the cutover decision.")

    return 0 if (ok and native_ok) else 1


if __name__ == "__main__":
    sys.exit(main())
