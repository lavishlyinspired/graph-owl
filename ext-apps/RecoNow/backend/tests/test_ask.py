"""RED tests for Plan 122b B1's own stated RED: "Ask returns 'not enough
evidence' when it cannot ground, rather than an uncited sentence." Grounded
against real `case_record` rows for the given client+period — a simple,
deterministic keyword match over invoice number and reason code, not an LLM
call, so every answer this returns is provably traceable to the rows it
cites.
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


def test_asking_about_a_real_case_returns_a_grounded_answer_with_citations(client):
    client_id, period_id = _client_and_period(client)
    client.post(
        f"/api/clients/{client_id}/periods/{period_id}/cases",
        json={"invoice_no": "INV-1025", "reason_code": "amount_mismatch"},
    )

    response = client.post(
        f"/api/clients/{client_id}/periods/{period_id}/ask", json={"question": "what is happening with INV-1025"}
    )
    assert response.status_code == 200
    body = response.json()
    assert body["grounded"] is True
    assert any("INV-1025" in citation for citation in body["citations"])


def test_asking_about_nothing_in_the_period_refuses_rather_than_making_something_up(client):
    client_id, period_id = _client_and_period(client)
    client.post(
        f"/api/clients/{client_id}/periods/{period_id}/cases",
        json={"invoice_no": "INV-1025", "reason_code": "amount_mismatch"},
    )

    response = client.post(
        f"/api/clients/{client_id}/periods/{period_id}/ask",
        json={"question": "what about invoice INV-9999-not-real"},
    )
    body = response.json()
    assert body["grounded"] is False
    assert body["citations"] == []
    assert "not enough evidence" in body["answer"].lower()


def test_ask_only_grounds_against_the_asking_clients_own_cases(client):
    """The same client/period isolation B0 proved at the repo layer, now
    checked through the HTTP surface that actually uses it."""
    client_a, period_a = _client_and_period(client)
    client_b = client.post(
        "/api/clients", json={"name": "Kaveri Textiles", "gstin": "29BBBBB1111B1Z4", "state": "Karnataka"}
    ).json()["id"]
    period_b = client.post(f"/api/clients/{client_b}/periods", json={"month": "August", "year": 2026}).json()["id"]

    client.post(
        f"/api/clients/{client_a}/periods/{period_a}/cases",
        json={"invoice_no": "INV-1025", "reason_code": "amount_mismatch"},
    )

    response = client.post(
        f"/api/clients/{client_b}/periods/{period_b}/ask", json={"question": "what about INV-1025"}
    )
    body = response.json()
    assert body["grounded"] is False
