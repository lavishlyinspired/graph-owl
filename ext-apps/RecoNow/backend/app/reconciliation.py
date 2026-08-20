"""GST reconciliation engine: matches purchase register (books) against GSTR-2B.

Implements the Section 16(2)(aa) invoice-matching model:
  - exact/fuzzy invoice match by (GSTIN, invoice number)
  - tolerance-based amount matching
  - mismatch classification into Only Books / Only GSTR-2B / Amount Discrepancy
"""

from __future__ import annotations

import re
import unicodedata
from typing import Any


STATUS_MATCHED = "matched"
STATUS_REVIEW = "review"
STATUS_ONLY_BOOKS = "only_books"
STATUS_ONLY_GSTR2B = "only_gstr2b"

REASON_EXACT = "Exact"
REASON_TOLERANCE = "Within Tolerance"
REASON_AMOUNT_DIFF = "Amount Diff"
REASON_NOT_IN_2B = "Not in GSTR-2B"
REASON_NOT_IN_BOOKS = "Not in Books"


def _strip_accents(value: str) -> str:
    return "".join(
        c for c in unicodedata.normalize("NFKD", value) if not unicodedata.combining(c)
    )


def normalize_invoice_no(value: Any) -> str:
    if value is None:
        return ""
    text = _strip_accents(str(value)).upper().strip()
    return re.sub(r"[^A-Z0-9]", "", text)


def normalize_gstin(value: Any) -> str:
    if value is None:
        return ""
    return re.sub(r"[^A-Z0-9]", "", str(value).upper())


def invoice_key(gstin: str, invoice_no: str) -> str:
    return f"{normalize_gstin(gstin)}::{normalize_invoice_no(invoice_no)}"


def to_float(value: Any) -> float:
    if value is None:
        return 0.0
    if isinstance(value, (int, float)):
        return float(value)
    cleaned = re.sub(r"[^\d.\-]", "", str(value))
    try:
        return float(cleaned) if cleaned not in ("", "-") else 0.0
    except ValueError:
        return 0.0


def record_tax(record: dict) -> float:
    return to_float(record.get("igst", 0)) + to_float(record.get("cgst", 0)) + to_float(record.get("sgst", 0)) + to_float(record.get("cess", 0))


def itc_amount(record: dict) -> float:
    return record_tax(record)


def display_tax(tax: float) -> str:
    return f"₹{tax:,.0f}"


def _find_portal_match(portal_records: list[dict], book: dict) -> dict | None:
    exact_key = invoice_key(book.get("supplier_gstin", ""), book.get("invoice_no", ""))
    for record in portal_records:
        if invoice_key(record.get("supplier_gstin", ""), record.get("invoice_no", "")) == exact_key:
            return record
    return None


def reconcile(books: list[dict], portal: list[dict], tolerance: float = 1.0) -> list[dict]:
    """Match books against GSTR-2B and annotate each book record with status.

    Returns a list of result rows, one per invoice, with book and portal views.
    """
    portal_pool = list(portal)
    results: list[dict] = []

    for book in books:
        match = _find_portal_match(portal_pool, book)
        taxable_book = to_float(book.get("taxable", 0))
        tax_book = record_tax(book)
        taxable_portal = to_float(match.get("taxable", 0)) if match else 0.0
        tax_portal = record_tax(match) if match else 0.0

        if match is None:
            status = STATUS_ONLY_BOOKS
            reason = REASON_NOT_IN_2B
            diff = None
            tax_diff = 0.0
        else:
            taxable_diff = abs(taxable_portal - taxable_book)
            tax_diff = abs(tax_portal - tax_book)
            if taxable_diff <= tolerance and tax_diff <= tolerance:
                status = STATUS_MATCHED
                reason = REASON_EXACT if (taxable_diff == 0 and tax_diff == 0) else REASON_TOLERANCE
                diff = 0.0
            else:
                status = STATUS_REVIEW
                reason = REASON_AMOUNT_DIFF
                diff = max(taxable_diff, tax_diff)

        itc = tax_book

        results.append(
            {
                "status": status,
                "reason": reason,
                "diff": diff,
                "tax_diff": round(tax_diff, 2),
                "itc": round(itc, 2),
                "book": _row_view(book, taxable_book, tax_book),
                "portal": _row_view(match, taxable_portal, tax_portal) if match else None,
            }
        )
        if match is not None:
            portal_pool.remove(match)

    for orphan in portal_pool:
        taxable_portal = to_float(orphan.get("taxable", 0))
        tax_portal = record_tax(orphan)
        results.append(
            {
                "status": STATUS_ONLY_GSTR2B,
                "reason": REASON_NOT_IN_BOOKS,
                "diff": None,
                "itc": round(tax_portal, 2),
                "book": None,
                "portal": _row_view(orphan, taxable_portal, tax_portal),
            }
        )

    return results


def _row_view(record: dict | None, taxable: float, tax: float) -> dict | None:
    if record is None:
        return None
    return {
        "gstin": record.get("supplier_gstin") or "",
        "supplier": record.get("supplier_name") or "",
        "invoice_no": record.get("invoice_no") or "",
        "voucher_no": record.get("voucher_no") or "",
        "taxable": taxable,
        "tax": tax,
        "hsn": str(record.get("hsn") or ""),
        "ims_status": str(record.get("ims_status") or ""),
    }


def match_stats(results: list[dict]) -> dict:
    total = len(results)
    counts = {s: 0 for s in (STATUS_MATCHED, STATUS_REVIEW, STATUS_ONLY_BOOKS, STATUS_ONLY_GSTR2B)}
    for row in results:
        counts[row["status"]] += 1
    matched = counts[STATUS_MATCHED]
    match_rate = round((matched / total) * 100, 1) if total else 0.0
    confirmed_itc = sum(row["itc"] for row in results if row["status"] == STATUS_MATCHED)
    at_risk_itc = sum(
        row["itc"] for row in results if row["status"] in (STATUS_REVIEW, STATUS_ONLY_BOOKS)
    )
    gross_itc = sum(
        row["itc"] for row in results if row["status"] in (STATUS_MATCHED, STATUS_REVIEW, STATUS_ONLY_GSTR2B)
    )
    return {
        "total": total,
        "matched": counts[STATUS_MATCHED],
        "review": counts[STATUS_REVIEW],
        "only_books": counts[STATUS_ONLY_BOOKS],
        "only_gstr2b": counts[STATUS_ONLY_GSTR2B],
        "match_rate": match_rate,
        "confirmed_itc": round(confirmed_itc, 2),
        "at_risk_itc": round(at_risk_itc, 2),
        "gross_itc": round(gross_itc, 2),
    }


def classify_mismatches(results: list[dict]) -> list[dict]:
    """Group at-risk rows into business classifications with legal references."""
    classifications = [
        {
            "key": "supplier_non_filing",
            "title": "Supplier Non-Filing",
            "reference": "Section 16(2)(aa), CGST Act",
            "action": "Send follow-up to supplier. Reverse ITC if unfiled by deadline.",
            "rows": [],
        },
        {
            "key": "amount_discrepancy",
            "title": "Amount Discrepancy",
            "reference": "Section 34, CGST Act",
            "action": "Verify with supplier. Adjust in GSTR-3B.",
            "rows": [],
        },
        {
            "key": "only_in_portal",
            "title": "Only in Portal",
            "reference": "Section 16(2)(aa), CGST Act",
            "action": "Verify if invoice was missed in books.",
            "rows": [],
        },
    ]
    for row in results:
        if row["status"] == STATUS_ONLY_BOOKS:
            classifications[0]["rows"].append(row)
        elif row["status"] == STATUS_REVIEW:
            classifications[1]["rows"].append(row)
        elif row["status"] == STATUS_ONLY_GSTR2B:
            classifications[2]["rows"].append(row)

    output = []
    for item in classifications:
        if item["key"] == "amount_discrepancy":
            itc = round(sum(r["tax_diff"] for r in item["rows"]), 2)
        else:
            # Only-in-portal carries its real tax: credit available on the
            # portal that nobody booked is unclaimed, not absent — reporting
            # it as zero hid the one bucket a reviewer can act on alone.
            itc = round(sum(r["itc"] for r in item["rows"]), 2)
        output.append(
            {
                "key": item["key"],
                "title": item["title"],
                "reference": item["reference"],
                "action": item["action"],
                "count": len(item["rows"]),
                "itc": itc,
                "rows": [
                    {
                        "supplier": (r["book"] or r["portal"])["supplier"],
                        "gstin": (r["book"] or r["portal"])["gstin"],
                        "invoice_no": (r["book"] or r["portal"])["invoice_no"],
                        "itc": r["tax_diff"] if item["key"] == "amount_discrepancy" else r["itc"],
                    }
                    for r in item["rows"]
                ],
            }
        )
    return output


def supplier_health(results: list[dict]) -> list[dict]:
    """A rollup of what this period showed per supplier — nothing more.

    This used to attach `risk: "Chronic Non-Filer"` to every supplier with
    any at-risk row and a blank `filing_6mo` beside it. That is a filing
    history this function never looked at, and one disputed invoice became a
    permanent-sounding judgement about a third party, printed on the
    dashboard and in exports. Cross-period recurrence — the only honest basis
    for such a label — is computed where the data for it exists:
    `capabilities.supplier_pattern`."""
    by_supplier: dict[str, dict] = {}
    for row in results:
        if row["status"] not in (STATUS_ONLY_BOOKS, STATUS_REVIEW):
            continue
        view = row["book"] or row["portal"]
        key = view["gstin"]
        entry = by_supplier.setdefault(
            key,
            {"gstin": view["gstin"], "supplier": view["supplier"], "itc": 0.0,
             "at_risk_invoices": 0},
        )
        value = row["tax_diff"] if row["status"] == STATUS_REVIEW else row["itc"]
        entry["itc"] = round(entry["itc"] + value, 2)
        entry["at_risk_invoices"] += 1
    return [
        {
            "gstin": entry["gstin"],
            "supplier": entry["supplier"],
            "at_risk_invoices": entry["at_risk_invoices"],
            "itc": entry["itc"],
        }
        for entry in by_supplier.values()
    ]


def ims_actions(results: list[dict]) -> list[dict]:
    accepted = [r for r in results if r["status"] == STATUS_MATCHED]
    follow_up = [r for r in results if r["status"] == STATUS_ONLY_BOOKS]
    investigate = [r for r in results if r["status"] == STATUS_ONLY_GSTR2B]
    return [
        {
            "key": "accept_ims",
            "title": "Accept in IMS",
            "count": len(accepted),
            "itc": round(sum(r["itc"] for r in accepted), 2),
            "action": "Matched invoices — accept in IMS (or leave as deemed accepted). ITC will flow to GSTR-2B.",
            "note": "→ Accept or leave as \"No Action\" (deemed accepted)",
            "invoices": [r["book"]["invoice_no"] for r in accepted],
        },
        {
            "key": "follow_up",
            "title": "Follow Up with Supplier",
            "count": len(follow_up),
            "itc": round(sum(r["itc"] for r in follow_up), 2),
            "action": "In your books but NOT in GSTR-2B — supplier hasn't filed. Cannot act in IMS until supplier files GSTR-1.",
            "note": "→ Send follow-up to supplier. ITC cannot be claimed until it appears in GSTR-2B.",
            "invoices": [r["book"]["invoice_no"] for r in follow_up],
        },
        {
            "key": "investigate",
            "title": "Investigate — Not in Books",
            "count": len(investigate),
            "itc": round(sum(r["itc"] for r in investigate), 2),
            "action": "In GSTR-2B but NOT in your books. Either book the invoice in Tally, or reject in IMS if it's not your purchase.",
            "note": "→ If valid purchase: book in Tally and accept. If wrong: reject in IMS.",
            "invoices": [r["portal"]["invoice_no"] for r in investigate],
        },
    ]
