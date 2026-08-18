"""What is wrong with an uploaded file, said when the file lands.

A payment row with no date is skipped by the ingestion — correctly, because an
event with no time cannot answer "how many days apart", and treating it as
never-paid would manufacture an ITC reversal the client does not owe. But it
was skipped *quietly*: the row simply never became a fact, and nothing told
the person who uploaded the file that seven of their eight payments would not
be counted.

That is the same failure the three-state rule outcome exists to prevent, one
layer earlier. Silence about data that was discarded reads exactly like data
that was fine.

**Everything here is a warning, never a rejection.** A file with problems is
still the best information available, and refusing it leaves a reviewer with
nothing. The job is to make the cost visible, not to gatekeep.
"""

from __future__ import annotations

import re
from collections import Counter
from datetime import datetime
from decimal import Decimal, InvalidOperation
from typing import Any

from .reconciliation import normalize_invoice_no

#: A GSTIN is 15 characters: 2 state code, 10 PAN, 1 entity, 1 'Z', 1 checksum.
#: Checked for shape only — a checksum check belongs with the authority, and a
#: wrong-but-well-formed GSTIN is a reconciliation finding, not a file problem.
_GSTIN = re.compile(r"^[0-9]{2}[A-Z]{5}[0-9]{4}[A-Z][0-9A-Z][Z][0-9A-Z]$")

_DATE_FORMATS = ("%d-%m-%Y", "%Y-%m-%d", "%d/%m/%Y", "%Y/%m/%d")

#: field -> (issue code, what its absence costs, severity)
_REQUIRED_BY_KIND: dict[str, tuple[tuple[str, str, str, str], ...]] = {
    "payments": (
        ("payment_date", "missing_payment_date",
         "Rule 37 needs a payment date to measure 180 days from the invoice. "
         "These rows are not counted as payments, and the invoices they name "
         "will look unpaid.", "blocking"),
    ),
    "grn": (
        ("receipt_date", "missing_receipt_date",
         "s.16(2)(b) needs a receipt date to tell whether the goods arrived "
         "before the credit was claimed. These rows are not counted.",
         "blocking"),
    ),
}

#: Every invoice-bearing file needs these to reconcile at all.
_REQUIRED_ALWAYS: tuple[tuple[str, str, str, str], ...] = (
    ("invoice_no", "missing_invoice_no",
     "A row with no invoice number cannot be matched to the other side.",
     "blocking"),
    ("supplier_gstin", "missing_supplier_gstin",
     "Without a GSTIN a row cannot be told apart from another supplier's "
     "invoice of the same number.", "blocking"),
)

_AMOUNT_FIELDS = ("taxable", "igst", "cgst", "sgst", "cess")


def _present(value: object) -> bool:
    if value is None:
        return False
    text = str(value).strip()
    return text != "" and text.lower() != "nan"


def _parses_as_date(value: object) -> bool:
    text = str(value).strip()
    return any(_try(text, fmt) for fmt in _DATE_FORMATS)


def _try(text: str, fmt: str) -> bool:
    try:
        datetime.strptime(text, fmt)
        return True
    except ValueError:
        return False


def _issue(code: str, detail: str, severity: str, rows: int, example_row: int) -> dict[str, Any]:
    return {
        "code": code,
        "detail": detail,
        "severity": severity,
        "rows": rows,
        # 1-based, so it matches what a spreadsheet shows the user.
        "example_row": example_row,
    }


def inspect_rows(rows: list[dict], kind: str) -> list[dict[str, Any]]:
    """Problems in one uploaded file, worst first.

    Grouped by kind of problem rather than listed per row: a reviewer needs
    "31 rows have no payment date, e.g. row 4", not thirty-one identical
    lines.
    """
    if not rows:
        return [
            _issue("empty_file", "This file has no rows.", "blocking", 0, 0)
        ]

    counts: Counter[str] = Counter()
    first_row: dict[str, int] = {}
    details: dict[str, tuple[str, str]] = {}

    def note(code: str, detail: str, severity: str, index: int) -> None:
        counts[code] += 1
        details[code] = (detail, severity)
        first_row.setdefault(code, index + 1)

    required = _REQUIRED_ALWAYS + _REQUIRED_BY_KIND.get(kind, ())
    date_fields = [f for f in ("invoice_date", "payment_date", "receipt_date") if f in (rows[0] or {})]

    seen_invoices: Counter[tuple[str, str]] = Counter()

    for index, row in enumerate(rows):
        for field, code, detail, severity in required:
            if not _present(row.get(field)):
                note(code, detail, severity, index)

        for field in date_fields:
            value = row.get(field)
            # A missing date is already reported above where it is required;
            # "supplied but unreadable" is a different problem with a different
            # fix — a mapping or an export format, not a gap in the source.
            if _present(value) and not _parses_as_date(value):
                note(
                    "unparseable_date",
                    f"A date could not be read (for example {value!r} in "
                    f"{field}). Expected day-month-year.",
                    "blocking",
                    index,
                )

        gstin = str(row.get("supplier_gstin") or "").strip().upper()
        if gstin and not _GSTIN.match(gstin):
            note(
                "malformed_gstin",
                f"A GSTIN is not the expected 15-character shape (for example "
                f"{gstin!r}). It will never match a portal row, so the invoice "
                f"will read as unfiled.",
                "warning",
                index,
            )

        for field in _AMOUNT_FIELDS:
            value = row.get(field)
            if _present(value):
                try:
                    Decimal(str(value))
                except (InvalidOperation, ValueError):
                    note(
                        "non_numeric_amount",
                        f"An amount could not be read as a number (for example "
                        f"{value!r} in {field}), and is treated as zero.",
                        "blocking",
                        index,
                    )

        if _present(row.get("invoice_no")):
            key = (gstin, normalize_invoice_no(row.get("invoice_no")))
            seen_invoices[key] += 1
            if seen_invoices[key] == 2:
                note(
                    "duplicate_invoice",
                    "The same supplier and invoice number appears more than "
                    "once. Rate lines of one invoice are summed, which is "
                    "right for a multi-rate invoice and wrong for a double "
                    "entry — only the source can say which.",
                    "warning",
                    index,
                )

    order = {"blocking": 0, "warning": 1}
    return sorted(
        (
            _issue(code, details[code][0], details[code][1], count, first_row[code])
            for code, count in counts.items()
        ),
        key=lambda i: (order[i["severity"]], -i["rows"]),
    )
