"""Reco Now using what graph-owl already ships — Plan 123 Slice E.

graph-owl carries memories, waivers, threads, lineage and an explain endpoint.
Reco Now used none of them and kept its own approximations, so knowledge that
should outlive a period died with it.

**The memory case is the one that changes how the product behaves.** "This
supplier always files late" is exactly the judgement a CA builds over months
and cannot record anywhere today: next period the same supplier is flagged
again, with no indication that this is the fourth time. A memory survives
periods, is correctable by superseding rather than editing, and is never
destroyed — so a wrong one can be withdrawn without erasing the fact that it
was once believed.

**A waiver that does not expire is a rule change nobody voted for.** Accepting
an exception permanently means the check stops running and nobody remembers
deciding that. graph-owl requires both a reason and an expiry, which is the
right shape; Reco Now had an `approval` table with neither.
"""

from __future__ import annotations

import os
import uuid
from datetime import datetime, timedelta, timezone

import asyncpg
import pytest
from fastapi.testclient import TestClient

from app.capabilities import (
    MIN_PERIODS_FOR_A_PATTERN,
    memory_for_supplier,
    supplier_pattern,
    waiver_request,
)


ADMIN_DSN = os.environ.get(
    "RECONOW_TEST_ADMIN_DSN", "postgresql://postgres:postgres@localhost:55000/postgres"
)


@pytest.fixture
async def client():
    """The real-database fixture `test_client_routes.py` uses — a fresh
    database per test, with `DATABASE_URL` set before `app.main` is imported."""
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


def _obs(period: str, label: str) -> dict:
    return {"period": period, "reason_code": label}


class TestWhenASupplierPatternIsWorthRecording:
    def test_one_late_period_is_not_a_pattern(self):
        """A supplier late once is an incident. Recording it as a
        characteristic would prejudice every future period against them on a
        single data point."""
        assert supplier_pattern([_obs("2026-01", "gst:SupplierNotFiled")]) is None

    def test_two_periods_is_still_not_a_pattern(self):
        """The boundary, which the one-period test cannot reach. A threshold
        off by one is invisible to a test that only checks 1 and 3, and this
        one decides whether a supplier gets a permanent characteristic
        recorded against them."""
        observations = [
            _obs("2026-01", "gst:SupplierNotFiled"),
            _obs("2026-02", "gst:SupplierNotFiled"),
        ]

        assert supplier_pattern(observations) is None

    def test_exactly_the_threshold_qualifies(self):
        """The other side of the same boundary."""
        observations = [
            _obs(f"2026-0{i}", "gst:SupplierNotFiled")
            for i in range(1, MIN_PERIODS_FOR_A_PATTERN + 1)
        ]

        assert supplier_pattern(observations) is not None

    def test_the_same_problem_across_enough_periods_is_a_pattern(self):
        observations = [
            _obs("2026-01", "gst:SupplierNotFiled"),
            _obs("2026-02", "gst:SupplierNotFiled"),
            _obs("2026-03", "gst:SupplierNotFiled"),
        ]

        pattern = supplier_pattern(observations)

        assert pattern is not None
        assert pattern["label"] == "gst:SupplierNotFiled"
        assert pattern["periods"] == 3

    def test_the_same_problem_repeated_within_one_period_is_not_a_pattern(self):
        """Three invoices unfiled in one month is one event, not a habit. The
        distinction is periods, not occurrences — and counting occurrences
        would turn a single bad month into a permanent judgement."""
        observations = [
            _obs("2026-01", "gst:SupplierNotFiled"),
            _obs("2026-01", "gst:SupplierNotFiled"),
            _obs("2026-01", "gst:SupplierNotFiled"),
            _obs("2026-01", "gst:SupplierNotFiled"),
        ]

        assert supplier_pattern(observations) is None

    def test_different_problems_across_periods_are_not_one_pattern(self):
        """A supplier late once and mismatched twice has no single
        characteristic to record. Merging them would invent a claim."""
        observations = [
            _obs("2026-01", "gst:SupplierNotFiled"),
            _obs("2026-02", "gst:AmountMismatch"),
            _obs("2026-03", "gst:TaxHeadMismatch"),
        ]

        assert supplier_pattern(observations) is None

    def test_the_strongest_pattern_wins_when_two_qualify(self):
        observations = [
            _obs(f"2026-0{i}", "gst:SupplierNotFiled") for i in range(1, 6)
        ] + [_obs(f"2026-0{i}", "gst:AmountMismatch") for i in range(1, 4)]

        pattern = supplier_pattern(observations)

        assert pattern["label"] == "gst:SupplierNotFiled"
        assert pattern["periods"] == 5

    def test_the_threshold_is_stated_rather_than_hidden(self):
        """Every magic number needs a reason. Three periods is a quarter — the
        shortest span over which "always" is a defensible word."""
        assert MIN_PERIODS_FOR_A_PATTERN == 3


class TestWhatTheMemorySays:
    def test_the_memory_names_the_supplier_the_pattern_and_its_evidence(self):
        memory = memory_for_supplier(
            gstin="27AABCS1429B1Z8",
            name="Sharma Infrastructure Pvt Ltd",
            pattern={"label": "gst:SupplierNotFiled", "periods": 4, "seen": ["2026-01", "2026-02", "2026-03", "2026-04"]},
        )

        assert "Sharma Infrastructure" in memory["content"]
        assert "4" in memory["content"]
        assert "27AABCS1429B1Z8" in memory["content"]

    def test_the_memory_is_anchored_to_the_supplier_not_to_a_period(self):
        """A memory linked to a period dies with it, which is the exact
        failure this replaces."""
        memory = memory_for_supplier(
            gstin="27AABCS1429B1Z8",
            name="Sharma",
            pattern={"label": "gst:SupplierNotFiled", "periods": 3, "seen": []},
        )

        assert memory["links"]
        assert any("27AABCS1429B1Z8" in str(link) for link in memory["links"])

    def test_confidence_rises_with_the_number_of_periods_and_is_capped(self):
        """Four periods is a stronger claim than three; twelve is not
        certainty. A confidence of 1.0 on an inference would make it
        indistinguishable from something a human confirmed."""
        three = memory_for_supplier(gstin="G", name="N", pattern={"label": "L", "periods": 3, "seen": []})
        twelve = memory_for_supplier(gstin="G", name="N", pattern={"label": "L", "periods": 12, "seen": []})

        assert three["confidence"] < twelve["confidence"]
        assert twelve["confidence"] < 1.0

    def test_confidence_never_reaches_certainty_however_long_the_history(self):
        """The ceiling, which a test comparing two values cannot pin. An
        inference at 1.0 becomes indistinguishable from a human confirmation,
        and this one is a statement about a trend."""
        forever = memory_for_supplier(
            gstin="G", name="N", pattern={"label": "L", "periods": 500, "seen": []}
        )

        assert forever["confidence"] <= 0.95
        assert forever["confidence"] < 1.0

    def test_the_memory_states_it_was_inferred_rather_than_observed_by_a_person(self):
        memory = memory_for_supplier(
            gstin="G", name="N", pattern={"label": "L", "periods": 3, "seen": []}
        )

        assert memory["kind"] == "Observation"


class TestWaiversExpire:
    def test_a_waiver_carries_a_reason_and_an_expiry(self):
        expires = datetime.now(timezone.utc) + timedelta(days=90)

        request = waiver_request(
            shape="gst:AmountMismatch",
            focus_node="urn:invoice:INV-1",
            constraint="gst:Rule36-4",
            reason="Supplier confirmed the difference is a rounding artefact",
            expires_at=expires,
        )

        assert request["reason"]
        assert request["expiresAt"].endswith("Z") or "+" in request["expiresAt"]

    def test_a_waiver_without_a_reason_is_refused(self):
        """An exception accepted for no stated reason is indistinguishable
        from a check that was switched off."""
        with pytest.raises(ValueError, match="reason"):
            waiver_request(
                shape="s",
                focus_node="f",
                constraint="c",
                reason="   ",
                expires_at=datetime.now(timezone.utc) + timedelta(days=1),
            )

    def test_a_waiver_that_has_already_expired_is_refused(self):
        """Backdating an expiry is how a permanent waiver gets written while
        appearing to have one."""
        with pytest.raises(ValueError, match="future"):
            waiver_request(
                shape="s",
                focus_node="f",
                constraint="c",
                reason="because",
                expires_at=datetime.now(timezone.utc) - timedelta(days=1),
            )

    def test_a_waiver_is_keyed_on_the_findings_identity_not_its_row_id(self):
        """Findings are replaced wholesale each pass and every row gets a fresh
        id. A waiver keyed on one would survive until the next run and then
        point at nothing."""
        request = waiver_request(
            shape="gst:AmountMismatch",
            focus_node="urn:invoice:INV-1",
            constraint="gst:Rule36-4",
            reason="checked",
            expires_at=datetime.now(timezone.utc) + timedelta(days=30),
        )

        assert request["shape"] == "gst:AmountMismatch"
        assert request["focusNode"] == "urn:invoice:INV-1"
        assert "id" not in request


class TestTheEndpoints:
    """Against a real database — the layer the pure tests cannot reach, whose
    own risk is different: not whether the pattern logic is right, but whether
    the observations fed to it actually span periods."""

    def test_a_supplier_seen_in_one_period_reports_no_pattern_and_says_why(self, client):
        created = client.post(
            "/api/clients",
            json={"name": "Mem Co", "gstin": "27AABCU9603R1ZM", "state": "Maharashtra"},
        ).json()
        period = client.post(
            f"/api/clients/{created['id']}/periods", json={"month": "March", "year": 2026}
        ).json()
        client.post(
            f"/api/clients/{created['id']}/periods/{period['id']}/cases",
            json={
                "invoice_no": "INV-1",
                "reason_code": "gst:SupplierNotFiled",
                "supplier_gstin": "29AACCG0527D1Z8",
                "supplier_name": "Late Ltd",
            },
        )

        body = client.get(
            f"/api/clients/{created['id']}/suppliers/29AACCG0527D1Z8/memory"
        ).json()

        assert body["pattern"] is None
        # "Nothing recurring" and "not enough history to say" are different
        # answers, and only one of them is reassuring.
        assert body["periods_seen"] == 1
        assert body["threshold"] == MIN_PERIODS_FOR_A_PATTERN

    def test_the_same_problem_across_three_periods_becomes_a_memory(self, client):
        created = client.post(
            "/api/clients",
            json={"name": "Mem Co 2", "gstin": "29AADCB2230M1ZT", "state": "Karnataka"},
        ).json()
        for month in ("January", "February", "March"):
            period = client.post(
                f"/api/clients/{created['id']}/periods", json={"month": month, "year": 2026}
            ).json()
            client.post(
                f"/api/clients/{created['id']}/periods/{period['id']}/cases",
                json={
                    "invoice_no": f"INV-{month}",
                    "reason_code": "gst:SupplierNotFiled",
                    "supplier_gstin": "29AACCG0527D1Z8",
                    "supplier_name": "Late Ltd",
                },
            )

        body = client.get(
            f"/api/clients/{created['id']}/suppliers/29AACCG0527D1Z8/memory"
        ).json()

        assert body["pattern"]["periods"] == 3
        assert body["pattern"]["label"] == "gst:SupplierNotFiled"
        assert "Late Ltd" in body["memory"]["content"]

    def test_one_clients_history_never_informs_anothers(self, client):
        """The isolation property every read in this product carries. A memory
        built from another client's data would be a confidentiality breach
        wearing the clothes of a feature."""
        a = client.post(
            "/api/clients", json={"name": "A", "gstin": "27AAAAA0000A1Z5", "state": "MH"}
        ).json()
        b = client.post(
            "/api/clients", json={"name": "B", "gstin": "29BBBBB1111B1Z4", "state": "KA"}
        ).json()
        for month in ("January", "February", "March"):
            period = client.post(
                f"/api/clients/{a['id']}/periods", json={"month": month, "year": 2026}
            ).json()
            client.post(
                f"/api/clients/{a['id']}/periods/{period['id']}/cases",
                json={
                    "invoice_no": f"INV-{month}",
                    "reason_code": "gst:SupplierNotFiled",
                    "supplier_gstin": "29AACCG0527D1Z8",
                    "supplier_name": "Shared Supplier",
                },
            )

        seen_by_b = client.get(
            f"/api/clients/{b['id']}/suppliers/29AACCG0527D1Z8/memory"
        ).json()

        assert seen_by_b["pattern"] is None
        assert seen_by_b["periods_seen"] == 0

    def test_recording_a_memory_with_no_pattern_is_refused_rather_than_invented(self, client):
        created = client.post(
            "/api/clients", json={"name": "C", "gstin": "27AABCU9603R1ZM", "state": "MH"}
        ).json()

        response = client.post(
            f"/api/clients/{created['id']}/suppliers/29AACCG0527D1Z8/memory"
        )

        assert response.status_code == 409, response.text
        assert "pattern" in response.json()["detail"]
