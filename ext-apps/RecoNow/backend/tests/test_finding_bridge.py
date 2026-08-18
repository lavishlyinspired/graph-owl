"""What a graph-owl finding becomes when it lands as a Reco Now case.

The fixture in `fixtures/real_findings.json` was captured from a live
graph-owl run over the government-format March 2026 purchase register and
GSTR-2B — real rule output, real evidence bindings, not a hand-written guess
at the shape. Three defects this pins, all found by reading that real output
against the bridge:

1. `governedBy` is camelCase on the wire; the bridge read `governed_by` and
   so recorded no rule reference on any case. Same class of bug as the
   earlier `finding["rule"]` one, which the bridge's own comment describes.
2. The amounts never left the evidence. `claimed`/`filed` (and the other
   rules' equivalents) were dropped, so every case stored NULL amounts and
   computed an exposure of zero — the dashboard, ITC, at-risk and supplier
   screens all read zero rupees against real mismatches.
3. Two findings on one invoice collapsed to one case, keyed on invoice
   number alone. INV-MAR-011 has both a TaxHeadMismatch and an
   AmountMismatch; the money-carrying one was the one dropped.
"""

from __future__ import annotations

import json
import pathlib

import pytest

from app.main import case_from_finding


FIXTURE = pathlib.Path(__file__).parent / "fixtures" / "real_findings.json"
FINDINGS = json.loads(FIXTURE.read_text())


def _by_label(label: str) -> list[dict]:
    return [f for f in FINDINGS if f["label"] == label]


def test_the_fixture_is_the_real_shape():
    """Guards the test itself: if graph-owl's wire shape changes, these tests
    should be re-derived rather than quietly testing a stale contract."""
    assert len(FINDINGS) == 6
    assert {f["label"] for f in FINDINGS} == {
        "gst:AmountMismatch",
        "gst:TaxHeadMismatch",
        "gst:SupplierNotFiled",
        "gst:PotentialMismatch",
    }
    assert all("governedBy" in f for f in FINDINGS), "wire field is camelCase"


def test_amount_mismatch_carries_both_sides_and_the_difference():
    case = case_from_finding(_by_label("gst:AmountMismatch")[0])

    assert case["invoice_no"] == "INV-MAR-011"
    assert case["supplier_gstin"] == "27AABCS1429B1Z8"
    # claimed 180000.0 in books, filed 180500 on the portal.
    assert case["books_amount"] == pytest.approx(180000.0)
    assert case["portal_amount"] == pytest.approx(180500.0)


def test_the_rule_reference_survives_the_camelcase_boundary():
    case = case_from_finding(_by_label("gst:AmountMismatch")[0])

    assert case["governed_by"] == "gst:Rule36-4"
    assert case["reason_code"] == "gst:AmountMismatch"
    assert case["evidence_count"] == 6


def test_tax_head_mismatch_uses_its_own_two_sides():
    case = case_from_finding(_by_label("gst:TaxHeadMismatch")[0])

    # bookedIgst vs filedIgst — a different pair of variables to
    # AmountMismatch's, and the bridge must read the rule's own.
    assert case["books_amount"] is not None
    assert case["portal_amount"] is not None
    assert case["books_amount"] != case["portal_amount"]


def test_supplier_not_filed_puts_the_whole_tax_amount_at_risk():
    case = case_from_finding(_by_label("gst:SupplierNotFiled")[0])

    # The supplier filed nothing, so there is no portal side. The booked tax
    # is the amount at risk, not zero.
    assert case["books_amount"] is not None
    assert case["books_amount"] > 0
    assert case["portal_amount"] is None


def test_a_finding_with_no_amount_evidence_reports_no_amount_rather_than_zero():
    """Zero and unknown are different claims. A case with no amount evidence
    must not assert that nothing is at stake."""
    bare = {
        "label": "gst:SomethingNew",
        "subject": "https://example.test/x",
        "summary": "s",
        "governedBy": "gst:Rule1",
        "evidence": [{"subject": "x", "predicate": "p", "value": "INV-1", "var": "number"}],
    }
    case = case_from_finding(bare)

    assert case["books_amount"] is None
    assert case["portal_amount"] is None


def test_two_findings_on_one_invoice_are_two_distinct_cases():
    """Both are real problems with the same invoice and must both survive."""
    tax_head = case_from_finding(_by_label("gst:TaxHeadMismatch")[0])
    amount = case_from_finding(_by_label("gst:AmountMismatch")[0])

    assert tax_head["invoice_no"] == amount["invoice_no"] == "INV-MAR-011"
    # The dedup key the reconcile loop uses must separate them.
    assert tax_head["dedup_key"] != amount["dedup_key"]
