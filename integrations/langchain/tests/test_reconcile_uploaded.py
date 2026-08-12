"""`agent_service.reconcile_uploaded.compare_purchase_records` — the pure
comparison core behind the ad hoc "reconcile these two uploaded files"
agent tool (see that module's own docstring for how this differs from
the pack-backed reconciliation the console's Governance tab already
runs). Pure function, no I/O: every test hand-builds its own minimal
GSTR-2B/purchase-register fixture rather than sharing one large corpus,
so each test names exactly the one condition it proves.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

import json

from agent_service.files import store_file  # noqa: E402
from agent_service.reconcile_uploaded import (  # noqa: E402
    AMOUNT_MISMATCH_TOLERANCE,
    InvoiceFinding,
    compare_purchase_records,
    reconcile_uploaded_files,
)


def _gstr2b(*invoices: dict) -> dict:
    return {"gstin": "27AABCU9603R1ZM", "returnPeriod": "062026", "invoices": list(invoices)}


def _register(*entries: dict) -> dict:
    return {"gstin": "27AABCU9603R1ZM", "period": "June 2026", "entries": list(entries)}


def _invoice(**overrides) -> dict:
    base = {
        "invoiceNumber": "INV-1",
        "invoiceDate": "2026-06-01",
        "supplierGstin": "29AAACG1234A1Z5",
        "supplierName": "Bright Textiles Pvt Ltd",
        "taxableValue": 10000.0,
        "totalTax": 1800.0,
        "itcAvailability": "Available",
        "filingStatus": "Filed",
    }
    base.update(overrides)
    return base


def _entry(**overrides) -> dict:
    base = {
        "invoiceNumber": "INV-1",
        "invoiceDate": "2026-06-01",
        "vendorGstin": "29AAACG1234A1Z5",
        "vendorName": "Bright Textiles Pvt Ltd",
        "taxableValue": 10000.0,
        "totalTax": 1800.0,
    }
    base.update(overrides)
    return base


def test_identical_invoice_on_both_sides_is_matched():
    findings = compare_purchase_records(_gstr2b(_invoice()), _register(_entry()))

    assert findings == [
        InvoiceFinding("INV-1", "Matched", "Amounts, GSTIN and status agree on both sides.")
    ]


def test_amount_difference_exactly_at_the_tolerance_boundary_is_still_matched():
    """Boundary case for the stated ₹1 rounding tolerance — a mutant that
    changed `>` to `>=` (or the tolerance itself) would flip this."""
    findings = compare_purchase_records(
        _gstr2b(_invoice(taxableValue=10000.0)),
        _register(_entry(taxableValue=10000.0 + AMOUNT_MISMATCH_TOLERANCE)),
    )

    assert findings[0].category == "Matched"


def test_amount_difference_beyond_tolerance_is_flagged():
    findings = compare_purchase_records(
        _gstr2b(_invoice(taxableValue=10000.0)),
        _register(_entry(taxableValue=10000.0 + AMOUNT_MISMATCH_TOLERANCE + 0.01)),
    )

    assert findings[0].category == "AmountMismatch"


def test_invoice_claimed_in_register_but_never_filed_by_supplier():
    findings = compare_purchase_records(_gstr2b(), _register(_entry(invoiceNumber="INV-9")))

    assert findings == [
        InvoiceFinding(
            "INV-9",
            "MissingFromGstr2b",
            "Claimed in the purchase register; the supplier has not filed it in GSTR-2B.",
        )
    ]


def test_invoice_filed_by_supplier_but_never_booked_in_register():
    findings = compare_purchase_records(_gstr2b(_invoice(invoiceNumber="INV-9")), _register())

    assert findings == [
        InvoiceFinding(
            "INV-9",
            "MissingFromRegister",
            "Filed by the supplier in GSTR-2B; not recorded in the purchase register.",
        )
    ]


def test_a_two_digit_swap_in_the_gstin_is_flagged_as_a_transposition():
    findings = compare_purchase_records(
        _gstr2b(_invoice(supplierGstin="29AAACG1234A1Z5")),
        _register(_entry(vendorGstin="29AAACG1243A1Z5")),  # "34" -> "43"
    )

    assert findings[0].category == "GstinTransposition"


def test_a_genuinely_different_gstin_is_not_called_a_transposition():
    """The negative half of the transposition check — proves it is not
    just 'the GSTINs differ'. A wholesale different GSTIN (different
    supplier entirely) must not be mistaken for a two-character typo."""
    findings = compare_purchase_records(
        _gstr2b(_invoice(supplierGstin="29AAACG1234A1Z5")),
        _register(_entry(vendorGstin="07XYZAB5678C1Z9")),
    )

    assert findings[0].category == "SupplierMismatch"


def test_itc_not_available_is_flagged_even_when_every_amount_matches():
    findings = compare_purchase_records(
        _gstr2b(_invoice(itcAvailability="Not Available")), _register(_entry())
    )

    assert findings[0].category == "ItcNotAvailable"


def test_a_cancelled_filing_is_flagged_as_reversed_even_when_amounts_match():
    findings = compare_purchase_records(
        _gstr2b(_invoice(filingStatus="Cancelled")), _register(_entry())
    )

    assert findings[0].category == "Reversed"


def test_cancelled_status_outranks_an_amount_mismatch_on_the_same_invoice():
    """Priority-ordering test: a cancelled invoice is void regardless of
    what the amounts say, so it must report as Reversed, not
    AmountMismatch, even when both conditions are true at once."""
    findings = compare_purchase_records(
        _gstr2b(_invoice(filingStatus="Cancelled", taxableValue=10000.0)),
        _register(_entry(taxableValue=99999.0)),
    )

    assert findings[0].category == "Reversed"


def test_multiple_invoices_are_each_classified_independently():
    findings = compare_purchase_records(
        _gstr2b(
            _invoice(invoiceNumber="INV-1"),
            _invoice(invoiceNumber="INV-2", itcAvailability="Not Available"),
        ),
        _register(
            _entry(invoiceNumber="INV-1"),
            _entry(invoiceNumber="INV-2"),
        ),
    )

    categories = {f.invoice_number: f.category for f in findings}
    assert categories == {"INV-1": "Matched", "INV-2": "ItcNotAvailable"}


def test_the_agent_tool_reads_two_uploaded_files_by_id_and_summarizes():
    """The LangChain-tool wrapper around `compare_purchase_records` — the
    agent's actual entry point, given the two file IDs a `POST
    /questions` attachment note put in front of it (see `server.py`'s
    `_files_context_note`). Proves the tool reads from the shared
    `agent_service.files` store rather than taking parsed JSON
    directly, since that store is exactly what a real question's
    attachment note would name."""
    gstr2b = store_file(
        "gstr2b.json",
        "application/json",
        json.dumps(
            {
                "invoices": [
                    _invoice(),
                    _invoice(invoiceNumber="INV-2", itcAvailability="Not Available"),
                ]
            }
        ),
    )
    register = store_file(
        "register.json",
        "application/json",
        json.dumps({"entries": [_entry(), _entry(invoiceNumber="INV-2")]}),
    )

    result = json.loads(
        reconcile_uploaded_files.invoke(
            {"gstr2b_file_id": gstr2b.file_id, "purchase_register_file_id": register.file_id}
        )
    )

    assert result["totalInvoices"] == 2
    assert result["byCategory"] == {"Matched": 1, "ItcNotAvailable": 1}
    assert {f["invoiceNumber"] for f in result["findings"]} == {"INV-1", "INV-2"}


def test_the_agent_tool_reports_an_unknown_file_id_instead_of_crashing():
    """A model can hallucinate or mistype a file ID — the tool must
    return a readable error the agent can react to (e.g. by asking the
    user to re-attach), not raise and abort the whole investigation."""
    real_file = store_file("register.json", "application/json", json.dumps({"entries": []}))

    result = reconcile_uploaded_files.invoke(
        {"gstr2b_file_id": "not-a-real-id", "purchase_register_file_id": real_file.file_id}
    )

    assert "no such uploaded file" in result
