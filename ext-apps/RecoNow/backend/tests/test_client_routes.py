"""RED tests for Plan 122b B1's client/period HTTP surface — the first
routes built directly on B0's repository layer, against a real, freshly
created Postgres database set as `DATABASE_URL` before the app starts (so
`app.state.db_pool` connects for real, not the best-effort/absent path
`_install_graphowl_pack` already has for graph-owl)."""

from __future__ import annotations

import os
import uuid

import asyncpg
import pytest
from fastapi.testclient import TestClient

from app import db

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
        # Imported after DATABASE_URL is set, and freshly per test — the
        # module reads the env var once, at startup time, not at import
        # time, so re-importing the already-loaded module would just reuse
        # whatever pool the first test connected.
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


def test_creating_a_client_returns_it_with_a_real_id(client):
    response = client.post("/api/clients", json={"name": "ABC Manufacturing", "gstin": "27AAAAA0000A1Z5", "state": "Maharashtra"})
    assert response.status_code == 201
    body = response.json()
    assert body["name"] == "ABC Manufacturing"
    assert body["id"]


def test_listing_clients_returns_created_clients(client):
    client.post("/api/clients", json={"name": "ABC Manufacturing", "gstin": "27AAAAA0000A1Z5", "state": "Maharashtra"})
    client.post("/api/clients", json={"name": "Kaveri Textiles", "gstin": "29BBBBB1111B1Z4", "state": "Karnataka"})

    response = client.get("/api/clients")
    assert response.status_code == 200
    names = {c["name"] for c in response.json()}
    assert names == {"ABC Manufacturing", "Kaveri Textiles"}


def test_creating_a_period_under_a_client_and_listing_it_back(client):
    created = client.post("/api/clients", json={"name": "ABC Manufacturing", "gstin": "27AAAAA0000A1Z5", "state": "Maharashtra"})
    client_id = created.json()["id"]

    period_response = client.post(f"/api/clients/{client_id}/periods", json={"month": "August", "year": 2026})
    assert period_response.status_code == 201
    assert period_response.json()["month"] == "August"

    listed = client.get(f"/api/clients/{client_id}/periods")
    assert listed.status_code == 200
    assert len(listed.json()) == 1
    assert listed.json()[0]["year"] == 2026


def test_periods_for_one_client_are_not_returned_for_another(client):
    a = client.post("/api/clients", json={"name": "ABC Manufacturing", "gstin": "27AAAAA0000A1Z5", "state": "Maharashtra"}).json()["id"]
    b = client.post("/api/clients", json={"name": "Kaveri Textiles", "gstin": "29BBBBB1111B1Z4", "state": "Karnataka"}).json()["id"]

    client.post(f"/api/clients/{a}/periods", json={"month": "August", "year": 2026})

    assert client.get(f"/api/clients/{a}/periods").json() != []
    assert client.get(f"/api/clients/{b}/periods").json() == []
