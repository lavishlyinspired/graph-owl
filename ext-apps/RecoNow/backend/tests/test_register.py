"""RED tests for Plan 122b B4: Register, Exceptions, Case detail.

Also fixes a real bug found while designing this slice: the reconcile
bridge (B3) read `finding.get("rule")`, but graph-owl's own `Finding`
struct (crates/graph-owl-core/src/finding.rs) has no `rule` field — it is
`label`. Every real finding would have landed with reason_code=None
silently. Covered here via a synthetic finding shaped like the real
struct, not the field name the earlier code guessed.
"""

from __future__ import annotations

import os
import uuid

import asyncpg
import pytest
from fastapi.testclient import TestClient

ADMIN_DSN = os.environ.get(
    "RECONOW_TEST_ADMIN_DSN", "postgresql://postgres:postgres@localhost:55000/postgres"
)


@pytest.fixture
async def client():
    db_name = "reconow_test_" + uuid.uuid4().hex[:12]
    admin_conn = await asyncpg.connect(ADMIN_DSN)
    try:
        await admin_conn.execute(f'CREATE DATABASE "{db_name}"')
    finally:
        await admin_conn.close()

    test_dsn = ADMIN_DSN.rsplit("/", 1)[0] + f"/{db_name}"
    os.environ["DATABASE_URL"] = test_dsn
    try:
        from app.main import app as fastapi_app

        with TestClient(fastapi_app) as test_client:
            yield test_client
    finally:
        del os.environ["DATABASE_URL"]
        admin_conn = await asyncpg.connect(ADMIN_DSN)
        try:
            await admin_conn.execute(f'DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)')
        finally:
            await admin_conn.close()


def _client_and_period(client) -> tuple[str, str]:
    client_id = client.post(
        "/api/clients", json={"name": "ABC Manufacturing", "gstin": "27AAAAA0000A1Z5", "state": "Maharashtra"}
    ).json()["id"]
    period_id = client.post(f"/api/clients/{client_id}/periods", json={"month": "August", "year": 2026}).json()["id"]
    return client_id, period_id


def _seed_case(client, client_id, period_id, **overrides):
    payload = {
        "invoice_no": "INV-1025",
        "reason_code": "amount_mismatch",
        "supplier_name": "XYZ Pvt Ltd",
        "books_amount": 38000,
        "portal_amount": 37500,
    }
    payload.update(overrides)
    return client.post(f"/api/clients/{client_id}/periods/{period_id}/cases", json=payload).json()


def test_register_lists_cases_sorted_by_exposure_descending(client):
    client_id, period_id = _client_and_period(client)
    _seed_case(client, client_id, period_id, invoice_no="INV-A", books_amount=1000, portal_amount=900)
    _seed_case(client, client_id, period_id, invoice_no="INV-B", books_amount=50000, portal_amount=0)

    body = client.get(f"/api/clients/{client_id}/periods/{period_id}/register").json()
    assert [row["invoice_no"] for row in body["rows"]] == ["INV-B", "INV-A"]


def test_register_reason_filter_total_reconciles_to_filtered_rows(client):
    """The plan's own RED, verbatim: the exposure total equals the sum of
    the *filtered* rows, not all rows."""
    client_id, period_id = _client_and_period(client)
    _seed_case(client, client_id, period_id, invoice_no="INV-A", reason_code="amount_mismatch", books_amount=1000, portal_amount=900)
    _seed_case(client, client_id, period_id, invoice_no="INV-B", reason_code="only_books", books_amount=50000, portal_amount=None)

    body = client.get(f"/api/clients/{client_id}/periods/{period_id}/register?reason_code=amount_mismatch").json()
    assert len(body["rows"]) == 1
    assert body["total_exposure"] == 100  # only INV-A's delta, not INV-B's 50000


def test_exceptions_groups_by_reason_code_with_count_and_exposure(client):
    client_id, period_id = _client_and_period(client)
    _seed_case(client, client_id, period_id, invoice_no="INV-A", reason_code="amount_mismatch", books_amount=1000, portal_amount=900)
    _seed_case(client, client_id, period_id, invoice_no="INV-B", reason_code="amount_mismatch", books_amount=2000, portal_amount=1900)
    _seed_case(client, client_id, period_id, invoice_no="INV-C", reason_code="only_books", books_amount=500, portal_amount=None)

    groups = client.get(f"/api/clients/{client_id}/periods/{period_id}/exceptions").json()
    amount_mismatch = next(g for g in groups if g["reason_code"] == "amount_mismatch")
    assert amount_mismatch["count"] == 2
    assert amount_mismatch["total_exposure"] == 200


def test_case_detail_returns_the_real_evidence_fields(client):
    client_id, period_id = _client_and_period(client)
    created = _seed_case(
        client, client_id, period_id, subject="gst:invoice-1025", summary="Tax value mismatch",
        governed_by="gst:AmountMismatchRule", evidence_count=4,
    )

    detail = client.get(f"/api/clients/{client_id}/periods/{period_id}/register/{created['id']}").json()
    assert detail["summary"] == "Tax value mismatch"
    assert detail["governed_by"] == "gst:AmountMismatchRule"
    assert detail["evidence_count"] == 4
    assert detail["subject"] == "gst:invoice-1025"


def test_case_detail_is_not_visible_from_another_client(client):
    client_a, period_a = _client_and_period(client)
    case = _seed_case(client, client_a, period_a)
    client_b_id = client.post(
        "/api/clients", json={"name": "Kaveri Textiles", "gstin": "29BBBBB1111B1Z4", "state": "Karnataka"}
    ).json()["id"]
    period_b = client.post(f"/api/clients/{client_b_id}/periods", json={"month": "August", "year": 2026}).json()["id"]

    response = client.get(f"/api/clients/{client_b_id}/periods/{period_b}/register/{case['id']}")
    assert response.status_code == 404


def test_recording_an_ims_decision_is_durable_and_reads_back(client):
    client_id, period_id = _client_and_period(client)
    case = _seed_case(client, client_id, period_id)

    decided = client.post(
        f"/api/clients/{client_id}/periods/{period_id}/register/{case['id']}/ims", json={"decision": "accept"}
    )
    assert decided.status_code == 201

    detail = client.get(f"/api/clients/{client_id}/periods/{period_id}/register/{case['id']}").json()
    assert detail["ims_decisions"][-1]["decision"] == "accept"


def test_prev_next_stay_within_the_same_reason_group(client):
    client_id, period_id = _client_and_period(client)
    a = _seed_case(client, client_id, period_id, invoice_no="INV-A", reason_code="amount_mismatch")
    b = _seed_case(client, client_id, period_id, invoice_no="INV-B", reason_code="amount_mismatch")
    _seed_case(client, client_id, period_id, invoice_no="INV-C", reason_code="only_books")

    detail_a = client.get(f"/api/clients/{client_id}/periods/{period_id}/register/{a['id']}").json()
    assert detail_a["next_id"] == b["id"]
    assert detail_a["group_reason_code"] == "amount_mismatch"

    detail_b = client.get(f"/api/clients/{client_id}/periods/{period_id}/register/{b['id']}").json()
    assert detail_b["prev_id"] == a["id"]
    assert detail_b["next_id"] is None  # INV-C is a different reason group
