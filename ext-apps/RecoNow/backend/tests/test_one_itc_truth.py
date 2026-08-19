"""Every screen's ITC figures come from one computation — and say how.

**Found 19 August 2026 by reading three screens against each other on real
data.** They disagreed, and none of them showed its working:

| screen | figure | value |
|---|---|---|
| ITC position | "exposure" | ₹14,750 |
| Reconcile | confirmed / pending / blocked / under-review / unclaimed | ₹72,900 / 25,740 / 89,800 / 2,250 / 9,990 |
| GSTR-3B working paper | ITC available → net claimable | ₹2,79,340 → ₹1,22,940 |

`/itc` was summing **`case_record`** — only the *flagged* invoices, double
counting any invoice carrying two findings, excluding every clean one — and
labelling the result `books_amount`, `portal_amount` and `exposure` as though
they were period totals. It was not an ITC position at all.

`reconcile_result.itc_position` is the real one: five classes, each documented,
computed from every uploaded row. This makes `/itc` return that, so the two
screens cannot disagree — they are now the same numbers.

**The working paper legitimately measures a different population** (portal-side
gross ITC, against books-side classification), and that is exactly why every
figure now carries its own derivation: two correct numbers that differ look
like a bug unless each says what it counted.
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
    os.environ["DATABASE_URL"] = ADMIN_DSN.rsplit("/", 1)[0] + f"/{db_name}"
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


def _period(client) -> tuple[str, str]:
    created = client.post(
        "/api/clients",
        json={"name": "Truth Co", "gstin": "27AABCU9603R1ZM", "state": "Maharashtra"},
    ).json()
    period = client.post(
        f"/api/clients/{created['id']}/periods", json={"month": "March", "year": 2026}
    ).json()
    return created["id"], period["id"]


class TestOneComputation:
    def test_the_itc_screen_reports_the_same_classes_the_reconciliation_does(self, client):
        """Two screens showing different numbers for one question is the
        product lying, and the user cannot tell which one to believe."""
        client_id, period_id = _period(client)
        base = f"/api/clients/{client_id}/periods/{period_id}"

        itc = client.get(f"{base}/itc").json()
        recon = client.get(f"{base}/reconciliation").json()

        for key in ("confirmed", "pending", "blocked", "under_review", "unclaimed"):
            assert itc["position"][key] == recon["itc"][key], key

    def test_the_itc_screen_no_longer_reports_a_sum_over_findings(self, client):
        """The old shape summed `case_record`, so an invoice with two findings
        counted twice and a clean invoice counted not at all. Those keys are
        gone rather than silently redefined — a caller still reading
        `books_amount` should break loudly."""
        client_id, period_id = _period(client)

        itc = client.get(f"/api/clients/{client_id}/periods/{period_id}/itc").json()

        assert "exposure" not in itc
        assert "books_amount" not in itc


class TestEveryFigureShowsItsWorking:
    def test_each_class_carries_the_rule_that_put_money_in_it(self, client):
        """The user's own ask: "how is this value derived, what's the
        calculation". A figure a reader cannot derive is a figure they have to
        take on trust, and this product's whole argument is that they should
        not have to."""
        client_id, period_id = _period(client)

        itc = client.get(f"/api/clients/{client_id}/periods/{period_id}/itc").json()

        for key, figure in itc["explain"].items():
            assert figure["formula"], f"{key} has no stated derivation"
            assert figure["means"], f"{key} does not say what it means"

    def test_the_explanation_names_what_to_do_about_each_class(self, client):
        """"Blocked" and "pending" are the same size of number and opposite
        situations — one is lost and one is a phone call. A figure without its
        remedy is half an answer."""
        client_id, period_id = _period(client)

        itc = client.get(f"/api/clients/{client_id}/periods/{period_id}/itc").json()

        assert "claim" in itc["explain"]["confirmed"]["action"].lower()
        assert itc["explain"]["blocked"]["action"]
        assert itc["explain"]["pending"]["action"] != itc["explain"]["blocked"]["action"]

    def test_the_total_says_which_classes_it_adds(self, client):
        """`total_considered` sums five classes of which one — under review —
        contributes only a *difference*. A total that does not say what it
        added invites the reader to check it against something else and
        conclude the product is broken."""
        client_id, period_id = _period(client)

        itc = client.get(f"/api/clients/{client_id}/periods/{period_id}/itc").json()

        assert "under review" in itc["explain"]["total_considered"]["formula"].lower()

    def test_it_says_why_it_differs_from_the_working_paper(self, client):
        """Two correct numbers measuring different populations look like a bug
        unless each says what it counted. This is the single most confusing
        pair of screens in the product."""
        client_id, period_id = _period(client)

        itc = client.get(f"/api/clients/{client_id}/periods/{period_id}/itc").json()

        assert "working paper" in itc["compare_note"].lower()
