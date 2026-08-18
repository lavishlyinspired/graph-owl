"""Characterisation tests for Plan 122b B0 — pin the current, SESSION-backed
behaviour of `reconciliation.py`, `exporters.py` and the `/api/*` endpoints
that orchestrate them, **before** the persistence rewrite replaces `SESSION`
and `AI_JOBS` with repository-backed storage.

Written first, against the pre-change code, per the plan's own instruction
("write characterisation tests first ... so the persistence change is
provably behaviour-preserving"). `reconciliation.py`, `graphowl_client.py`,
`native_findings.py` and `exporters.py` are not edited by B0 — the risk this
guards against is main.py's *orchestration* silently changing what it hands
those modules (argument order, a dropped field) while the modules
themselves stay untouched. No graph-owl server is required or assumed: with
none reachable, `_select_results` falls back to `reconciliation.py`'s own
tolerance-matching path (`_select_results`'s own documented reasoning in
main.py), which is exactly the path this file pins.
"""

from __future__ import annotations

import csv
import io

from fastapi.testclient import TestClient

from app.main import app


def _client() -> TestClient:
    return TestClient(app)


def test_the_sample_flow_reconciles_to_a_stable_stats_shape():
    """`/api/sample` -> `/api/reconcile` -> `/api/overview` on the fixed
    sample fixture (`app/sample_data.py`) must always land on the same
    match counts and ITC totals — the exact numbers a persistence-layer
    regression would silently perturb."""
    with _client() as client:
        client.post("/api/reset")
        sample_response = client.post("/api/sample")
        assert sample_response.status_code == 200

        reconcile_response = client.post("/api/reconcile")
        assert reconcile_response.status_code == 200

        overview_response = client.get("/api/overview")
        body = overview_response.json()

        assert body["ok"] is True
        stats = body["stats"]
        assert stats is not None
        # Pinned from the current sample_data.py fixture (10 books rows, 8
        # gstr2b rows) — a genuine behaviour change to reconciliation.py
        # would need to update this pin deliberately, not silently.
        assert stats["total"] == 11
        assert stats["matched"] == 6
        assert stats["review"] == 1
        assert stats["only_books"] == 3
        assert stats["only_gstr2b"] == 1
        assert stats["confirmed_itc"] == 234900.0
        assert stats["at_risk_itc"] == 57600.0
        assert stats["gross_itc"] == 256500.0


def test_the_sample_flow_produces_a_stable_working_paper_csv():
    """`exporters.export_working_paper_csv` — pinned by its actual output
    shape (header row + row count), not the full byte content, since ₹
    formatting is intentionally locale-sensitive and not the concern here."""
    with _client() as client:
        client.post("/api/reset")
        client.post("/api/sample")
        client.post("/api/reconcile")

        csv_response = client.get("/api/export/csv")
        assert csv_response.status_code == 200
        assert csv_response.headers["content-type"].startswith("text/csv")

        rows = list(csv.reader(io.StringIO(csv_response.text)))
        assert len(rows) == 12  # header + 11 reconciled rows
        header = rows[0]
        assert "Status" in header or "status" in [h.lower() for h in header]


def test_reset_then_overview_reports_an_empty_session():
    """The empty-state shape `/api/overview` reports before anything is
    loaded — the baseline every persistence-backed client/period pair must
    also be able to report, on its first read."""
    with _client() as client:
        client.post("/api/reset")
        body = client.get("/api/overview").json()

        assert body["ok"] is True
        assert body["datasets"] == {}
        assert body["stats"] is None
        assert body["classifications"] == []
        assert body["results"] is None


def test_mapping_and_tolerance_are_read_back_exactly_as_saved():
    """`/api/mapping` -> `/api/overview`'s per-dataset `mapping` field — the
    exact round trip a repository-backed mapping-template store must
    preserve too."""
    with _client() as client:
        client.post("/api/reset")
        client.post("/api/sample")

        client.post(
            "/api/mapping",
            json={
                "mapping": {"books": {"invoice_no": 0, "taxable": 4}},
                "tolerance": 2.5,
                "period": {"month": "August", "year": 2026},
            },
        )

        body = client.get("/api/overview").json()
        assert body["tolerance"] == 2.5
        assert body["period"] == {"month": "August", "year": 2026}
        assert body["datasets"]["books"]["mapping"] == {"invoice_no": 0, "taxable": 4}
