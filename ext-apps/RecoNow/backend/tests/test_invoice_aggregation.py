"""One invoice is one comparison, whatever the source file's row shape.

Two defects found by auditing the reconciliation against real GST return
structure rather than against the sample files:

1. **Multi-rate lines.** A real GSTR-2B carries one row per rate slab per
   invoice — a 5%/12%/18% invoice is three rows. `_subject_iri` mints one
   subject per (kind, GSTIN, invoice number), so all three rows landed on the
   same subject, each writing `gst:taxableValue`. No rule in `packs/gst` uses
   `SUM` or `GROUP BY`, so the comparison bound `?filed` three times and
   compared the *invoice* total against a *single rate line* — three findings
   for one invoice, or one badly wrong one. The sample files carry one line
   per invoice, which is why it never showed.

2. **Credit notes.** `note_type` and `original_invoice_no` are captured at
   upload and never used. A supplier issuing a ₹10,000 credit note against a
   ₹1,00,000 invoice legitimately reports ₹90,000 in the 2B. Comparing the
   original invoice against that raises a ₹10,000 mismatch that is not a
   mismatch.

Both are pure functions of the parsed rows, so they are tested here without a
database or a running graph-owl.
"""

from __future__ import annotations

import pytest

from app.graphowl_client import aggregate_invoice_lines, net_credit_notes


def _line(invoice="INV-1", gstin="27AABCS1429B1Z8", taxable=0, igst=0, cgst=0, sgst=0, **kw):
    row = {
        "invoice_no": invoice,
        "supplier_gstin": gstin,
        "supplier_name": "Sharma Infra",
        "invoice_date": "01-03-2026",
        "taxable": taxable,
        "igst": igst,
        "cgst": cgst,
        "sgst": sgst,
        "cess": 0,
    }
    row.update(kw)
    return row


class TestMultiRateLines:
    def test_three_rate_lines_become_one_invoice(self):
        rows = [
            _line(taxable=10000, igst=500),    # 5%
            _line(taxable=20000, igst=2400),   # 12%
            _line(taxable=30000, igst=5400),   # 18%
        ]

        invoices = aggregate_invoice_lines(rows)

        assert len(invoices) == 1
        assert invoices[0]["taxable"] == pytest.approx(60000)
        assert invoices[0]["igst"] == pytest.approx(8300)

    def test_every_tax_head_is_summed_not_just_taxable(self):
        rows = [
            _line(taxable=1000, cgst=90, sgst=90, cess=10),
            _line(taxable=2000, cgst=180, sgst=180, cess=20),
        ]

        invoice = aggregate_invoice_lines(rows)[0]

        assert invoice["cgst"] == pytest.approx(270)
        assert invoice["sgst"] == pytest.approx(270)
        assert invoice["cess"] == pytest.approx(30)

    def test_different_invoices_stay_separate(self):
        rows = [_line(invoice="INV-1", taxable=100), _line(invoice="INV-2", taxable=200)]

        invoices = aggregate_invoice_lines(rows)

        assert sorted(i["taxable"] for i in invoices) == [100, 200]

    def test_same_invoice_number_from_different_suppliers_stays_separate(self):
        """Two suppliers reuse an invoice number constantly. Merging them would
        claim one supplier's credit against the other's invoice."""
        rows = [
            _line(invoice="001", gstin="27AABCS1429B1Z8", taxable=100),
            _line(invoice="001", gstin="29AACCS9460D1Z4", taxable=200),
        ]

        assert len(aggregate_invoice_lines(rows)) == 2

    def test_lines_are_grouped_on_the_normalised_key(self):
        """`INV/2026/1` and `INV-2026-1` are the same invoice — the same key
        the canonical subject already uses."""
        rows = [_line(invoice="INV/2026/1", taxable=100), _line(invoice="INV-2026-1", taxable=200)]

        invoices = aggregate_invoice_lines(rows)

        assert len(invoices) == 1
        assert invoices[0]["taxable"] == pytest.approx(300)

    def test_the_printed_invoice_number_survives_aggregation(self):
        """A CA searches for what is printed on the document, not the key."""
        rows = [_line(invoice="INV/2026/1", taxable=100), _line(invoice="INV/2026/1", taxable=200)]

        assert aggregate_invoice_lines(rows)[0]["invoice_no"] == "INV/2026/1"

    def test_non_numeric_fields_are_carried_not_summed(self):
        rows = [_line(taxable=100), _line(taxable=200)]

        invoice = aggregate_invoice_lines(rows)[0]

        assert invoice["supplier_name"] == "Sharma Infra"
        assert invoice["invoice_date"] == "01-03-2026"

    def test_a_single_line_invoice_is_unchanged(self):
        """The overwhelmingly common case must not be disturbed."""
        rows = [_line(taxable=250000, igst=45000)]

        invoice = aggregate_invoice_lines(rows)[0]

        assert invoice["taxable"] == pytest.approx(250000)
        assert invoice["igst"] == pytest.approx(45000)

    def test_blank_amounts_do_not_break_the_sum(self):
        rows = [_line(taxable=100, igst=None), _line(taxable="", igst=50)]

        invoice = aggregate_invoice_lines(rows)[0]

        assert invoice["taxable"] == pytest.approx(100)
        assert invoice["igst"] == pytest.approx(50)


class TestCreditNotes:
    def test_a_credit_note_reduces_its_original_invoice(self):
        rows = [
            _line(invoice="INV-1", taxable=100000, igst=18000),
            _line(invoice="CN-1", taxable=10000, igst=1800,
                  note_type="C", original_invoice_no="INV-1"),
        ]

        netted = net_credit_notes(rows)

        assert len(netted) == 1
        assert netted[0]["invoice_no"] == "INV-1"
        assert netted[0]["taxable"] == pytest.approx(90000)
        assert netted[0]["igst"] == pytest.approx(16200)

    def test_a_debit_note_increases_its_original_invoice(self):
        rows = [
            _line(invoice="INV-1", taxable=100000, igst=18000),
            _line(invoice="DN-1", taxable=5000, igst=900,
                  note_type="D", original_invoice_no="INV-1"),
        ]

        netted = net_credit_notes(rows)

        assert netted[0]["taxable"] == pytest.approx(105000)
        assert netted[0]["igst"] == pytest.approx(18900)

    def test_a_credit_note_naming_an_absent_invoice_is_kept_not_dropped(self):
        """The original may sit in a different period. Dropping it would hide
        a real document; keeping it lets a cross-period rule find it."""
        rows = [_line(invoice="CN-9", taxable=5000, note_type="C",
                      original_invoice_no="INV-FROM-LAST-MONTH")]

        netted = net_credit_notes(rows)

        assert len(netted) == 1
        assert netted[0]["invoice_no"] == "CN-9"

    def test_a_credit_note_matches_its_original_on_the_normalised_key(self):
        rows = [
            _line(invoice="INV/2026/1", taxable=100000),
            _line(invoice="CN-1", taxable=10000, note_type="C",
                  original_invoice_no="inv-2026-1"),
        ]

        netted = net_credit_notes(rows)

        assert len(netted) == 1
        assert netted[0]["taxable"] == pytest.approx(90000)

    def test_note_type_spellings_all_work(self):
        """Portals and ERPs spell this differently; the meaning is the same."""
        for spelling in ("C", "c", "CR", "Credit", "Credit Note", "CREDIT NOTE"):
            rows = [
                _line(invoice="INV-1", taxable=1000),
                _line(invoice="CN", taxable=100, note_type=spelling,
                      original_invoice_no="INV-1"),
            ]
            assert net_credit_notes(rows)[0]["taxable"] == pytest.approx(900), spelling

    def test_rows_with_no_note_type_are_untouched(self):
        rows = [_line(invoice="INV-1", taxable=1000), _line(invoice="INV-2", taxable=2000)]

        netted = net_credit_notes(rows)

        assert sorted(r["taxable"] for r in netted) == [1000, 2000]

    def test_a_credit_note_cannot_take_an_invoice_below_zero(self):
        """A CN larger than the invoice it names means the data is wrong, or
        the original is in another period. Clamping at zero would state a
        position nobody computed, so the row is left for a human instead."""
        rows = [
            _line(invoice="INV-1", taxable=1000),
            _line(invoice="CN", taxable=5000, note_type="C", original_invoice_no="INV-1"),
        ]

        netted = net_credit_notes(rows)

        assert len(netted) == 2, "an over-large credit note is surfaced, not silently applied"


class TestTurtleIsInvoiceLevel:
    """The end-to-end property: whatever the file's row shape, the graph gets
    one subject per invoice carrying the invoice's own totals."""

    def test_three_rate_lines_emit_one_subject(self):
        from app.graphowl_client import rows_to_turtle

        turtle = rows_to_turtle(
            [
                _line(taxable=10000, igst=500),
                _line(taxable=20000, igst=2400),
                _line(taxable=30000, igst=5400),
            ],
            "gstr2b",
        )

        assert turtle.count("a gst:Gstr2bInvoice") == 1
        assert '"60000"' in turtle, turtle

    def test_a_netted_invoice_reaches_the_graph_netted(self):
        from app.graphowl_client import rows_to_turtle

        turtle = rows_to_turtle(
            [
                _line(invoice="INV-1", taxable=100000),
                _line(invoice="CN-1", taxable=10000, note_type="C",
                      original_invoice_no="INV-1"),
            ],
            "gstr2b",
        )

        assert turtle.count("a gst:Gstr2bInvoice") == 1
        assert '"90000"' in turtle, turtle


def test_a_credit_note_is_judged_against_the_invoice_not_one_rate_line():
    """Pins the ordering: aggregate, then net.

    A three-line invoice totalling 60,000 with a 25,000 credit note. If
    netting runs before aggregation, the note is compared against the *first
    rate line* (10,000), judged over-large, and refused — leaving a 25,000
    phantom mismatch. Aggregating first compares it against the invoice.

    This is the negative case that was missing when the ordering mutant
    survived: both orders give the same arithmetic when nothing is refused,
    so only a refusal boundary can tell them apart.
    """
    from app.graphowl_client import rows_to_turtle

    rows = [
        _line(invoice="INV-1", taxable=10000),
        _line(invoice="INV-1", taxable=20000),
        _line(invoice="INV-1", taxable=30000),
        _line(invoice="CN-1", taxable=25000, note_type="C", original_invoice_no="INV-1"),
    ]

    turtle = rows_to_turtle(rows, "gstr2b")

    assert turtle.count("a gst:Gstr2bInvoice") == 1, "the note is absorbed, not a second subject"
    assert '"35000"' in turtle, turtle


class TestSignedCreditNotes:
    """Real GST files sign credit notes negative.

    The March 2026 sample carries `CN-MAR-001` at **-12000** with
    `Note Type = Credit Note`. Applying a negative sign to an already-negative
    amount adds instead of subtracting: a 42,000 invoice with a 12,000 credit
    note netted to 54,000, and the phantom 12,000 then showed up as a review
    item against a portal side that had it right. Found by running the real
    file, not the fixtures.

    The note's *kind* decides the direction; its recorded sign is only how the
    file happens to write it.
    """

    def test_a_negative_credit_note_subtracts(self):
        rows = [
            _line(invoice="INV-3", taxable=42000, igst=7560),
            _line(invoice="CN-1", taxable=-12000, igst=-2160,
                  note_type="Credit Note", original_invoice_no="INV-3"),
        ]

        netted = net_credit_notes(rows)

        assert netted[0]["taxable"] == pytest.approx(30000)
        assert netted[0]["igst"] == pytest.approx(5400)

    def test_a_positive_credit_note_also_subtracts(self):
        """Some ERPs write the magnitude and rely on the note type."""
        rows = [
            _line(invoice="INV-3", taxable=42000),
            _line(invoice="CN-1", taxable=12000,
                  note_type="Credit Note", original_invoice_no="INV-3"),
        ]

        assert net_credit_notes(rows)[0]["taxable"] == pytest.approx(30000)

    def test_a_negative_debit_note_still_increases(self):
        rows = [
            _line(invoice="INV-3", taxable=42000),
            _line(invoice="DN-1", taxable=-5000,
                  note_type="Debit Note", original_invoice_no="INV-3"),
        ]

        assert net_credit_notes(rows)[0]["taxable"] == pytest.approx(47000)

    def test_the_over_large_guard_uses_magnitude_not_sign(self):
        """A -50,000 note against a 42,000 invoice is over-large however it is
        signed, and must be surfaced rather than applied."""
        rows = [
            _line(invoice="INV-3", taxable=42000),
            _line(invoice="CN-1", taxable=-50000,
                  note_type="Credit Note", original_invoice_no="INV-3"),
        ]

        assert len(net_credit_notes(rows)) == 2
