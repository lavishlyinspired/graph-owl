"""reco-now's own client for graph-owl's canonical GST pack.

plans/118-reco-now-integration.md, Slice 1; corrected twice per
plans/119-architecture-audit.md. First correction (§3.1): stop
re-declaring predicates `packs/gst` already has under an unrelated
namespace. Second, larger correction (16 August 2026, this version): stop
maintaining a *second* pack at all. reco-now had its own
`graphowl-pack/pack.toml` registering 6 fields (`hsnCode`, `imsStatus`,
`noteType`, `originalInvoiceNumber`, `voucherType`, `voucherNumber`)
`packs/gst` didn't carry — but none of the six turned out to be specific
to how reco-now's own CSVs happen to be shaped; four are genuine
GST-document fields (HSN code, IMS status, note type, an original invoice
reference) and the other two are common bookkeeping vocabulary, not
reco-now's invention. Merged into `packs/gst/ontology.ttl` and
`pack.toml` directly, so there is exactly one GST pack, one file to keep
in sync as the domain grows, and this module now ingests entirely under
`packs/gst`'s own namespace — no `reco:` predicates, no second pack
registration, no second `POST /packs/{id}/reconcile` call.

stdlib `urllib` only, matching this repo's own convention for every
graph-owl Python client (`connectors/python/graph_owl_packs/loader.py`'s
own words: "a loader is not a place to acquire an HTTP dependency").

Two responsibilities, kept separate because one is pure and one is not:

- `rows_to_turtle` — normalized rows in, one RDF subject per row out.
  No I/O, so every case in it is a fast unit test.
- `import_document` — one `POST /graph/import/rdf` call, landing a
  Turtle document under a caller-named source. Installing `packs/gst`'s
  vocabulary (namespace + predicates, no demo fixtures — `main.py`'s
  `_install_graphowl_pack`) is a one-time step at backend startup using
  the already-shipped, already-tested `graph_owl_packs.loader.load_pack`
  — this module does not reimplement that.
"""

from __future__ import annotations

import json
import math
import urllib.error
import urllib.request
from datetime import datetime
from decimal import Decimal, InvalidOperation
from urllib.parse import quote

from graph_owl_packs.gst_identity import canonical_local_name, supplier_local_name, turtle_literal

from .reconciliation import normalize_invoice_no

#: The one pack this whole module speaks — graph-owl's own canonical GST
#: pack. Must match `packs/gst/pack.toml`'s `[pack]` table exactly.
PACK_ID = "gst"
NAMESPACE = "https://graph-owl.dev/packs/gst#"

#: reco-now's dataset kinds (main.py's `kind` values) -> the `packs/gst`
#: class each becomes — not reco-now's own classes: the ontology already
#: draws these distinctions ("Purchase invoice (taxpayer's register)" /
#: "GSTR-2B invoice (as filed by the supplier)"). `gstr1` also covers a
#: GSTR-2A upload: `packs/gst/ontology.ttl`'s own comment is explicit that
#: **`gstr2a` reverses an earlier decision here, deliberately — Plan 123
#: Slice C.** This comment used to read "there is deliberately no separate
#: `Gstr2aInvoice` class — 2A is a revolving view over the same
#: supplier-declared data `gst:Gstr1Invoice` already carries". That is true of
#: the *content* and misses why 2A is held at all: a revolving view has a
#: **pull date**, and a filing does not. GSTR-2B is frozen on the 14th and is
#: what a claim rests on; 2A keeps moving after that, and the only reason to
#: keep it is to answer "what has the portal said since the 2B I claimed
#: against". A class carrying no observation time cannot express that.
#:
#: The second thing collapsing them lost: GSTR-1 is what the **supplier** says
#: they filed, 2A is what the **portal** shows. A reviewer chasing a late
#: filing needs to know which one is speaking.
CLASS_BY_KIND = {
    "books": "gst:PurchaseInvoice",
    # Event kinds. Optional uploads: a firm that cannot export a payment
    # ledger still gets every other check, and `checks_disabled` names the
    # rules the absence switches off rather than reporting a clean result the
    # data never earned.
    "payments": "gst:PaymentEvent",
    "grn": "gst:GoodsReceipt",
    "portal": "gst:Gstr2bInvoice",
    "gstr2b": "gst:Gstr2bInvoice",
    "gstr1": "gst:Gstr1Invoice",
    "gstr2a": "gst:Gstr2aInvoice",
    "gstr3b": "gst:Gstr3bReturn",
}

#: The canonical-subject edge each kind's upload asserts — the half it
#: can, from its own named graph. The other kinds' edges meet it there
#: when their own uploads land, never in the same call.
LINK_PREDICATE_BY_KIND = {
    "books": "gst:recordedIn",
    "payments": "gst:recordedIn",
    "grn": "gst:recordedIn",
    "portal": "gst:reflectedIn",
    "gstr2b": "gst:reflectedIn",
    "gstr1": "gst:appearsIn",
    # "Observed", not "reflected": 2B *reflects* a frozen position, 2A only
    # records what the portal showed at one moment.
    "gstr2a": "gst:observedIn",
}

#: Kinds whose invoice subject needs `gst:invoiceKey`. `books` and
#: `gstr1` for the any-GSTIN keying-error guards
#: (`missing-in-gstr1.sparql`/`missing-in-books.sparql`); `gstr2b` too —
#: `missing-in-gstr1.sparql` reads it off the Gstr2bInvoice side as
#: conclusive proof the supplier filed (`?availableIn2b a
#: gst:Gstr2bInvoice ; gst:invoiceKey ?key`), and its own comment records
#: what omitting it does: "every 2B-matched invoice with no [GSTR-1/2A]
#: row was reported as one the supplier had never filed" — found live,
#: 16 August 2026, the moment GSTR-1 data first existed in the store for
#: this rule to actually run against (verify-reconcile-parity.py's
#: SupplierNotFiled count came back 10 against an expected 2).
KINDS_NEEDING_INVOICE_KEY = {"books", "gstr1", "gstr2b", "portal", "gstr2a"}

#: Kinds whose invoice subject needs a combined `gst:taxAmount` —
#: `missing-in-gstr2b.sparql` reads it off the books side,
#: `missing-in-books.sparql` off the gstr1 side. Never gstr2b.
#: `gstr2a` is here because every Slice C rule compares a 2A tax amount
#: against the 2B one — an amendment that changes the value is the finding.
KINDS_NEEDING_TAX_AMOUNT = {"books", "gstr1", "gstr2a"}

#: An event kind is not a document: it has no taxable value or tax heads, only
#: a time and the invoice it happened to. `gst:onInvoice` points at the *books*
#: invoice subject, which is what `payment-overdue.sparql` and
#: `goods-receipt-timing.sparql` join on — not at the canonical subject.
EVENT_KINDS = {"payments", "grn"}

#: **A summary return is not a document either, and is unlike an event too.**
#: Every other kind here is line-level — one row per invoice, keyed by invoice
#: number and supplier GSTIN. GSTR-3B has neither: it is one figure per Table 4
#: row, per period. It gets its own path for the same reason events do, and the
#: consequence is stated in `rows_to_turtle`: netting and rate-line aggregation
#: are meaningless on a figure that is already a total.
SUMMARY_KINDS = {"gstr3b"}

#: Row field -> `gst:` predicate, for the summary kinds. Named for the rows a
#: preparer actually sees on the return rather than flattened to one "itc"
#: figure — the working paper's gross -> reversals -> net chain is the thing
#: being built, and a single total makes every step of it untraceable.
#:
#: Table 4's own structure (current format, August 2022 onward):
#:   4A     gross ITC, auto-populated from GSTR-2B — eligible and ineligible
#:   4B(1)  permanent reversals: Rule 38, Rules 42/43, s.17(5). Not reclaimable.
#:   4B(2)  temporary reversals: Rule 37, s.16(2)(b)/(c). Reclaimable later.
#:   4C     net ITC to the credit ledger, 4A - 4B
#:   4D(1)  ITC reclaimed that an earlier period reversed
#:   4D(2)  ITC unavailable by law — s.16(4) time bar, place of supply
SUMMARY_PREDICATES: dict[str, str] = {
    "itc_4a": "itcAvailable4A",
    "itc_reversed_4b1": "itcReversed4B1",
    "itc_reversed_4b2": "itcReversed4B2",
    "itc_net_4c": "itcNet4C",
    "itc_reclaimed_4d1": "itcReclaimed4D1",
    "itc_unavailable_4d2": "itcUnavailable4D2",
}
EVENT_DATE_FIELD = {"payments": "payment_date", "grn": "receipt_date"}

#: Row field (main.py's FIELD_LABELS keys) -> `gst:` predicate local
#: name. Matches `packs/gst/pack.toml`'s `[[predicates]]` and
#: `ontology.ttl` exactly, as one table, so they cannot drift silently
#: against each other. `supplier_gstin` and `supplier_name` are
#: deliberately not here — `packs/gst`'s own finding queries read
#: `supplierGstin` off the *Supplier* subject (`?supplier
#: gst:supplierGstin ?gstin`), reached via `gst:issuedBy`, never as a
#: direct literal on the invoice, and `[console.labels]`
#: (`packs/gst/pack.toml`, Plan 120 Slice C / Plan 121) plus
#: `ReconciliationWorkspace.tsx`'s own party-name query read
#: `supplierName` the identical way. Both `rows_to_turtle` asserts once,
#: on the Supplier subject — this was found the hard way, 16 August
#: 2026: `supplier_name` sat in this table until then, so a real
#: Supplier subject's evidence-graph node resolved its class correctly
#: but never its name, because the literal was on the invoice instead.
PREDICATES: dict[str, str] = {
    "invoice_no": "invoiceNumber",
    "taxable": "taxableValue",
    "invoice_date": "invoiceDate",
    "place_of_supply": "placeOfSupply",
    "hsn": "hsnCode",
    "ims_status": "imsStatus",
    "reverse_charge": "reverseCharge",
    "note_type": "noteType",
    "voucher_type": "voucherType",
    "original_invoice_no": "originalInvoiceNumber",
    "voucher_no": "voucherNumber",
    "igst": "igst",
    "cgst": "cgst",
    "sgst": "sgst",
    "cess": "cess",
}


class IngestError(RuntimeError):
    """A row set or a document could not be landed in graph-owl."""


def _is_present(value: object) -> bool:
    """False for "nothing was recorded"; true for every real value,
    including a numeric zero — `cgst=0` is a fact (no central tax on an
    inter-state supply), not an absence."""
    if value is None:
        return False
    if isinstance(value, float) and math.isnan(value):
        return False
    if isinstance(value, str) and value.strip() == "":
        return False
    return True


def _normalize_date(value: object) -> object:
    """DD-MM-YYYY (reco-now's own CSV/UI format, e.g. "07-08-2026") to ISO
    8601 (`2026-08-07`).

    **Load-bearing, not cosmetic.** `amount-mismatch.sparql` picks the law
    provision in force with `FILTER (?from <= ?date)` — a plain string
    comparison, no `xsd:date` cast. `gst:effectiveFrom` is always ISO
    (`packs/gst/law/rule-36-4.ttl`); a DD-MM-YYYY `?date` sorts against it
    character-by-character ('0' < '2'), so every provision fails the
    filter and the finding silently never fires — found by running the
    query against real data, not by reading it. An unparseable date is
    passed through unchanged: a data-quality problem to surface, not one
    for this function to swallow or crash on."""
    try:
        return datetime.strptime(str(value), "%d-%m-%Y").strftime("%Y-%m-%d")
    except ValueError:
        return value


def _turtle_string(value: object) -> str:
    """A quoted Turtle string literal for `value`. The escaping itself is
    `graph_owl_packs.gst_identity.turtle_literal`, shared with
    `gstr2b.py` — this wrapper only adds the surrounding quotes every
    call site here expects."""
    return f'"{turtle_literal(value)}"'


def _subject_iri(kind: str, row: dict) -> str:
    """One subject per (kind, supplier, invoice number) — not per invoice
    number alone. Two suppliers can and do reuse the same invoice number
    text; an exact-string subject key would silently merge their rows
    into one graph subject.

    **Minted under `packs/gst`'s own namespace**, alongside its own
    fixture subjects (`gst:pr-INV-1001` etc.) but never colliding with
    them — this module's local names always start `books-`/`gstr2b-`/
    `supplier-`/`invoice-`, `packs/gst`'s own fixtures never do. Reco-now's
    deployment never loads those fixtures anyway (`include_documents=False`,
    `main.py`), so the only reason this matters is defence in depth, not
    day-to-day correctness."""
    gstin = quote(str(row.get("supplier_gstin") or "").strip(), safe="")
    invoice_no = quote(str(row.get("invoice_no") or "").strip(), safe="")
    return f"{NAMESPACE}{kind}-{gstin}-{invoice_no}"


def _supplier_iri(gstin_raw: str) -> str:
    """Shared with `gstr2b.py`'s own `Gstr2bInvoice.supplier_subject` via
    `graph_owl_packs.gst_identity.supplier_local_name` — a books upload
    and a live GSTR-2B pull for the same GSTIN must resolve to the same
    `gst:Supplier` subject."""
    return f"{NAMESPACE}{supplier_local_name(gstin_raw)}"


def _filing_iri(gstin_raw: str, period_raw: str) -> str:
    """One `gst:Gstr1Filing` subject per (supplier, period) — `gstr1-not-
    in-2b.sparql` reads `filedDate`/`period` off it, reached via
    `gst:filedIn` from each declared invoice. Two invoice rows from the
    same supplier and period share this IRI, so they share one Filing
    subject rather than minting a duplicate per invoice line."""
    gstin = quote(str(gstin_raw or "").strip(), safe="")
    period = quote(str(period_raw or "").strip(), safe="")
    return f"{NAMESPACE}filing-{gstin}-{period}"


def _snapshot_iri(period_raw: object, pulled_on_raw: object) -> str:
    """One `gst:Gstr2aSnapshot` per (period, pull date) — Plan 123 Slice C.

    **Keyed by the pull date as well as the period, which is the whole
    point.** A snapshot keyed by period alone would let May's pull overwrite
    April's, destroying exactly the history drift is computed from: two
    observations of one period, taken at different times, are two facts and
    not one fact revised.

    A pull with no date still gets a snapshot — a firm whose export carries no
    pull-date column loses drift, not the invoice.
    """
    period = quote(str(period_raw).strip(), safe="") if _is_present(period_raw) else "unknown-period"
    pulled = quote(str(pulled_on_raw).strip(), safe="") if _is_present(pulled_on_raw) else "undated"
    return f"{NAMESPACE}gstr2a-snapshot-{period}-{pulled}"


def _canonical_iri(gstin_raw: str, invoice_no_raw: str) -> str:
    """One subject per real invoice, **kind-independent** — a books upload
    and a gstr2b upload for the same (gstin, invoice number) must mint the
    identical IRI, or `gst:recordedIn` and `gst:reflectedIn` would land on
    two different subjects instead of meeting on one, and every finding
    query that joins through `?canonical` would silently match nothing.

    Delegates to `graph_owl_packs.gst_identity.canonical_local_name` —
    shared with `gstr2b.py`'s own `invoice_subject` since 16 August 2026.
    Fixed at the same time: this used to build the IRI from the *raw*
    invoice number, where `gstr2b.py` already normalized it first — a
    books row for "INV-2024/001" and a live 2B pull for "inv2024001"
    would have computed two different canonical subjects, silently, the
    moment both wrote to the same store. Normalizing here closes that."""
    return f"{NAMESPACE}{canonical_local_name(gstin_raw, invoice_no_raw)}"


def _combined_tax_amount(row: dict) -> str:
    """igst + cgst + sgst + cess, absent/blank/NaN components treated as
    zero. Read by `missing-in-gstr2b.sparql` (books side) and
    `missing-in-books.sparql` (gstr1 side) — see
    `KINDS_NEEDING_TAX_AMOUNT`."""
    total = Decimal("0")
    for field in ("igst", "cgst", "sgst", "cess"):
        value = row.get(field)
        if not _is_present(value):
            continue
        try:
            total += Decimal(str(value))
        except InvalidOperation:
            continue
    return str(total)


def rows_to_turtle(rows: list[dict], kind: str) -> str:
    """Normalized rows (main.py's `_normalize` output) as one Turtle
    document, ready for `import_document`.

    A field with no value is **omitted, never written as an empty
    literal** — the same "absent vs. recorded blank" distinction
    `graph_owl_packs/erpnext.py` already draws, for the same reason: a
    reconciliation asking "was this recorded" needs the two states to
    stay distinguishable.

    # Raises

    `ValueError` if `kind` is not one of `CLASS_BY_KIND`.
    """
    if kind not in CLASS_BY_KIND:
        raise ValueError(f"unknown kind {kind!r} — expected one of {sorted(CLASS_BY_KIND)}")
    if not rows:
        return ""

    # A GST return is line-structured; a reconciliation is invoice-level.
    # Credit and debit notes are applied first (a netted invoice is what the
    # portal actually reports), then a document's rate lines are summed —
    # otherwise three rate lines land on one subject and every finding query
    # compares an invoice total against a single line. See
    # `net_credit_notes` / `aggregate_invoice_lines`.
    # An event kind carries no money, so aggregating and netting it would be
    # meaningless — and summing two payments on one invoice would destroy the
    # very thing the 180-day test reads, which is *when* each happened.
    # A summary kind is already a total; netting and rate-line aggregation
    # would be applied to a figure that has had both done to it already, by
    # the person who filed the return.
    if kind not in EVENT_KINDS and kind not in SUMMARY_KINDS:
        rows = net_credit_notes(aggregate_invoice_lines(rows))

    lines = [f"@prefix gst: <{NAMESPACE}> .", ""]

    if kind in SUMMARY_KINDS:
        return _summary_to_turtle(rows, kind, lines)
    if kind in EVENT_KINDS:
        return _events_to_turtle(rows, kind, lines)
    seen_filings: set[str] = set()
    for row in rows:
        subject = _subject_iri(kind, row)
        gstin_raw = row.get("supplier_gstin")
        supplier = _supplier_iri(gstin_raw)
        canonical = _canonical_iri(gstin_raw, row.get("invoice_no"))

        # The Supplier subject — packs/gst's finding queries read the GSTIN
        # off it (`?supplier gst:supplierGstin ?gstin`), not off the
        # invoice directly, and [console.labels]/ReconciliationWorkspace.tsx
        # read the display name the same way. `supplier_name` is optional
        # (a row missing it is still a real supplier, just an unnamed one)
        # — omitted rather than written as a blank literal, matching every
        # other optional field's own rule.
        supplier_triples = ["a gst:Supplier", f"gst:supplierGstin {_turtle_string(gstin_raw)}"]
        supplier_name_raw = row.get("supplier_name")
        if _is_present(supplier_name_raw):
            supplier_triples.append(f"gst:supplierName {_turtle_string(supplier_name_raw)}")
        lines.append(f"<{supplier}>\n    " + " ;\n    ".join(supplier_triples) + " .\n")

        # The canonical link — one triple, the half this (books, gstr2b
        # or gstr1) upload can assert. The other half(s) land when the
        # other side is uploaded, in its own named graph; all the edges
        # meeting on the same `canonical` subject is what makes it
        # canonical.
        lines.append(f"<{canonical}>\n    {LINK_PREDICATE_BY_KIND[kind]} <{subject}> .\n")

        # The Filing subject — only a gstr1 (GSTR-1/GSTR-2A) row has one.
        # gstr1-not-in-2b.sparql reaches filedDate/period through it via
        # gst:filedIn.
        if kind == "gstr1":
            period_raw = row.get("period")
            filing = _filing_iri(gstin_raw, period_raw)
            if filing not in seen_filings:
                seen_filings.add(filing)
                filing_triples = ["a gst:Gstr1Filing"]
                if _is_present(period_raw):
                    filing_triples.append(f"gst:period {_turtle_string(period_raw)}")
                filed_date_raw = row.get("filed_date")
                if _is_present(filed_date_raw):
                    filing_triples.append(
                        f"gst:filedDate {_turtle_string(_normalize_date(filed_date_raw))}"
                    )
                lines.append(f"<{filing}>\n    " + " ;\n    ".join(filing_triples) + " .\n")

        triples = [f"a {CLASS_BY_KIND[kind]}", f"gst:issuedBy <{supplier}>"]
        if kind in KINDS_NEEDING_INVOICE_KEY and _is_present(row.get("invoice_no")):
            triples.append(f"gst:invoiceKey {_turtle_string(normalize_invoice_no(row.get('invoice_no')))}")
        if kind == "gstr1":
            triples.append(f"gst:filedIn <{filing}>")
        for field, predicate in PREDICATES.items():
            value = row.get(field)
            if not _is_present(value):
                continue
            if field == "invoice_date":
                value = _normalize_date(value)
            triples.append(f"gst:{predicate} {_turtle_string(value)}")
        if kind in KINDS_NEEDING_TAX_AMOUNT:
            triples.append(f"gst:taxAmount {_turtle_string(_combined_tax_amount(row))}")

        # The 2B's own ITC-eligibility flag. `itc-not-available.sparql` filters
        # `?itcAvailable != "Y"`; with the predicate absent the rule matches
        # nothing and every credit the portal has blocked reads as claimable.
        if kind in ("gstr2b", "portal") and _is_present(row.get("itc_available")):
            triples.append(f"gst:itcAvailable {_turtle_string(row.get('itc_available'))}")

        # The statement this 2B line belongs to. `goods-receipt-timing.sparql`
        # reaches the period through `?filed gst:reflectedIn ?statement`, so
        # without it the receipt date has nothing to be compared against.
        if kind in ("gstr2b", "portal") and _is_present(row.get("period")):
            statement = f"{NAMESPACE}gstr2b-statement-{quote(str(row.get('period')).strip(), safe='')}"
            if statement not in seen_filings:
                seen_filings.add(statement)
                lines.append(
                    f"<{statement}>\n    a gst:Gstr2bStatement ;\n"
                    f"    gst:period {_turtle_string(row.get('period'))} .\n"
                )
            triples.append(f"gst:reflectedIn <{statement}>")

        # The 2A snapshot this line was observed in — Plan 123 Slice C. The
        # date is on the *snapshot*, not the invoice: one pull is one
        # observation however many lines it carried, and putting the date on
        # each line would report drift between two invoices seen at the same
        # moment.
        if kind == "gstr2a":
            snapshot = _snapshot_iri(row.get("period"), row.get("pulled_on"))
            if snapshot not in seen_filings:
                seen_filings.add(snapshot)
                snapshot_triples = ["a gst:Gstr2aSnapshot"]
                if _is_present(row.get("period")):
                    snapshot_triples.append(f"gst:period {_turtle_string(row.get('period'))}")
                if _is_present(row.get("pulled_on")):
                    snapshot_triples.append(
                        f"gst:pulledOn {_turtle_string(_normalize_date(row.get('pulled_on')))}"
                    )
                lines.append(f"<{snapshot}>\n    " + " ;\n    ".join(snapshot_triples) + " .\n")
            # `seenIn`, not `observedIn`: `observedIn` is the canonical
            # invoice's edge to this 2A line, `seenIn` is this line's edge to
            # the pull it was read in. Following `appearsIn`/`filedIn`'s split
            # rather than `reflectedIn`'s reuse — the drift rules traverse
            # invoice -> snapshot -> pulledOn, and one verb per level means a
            # reader of the SPARQL never has to work out which level they are
            # standing on.
            triples.append(f"gst:seenIn <{snapshot}>")

        body = " ;\n    ".join(triples)
        lines.append(f"<{subject}>\n    {body} .\n")

        # A purchase event anchors `payment-overdue.sparql`: the rule joins on
        # `gst:PurchaseEvent` and, without one, matches nothing however many
        # payments are loaded. The invoice date is when the purchase happened.
        if kind == "books" and _is_present(row.get("invoice_date")):
            event = f"{NAMESPACE}purchase-event-{subject.rsplit('#', 1)[-1]}"
            lines.append(
                f"<{event}>\n    a gst:PurchaseEvent ;\n"
                f"    gst:onInvoice <{subject}> ;\n"
                f"    gst:atTime {_turtle_string(_normalize_date(row.get('invoice_date')))} .\n"
            )
    return "\n".join(lines)


def _request(
    url: str,
    *,
    method: str = "GET",
    body: bytes | None = None,
    token: str | None = None,
    content_type: str = "application/json",
    timeout: int = 10,
) -> dict:
    """One JSON round trip to graph-owl.

    **Added 19 August 2026 after four call sites used it without it
    existing.** `console_guidance`, the memory write, the waiver write and the
    explain read all called `graphowl_client._request(...)` — a name this
    module never defined or imported — and every one of them sat inside a bare
    `except Exception`, so the `NameError` was swallowed and each feature
    silently did nothing. The guidance fetch is what surfaced it: every rule
    rendered a fallback title, which looked like missing pack data rather than
    a broken call.

    The lesson is the bare except, not the missing function: an `except
    Exception` around a call that can fail for *programming* reasons as well as
    network ones cannot tell the two apart, and reports both as "graph-owl was
    unavailable".

    # Raises

    `IngestError` on refusal, unreachability or timeout — the same error every
    other call in this module raises, so a caller has one thing to catch.
    """
    request = urllib.request.Request(url, data=body, method=method)
    if body is not None:
        request.add_header("content-type", content_type)
    if token:
        request.add_header("authorization", f"Bearer {token}")
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            raw = response.read()
            return json.loads(raw) if raw else {}
    except urllib.error.HTTPError as refused:
        detail = refused.read().decode("utf-8", errors="replace")
        raise IngestError(f"{method} {url} failed: HTTP {refused.code} {detail}") from refused
    except urllib.error.URLError as unreachable:
        raise IngestError(f"{method} {url} was unreachable: {unreachable.reason}") from unreachable
    except TimeoutError:
        raise IngestError(f"{method} {url} timed out") from None


def console_guidance(server: str, pack: str = "gst", token: str | None = None) -> dict:
    """A pack's per-finding guidance — `GET /packs/{pack}/console`.

    **Best-effort, and the failure mode is deliberate.** A graph-owl that is
    unreachable costs readable titles, not the screen: `rule_guidance.decorate`
    falls back to a readable phrase derived from the label, so the worst case
    is a slightly worse noun rather than a raw IRI or an error page.
    """
    try:
        config = _request(f"{server.rstrip('/')}/packs/{pack}/console", method="GET", token=token)
    except Exception:  # noqa: BLE001
        return {}
    return (config or {}).get("guidance") or {}


def record_alignments(*, server: str, requests: list[dict], token: str | None = None) -> int:
    """Post each alignment, returning how many landed — Plan 123 Slice G.

    **Counts successes rather than raising on the first failure.** These are
    independent facts: one malformed header should not cost the other twenty
    their alignment, and the caller wants to know how many were recorded, not
    which one failed first. A total failure is still visible — the count is
    zero.
    """
    landed = 0
    for request in requests:
        try:
            _request(
                f"{server.rstrip('/')}/alignments",
                method="POST",
                token=token,
                body=json.dumps(request).encode(),
            )
            landed += 1
        except Exception:  # noqa: BLE001, S110
            # Deliberately swallowed per the docstring above. The count is the
            # signal; a partial result is more useful than an exception that
            # discards the alignments that did land.
            continue
    return landed


def graph_context(
    server: str, seed: str, *, hops: int = 2, max_nodes: int = 40, token: str | None = None
) -> dict:
    """`POST /graph/context` seeded on one subject — the same call the
    console's own Explore screen makes.

    **Best-effort.** A case's own explanation still renders without its graph
    neighbourhood; a graph-owl that is unreachable costs a panel, not the
    screen.
    """
    try:
        return _request(
            f"{server.rstrip('/')}/graph/context",
            method="POST",
            token=token,
            body=json.dumps(
                {"seed": seed, "direction": "both", "hops": hops, "maxNodes": max_nodes}
            ).encode(),
        )
    except Exception:  # noqa: BLE001
        return {"nodes": [], "edges": [], "truncated": False}


def node_classes(server: str, iris: list[str], *, token: str | None = None) -> dict[str, str]:
    """`{node id -> its rdf:type}` for a set of subjects.

    **Not the same thing as `/graph/context`'s own `sources` field**, which
    names the import graphs a subject appears in rather than its class — using
    that for a badge produced the same generic label for every node.

    A `VALUES` clause rather than one call per node: the proven-safe idiom in
    this pack (`period-diff.sparql`'s own two-row `VALUES`), and one round
    trip for the whole walk rather than one per node.

    Best-effort: a subject with no resolvable class is simply absent from the
    returned map, and the caller falls back to a generic badge for it.
    """
    if not iris:
        return {}
    values = " ".join(f"<{iri}>" for iri in iris)
    query = (
        "PREFIX rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>\n"
        f"SELECT ?s ?class WHERE {{ VALUES ?s {{ {values} }} "
        "GRAPH ?g { ?s rdf:type ?class } }"
    )
    try:
        result = _request(
            f"{server.rstrip('/')}/sparql", method="POST", token=token,
            body=json.dumps({"query": query}).encode(),
        )
    except Exception:  # noqa: BLE001
        return {}

    classes: dict[str, str] = {}
    for row in result.get("rows") or []:
        subject = row.get("s", "").strip("<>")
        class_iri = row.get("class", "").strip("<>")
        if subject and class_iri:
            classes[subject] = "gst:" + class_iri.rsplit("#", 1)[-1]
    return classes


def import_document(
    server: str, source: str, turtle: str, token: str | None = None
) -> dict:
    """`POST /graph/import/rdf?source=...&format=turtle` — lands `turtle`
    as a new named import graph, `graph:import:{source}`. Idempotent on
    the server side, same as every other pack document import: re-posting
    an unchanged document is a no-op, not an error.

    # Raises

    `IngestError` if the server refuses or is unreachable.
    """
    base = server.rstrip("/")
    url = f"{base}/graph/import/rdf?source={quote(source, safe='')}&format=turtle"
    request = urllib.request.Request(url, data=turtle.encode("utf-8"), method="POST")
    request.add_header("content-type", "text/turtle")
    if token:
        request.add_header("authorization", f"Bearer {token}")
    try:
        with urllib.request.urlopen(request, timeout=5) as response:
            raw = response.read()
            return json.loads(raw) if raw else {}
    except urllib.error.HTTPError as refused:
        detail = refused.read().decode("utf-8", errors="replace")
        raise IngestError(f"POST {url} failed: HTTP {refused.code} {detail}") from refused
    except urllib.error.URLError as unreachable:
        raise IngestError(f"POST {url} was unreachable: {unreachable.reason}") from unreachable
    except TimeoutError:
        raise IngestError(f"POST {url} timed out") from None


def delete_document(server: str, source: str, token: str | None = None) -> dict:
    """`DELETE /graph/import/rdf?source=...` — drops every triple `source`
    has ever landed. Plan 120 Slice D
    (`plans/120-domain-agnostic-console-and-investigation-workspace.md`):
    called immediately before `import_document` for the same source on
    every upload, so a re-upload *replaces* what that source last landed
    instead of accumulating alongside it under a fresh random source name
    — the confirmed root cause of totals that grew across every upload a
    session ever made, never just reflecting the latest one.

    A source with nothing landed yet (the first upload of a kind) is not
    an error — the server reports `{"deleted": 0}` — so this is safe to
    call unconditionally, with no "is this the first upload" check needed
    here.

    # Raises

    `IngestError` if the server refuses or is unreachable.
    """
    base = server.rstrip("/")
    url = f"{base}/graph/import/rdf?source={quote(source, safe='')}"
    request = urllib.request.Request(url, method="DELETE")
    if token:
        request.add_header("authorization", f"Bearer {token}")
    try:
        with urllib.request.urlopen(request, timeout=5) as response:
            raw = response.read()
            return json.loads(raw) if raw else {}
    except urllib.error.HTTPError as refused:
        detail = refused.read().decode("utf-8", errors="replace")
        raise IngestError(f"DELETE {url} failed: HTTP {refused.code} {detail}") from refused
    except urllib.error.URLError as unreachable:
        raise IngestError(f"DELETE {url} was unreachable: {unreachable.reason}") from unreachable
    except TimeoutError as timeout:
        raise IngestError(f"DELETE {url} timed out") from timeout


def list_findings(server: str, token: str | None = None) -> list:
    """`GET /findings?pack=gst` — every finding graph-owl's native engine
    has recorded, evidence included. Read-only and not admin-gated
    server-side (`crates/graph-owl-server/src/lib.rs`'s own comment: "an
    operator who cannot see the queue cannot do it"), so this needs no
    special principal beyond whatever `token` main.py already carries.

    # Raises

    `IngestError` if the server refuses or is unreachable.
    """
    base = server.rstrip("/")
    url = f"{base}/findings?pack={PACK_ID}"
    request = urllib.request.Request(url, method="GET")
    if token:
        request.add_header("authorization", f"Bearer {token}")
    try:
        with urllib.request.urlopen(request, timeout=5) as response:
            raw = response.read()
            return json.loads(raw) if raw else []
    except urllib.error.HTTPError as refused:
        detail = refused.read().decode("utf-8", errors="replace")
        raise IngestError(f"GET {url} failed: HTTP {refused.code} {detail}") from refused
    except urllib.error.URLError as unreachable:
        raise IngestError(f"GET {url} was unreachable: {unreachable.reason}") from unreachable
    except TimeoutError as timeout:
        raise IngestError(f"GET {url} timed out") from timeout


def list_packs(server: str, token: str | None = None) -> list:
    """`GET /ontology-packs` — which packs graph-owl actually has installed.

    The console header used to state a pack version as a literal string, so
    it reported one whether or not a pack was installed and whether or not
    graph-owl was reachable. Reading it lets the header say what is true,
    including "not reachable".

    # Raises

    `IngestError` if the server refuses or is unreachable.
    """
    base = server.rstrip("/")
    url = f"{base}/ontology-packs"
    request = urllib.request.Request(url, method="GET")
    if token:
        request.add_header("authorization", f"Bearer {token}")
    try:
        with urllib.request.urlopen(request, timeout=5) as response:
            raw = response.read()
            return json.loads(raw) if raw else []
    except urllib.error.HTTPError as refused:
        detail = refused.read().decode("utf-8", errors="replace")
        raise IngestError(f"GET {url} failed: HTTP {refused.code} {detail}") from refused
    except urllib.error.URLError as unreachable:
        raise IngestError(f"GET {url} was unreachable: {unreachable.reason}") from unreachable
    except TimeoutError as timeout:
        raise IngestError(f"GET {url} timed out") from timeout


__all__ = [
    "IngestError", "PACK_ID", "PREDICATES", "delete_document", "import_document",
    "list_findings", "list_packs", "rows_to_turtle",
]


# --- Invoice-level aggregation -------------------------------------------
#
# A real GST return is line-structured: GSTR-2B carries one row per rate slab
# per invoice, so a 5%/12%/18% invoice arrives as three rows. Reconciliation
# is invoice-level — the books total is compared against the *invoice*, not
# against one of its rate lines — so the lines are summed before they become
# graph subjects.
#
# Doing it here rather than in SPARQL is deliberate: no rule in packs/gst uses
# SUM or GROUP BY, and adding aggregation to thirteen queries would put the
# same arithmetic in thirteen places. One invoice subject carrying the summed
# value keeps every rule reading what it already reads.

#: Fields summed across the lines of one document. Everything else is carried
#: from the first line, because a rate line does not change who issued the
#: invoice or when.
_SUMMED_FIELDS = ("taxable", "igst", "cgst", "sgst", "cess")

#: How a portal or an ERP spells a credit note. Compared case-insensitively
#: against the stripped value, so "Credit Note" and "CR" both land.
_CREDIT_NOTE_MARKERS = ("C", "CR", "CN", "CREDIT", "CREDITNOTE", "CREDIT NOTE")
_DEBIT_NOTE_MARKERS = ("D", "DR", "DN", "DEBIT", "DEBITNOTE", "DEBIT NOTE")


def _amount(value: object) -> Decimal:
    """A blank cell, a None, or unparseable text all mean "nothing here",
    which sums as zero. A wrong number would be worse than an absent one."""
    if value is None or (isinstance(value, str) and not value.strip()):
        return Decimal(0)
    if isinstance(value, float) and math.isnan(value):
        return Decimal(0)
    try:
        return Decimal(str(value))
    except (InvalidOperation, ValueError):
        return Decimal(0)


def _document_key(row: dict) -> tuple[str, str]:
    """Two rows are the same document by supplier **and** normalised invoice
    number. Supplier is part of the key because two suppliers reuse an invoice
    number constantly, and merging them would claim one supplier's credit
    against the other's invoice."""
    return (
        str(row.get("supplier_gstin") or "").strip().upper(),
        normalize_invoice_no(row.get("invoice_no")),
    )


def aggregate_invoice_lines(rows: list[dict]) -> list[dict]:
    """Collapse a document's rate lines into one row, summing the tax heads.

    Order is preserved on first appearance, so a reviewer reading the file and
    the screen side by side sees the same sequence.
    """
    grouped: dict[tuple[str, str], dict] = {}
    for row in rows:
        key = _document_key(row)
        existing = grouped.get(key)
        if existing is None:
            merged = dict(row)
            for field in _SUMMED_FIELDS:
                merged[field] = _amount(row.get(field))
            grouped[key] = merged
            continue
        for field in _SUMMED_FIELDS:
            existing[field] = _amount(existing.get(field)) + _amount(row.get(field))
        # A later line may carry a value the first one left blank.
        for field, value in row.items():
            if field in _SUMMED_FIELDS:
                continue
            if not _is_present(existing.get(field)) and _is_present(value):
                existing[field] = value
    return list(grouped.values())


def _note_kind(row: dict) -> str | None:
    """`"credit"`, `"debit"`, or None for an ordinary invoice."""
    raw = str(row.get("note_type") or "").strip().upper()
    if not raw:
        return None
    if raw in _CREDIT_NOTE_MARKERS:
        return "credit"
    if raw in _DEBIT_NOTE_MARKERS:
        return "debit"
    return None


def net_credit_notes(rows: list[dict]) -> list[dict]:
    """Apply credit and debit notes to the invoices they amend (s.34).

    A supplier who issues a Rs 10,000 credit note against a Rs 1,00,000
    invoice reports Rs 90,000 on the portal. Comparing the original against
    that raises a Rs 10,000 mismatch that is not one.

    **Expects one row per document** — run `aggregate_invoice_lines` first.
    An earlier version of this keyed a dict over its own input to find note
    targets, which silently kept only the *last* of a multi-rate invoice's
    lines and discarded the rest before they could ever be summed. The bug was
    invisible in the output shape: one subject, one value, plausibly wrong.

    Two cases are deliberately *not* netted, and are returned as their own rows
    for a human instead of being silently absorbed:

    - a note naming an invoice absent from this file — the original is
      probably in another period, and dropping the note would hide a real
      document from the cross-period rules;
    - a note larger than the invoice it names — either the data is wrong or
      the original is elsewhere, and clamping at zero would state a position
      nobody computed.
    """
    targets: dict[tuple[str, str], dict] = {}
    for row in rows:
        if _note_kind(row) is None:
            targets.setdefault(_document_key(row), row)

    absorbed: set[int] = set()
    for row in rows:
        kind = _note_kind(row)
        if kind is None:
            continue
        target = targets.get((
            str(row.get("supplier_gstin") or "").strip().upper(),
            normalize_invoice_no(row.get("original_invoice_no")),
        ))
        if target is None:
            continue
        # Real GST files sign credit notes negative (the March 2026 sample
        # writes CN-MAR-001 as -12000), but some ERPs write the magnitude and
        # rely on the note type. The *kind* decides the direction; the recorded
        # sign is only how the file happens to write it. Multiplying a negative
        # amount by -1 added instead of subtracting, netting a 42,000 invoice
        # with a 12,000 credit note to 54,000.
        if kind == "credit" and abs(_amount(row.get("taxable"))) > abs(_amount(target.get("taxable"))):
            continue  # over-large: surfaced for a human, not applied
        sign = Decimal(-1) if kind == "credit" else Decimal(1)
        for field in _SUMMED_FIELDS:
            target[field] = _amount(target.get(field)) + sign * abs(_amount(row.get(field)))
        absorbed.add(id(row))

    return [r for r in rows if id(r) not in absorbed]


def _summary_to_turtle(rows: list[dict], kind: str, lines: list[str]) -> str:
    """One `gst:Gstr3bReturn` per period — Plan 123, GSTR-3B.

    **A period is mandatory here, unlike every other field in this module.**
    Elsewhere an absent field is omitted and the row still lands, because a
    partial invoice is still the best information available. A 3B with no
    period is different in kind: it cannot be compared against the 2B it is
    supposed to agree with, or placed on any timeline. It is not a partial
    answer, it is an unplaceable one, so it is refused rather than landed
    where it would silently never match.
    """
    for row in rows:
        period_raw = row.get("period")
        if not _is_present(period_raw):
            raise ValueError(
                f"a {kind} row needs a period — a summary return that names no "
                "period cannot be compared against anything"
            )
        period = quote(str(period_raw).strip(), safe="")
        subject = f"{NAMESPACE}{kind}-return-{period}"
        triples = [f"a {CLASS_BY_KIND[kind]}", f"gst:period {_turtle_string(period_raw)}"]
        for field, predicate in SUMMARY_PREDICATES.items():
            value = row.get(field)
            # `_is_present` treats 0 as present, which matters more here than
            # anywhere else: 4D(1) is legitimately zero most months, and a
            # reclaim of zero is a different statement from no reclaim figure.
            if not _is_present(value):
                continue
            triples.append(f"gst:{predicate} {_turtle_string(value)}")
        lines.append(f"<{subject}>\n    " + " ;\n    ".join(triples) + " .\n")
    return "\n".join(lines)


def _events_to_turtle(rows: list[dict], kind: str, lines: list[str]) -> str:
    """One `gst:PaymentEvent` / `gst:GoodsReceipt` per row.

    An event is a time and the invoice it happened to. It points at the
    **books** invoice subject via `gst:onInvoice`, because that is what
    `payment-overdue.sparql` and `goods-receipt-timing.sparql` join on —
    pointing at the canonical subject instead would match nothing, silently.

    A row with no date is skipped rather than emitted dateless: an event with
    no time cannot answer "how many days apart", and a rule reading one would
    treat an invoice that *was* paid as never paid — a reversal the client
    does not owe.
    """
    date_field = EVENT_DATE_FIELD[kind]
    for index, row in enumerate(rows):
        when = row.get(date_field)
        if not _is_present(when):
            continue
        invoice = _subject_iri("books", row)
        subject = f"{NAMESPACE}{kind}-{index}-{_subject_iri(kind, row).rsplit('#', 1)[-1]}"
        triples = [
            f"a {CLASS_BY_KIND[kind]}",
            f"gst:onInvoice <{invoice}>",
            f"gst:atTime {_turtle_string(_normalize_date(when))}",
        ]
        lines.append(f"<{subject}>\n    " + " ;\n    ".join(triples) + " .\n")
    return "\n".join(lines) if len(lines) > 2 else ""
