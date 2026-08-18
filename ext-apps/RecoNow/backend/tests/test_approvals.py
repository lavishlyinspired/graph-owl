"""RED tests for Plan 122b B1's inbox — listing and deciding pending
approvals, scoped to client+period exactly like everything else B0/B1 built.
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


def test_a_new_period_has_no_pending_approvals(client):
    client_id, period_id = _client_and_period(client)
    response = client.get(f"/api/clients/{client_id}/periods/{period_id}/approvals")
    assert response.status_code == 200
    assert response.json() == []


def test_approving_a_pending_item_changes_its_status(client):
    client_id, period_id = _client_and_period(client)
    created = client.post(
        f"/api/clients/{client_id}/periods/{period_id}/approvals",
        json={"decision_type": "write_off", "amount": 31700},
    ).json()

    decided = client.post(
        f"/api/clients/{client_id}/periods/{period_id}/approvals/{created['id']}/decide",
        json={"status": "approved"},
    )
    assert decided.status_code == 200
    assert decided.json()["status"] == "approved"

    remaining = client.get(f"/api/clients/{client_id}/periods/{period_id}/approvals?status=pending").json()
    assert remaining == []
