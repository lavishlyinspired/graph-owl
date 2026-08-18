"""RED tests for Plan 123 Slice C — GSTR-2A alongside GSTR-2B.

**Reverses a documented decision, deliberately.** `graphowl_client.py` said
there is "no separate `Gstr2aInvoice` class — 2A is a revolving view over the
same supplier-declared data `gst:Gstr1Invoice` already carries". That is true
of the *content* and misses the reason 2A matters: a revolving view has a
**pull date** and a filing does not. 2B is frozen on the 14th and is what a
claim rests on; 2A keeps moving afterwards, and the whole value of holding it
is answering "what has the portal said since the 2B I already claimed
against". A class with no time cannot express that, so 2A gets its own subject
and its own snapshot — not because its columns differ, but because *when it
was observed* is load-bearing and GSTR-1's is not.

Collapsing 2A into `gstr1` also loses the distinction between two different
authorities: GSTR-1 is what the supplier says they filed, 2A is what the
portal shows. A reviewer chasing a supplier needs to know which one is
speaking.
"""

from __future__ import annotations

import pytest

from app.graphowl_client import rows_to_turtle

PERIOD = "2026-03"


def a_2a_row(**overrides) -> dict:
    row = {
        "invoice_no": "INV-MAR-021",
        "supplier_gstin": "27AABCS1429B1Z8",
        "supplier_name": "Sharma Infrastructure Pvt Ltd",
        "invoice_date": "2026-03-18",
        "taxable_value": "100000",
        "igst": "18000",
        "period": PERIOD,
        "pulled_on": "2026-05-02",
    }
    row.update(overrides)
    return row


class TestGstr2aIsItsOwnKind:
    def test_a_2a_row_mints_its_own_class_not_a_gstr1_invoice(self):
        """The two are different authorities. A reviewer chasing a late filing
        needs to know whether the supplier said so or the portal did."""
        turtle = rows_to_turtle([a_2a_row()], "gstr2a")

        assert "gst:Gstr2aInvoice" in turtle
        assert "gst:Gstr1Invoice" not in turtle

    def test_a_2a_row_links_to_the_canonical_invoice_by_its_own_predicate(self):
        """`recordedIn` (books), `reflectedIn` (2B) and `appearsIn` (GSTR-1)
        each name who is speaking. 2A needs its own or its facts are
        indistinguishable from the supplier's own declaration."""
        turtle = rows_to_turtle([a_2a_row()], "gstr2a")

        # `observedIn` links the canonical invoice to this 2A line; `seenIn`
        # links the line to the pull it was read in. Asserting only the first
        # would pass with both levels collapsed onto one verb.
        assert "gst:observedIn" in turtle
        assert "gst:seenIn" in turtle

    def test_the_snapshot_carries_the_date_the_2a_was_pulled(self):
        """The reason 2A is not 2B. Without a pull date every snapshot is
        indistinguishable and "what changed since I claimed" is unanswerable."""
        turtle = rows_to_turtle([a_2a_row()], "gstr2a")

        assert "gst:Gstr2aSnapshot" in turtle
        assert "gst:pulledOn" in turtle
        assert '"2026-05-02"' in turtle
        # The date belongs to the snapshot, never to the line — a date on each
        # line would report drift between two invoices read at one moment.
        assert "gst:pulledOn" not in turtle.split("a gst:Gstr2aInvoice")[-1]

    def test_two_pulls_of_one_period_are_two_distinct_snapshots(self):
        """A snapshot keyed only by period would let a later pull overwrite
        the earlier one — destroying the very history drift is computed from."""
        turtle = rows_to_turtle(
            [
                a_2a_row(pulled_on="2026-04-14"),
                a_2a_row(invoice_no="INV-MAR-022", pulled_on="2026-05-02"),
            ],
            "gstr2a",
        )

        assert turtle.count("a gst:Gstr2aSnapshot") == 2

    def test_two_rows_from_one_pull_share_a_single_snapshot(self):
        """The converse: one pull is one observation, however many lines it
        carried. A snapshot per row would report drift between two invoices
        seen at the same moment."""
        turtle = rows_to_turtle(
            [a_2a_row(), a_2a_row(invoice_no="INV-MAR-022")],
            "gstr2a",
        )

        assert turtle.count("a gst:Gstr2aSnapshot") == 1

    def test_a_2a_row_with_no_pull_date_still_lands(self):
        """A firm exporting 2A without a pull-date column loses drift, not the
        invoice. Refusing the file would leave the reviewer with nothing."""
        row = a_2a_row()
        del row["pulled_on"]

        turtle = rows_to_turtle([row], "gstr2a")

        assert "gst:Gstr2aInvoice" in turtle
        assert "gst:pulledOn" not in turtle

    def test_the_2a_invoice_carries_the_key_the_join_needs(self):
        """Without `invoiceKey` the 2A side cannot be matched to a 2B line at
        all, and every rule built on the comparison silently matches nothing."""
        turtle = rows_to_turtle([a_2a_row()], "gstr2a")

        assert "gst:invoiceKey" in turtle
        assert "gst:taxAmount" in turtle

    def test_2a_does_not_disturb_the_2b_kind(self):
        """2A is ingested *alongside* 2B, never instead of it. A claim rests on
        the frozen 2B and must be unaffected by a later 2A pull."""
        turtle = rows_to_turtle([a_2a_row()], "gstr2b")

        assert "gst:Gstr2bInvoice" in turtle
        assert "gst:Gstr2aInvoice" not in turtle
        assert "gst:observedIn" not in turtle
        assert "gst:reflectedIn" in turtle

    def test_gstr1_still_mints_a_filing_not_a_snapshot(self):
        """The reversal must not leak backwards: a supplier's own declaration
        has a filing date, not a pull date."""
        turtle = rows_to_turtle([a_2a_row(filed_date="2026-04-11")], "gstr1")

        assert "gst:Gstr1Filing" in turtle
        assert "gst:Gstr2aSnapshot" not in turtle
