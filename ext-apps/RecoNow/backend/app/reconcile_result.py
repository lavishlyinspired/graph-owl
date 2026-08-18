"""The reconciliation result: four buckets, a match rate, and an ITC position.

Reco Now surfaced findings and nothing else. A finding is raised only when
something is *wrong*, so a matched invoice produces none — which means a
screen built from findings can never state how much of a period is done, and
that is the first question a CA asks.

Everything here is a pure function of the parsed rows and the findings, so it
tests without a database or a running graph-owl.
"""

from __future__ import annotations

from dataclasses import dataclass
from decimal import Decimal, InvalidOperation

from .reconciliation import normalize_invoice_no

BUCKET_MATCHED = "matched"
BUCKET_REVIEW = "review"
BUCKET_ONLY_BOOKS = "only_books"
BUCKET_ONLY_PORTAL = "only_portal"

BUCKETS = (BUCKET_MATCHED, BUCKET_REVIEW, BUCKET_ONLY_BOOKS, BUCKET_ONLY_PORTAL)

#: Findings that mean the credit is *blocked* rather than merely disputed —
#: s.17(5) and reverse charge. Everything else on a matched pair is a
#: disagreement about value, which puts the difference under review, not the
#: whole invoice.
BLOCKING_LABELS = frozenset({"gst:ITCNotAvailable", "gst:Reversed"})

_TAX_FIELDS = ("igst", "cgst", "sgst", "cess")


def _amount(value: object) -> Decimal:
    if value is None or (isinstance(value, str) and not value.strip()):
        return Decimal(0)
    try:
        return Decimal(str(value))
    except (InvalidOperation, ValueError):
        return Decimal(0)


def _tax(row: dict | None) -> Decimal:
    """The credit an invoice carries — the tax, not the taxable value. ITC is
    the tax; the taxable value is only what it was charged on."""
    if row is None:
        return Decimal(0)
    return sum((_amount(row.get(f)) for f in _TAX_FIELDS), Decimal(0))


def _key(row: dict) -> tuple[str, str]:
    """Supplier and normalised invoice number — the same key the canonical
    graph subject uses, so a bucket and a finding cannot disagree about which
    invoice they mean."""
    return (
        str(row.get("supplier_gstin") or "").strip().upper(),
        normalize_invoice_no(row.get("invoice_no")),
    )


#: Below this, the two sides are treated as agreeing. The same de-minimis
#: floor `packs/gst/queries/amount-mismatch.sparql` documents and for the same
#: reason: GSTR-3B is filed in whole rupees, so a sub-rupee difference cannot
#: change what is claimed, and a queue full of one-paisa findings is one a
#: reviewer stops reading — which buries the real ones.
DE_MINIMIS = Decimal("1")


def _differs(book_row: dict, portal_row: dict) -> bool:
    """Whether the two sides disagree on the money, observed directly.

    Both the taxable value and the tax are checked: they can agree on the
    taxable value and disagree on the tax, which is a wrong rate applied.
    """
    return (
        abs(_amount(book_row.get("taxable")) - _amount(portal_row.get("taxable"))) > DE_MINIMIS
        or abs(_tax(book_row) - _tax(portal_row)) > DE_MINIMIS
    )


@dataclass(frozen=True)
class Reconciliation:
    """One period's reconciliation. `rows` carries every invoice with the
    bucket it landed in, so the summary and the list cannot disagree — they
    are computed once, together."""

    rows: list[dict]

    @property
    def total(self) -> int:
        return len(self.rows)

    @property
    def counts(self) -> dict[str, int]:
        counts = {b: 0 for b in BUCKETS}
        for row in self.rows:
            counts[row["bucket"]] += 1
        return counts

    @property
    def match_rate(self) -> float:
        """Matched over every distinct invoice seen on either side. Zero for an
        empty period rather than a division by zero — nothing reconciled is
        not the same as everything reconciled."""
        if not self.rows:
            return 0.0
        return self.counts[BUCKET_MATCHED] / len(self.rows)


def reconcile_buckets(
    books_rows: list[dict], portal_rows: list[dict], findings: list[dict]
) -> Reconciliation:
    """Every distinct invoice across both sides, each in exactly one bucket.

    The buckets partition the period. That property is asserted in the tests
    because every headline figure is derived from it: if an invoice could land
    in two buckets, or none, the match rate and the ITC position would both be
    quietly wrong.
    """
    books = {_key(r): r for r in books_rows}
    portal = {_key(r): r for r in portal_rows}

    # Invoice numbers that identify exactly one invoice in this period. A
    # finding that omits the supplier can be attached to one of these safely;
    # anything else would be a guess about whose credit is affected.
    seen_keys = set(books) | set(portal)
    unique_by_invoice: dict[str, tuple[str, str]] = {}
    for gstin, invoice in seen_keys:
        if invoice in unique_by_invoice:
            unique_by_invoice[invoice] = ("", "")  # ambiguous — never matched
        else:
            unique_by_invoice[invoice] = (gstin, invoice)

    findings_by_key: dict[tuple[str, str], list[str]] = {}
    for finding in findings:
        label = finding.get("reason_code")
        if not label:
            continue
        key = _key(finding)
        # A finding need not carry a supplier GSTIN — `gst:ITCNotAvailable`
        # binds only the invoice number and tax amount as evidence, so its
        # cases have none. Matching strictly on (gstin, invoice) dropped those
        # labels silently, and two invoices with blocked credit were reported
        # as matched with zero blocked ITC. Fall back to the invoice number,
        # but only where it is unambiguous in the period.
        if key not in seen_keys and not key[0]:
            fallback = unique_by_invoice.get(key[1])
            if fallback and fallback[0]:
                key = fallback
        findings_by_key.setdefault(key, []).append(label)

    rows: list[dict] = []
    for key in list(books) + [k for k in portal if k not in books]:
        book_row, portal_row = books.get(key), portal.get(key)
        labels = findings_by_key.get(key, [])

        if book_row is not None and portal_row is not None:
            # A difference is observable directly; a finding explains *why*
            # there is one, it is not what makes it true. Bucketing on the
            # finding alone trusted every rule to have run and to have had the
            # data it needs — and nine of thirteen rules in this pack are
            # currently starved of input. Found live: INV-MAR-003, books
            # 54,000 against portal 42,000, reported as matched.
            bucket = BUCKET_REVIEW if (labels or _differs(book_row, portal_row)) else BUCKET_MATCHED
        elif book_row is not None:
            bucket = BUCKET_ONLY_BOOKS
        else:
            bucket = BUCKET_ONLY_PORTAL

        source = book_row if book_row is not None else portal_row
        rows.append(
            {
                "key": key,
                "bucket": bucket,
                "invoice_no": (source or {}).get("invoice_no"),
                "supplier_gstin": (source or {}).get("supplier_gstin"),
                "supplier_name": (source or {}).get("supplier_name"),
                "books_tax": _tax(book_row),
                "portal_tax": _tax(portal_row),
                "books_taxable": _amount((book_row or {}).get("taxable")),
                "portal_taxable": _amount((portal_row or {}).get("taxable")),
                "labels": labels,
                "blocked": any(lbl in BLOCKING_LABELS for lbl in labels),
            }
        )
    return Reconciliation(rows=rows)


def itc_position(result: Reconciliation) -> dict[str, Decimal]:
    """Where a period's input tax credit actually stands.

    Five classes, because collapsing them loses the distinction that matters
    most to a client:

    - **confirmed** — matched, nothing blocking it. Claim it.
    - **pending** — booked, the supplier has not filed. *Deferred, not lost*;
      claimable in a later period once they file. Reco Now called this "at
      risk", which overstates the loss and understates the follow-up.
    - **blocked** — s.17(5) or reverse charge. Lost, and no amount of chasing
      changes that.
    - **under_review** — a matched pair whose values disagree. Only the
      *difference* is in doubt, not the whole invoice.
    - **unclaimed** — on the portal, not in the books. Credit that is
      available and nobody recorded the purchase for. Not confirmed: claiming
      it without an invoice is how a notice starts.
    """
    position = {
        "confirmed": Decimal(0),
        "pending": Decimal(0),
        "blocked": Decimal(0),
        "under_review": Decimal(0),
        "unclaimed": Decimal(0),
    }

    for row in result.rows:
        if row["blocked"]:
            position["blocked"] += row["books_tax"] or row["portal_tax"]
        elif row["bucket"] == BUCKET_MATCHED:
            position["confirmed"] += row["books_tax"]
        elif row["bucket"] == BUCKET_REVIEW:
            position["under_review"] += abs(row["books_tax"] - row["portal_tax"])
        elif row["bucket"] == BUCKET_ONLY_BOOKS:
            position["pending"] += row["books_tax"]
        else:
            position["unclaimed"] += row["portal_tax"]

    position["total_considered"] = sum(position.values(), Decimal(0))
    return position
