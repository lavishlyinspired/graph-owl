"""Ad hoc, session-only reconciliation between two uploaded files — a
GSTR-2B-shaped feed and a purchase-register-shaped feed attached
directly to a chat question — as distinct from the pack-backed
reconciliation the console's Governance tab already runs (`POST
/packs/{id}/reconcile`, `packs/gst/pack.toml`'s six registered finding
rules, evaluated against the graph with a law-graph-traversed,
date-sensitive tolerance and a persisted evidence graph).

**Deliberately not the same mechanism, and not trying to be.** The pack
reconciliation resolves its `AmountMismatch` tolerance from Rule
36(4)'s legally mandated cap, read from the law graph by traversal,
dated to the invoice. This tool exists for someone who just attached
two files to a chat message and wants an answer immediately, with
nothing cataloged as a pack asset first — so its tolerance is a fixed,
stated rounding allowance (`AMOUNT_MISMATCH_TOLERANCE`), not a legal
cap, and its output is never written to the graph or the findings
queue. Conversation-scoped, matching what it actually is.
"""

from __future__ import annotations

import json
from dataclasses import dataclass
from typing import Any

from langchain_core.tools import tool

# Each of CGST/SGST/IGST is independently rounded to the nearest rupee by
# the two systems producing this comparison (a GSTR-2B feed and an
# internal purchase register), so a total difference of up to ₹1 across
# an invoice's tax lines is expected rounding noise, not a genuine
# mismatch. Larger than that is flagged. This is a fixed rounding
# allowance, not the pack's own legally-derived cap (see this module's
# docstring) — stated here rather than picked silently.
AMOUNT_MISMATCH_TOLERANCE = 1.0


@dataclass(frozen=True)
class InvoiceFinding:
    invoice_number: str
    category: str
    detail: str


def _is_transposition(a: str, b: str) -> bool:
    """True if `b` is `a` with exactly two characters swapped — the
    specific, narrow typo class "transposition" names (e.g. a
    data-entry error swapping two digits of a GSTIN), not merely "these
    two strings differ". Same length and same multiset of characters
    (so no character was actually changed, only reordered), and exactly
    two positions differ (so it is one swap, not several)."""
    if len(a) != len(b) or sorted(a) != sorted(b):
        return False
    diff_positions = [i for i in range(len(a)) if a[i] != b[i]]
    return len(diff_positions) == 2


def _amounts_match(entry_2b: dict[str, Any], entry_register: dict[str, Any]) -> bool:
    for field in ("taxableValue", "totalTax"):
        a = float(entry_2b.get(field, 0))
        b = float(entry_register.get(field, 0))
        if abs(a - b) > AMOUNT_MISMATCH_TOLERANCE:
            return False
    return True


def compare_purchase_records(
    gstr2b: dict[str, Any], purchase_register: dict[str, Any]
) -> list[InvoiceFinding]:
    """Match `gstr2b["invoices"]` against `purchase_register["entries"]`
    by `invoiceNumber` and classify every invoice number seen on either
    side.

    For an invoice present on both sides, checked top to bottom: a
    cancelled GSTR-2B filing outranks every other signal (the invoice
    is void regardless of what the amounts say), then blocked ITC, then
    a GSTIN identity problem, then an amount difference — the same
    "identity and status before amount" ordering the pack's own six
    finding rules already use, applied here to ad hoc data instead of
    the graph.
    """
    by_number_2b = {inv["invoiceNumber"]: inv for inv in gstr2b.get("invoices", [])}
    by_number_register = {
        entry["invoiceNumber"]: entry for entry in purchase_register.get("entries", [])
    }
    all_numbers = sorted(set(by_number_2b) | set(by_number_register))

    findings: list[InvoiceFinding] = []
    for number in all_numbers:
        entry_2b = by_number_2b.get(number)
        entry_register = by_number_register.get(number)

        if entry_2b is None:
            findings.append(
                InvoiceFinding(
                    number,
                    "MissingFromGstr2b",
                    "Claimed in the purchase register; the supplier has not filed it in GSTR-2B.",
                )
            )
            continue
        if entry_register is None:
            findings.append(
                InvoiceFinding(
                    number,
                    "MissingFromRegister",
                    "Filed by the supplier in GSTR-2B; not recorded in the purchase register.",
                )
            )
            continue

        if entry_2b.get("filingStatus") == "Cancelled":
            findings.append(
                InvoiceFinding(
                    number, "Reversed", "The supplier has cancelled this invoice in GSTR-2B."
                )
            )
            continue
        if entry_2b.get("itcAvailability") == "Not Available":
            findings.append(
                InvoiceFinding(
                    number,
                    "ItcNotAvailable",
                    "GSTR-2B reports input tax credit as unavailable for this invoice.",
                )
            )
            continue

        gstin_2b = entry_2b.get("supplierGstin", "")
        gstin_register = entry_register.get("vendorGstin", "")
        if gstin_2b != gstin_register:
            if _is_transposition(gstin_2b, gstin_register):
                findings.append(
                    InvoiceFinding(
                        number,
                        "GstinTransposition",
                        f"GSTR-2B has {gstin_2b}; the purchase register has {gstin_register} — "
                        "two characters transposed.",
                    )
                )
            else:
                findings.append(
                    InvoiceFinding(
                        number,
                        "SupplierMismatch",
                        f"GSTR-2B has {gstin_2b}; the purchase register has {gstin_register} — "
                        "not a simple transposition.",
                    )
                )
            continue

        if not _amounts_match(entry_2b, entry_register):
            findings.append(
                InvoiceFinding(
                    number,
                    "AmountMismatch",
                    f"GSTR-2B taxable value {entry_2b.get('taxableValue')}, "
                    f"tax {entry_2b.get('totalTax')}; register taxable value "
                    f"{entry_register.get('taxableValue')}, tax {entry_register.get('totalTax')}.",
                )
            )
            continue

        findings.append(
            InvoiceFinding(number, "Matched", "Amounts, GSTIN and status agree on both sides.")
        )

    return findings


@tool
def reconcile_uploaded_files(gstr2b_file_id: str, purchase_register_file_id: str) -> str:
    """Compare an uploaded GSTR-2B-shaped file against an uploaded
    purchase-register-shaped file, matching invoices by invoiceNumber.
    Use the exact file IDs named in the question's attached-files note —
    never guess or invent an ID. Returns a JSON object: totalInvoices,
    byCategory (a count per outcome: Matched, AmountMismatch,
    MissingFromGstr2b, MissingFromRegister, GstinTransposition,
    SupplierMismatch, ItcNotAvailable, Reversed), and findings (one
    entry per invoice number with its category and a plain-language
    detail).
    """
    # Imported here, not at module scope, so importing this module
    # (e.g. from a pure-logic unit test) never requires FastAPI/uvicorn
    # to be installed — the same lazy-import discipline
    # `gst_investigation_agent.py`'s `build_chat_model` already uses for
    # its own optional dependency.
    from agent_service.files import parse_json_file

    try:
        gstr2b = parse_json_file(gstr2b_file_id)
        purchase_register = parse_json_file(purchase_register_file_id)
    except ValueError as bad_file:
        return str(bad_file)

    findings = compare_purchase_records(gstr2b, purchase_register)
    by_category: dict[str, int] = {}
    for finding in findings:
        by_category[finding.category] = by_category.get(finding.category, 0) + 1

    return json.dumps(
        {
            "totalInvoices": len(findings),
            "byCategory": by_category,
            "findings": [
                {
                    "invoiceNumber": finding.invoice_number,
                    "category": finding.category,
                    "detail": finding.detail,
                }
                for finding in findings
            ],
        }
    )
