"""RED tests for Plan 122b B2's dashboard aggregation endpoint.

Scoped honestly, not to the mockup's full 6-panel layout: this backend can
currently compute real totals from case_record/approval/follow_up
(exposure, counts, "what needs a decision" sorted by exposure) — it cannot
yet compute a genuine match-rate *trend* (needs period-over-period
history no screen has produced yet) or graph-engine run metadata beyond
what reconcile already returns. Those stay named gaps, not fabricated
numbers.
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


def test_dashboard_on_an_empty_period_is_honestly_zero(client):
    client_id, period_id = _client_and_period(client)
    body = client.get(f"/api/clients/{client_id}/periods/{period_id}/dashboard").json()
    assert body["case_count"] == 0
    assert body["total_exposure"] == 0
    assert body["needs_decision"] == []
    assert body["pending_approvals"] == 0


def test_dashboard_totals_reconcile_to_the_cases_that_produced_them(client):
    """The plan's own RED for B2: every card total reconciles to the
    filtered sum — assert equality, not just presence, because two
    independently computed totals that disagree is the actual defect a
    dashboard ships with."""
    client_id, period_id = _client_and_period(client)

    async def _seed():
        import asyncpg as _asyncpg

        conn = await _asyncpg.connect(os.environ["DATABASE_URL"])
        try:
            from app import repo

            await repo.create_case(
                conn, client_id=client_id, period_id=period_id, invoice_no="INV-1025",
                reason_code="amount_mismatch", supplier_name="XYZ Pvt Ltd", supplier_gstin="29X",
                books_amount=38000, portal_amount=37500,
            )
            await repo.create_case(
                conn, client_id=client_id, period_id=period_id, invoice_no="INV-1026",
                reason_code="only_books", supplier_name="PQR Industries", supplier_gstin="27X",
                books_amount=118200, portal_amount=None,
            )
        finally:
            await conn.close()

    import asyncio

    asyncio.get_event_loop().run_until_complete(_seed())

    body = client.get(f"/api/clients/{client_id}/periods/{period_id}/dashboard").json()
    assert body["case_count"] == 2
    # 500 (38000-37500) + 118200 (portal missing entirely -> full books amount at risk)
    assert body["total_exposure"] == 500 + 118200
    assert len(body["needs_decision"]) == 2
    # sorted by exposure, descending
    assert body["needs_decision"][0]["invoice_no"] == "INV-1026"
