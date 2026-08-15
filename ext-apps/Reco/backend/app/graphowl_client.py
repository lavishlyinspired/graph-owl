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
#: there is deliberately no separate `Gstr2aInvoice` class — 2A is "a
#: revolving view over the same supplier-declared data
#: [gst:Gstr1Invoice] already carries."
CLASS_BY_KIND = {
    "books": "gst:PurchaseInvoice",
    "gstr2b": "gst:Gstr2bInvoice",
    "gstr1": "gst:Gstr1Invoice",
}

#: The canonical-subject edge each kind's upload asserts — the half it
#: can, from its own named graph. The other kinds' edges meet it there
#: when their own uploads land, never in the same call.
LINK_PREDICATE_BY_KIND = {
    "books": "gst:recordedIn",
    "gstr2b": "gst:reflectedIn",
    "gstr1": "gst:appearsIn",
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
KINDS_NEEDING_INVOICE_KEY = {"books", "gstr1", "gstr2b"}

#: Kinds whose invoice subject needs a combined `gst:taxAmount` —
#: `missing-in-gstr2b.sparql` reads it off the books side,
#: `missing-in-books.sparql` off the gstr1 side. Never gstr2b.
KINDS_NEEDING_TAX_AMOUNT = {"books", "gstr1"}

#: Row field (main.py's FIELD_LABELS keys) -> `gst:` predicate local
#: name. Matches `packs/gst/pack.toml`'s `[[predicates]]` and
#: `ontology.ttl` exactly, as one table, so they cannot drift silently
#: against each other. `supplier_gstin` is deliberately not here —
#: `packs/gst`'s own finding queries read it off the *Supplier* subject
#: (`?supplier gst:supplierGstin ?gstin`), reached via `gst:issuedBy`,
#: never as a direct literal on the invoice. `rows_to_turtle` asserts it
#: once, on the Supplier subject.
PREDICATES: dict[str, str] = {
    "invoice_no": "invoiceNumber",
    "supplier_name": "supplierName",
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

    lines = [f"@prefix gst: <{NAMESPACE}> .", ""]
    seen_filings: set[str] = set()
    for row in rows:
        subject = _subject_iri(kind, row)
        gstin_raw = row.get("supplier_gstin")
        supplier = _supplier_iri(gstin_raw)
        canonical = _canonical_iri(gstin_raw, row.get("invoice_no"))

        # The Supplier subject — packs/gst's finding queries read the GSTIN
        # off it (`?supplier gst:supplierGstin ?gstin`), not off the
        # invoice directly.
        lines.append(
            f"<{supplier}>\n    a gst:Supplier ;\n    gst:supplierGstin "
            f"{_turtle_string(gstin_raw)} .\n"
        )

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
        body = " ;\n    ".join(triples)
        lines.append(f"<{subject}>\n    {body} .\n")
    return "\n".join(lines)


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
        with urllib.request.urlopen(request) as response:
            raw = response.read()
            return json.loads(raw) if raw else {}
    except urllib.error.HTTPError as refused:
        detail = refused.read().decode("utf-8", errors="replace")
        raise IngestError(f"POST {url} failed: HTTP {refused.code} {detail}") from refused
    except urllib.error.URLError as unreachable:
        raise IngestError(f"POST {url} was unreachable: {unreachable.reason}") from unreachable


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
        with urllib.request.urlopen(request) as response:
            raw = response.read()
            return json.loads(raw) if raw else []
    except urllib.error.HTTPError as refused:
        detail = refused.read().decode("utf-8", errors="replace")
        raise IngestError(f"GET {url} failed: HTTP {refused.code} {detail}") from refused
    except urllib.error.URLError as unreachable:
        raise IngestError(f"GET {url} was unreachable: {unreachable.reason}") from unreachable


__all__ = [
    "IngestError", "PACK_ID", "PREDICATES", "import_document", "list_findings", "rows_to_turtle",
]
