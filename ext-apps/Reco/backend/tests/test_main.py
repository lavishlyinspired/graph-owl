"""RED tests for reco-now's column auto-mapping.

Found while wiring the native graph-owl reconciliation engine
(plans/119-architecture-audit.md §5b step 5, parity testing): reco-now's
own `_auto_map` never detected `igst`/`cgst`/`sgst` against a real GSTR-2B
export's own column names — "Integrated Tax"/"Central Tax"/"State/UT Tax"
— because `_FIELD_KEYWORDS` only matched the literal substrings
"igst"/"cgst"/"sgst". This silently zeroed every tax component on the
portal side of every reconciliation run against realistic data (reco-now's
own SAMPLE/gstr2b_mar2026.csv and gstr2b_aug2026.csv both use these exact
header names), producing 0 matches out of a fixture with 7 genuinely
matching invoices — not a graph-owl integration bug, a pre-existing gap
this session's use of realistic sample data happened to surface.
"""

from __future__ import annotations

from app.main import _auto_map, _select_results
from app.reconciliation import STATUS_MATCHED


def test_maps_gstr2b_style_tax_component_headers():
    headers = [
        "Invoice No", "Invoice Date", "GSTIN of Supplier", "Supplier Name",
        "Taxable Value", "Integrated Tax", "Central Tax", "State/UT Tax",
        "Cess", "HSN/SAC", "Place of Supply", "IMS Status", "Note Type",
    ]
    mapping = _auto_map(headers)
    assert mapping["igst"] == headers.index("Integrated Tax")
    assert mapping["cgst"] == headers.index("Central Tax")
    assert mapping["sgst"] == headers.index("State/UT Tax")


def test_still_maps_the_books_style_short_headers():
    # The existing convention — must not regress.
    headers = ["Invoice No", "IGST", "CGST", "SGST", "Cess"]
    mapping = _auto_map(headers)
    assert mapping["igst"] == 1
    assert mapping["cgst"] == 2
    assert mapping["sgst"] == 3


class TestSelectResultsCutsOverToNativeFindings:
    """plans/119-architecture-audit.md §9 — reconciliation.py's own
    tolerance/matching math is no longer the primary source `/api/reconcile`
    returns. `_select_results` is the one decision point that chooses
    between the two, kept small and pure so this decision has its own test
    rather than being buried inside the endpoint."""

    def _book(self, **overrides):
        base = {
            "invoice_no": "INV-1", "supplier_gstin": "27AAAFN2938K1Z2",
            "supplier_name": "Nimbus", "taxable": 100000, "igst": 18000,
            "cgst": 0, "sgst": 0, "cess": 0,
        }
        base.update(overrides)
        return base

    def test_a_healthy_graphowl_reconcile_uses_native_findings(self):
        # No findings at all for this invoice — native says Matched, which
        # reconciliation.py's own tolerance math (never consulted here)
        # would not necessarily agree with; using native's answer is the
        # whole point of the cutover.
        results = _select_results(
            books=[self._book()],
            portal=[self._book()],
            gstr1=[],
            graphowl_reconcile={"findings": []},
            tolerance=1.0,
        )
        assert results[0]["status"] == STATUS_MATCHED

    def test_graphowl_unreachable_falls_back_to_reconciliation_py(self):
        # Best-effort, matching every other graph-owl integration point in
        # this file: a laptop with no graph-owl running must not break the
        # app, so an unreachable native engine degrades to the old
        # Python-side math rather than returning nothing at all.
        results = _select_results(
            books=[self._book()],
            portal=[self._book()],
            gstr1=[],
            graphowl_reconcile={"error": "connection refused"},
            tolerance=1.0,
        )
        assert results[0]["status"] == STATUS_MATCHED
        assert len(results) == 1
