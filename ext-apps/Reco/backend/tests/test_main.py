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

from app.main import _auto_map


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
