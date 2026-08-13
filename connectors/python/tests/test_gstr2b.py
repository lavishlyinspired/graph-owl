"""GSTR-2B as an ingestion connector — Epic 105 P2.

**The authority's JSON is evidence, not a schema to adopt.** This normalizes
it into the vocabulary `packs/gst/ontology.ttl` already defines, so the
reconciliation rules cannot tell a fixture from a live GSP response. That is
the whole point of the split: the intelligence layer is developed and tested
at zero API cost, and going live changes where the bytes come from and
nothing else.

Field names are taken from a published GSP API reference, not from memory —
`ctin`, `inum`, `dt`, `txval`, `igst`/`cgst`/`sgst`/`cess`, `itcavl`, `rev`,
`typ`, `pos`, nested under `docdata.b2b[].inv[]`.
"""

from __future__ import annotations

import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from graph_owl_packs.gstr2b import (  # noqa: E402
    Gstr2bError,
    normalize,
    return_period,
    to_turtle,
)


def get_payload(**overrides: object) -> dict:
    """A GSTR-2B response in the documented shape.

    Two suppliers and three invoices, because a normalizer that only ever sees
    one of each cannot show that it walks the nesting rather than reaching for
    the first element.
    """
    payload = {
        "data": {
            "data": {
                "docdata": {
                    "b2b": [
                        {
                            "ctin": "27AABCU9603R1ZM",
                            "trdnm": "Umbrella Supplies",
                            "supfildt": "11-08-2026",
                            "supprd": "072026",
                            "inv": [
                                {
                                    "inum": "INV-1001",
                                    "dt": "04-07-2026",
                                    "txval": 100000.00,
                                    "igst": 18000.00,
                                    "cgst": 0,
                                    "sgst": 0,
                                    "cess": 0,
                                    "itcavl": "Y",
                                    "rev": "N",
                                    "typ": "R",
                                    "pos": "27",
                                },
                                {
                                    "inum": "INV-1005",
                                    "dt": "24-07-2026",
                                    "txval": 40000.00,
                                    "igst": 0,
                                    "cgst": 3600.00,
                                    "sgst": 3600.00,
                                    "cess": 0,
                                    "itcavl": "N",
                                    "rev": "N",
                                    "typ": "R",
                                    "pos": "27",
                                },
                            ],
                        },
                        {
                            "ctin": "29AACCG0527D1Z8",
                            "trdnm": "Globex Trading",
                            "supprd": "072026",
                            "inv": [
                                {
                                    "inum": "INV-1003",
                                    "dt": "15-07-2026",
                                    "txval": 250000.00,
                                    "igst": 45000.00,
                                    "cgst": 0,
                                    "sgst": 0,
                                    "cess": 0,
                                    "itcavl": "Y",
                                    "rev": "N",
                                    "typ": "R",
                                    "pos": "29",
                                }
                            ],
                        },
                    ]
                }
            }
        }
    }
    payload.update(overrides)
    return payload


def test_every_invoice_under_every_supplier_is_found() -> None:
    records = normalize(get_payload())

    assert [r.invoice_number for r in records] == ["INV-1001", "INV-1005", "INV-1003"]
    assert records[2].supplier_gstin == "29AACCG0527D1Z8"


def test_dates_are_converted_to_iso_because_the_rules_depend_on_ordering() -> None:
    """**The trap this connector exists to close.**

    GST returns dates as `DD-MM-YYYY`. Every finding rule in `packs/gst`
    compares dates as ISO strings, relying on lexicographic order being
    chronological order — that is what makes "which provision was in force"
    answerable at all, since the query engine has no date type. Passing
    `04-07-2026` straight through breaks that ordering *silently*: the string
    sorts by day-of-month, so the cap resolution would pick the wrong
    provision and the finding would cite the wrong notification.
    """
    records = normalize(get_payload())

    assert records[0].invoice_date == "2026-07-04"
    assert records[1].invoice_date == "2026-07-24"


def test_an_iso_date_is_passed_through_unchanged() -> None:
    """Not every GSP normalizes the same way, and re-parsing an ISO date as
    day-first would turn 2026-07-04 into something nonsensical."""
    payload = get_payload()
    payload["data"]["data"]["docdata"]["b2b"][0]["inv"][0]["dt"] = "2026-07-04"

    assert normalize(payload)[0].invoice_date == "2026-07-04"


def test_an_unreadable_date_is_refused_rather_than_guessed() -> None:
    """**Never passed through.** A date the connector cannot place is a date
    the rules would silently mis-order, and a mis-ordered date produces a
    finding citing the wrong statute — which is worse than no finding, because
    it looks authoritative."""
    payload = get_payload()
    payload["data"]["data"]["docdata"]["b2b"][0]["inv"][0]["dt"] = "July 4th"

    with pytest.raises(Gstr2bError, match="July 4th"):
        normalize(payload)


def test_the_tax_total_is_the_sum_of_its_components() -> None:
    """A purchase register records one tax figure; GSTR-2B splits it four ways.

    Both are kept: the total is what reconciles against the register, and the
    components are evidence a reviewer needs to see, because an intra-state
    supply reported as inter-state is a real and common error that the total
    alone hides completely.
    """
    records = normalize(get_payload())

    assert records[0].tax_amount == "18000.00"
    assert records[1].tax_amount == "7200.00", "3600 CGST + 3600 SGST"
    assert records[1].cgst == "3600.00"
    assert records[1].sgst == "3600.00"
    assert records[1].igst == "0.00"


def test_money_keeps_two_decimals_rather_than_becoming_a_float_string() -> None:
    """`str(40000.0)` is `"40000.0"`, which does not equal the register's
    `"40000.00"` — and the reconciliation compares these as strings."""
    records = normalize(get_payload())

    assert records[1].taxable_value == "40000.00"


def test_the_response_wrapper_depth_is_tolerated() -> None:
    """Providers disagree about how deeply they wrap the payload — the
    reference shows `data.data.docdata`, and a provider that returns
    `docdata` at the top is not wrong, just different. Finding `docdata`
    wherever it sits costs nothing and removes a per-provider adapter."""
    inner = get_payload()["data"]["data"]

    assert len(normalize(inner)) == 3
    assert len(normalize({"docdata": inner["docdata"]})) == 3


def test_a_payload_with_no_b2b_section_is_an_empty_result_not_an_error() -> None:
    """A period in which nobody filed against this taxpayer is a legitimate,
    common answer — and an empty return is exactly when a reconciliation is
    most interesting, because everything claimed is then unmatched."""
    assert normalize({"docdata": {}}) == []


def test_a_payload_that_is_not_a_gstr2b_response_at_all_fails_loudly() -> None:
    """An authentication error page or an empty body deserialized into
    "no invoices" would report a clean reconciliation on a failed fetch."""
    with pytest.raises(Gstr2bError, match="docdata"):
        normalize({"error": "unauthorized"})


def test_turtle_uses_the_vocabulary_the_pack_already_defines() -> None:
    """**The property that makes live and fixture data interchangeable.** The
    finding rules must not be able to tell which produced a subject."""
    turtle = to_turtle(normalize(get_payload()))

    assert "gst:2b-INV-1001 rdf:type gst:Gstr2bInvoice" in turtle
    assert 'gst:invoiceDate   "2026-07-04"' in turtle
    assert 'gst:itcAvailable  "Y"' in turtle
    assert 'gst:reverseCharge "N"' in turtle
    assert 'gst:period        "2026-07"' in turtle


def test_the_period_comes_from_the_declared_return_period_not_the_invoice_date() -> None:
    """A reconciliation is for a stated period; deriving it from the clock
    would silently change what a re-run means.

    **Nor from the invoice's own date.** GSTR-2B is a monthly snapshot of
    *filed* returns, not of invoice dates — an invoice dated in July can
    legitimately surface only in August's 2B, once the supplier files late.
    `supprd` (the supplier's own declared return period) is what `gst:period`
    means; deriving it from `dt` instead makes that carry-forward case, and
    the reasoning built on it, permanently untestable.
    """
    turtle = to_turtle(normalize(get_payload()))

    assert turtle.count('gst:period        "2026-07"') == 3


def test_the_declared_period_is_scoped_to_its_own_supplier() -> None:
    payload = get_payload()
    payload["data"]["data"]["docdata"]["b2b"][0]["supprd"] = "082026"  # Umbrella files for August
    # Globex (b2b[1]) keeps the factory default of "072026".

    records = normalize(payload)

    assert records[0].period == "2026-08"  # INV-1001, Umbrella
    assert records[1].period == "2026-08"  # INV-1005, Umbrella
    assert records[2].period == "2026-07"  # INV-1003, Globex


def test_a_supplier_block_with_no_declared_return_period_is_refused() -> None:
    """Silently falling back to the invoice date would reintroduce the exact
    bug this field exists to close, invisibly."""
    payload = get_payload()
    del payload["data"]["data"]["docdata"]["b2b"][0]["supprd"]

    with pytest.raises(Gstr2bError, match="'' is not a return period"):
        normalize(payload)


def test_return_period_converts_mmyyyy_to_yyyy_mm() -> None:
    assert return_period("072026") == "2026-07"


def test_return_period_accepts_month_boundaries_01_and_12() -> None:
    assert return_period("012026") == "2026-01"
    assert return_period("122026") == "2026-12"


def test_return_period_refuses_month_00_or_13() -> None:
    with pytest.raises(Gstr2bError):
        return_period("002026")
    with pytest.raises(Gstr2bError):
        return_period("132026")


def test_return_period_refuses_anything_that_is_not_six_digits() -> None:
    with pytest.raises(Gstr2bError):
        return_period("2026-07")
    with pytest.raises(Gstr2bError):
        return_period("")


def test_return_period_refuses_extra_characters_before_or_after_the_six_digits() -> None:
    with pytest.raises(Gstr2bError):
        return_period("X072026")
    with pytest.raises(Gstr2bError):
        return_period("072026X")


def test_a_quote_in_a_trade_name_cannot_break_the_document() -> None:
    """One badly-named supplier would otherwise corrupt every triple after it."""
    payload = get_payload()
    payload["data"]["data"]["docdata"]["b2b"][0]["trdnm"] = 'The "Best" Co \\ Ltd'

    turtle = to_turtle(normalize(payload))

    assert r'\"Best\"' in turtle
    assert r"\\" in turtle


# ---- Supplier as a real graph node, not a literal on the invoice ----
#
# **The gap `plans/105c-gst-causal-graph.md` names directly.** `gst:Supplier`
# was declared in the ontology and never instantiated — every invoice carried
# `gst:supplierGstin` as a bare literal, so "who issued this invoice" was
# unanswerable by a graph traversal, only by string equality. These pin the
# fix: one `gst:Supplier` subject per unique GSTIN, and each invoice points at
# it with `gst:issuedBy` — a real edge, traversable the way `onInvoice`
# already is.


def test_each_unique_supplier_becomes_its_own_subject() -> None:
    turtle = to_turtle(normalize(get_payload()))

    assert "gst:supplier-27AABCU9603R1ZM rdf:type gst:Supplier" in turtle
    assert "gst:supplier-29AACCG0527D1Z8 rdf:type gst:Supplier" in turtle
    # Two suppliers in the fixture, not one block per invoice — three
    # invoices must not produce three supplier subjects for the two that
    # share a GSTIN.
    assert turtle.count("rdf:type gst:Supplier") == 2


def test_the_supplier_subject_carries_its_own_gstin_and_name() -> None:
    turtle = to_turtle(normalize(get_payload()))
    supplier_block = turtle[turtle.index("gst:supplier-27AABCU9603R1ZM") :]

    assert 'gst:supplierGstin "27AABCU9603R1ZM"' in supplier_block
    assert 'gst:supplierName  "Umbrella Supplies"' in supplier_block


def test_a_supplier_with_no_trade_name_still_gets_a_subject() -> None:
    turtle = to_turtle(normalize(get_payload()))
    supplier_block = turtle[turtle.index("gst:supplier-29AACCG0527D1Z8") :]

    assert 'gst:supplierGstin "29AACCG0527D1Z8"' in supplier_block


def test_an_invoice_points_at_its_supplier_by_edge_not_literal() -> None:
    turtle = to_turtle(normalize(get_payload()))
    invoice_block = turtle[turtle.index("gst:2b-INV-1001") :]
    invoice_block = invoice_block[: invoice_block.index("\n\n")]

    assert "gst:issuedBy      gst:supplier-27AABCU9603R1ZM" in invoice_block
    assert "gst:supplierGstin" not in invoice_block, (
        "the GSTIN belongs to the supplier subject now, not the invoice"
    )
    assert "gst:supplierName" not in invoice_block


def test_two_invoices_from_the_same_supplier_point_at_the_same_subject() -> None:
    # INV-1001 and INV-1005 are both Umbrella Supplies in get_payload() —
    # the property that makes "which invoices did this supplier issue" a
    # traversal rather than a second join on a string.
    turtle = to_turtle(normalize(get_payload()))

    first = turtle[turtle.index("gst:2b-INV-1001") :]
    first = first[: first.index("\n\n")]
    second = turtle[turtle.index("gst:2b-INV-1005") :]
    second = second[: second.index("\n\n")]

    assert "gst:issuedBy      gst:supplier-27AABCU9603R1ZM" in first
    assert "gst:issuedBy      gst:supplier-27AABCU9603R1ZM" in second
