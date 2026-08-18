"""Every screen quoting money must quote the same money.

Reco Now shows an exposure figure on the dashboard, the register, exceptions,
suppliers, authority and obligations. They were computed three different ways
— Python over cases, and two different SQL expressions — and disagreed.

The clearest case: `gst:SupplierNotFiled` on INV-MAR-013 has ₹17,100 booked
and no portal counterpart, because the supplier filed nothing. The register
reports ₹17,100 at risk. The suppliers SQL wrote
`COALESCE(portal_amount, books_amount)`, making the same case ABS(x - x) = 0,
so the supplier page reported that the vendor who filed nothing was the one
costing nothing.

These tests assert the screens agree, rather than asserting a number each,
because agreement is the property that was broken.
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
async def seeded():
    """A period whose cases mirror the real March 2026 shape: one invoice with
    both sides present, and two where the supplier filed nothing."""
    db_name = "reconow_test_" + uuid.uuid4().hex[:12]
    admin = await asyncpg.connect(ADMIN_DSN)
    try:
        await admin.execute(f'CREATE DATABASE "{db_name}"')
    finally:
        await admin.close()
    os.environ["DATABASE_URL"] = ADMIN_DSN.rsplit("/", 1)[0] + f"/{db_name}"
    try:
        from app.main import app as fastapi_app

        with TestClient(fastapi_app) as c:
            cid = c.post(
                "/api/clients",
                json={"name": "ABC", "gstin": "27AAAAA0000A1Z5", "state": "Maharashtra"},
            ).json()["id"]
            pid = c.post(
                f"/api/clients/{cid}/periods", json={"month": "March", "year": 2026}
            ).json()["id"]
            for case in (
                # Both sides present: exposure is the difference, 500.
                {
                    "invoice_no": "INV-MAR-011", "reason_code": "gst:AmountMismatch",
                    "supplier_gstin": "27AABCS1429B1Z8", "supplier_name": "Sharma Infrastructure",
                    "books_amount": 180000, "portal_amount": 180500,
                    "governed_by": "gst:Rule36-4",
                },
                # Supplier filed nothing: the whole booked tax is at risk.
                {
                    "invoice_no": "INV-MAR-013", "reason_code": "gst:SupplierNotFiled",
                    "supplier_gstin": "11AABCZ9999A1Z1", "supplier_name": "Ghost Vendor",
                    "books_amount": 17100, "portal_amount": None,
                    "governed_by": "gst:Section16-2-aa",
                },
                {
                    "invoice_no": "INV-MAR-014", "reason_code": "gst:SupplierNotFiled",
                    "supplier_gstin": "22AABCX8888B1ZQ", "supplier_name": "Phantom Supplies",
                    "books_amount": 8640, "portal_amount": None,
                    "governed_by": "gst:Section16-2-aa",
                },
            ):
                r = c.post(f"/api/clients/{cid}/periods/{pid}/cases", json=case)
                assert r.status_code == 201, r.text
            yield c, cid, pid
    finally:
        del os.environ["DATABASE_URL"]
        admin = await asyncpg.connect(ADMIN_DSN)
        try:
            await admin.execute(f'DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)')
        finally:
            await admin.close()


EXPECTED_TOTAL = 500 + 17100 + 8640  # hand-derived: 26240


def test_register_and_dashboard_agree(seeded):
    c, cid, pid = seeded
    register = c.get(f"/api/clients/{cid}/periods/{pid}/register").json()
    dashboard = c.get(f"/api/clients/{cid}/periods/{pid}/dashboard").json()

    assert register["total_exposure"] == pytest.approx(EXPECTED_TOTAL)
    assert dashboard["total_exposure"] == pytest.approx(EXPECTED_TOTAL)


def test_a_supplier_who_filed_nothing_is_not_reported_as_costing_nothing(seeded):
    c, cid, pid = seeded
    suppliers = c.get(f"/api/clients/{cid}/periods/{pid}/suppliers").json()
    ghost = next(s for s in suppliers if s["gstin"] == "11AABCZ9999A1Z1")

    assert ghost["total_exposure"] == pytest.approx(17100)


def test_supplier_exposures_sum_to_the_period_total(seeded):
    c, cid, pid = seeded
    suppliers = c.get(f"/api/clients/{cid}/periods/{pid}/suppliers").json()

    assert sum(s["total_exposure"] for s in suppliers) == pytest.approx(EXPECTED_TOTAL)


def test_each_exception_group_reports_its_own_cases_correctly(seeded):
    c, cid, pid = seeded
    groups = {g["reason_code"]: g for g in c.get(f"/api/clients/{cid}/periods/{pid}/exceptions").json()}

    assert groups["gst:SupplierNotFiled"]["total_exposure"] == pytest.approx(17100 + 8640)
    assert groups["gst:AmountMismatch"]["total_exposure"] == pytest.approx(500)


def test_group_totals_may_exceed_the_period_total_and_that_is_correct(seeded_overlapping):
    """A grouped screen is not a partition.

    One invoice can carry two findings under different reason codes —
    INV-MAR-013 in the real March data is both `gst:SupplierNotFiled` and
    `gst:PotentialMismatch`. Each group correctly reports the full amount of
    the invoices *in that group*, so the groups overlap and their sum exceeds
    the period total.

    An earlier version of this file asserted the groups summed to the period
    total. It passed only because its fixture gave every invoice exactly one
    reason code, and "fixing" the code to satisfy it would have made each
    group's own figure wrong. The invariant to hold is that no screen presents
    a sum across groups *as* the period total.
    """
    c, cid, pid = seeded_overlapping
    groups = c.get(f"/api/clients/{cid}/periods/{pid}/exceptions").json()
    register = c.get(f"/api/clients/{cid}/periods/{pid}/register").json()

    assert sum(g["total_exposure"] for g in groups) > register["total_exposure"]
    # …and the period total itself still counts the invoice once.
    assert register["total_exposure"] == pytest.approx(17100)


def test_authority_groups_report_their_own_provisions(seeded):
    c, cid, pid = seeded
    rows = {r["authority"]: r for r in c.get(f"/api/clients/{cid}/periods/{pid}/authority").json()}

    assert rows["gst:Rule36-4"]["exposure"] == pytest.approx(500)
    assert rows["gst:Section16-2-aa"]["exposure"] == pytest.approx(17100 + 8640)


@pytest.fixture
async def seeded_overlapping():
    """One invoice carrying two findings under different reason codes — the
    real INV-MAR-013 shape, which the primary fixture deliberately lacks."""
    db_name = "reconow_test_" + uuid.uuid4().hex[:12]
    admin = await asyncpg.connect(ADMIN_DSN)
    try:
        await admin.execute(f'CREATE DATABASE "{db_name}"')
    finally:
        await admin.close()
    os.environ["DATABASE_URL"] = ADMIN_DSN.rsplit("/", 1)[0] + f"/{db_name}"
    try:
        from app.main import app as fastapi_app

        with TestClient(fastapi_app) as c:
            cid = c.post(
                "/api/clients",
                json={"name": "ABC", "gstin": "27AAAAA0000A1Z5", "state": "Maharashtra"},
            ).json()["id"]
            pid = c.post(
                f"/api/clients/{cid}/periods", json={"month": "March", "year": 2026}
            ).json()["id"]
            for reason in ("gst:SupplierNotFiled", "gst:PotentialMismatch"):
                c.post(
                    f"/api/clients/{cid}/periods/{pid}/cases",
                    json={
                        "invoice_no": "INV-MAR-013", "reason_code": reason,
                        "supplier_gstin": "11AABCZ9999A1Z1", "supplier_name": "Ghost Vendor",
                        "books_amount": 17100, "portal_amount": None,
                    },
                )
            yield c, cid, pid
    finally:
        del os.environ["DATABASE_URL"]
        admin = await asyncpg.connect(ADMIN_DSN)
        try:
            await admin.execute(f'DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)')
        finally:
            await admin.close()
