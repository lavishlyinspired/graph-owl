"""GST subject-encoding rules shared by every module that mints packs/gst
subjects: `gstr2b.py` (this package's own live-GSP connector) and
reco-now's `graphowl_client.py` (`ext-apps/Reco/backend/app`, outside this
package but importing from it — the same relationship `main.py` already
has to `graph_owl_packs.loader`/`reconcile`).

**Extracted 16 August 2026, after the two had already drifted.**
`graphowl_client.py`'s canonical `gst:Invoice` subject used the raw
invoice number where `gstr2b.py`'s own `invoice_subject` already
normalized it first. Harmless while nothing called `gstr2b.py` in
production, but it means a books upload (`graphowl_client.py`) and a live
GSTR-2B pull (`gstr2b.py`) for the *same real invoice* would have computed
two different canonical subjects the moment both wrote to the same store
— silently splitting one invoice's evidence across two subjects, which
every finding query's `?canonical gst:recordedIn/reflectedIn/appearsIn`
join relies on landing on one. This module is the fix: one definition of
"the same invoice, any punctuation" and "the same GSTIN, any percent-
encoding", so the two callers cannot independently drift again.

stdlib only, matching every module in this package — see `loader.py`'s
own words: "a loader is not a place to acquire an HTTP dependency."
"""

from __future__ import annotations

import re
import unicodedata


def invoice_key(value: object) -> str:
    """The key two records are the same invoice by: case and punctuation
    stripped, accented characters transliterated (not deleted — a plain
    `[^A-Z0-9]` strip on the un-normalized string would drop "É" outright,
    a silently different key for what a human reads as the same name).

    Leading zeros are **not** stripped — `INV-001` and `INV-1` are
    different invoices in plenty of numbering schemes, and a wrong match
    silently claims credit against the wrong invoice, where a missed one
    only leaves a finding unfired.
    """
    if value is None:
        return ""
    stripped_accents = "".join(
        c for c in unicodedata.normalize("NFKD", str(value)) if not unicodedata.combining(c)
    )
    return re.sub(r"[^A-Z0-9]", "", stripped_accents.upper())


def subject_suffix(value: object) -> str:
    """An identifier as the local part of a prefixed Turtle name.

    **Percent-encoded, never substituted** — mapping every unsafe
    character onto `-` would make `INV/1` and `INV-1` one subject, two
    different invoices silently merged. Percent-encoding is reversible and
    is explicitly legal in a Turtle `PN_LOCAL` (the `PLX`/`PERCENT`
    production) — Indian invoice numbers routinely carry a `/`
    (`RST/2026/0455`), which written straight into a prefixed name is not
    legal Turtle at all and the server rejects the whole import.
    """
    out = []
    for char in str(value or ""):
        if char.isascii() and (char.isalnum() or char in "_-"):
            out.append(char)
        else:
            out.extend(f"%{byte:02X}" for byte in char.encode("utf-8"))
    return "".join(out)


def turtle_literal(value: object) -> str:
    """A Turtle string literal for `value`. Backslash first, then quote,
    then the whitespace escapes — in that order, or escaping the quote
    would double-escape the backslash just inserted before it."""
    text = str(value)
    text = text.replace("\\", "\\\\").replace('"', '\\"')
    return text.replace("\n", "\\n").replace("\r", "\\r").replace("\t", "\\t")


def canonical_local_name(gstin: object, invoice_number: object) -> str:
    """The local name half of the canonical `gst:Invoice` subject —
    `invoice-{gstin}-{invoiceKey}`, **kind-independent**: a books upload
    and a GSTR-2B/GSTR-1 upload for the same (gstin, invoice number) must
    mint the identical name, or `gst:recordedIn`/`gst:reflectedIn`/
    `gst:appearsIn` would land on two different subjects instead of
    meeting on one, and every finding query that joins through
    `?canonical` would silently match nothing.

    Keyed on the *normalized* invoice number (`invoice_key`), not the
    printed one — the case/punctuation-insensitive match this exists for.
    """
    return f"invoice-{subject_suffix(gstin)}-{invoice_key(invoice_number)}"


def supplier_local_name(gstin: object) -> str:
    """The local name half of a `gst:Supplier` subject — keyed on the
    GSTIN alone, never the invoice, so two invoices from the same
    supplier resolve to the same node."""
    return f"supplier-{subject_suffix(gstin)}"


__all__ = [
    "canonical_local_name",
    "invoice_key",
    "subject_suffix",
    "supplier_local_name",
    "turtle_literal",
]
