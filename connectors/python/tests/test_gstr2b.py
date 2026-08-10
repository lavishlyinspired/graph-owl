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
    assert 'gst:supplierGstin "27AABCU9603R1ZM"' in turtle
    assert 'gst:invoiceDate   "2026-07-04"' in turtle
    assert 'gst:itcAvailable  "Y"' in turtle
    assert 'gst:reverseCharge "N"' in turtle
    assert 'gst:period        "2026-07"' in turtle


def test_the_period_comes_from_the_invoice_date_not_from_today() -> None:
    """A reconciliation is for a stated period; deriving it from the clock
    would silently change what a re-run means."""
    turtle = to_turtle(normalize(get_payload()))

    assert turtle.count('gst:period        "2026-07"') == 3


def test_a_supplier_trade_name_is_recorded_when_present() -> None:
    """Not required by any rule, and worth carrying anyway: a reviewer deciding
    a transposition needs to see who the two GSTINs claim to be."""
    turtle = to_turtle(normalize(get_payload()))

    assert 'gst:supplierName  "Umbrella Supplies"' in turtle


def test_a_quote_in_a_trade_name_cannot_break_the_document() -> None:
    """One badly-named supplier would otherwise corrupt every triple after it."""
    payload = get_payload()
    payload["data"]["data"]["docdata"]["b2b"][0]["trdnm"] = 'The "Best" Co \\ Ltd'

    turtle = to_turtle(normalize(payload))

    assert r'\"Best\"' in turtle
    assert r"\\" in turtle
