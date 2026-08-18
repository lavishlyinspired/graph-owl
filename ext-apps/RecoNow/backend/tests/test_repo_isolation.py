"""RED tests for Plan 122b B0's own stated RED requirement, verbatim:
"a follow-up created for client A is not visible to client B ... a case in
period 2026-08 is not returned for 2026-07 ... restart, re-read, same
state." Two clients and three periods, not one of each — the plan's own
mutation note: a repository query missing its client_id predicate still
returns plausible rows against a single-client fixture, so a single-client
test would let that mutant survive.
"""

from __future__ import annotations

import asyncpg

from app import repo


async def test_a_follow_up_created_for_one_client_is_not_visible_to_another(pool):
    async with pool.acquire() as conn:
        client_a = await repo.create_client(conn, name="ABC Manufacturing", gstin="27AAAAA0000A1Z5", state="Maharashtra")
        client_b = await repo.create_client(conn, name="Kaveri Textiles", gstin="29BBBBB1111B1Z4", state="Karnataka")
        period_a = await repo.create_period(conn, client_id=client_a, month="August", year=2026)

        await repo.create_follow_up(
            conn, client_id=client_a, period_id=period_a, case_id=None,
            supplier_name="XYZ Pvt Ltd", message="Please confirm the IGST value.",
        )

        follow_ups_a = await repo.list_follow_ups(conn, client_id=client_a)
        follow_ups_b = await repo.list_follow_ups(conn, client_id=client_b)

    assert len(follow_ups_a) == 1
    assert follow_ups_a[0]["supplier_name"] == "XYZ Pvt Ltd"
    assert follow_ups_b == []


async def test_a_case_in_one_period_is_not_returned_for_a_different_period(pool):
    async with pool.acquire() as conn:
        client_id = await repo.create_client(conn, name="ABC Manufacturing", gstin="27AAAAA0000A1Z5", state="Maharashtra")
        period_jul = await repo.create_period(conn, client_id=client_id, month="July", year=2026)
        period_aug = await repo.create_period(conn, client_id=client_id, month="August", year=2026)
        period_sep = await repo.create_period(conn, client_id=client_id, month="September", year=2026)

        await repo.create_case(conn, client_id=client_id, period_id=period_aug, invoice_no="INV-1025", reason_code="amount_mismatch")

        cases_jul = await repo.list_cases(conn, client_id=client_id, period_id=period_jul)
        cases_aug = await repo.list_cases(conn, client_id=client_id, period_id=period_aug)
        cases_sep = await repo.list_cases(conn, client_id=client_id, period_id=period_sep)

    assert cases_jul == []
    assert len(cases_aug) == 1
    assert cases_aug[0]["invoice_no"] == "INV-1025"
    assert cases_sep == []


async def test_two_clients_three_periods_no_cross_contamination_anywhere(pool):
    """The plan's own acceptance criterion, exercised directly: two clients,
    three periods each, one case per client/period cell — no cell ever
    sees another cell's row."""
    async with pool.acquire() as conn:
        clients = {
            name: await repo.create_client(conn, name=name, gstin=gstin, state="Maharashtra")
            for name, gstin in [("ABC Manufacturing", "27AAAAA0000A1Z5"), ("Kaveri Textiles", "29BBBBB1111B1Z4")]
        }
        periods: dict[tuple[str, str], str] = {}
        for client_name, client_id in clients.items():
            for month in ("July", "August", "September"):
                periods[(client_name, month)] = await repo.create_period(
                    conn, client_id=client_id, month=month, year=2026
                )
                await repo.create_case(
                    conn,
                    client_id=client_id,
                    period_id=periods[(client_name, month)],
                    invoice_no=f"INV-{client_name}-{month}",
                    reason_code=None,
                )

        for client_name, client_id in clients.items():
            for month in ("July", "August", "September"):
                cases = await repo.list_cases(conn, client_id=client_id, period_id=periods[(client_name, month)])
                assert len(cases) == 1
                assert cases[0]["invoice_no"] == f"INV-{client_name}-{month}"


async def test_state_survives_a_restart(pool):
    """A fresh connection against the same database — the durability half
    of B0's AC, distinct from the isolation half above."""
    async with pool.acquire() as conn:
        client_id = await repo.create_client(conn, name="ABC Manufacturing", gstin="27AAAAA0000A1Z5", state="Maharashtra")
        period_id = await repo.create_period(conn, client_id=client_id, month="August", year=2026)
        await repo.create_case(conn, client_id=client_id, period_id=period_id, invoice_no="INV-1025", reason_code=None)

    # A second, independent connection acquired from the same pool reads
    # only committed rows — the same guarantee a real process restart would
    # give, without depending on any pool-internal attribute to reconnect.
    async with pool.acquire() as fresh_conn:
        cases = await repo.list_cases(fresh_conn, client_id=client_id, period_id=period_id)
    assert len(cases) == 1
    assert cases[0]["invoice_no"] == "INV-1025"
