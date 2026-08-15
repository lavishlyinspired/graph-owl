"""reco-now's own client for its graph-owl pack — an EXTENSION of
packs/gst, not a parallel copy of it.

plans/118-reco-now-integration.md, Slice 1; corrected per
plans/119-architecture-audit.md §3.1/§6. The first version of this module
(15 August 2026) minted `reco:invoiceNumber`, `reco:supplierGstin`, etc. —
11 predicates that duplicate ones `packs/gst` already registers — under an
unrelated namespace nothing relates back to the pack that has them. Fixed
by reusing `gst:` directly for every field packs/gst already carries, and
registering `reco:` predicates only for the 6 it doesn't
(`graphowl-pack/pack.toml`). Row subjects mint `gst:PurchaseInvoice`/
`gst:Gstr2bInvoice` as their type for the same reason — packs/gst's
ontology already draws exactly reco-now's books/gstr2b distinction
("taxpayer's register" / "as filed by the supplier").

stdlib `urllib` only, matching this repo's own convention for every
graph-owl Python client (`connectors/python/graph_owl_packs/loader.py`'s
own words: "a loader is not a place to acquire an HTTP dependency").

Two responsibilities, kept separate because one is pure and one is not:

- `rows_to_turtle` — normalized rows in, one RDF subject per row out.
  No I/O, so every case in it is a fast unit test.
- `import_document` — one `POST /graph/import/rdf` call, landing a
  Turtle document under a caller-named source. Installing packs/gst and
  then this pack (order matters — see `main.py`'s `_install_graphowl_pack`)
  is a one-time step at backend startup using the already-shipped,
  already-tested `graph_owl_packs.loader.load_pack` — this module does
  not reimplement that.
"""

from __future__ import annotations

import json
import math
import urllib.error
import urllib.request
from urllib.parse import quote

#: Must match graphowl-pack/pack.toml's [pack] table.
NAMESPACE = "https://reconow.dev/pack#"
#: The pack this one extends. Must be loaded first — see main.py.
GST_NAMESPACE = "https://graph-owl.dev/packs/gst#"

#: reco-now's own two dataset kinds (main.py's `kind` values) -> the
#: *shared* gst: class each becomes. Not reco-now's own class: packs/gst's
#: ontology already draws this exact distinction ("Purchase invoice
#: (taxpayer's register)" / "GSTR-2B invoice (as filed by the supplier)").
CLASS_BY_KIND = {
    "books": "gst:PurchaseInvoice",
    "gstr2b": "gst:Gstr2bInvoice",
}

#: Row field (main.py's FIELD_LABELS keys) -> (namespace prefix, predicate
#: local name). 11 of the 17 fields reuse a `gst:` predicate packs/gst
#: already registers; only the 6 packs/gst does not have get `reco:`.
#: Matches graphowl-pack/pack.toml's [[predicates]] and ontology.ttl
#: exactly, as one table, so they cannot drift silently against each other.
PREDICATES: dict[str, tuple[str, str]] = {
    "invoice_no": ("gst", "invoiceNumber"),
    "supplier_gstin": ("gst", "supplierGstin"),
    "supplier_name": ("gst", "supplierName"),
    "taxable": ("gst", "taxableValue"),
    "invoice_date": ("gst", "invoiceDate"),
    "place_of_supply": ("gst", "placeOfSupply"),
    "hsn": ("reco", "hsnCode"),
    "ims_status": ("reco", "imsStatus"),
    "reverse_charge": ("gst", "reverseCharge"),
    "note_type": ("reco", "noteType"),
    "voucher_type": ("reco", "voucherType"),
    "original_invoice_no": ("reco", "originalInvoiceNumber"),
    "voucher_no": ("reco", "voucherNumber"),
    "igst": ("gst", "igst"),
    "cgst": ("gst", "cgst"),
    "sgst": ("gst", "sgst"),
    "cess": ("gst", "cess"),
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


def _turtle_string(value: object) -> str:
    """A Turtle string literal for `value`. Backslash first, then quote,
    then the whitespace escapes — in that order, or escaping the quote
    would double-escape the backslash just inserted before it."""
    text = str(value)
    text = text.replace("\\", "\\\\").replace('"', '\\"')
    text = text.replace("\n", "\\n").replace("\r", "\\r").replace("\t", "\\t")
    return f'"{text}"'


def _subject_iri(kind: str, row: dict) -> str:
    """One subject per (kind, supplier, invoice number) — not per invoice
    number alone. Two suppliers can and do reuse the same invoice number
    text; an exact-string subject key would silently merge their rows
    into one graph subject.

    **Minted under `NAMESPACE` itself, not a separate data namespace.**
    `POST /namespaces` (in `_install_graphowl_pack`) declares exactly one
    namespace for this pack; `Sid::from_iri` refuses any IRI outside a
    namespace it has a registered code for, and a second, undeclared
    `.../data/...` namespace is exactly such an IRI — confirmed against a
    live server (400, "not in a namespace this store recognises"), not
    inferred by reading the resolver. `packs/gst`'s own fixtures follow
    the identical pattern (`gst:pr-INV-1001`, not a separate `gst-data:`
    prefix) — one registered namespace covers both the pack's vocabulary
    and its instance data."""
    gstin = quote(str(row.get("supplier_gstin") or "").strip(), safe="")
    invoice_no = quote(str(row.get("invoice_no") or "").strip(), safe="")
    return f"{NAMESPACE}{kind}-{gstin}-{invoice_no}"


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

    lines = [f"@prefix gst: <{GST_NAMESPACE}> .", f"@prefix reco: <{NAMESPACE}> .", ""]
    for row in rows:
        subject = _subject_iri(kind, row)
        triples = [f"a {CLASS_BY_KIND[kind]}"]
        for field, (prefix, predicate) in PREDICATES.items():
            value = row.get(field)
            if not _is_present(value):
                continue
            triples.append(f"{prefix}:{predicate} {_turtle_string(value)}")
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


__all__ = ["IngestError", "PREDICATES", "import_document", "rows_to_turtle"]
