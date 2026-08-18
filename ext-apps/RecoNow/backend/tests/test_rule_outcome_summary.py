"""A rule outcome must carry the rule's own words, not only its label.

`gst:PaymentOverdue` tells a reviewer nothing about what was checked. The
engine already writes a one-line summary for every rule; if it is dropped on
the way to storage, the screen can only show a label, and a reviewer reading
"not evaluated" has no way to learn what did not happen.
"""

from __future__ import annotations

from app import repo

OVERDUE = "Credit taken on an invoice not paid within 180 days of its date"


async def _period(conn) -> tuple[str, str]:
    client_id = await repo.create_client(
        conn, name="Summary Co", gstin="27AABCU9603R1ZM", state="Maharashtra"
    )
    period_id = await repo.create_period(conn, client_id=client_id, month="March", year=2026)
    return client_id, period_id


async def test_a_stored_outcome_keeps_the_rules_own_summary(pool):
    async with pool.acquire() as conn:
        client_id, period_id = await _period(conn)
        await repo.replace_rule_outcomes(
            conn,
            client_id=client_id,
            period_id=period_id,
            outcomes=[
                {
                    "label": "gst:PaymentOverdue",
                    "governedBy": "gst:Section16-2-d",
                    "summary": OVERDUE,
                    "status": "flagged",
                    "found": 2,
                    "unmet": [],
                }
            ],
        )

        stored = await repo.list_rule_outcomes(conn, client_id=client_id, period_id=period_id)

    assert len(stored) == 1
    assert stored[0]["summary"] == OVERDUE


async def test_an_outcome_with_no_summary_stores_none_rather_than_inventing_text(pool):
    """A rule that supplied no summary must leave the screen with nothing to
    show, not a fabricated or empty-looking explanation."""
    async with pool.acquire() as conn:
        client_id, period_id = await _period(conn)
        await repo.replace_rule_outcomes(
            conn,
            client_id=client_id,
            period_id=period_id,
            outcomes=[
                {
                    "label": "gst:Unknown",
                    "governedBy": None,
                    "status": "not_evaluated",
                    "found": 0,
                    "unmet": ["gst:PaymentEvent"],
                }
            ],
        )

        stored = await repo.list_rule_outcomes(conn, client_id=client_id, period_id=period_id)

    assert stored[0]["summary"] is None
    assert stored[0]["unmet"] == ["gst:PaymentEvent"]


async def test_each_rules_summary_stays_with_its_own_label(pool):
    """Two rules stored together must not swap their explanations — the
    failure a single-row test cannot see."""
    async with pool.acquire() as conn:
        client_id, period_id = await _period(conn)
        await repo.replace_rule_outcomes(
            conn,
            client_id=client_id,
            period_id=period_id,
            outcomes=[
                {
                    "label": "gst:AmountMismatch",
                    "governedBy": "gst:Rule36-4",
                    "summary": "the values differ",
                    "status": "flagged",
                    "found": 2,
                    "unmet": [],
                },
                {
                    "label": "gst:Reversed",
                    "governedBy": "gst:Section16-2-aa",
                    "summary": "the authority flags it as reverse-charge",
                    "status": "passed",
                    "found": 0,
                    "unmet": [],
                },
            ],
        )

        stored = {
            r["label"]: r["summary"]
            for r in await repo.list_rule_outcomes(
                conn, client_id=client_id, period_id=period_id
            )
        }

    assert stored["gst:AmountMismatch"] == "the values differ"
    assert stored["gst:Reversed"] == "the authority flags it as reverse-charge"
