"""native_findings.reconcile: the UI's primary reconciliation source, built
from graph-owl's native rule engine instead of reconciliation.py's own
tolerance/matching math (plans/119-architecture-audit.md §9 — the cutover).

Every finding query projects `?gstin`/`?number` as evidence (confirmed by
reading all 13 packs/gst finding queries), so a finding is attached to a row
by that pair, not by a copy of any one query's own subject convention —
which is what lets one function handle all 13 labels uniformly."""

from __future__ import annotations

from app.native_findings import reconcile
from app.reconciliation import (
    STATUS_MATCHED,
    STATUS_ONLY_BOOKS,
    STATUS_ONLY_GSTR2B,
    STATUS_REVIEW,
)


def _book(**overrides) -> dict:
    base = {
        "invoice_no": "INV-1",
        "supplier_gstin": "27AAAFN2938K1Z2",
        "supplier_name": "Nimbus Freight Logistics",
        "taxable": 180000,
        "igst": 32400,
        "cgst": 0,
        "sgst": 0,
        "cess": 0,
    }
    base.update(overrides)
    return base


def _portal(**overrides) -> dict:
    base = {
        "invoice_no": "INV-1",
        "supplier_gstin": "27AAAFN2938K1Z2",
        "supplier_name": "Nimbus Freight Logistics",
        "taxable": 180000,
        "igst": 32400,
        "cgst": 0,
        "sgst": 0,
        "cess": 0,
    }
    base.update(overrides)
    return base


def _finding(
    label: str, gstin: str, number: str, *, priority: int | None = None, **extra_evidence
) -> dict:
    evidence = [
        {"var": "gstin", "value": gstin, "predicate": "gst:supplierGstin"},
        {"var": "number", "value": number, "predicate": "gst:invoiceNumber"},
    ]
    for var, value in extra_evidence.items():
        evidence.append({"var": var, "value": value, "predicate": f"gst:{var}"})
    return {"label": label, "evidence": evidence, "priority": priority}


class TestNoFindingIsMatched:
    def test_a_books_and_portal_pair_with_no_finding_is_matched(self):
        results = reconcile([_book()], [_portal()], [], [])
        assert len(results) == 1
        assert results[0]["status"] == STATUS_MATCHED

    def test_matched_still_reports_book_and_portal_views(self):
        results = reconcile([_book()], [_portal()], [], [])
        assert results[0]["book"]["invoice_no"] == "INV-1"
        assert results[0]["portal"]["invoice_no"] == "INV-1"


class TestSingleFindingMapsToStatus:
    def test_potentialmismatch_is_only_books(self):
        findings = [_finding("gst:PotentialMismatch", "27AAAFN2938K1Z2", "INV-1")]
        results = reconcile([_book()], [], [], findings)
        assert results[0]["status"] == STATUS_ONLY_BOOKS
        assert "GSTR-2B" in results[0]["reason"]

    def test_suppliernotfiled_is_only_books_with_its_own_reason(self):
        findings = [_finding("gst:SupplierNotFiled", "27AAAFN2938K1Z2", "INV-1")]
        results = reconcile([_book()], [], [], findings)
        assert results[0]["status"] == STATUS_ONLY_BOOKS
        assert "Supplier" in results[0]["reason"]

    def test_amountmismatch_is_review(self):
        findings = [_finding("gst:AmountMismatch", "27AAAFN2938K1Z2", "INV-1")]
        results = reconcile([_book()], [_portal(taxable=175000)], [], findings)
        assert results[0]["status"] == STATUS_REVIEW

    def test_taxheadmismatch_is_review(self):
        findings = [_finding("gst:TaxHeadMismatch", "27AAAFN2938K1Z2", "INV-1")]
        results = reconcile(
            [_book(igst=0, cgst=16200, sgst=16200)],
            [_portal(igst=32400, cgst=0, sgst=0)],
            [],
            findings,
        )
        assert results[0]["status"] == STATUS_REVIEW

    def test_missinginbooks_is_only_gstr2b_sourced_from_the_gstr1_row(self):
        gstr1_row = _book(invoice_no="INV-9", taxable=12000)
        findings = [_finding("gst:MissingInBooks", "27AAAFN2938K1Z2", "INV-9")]
        results = reconcile([], [], [gstr1_row], findings)
        assert len(results) == 1
        assert results[0]["status"] == STATUS_ONLY_GSTR2B
        assert results[0]["book"] is None

    def test_gstr1_not_in_2b_is_only_books(self):
        findings = [_finding("gst:Gstr1NotIn2b", "27AAAFN2938K1Z2", "INV-1")]
        results = reconcile([_book()], [], [_book()], findings)
        assert results[0]["status"] == STATUS_ONLY_BOOKS
        assert "2B" in results[0]["reason"]

    def test_booksgstr1mismatch_is_review(self):
        findings = [_finding("gst:BooksGstr1Mismatch", "27AAAFN2938K1Z2", "INV-1")]
        results = reconcile(
            [_book(taxable=55000)], [], [_book(taxable=53000)], findings
        )
        assert results[0]["status"] == STATUS_REVIEW


class TestMultipleFindingsOnOneInvoice:
    """Epic 105 P10 (plans/119-architecture-audit.md §10): each finding
    carries its own `priority` from graph-owl (packs/gst/pack.toml), read
    off the wire rather than hardcoded here — reco-now no longer maintains
    its own table ranking finding labels."""

    def test_a_lower_priority_number_outranks_a_higher_one(self):
        # The real case this ranking exists to get right: INV-AUG-114 in
        # plans/119-architecture-audit.md §8a is both Gstr1NotIn2b (not
        # yet available, packs/gst priority 2) AND BooksGstr1Mismatch (the
        # two sides actually disagree, priority 1) — the disagreement is
        # the more actionable fact, so it must win even though Gstr1NotIn2b
        # is listed first.
        findings = [
            _finding("gst:Gstr1NotIn2b", "27AAAFN2938K1Z2", "INV-1", priority=2),
            _finding("gst:BooksGstr1Mismatch", "27AAAFN2938K1Z2", "INV-1", priority=1),
        ]
        results = reconcile(
            [_book(taxable=55000)], [], [_book(taxable=53000)], findings
        )
        assert results[0]["status"] == STATUS_REVIEW

    def test_both_findings_reasons_appear_in_the_combined_reason_text(self):
        findings = [
            _finding("gst:Gstr1NotIn2b", "27AAAFN2938K1Z2", "INV-1", priority=2),
            _finding("gst:BooksGstr1Mismatch", "27AAAFN2938K1Z2", "INV-1", priority=1),
        ]
        results = reconcile(
            [_book(taxable=55000)], [], [_book(taxable=53000)], findings
        )
        assert "2B" in results[0]["reason"]
        assert "GSTR-1" in results[0]["reason"]

    def test_a_declared_priority_outranks_an_undeclared_one_regardless_of_number(self):
        # gst:PaymentOverdue declares no priority at all (packs/gst's own
        # odd one out — payment-timing, not a documents-disagree
        # conclusion). Any finding that *does* declare one must win over
        # it, not just one with a numerically lower value.
        findings = [
            _finding("gst:PaymentOverdue", "27AAAFN2938K1Z2", "INV-1", priority=None),
            _finding("gst:MissingInBooks", "27AAAFN2938K1Z2", "INV-1", priority=3),
        ]
        results = reconcile([_book()], [], [_book()], findings)
        assert results[0]["status"] == STATUS_ONLY_GSTR2B
        assert "Declared (GSTR-1), Not in Books" in results[0]["reason"]


class TestOrphanedPortalRow:
    def test_a_portal_row_with_no_books_match_and_no_finding_is_only_gstr2b(self):
        results = reconcile([], [_portal(invoice_no="INV-5")], [], [])
        assert len(results) == 1
        assert results[0]["status"] == STATUS_ONLY_GSTR2B
        assert results[0]["book"] is None


class TestFindingKeyingIsCaseAndPunctuationInsensitive:
    def test_a_finding_attaches_despite_gstin_case_or_invoice_number_punctuation(self):
        findings = [_finding("gst:AmountMismatch", "27aaafn2938k1z2", "inv-1")]
        results = reconcile([_book()], [_portal(taxable=175000)], [], findings)
        assert results[0]["status"] == STATUS_REVIEW
