"""GSTR-2B as an ingestion connector — Epic 105 P2.

**The authority's JSON is evidence, not a schema to adopt.** This module
normalizes a GSTR-2B response into the vocabulary `packs/gst/ontology.ttl`
already defines, so the six finding rules cannot tell a fixture from a live
GSP response. Everything downstream — matching, mismatch detection, ITC
availability, reverse charge, the 180-day span, the review queue — is
developed and tested at zero API cost, and going live changes where the bytes
come from and nothing else.

**Why there is no "GSTN client" here.** GSTN does not sell API access
directly; access is through an empanelled GSP, and every GSP wraps the same
returns behind its own auth and its own envelope. So the shape of this module
is deliberately: *one normalizer, many fetchers*. `normalize` is the part with
all the correctness risk and it is pure — it takes a `dict` and never touches
a network. A fetcher is a small function that produces that `dict`, and
swapping GSP is writing a new one.

**Field names are taken from a published GSP API reference, not from memory.**
`ctin`, `trdnm`, `inum`, `dt`, `txval`, `igst`/`cgst`/`sgst`/`cess`, `itcavl`,
`rev`, `typ`, `pos`, nested under `docdata.b2b[].inv[]`. This project has
already shipped one bug from writing an external format out of recall; the
rule now is that an external shape is read from documentation or a real
response.

stdlib only, like the loader beside it.
"""

from __future__ import annotations

import json
import urllib.error
import urllib.request
from dataclasses import dataclass
from datetime import datetime
from decimal import Decimal, InvalidOperation
from pathlib import Path

#: The pack whose vocabulary this produces. Named once so a second
#: authority-facing connector cannot drift from it silently.
PREFIX = "gst"

#: Money is carried as a fixed two-decimal string all the way to the graph.
#: `str(40000.0)` is `"40000.0"`, which does not equal a purchase register's
#: `"40000.00"` — and the reconciliation compares them as strings, so a float
#: that looks equal to a human would simply fail to match.
MONEY = Decimal("0.01")


class Gstr2bError(RuntimeError):
    """A response could not be read. **Always raised, never absorbed into an
    empty result** — an authentication failure or an error page silently
    deserializing into "no invoices" would report every claimed invoice as
    unmatched, which reads as a catastrophic reconciliation rather than as the
    failed fetch it is."""


@dataclass(frozen=True)
class Gstr2bInvoice:
    """One invoice line as the authority reports it."""

    supplier_gstin: str
    supplier_name: str
    invoice_number: str
    invoice_date: str
    taxable_value: str
    igst: str
    cgst: str
    sgst: str
    cess: str
    tax_amount: str
    itc_available: str
    reverse_charge: str
    invoice_type: str
    place_of_supply: str

    @property
    def period(self) -> str:
        """The return period, from the invoice's own date.

        Deriving it from the clock instead would silently change what a re-run
        of the same period means.
        """
        return self.invoice_date[:7]

    @property
    def subject(self) -> str:
        """The graph subject, matching the fixtures' own `2b-` convention so a
        live line and a fixture line are indistinguishable to the rules."""
        return f"{PREFIX}:2b-{self.invoice_number}"


def _money(value: object) -> str:
    """A monetary figure as a fixed two-decimal string."""
    if value is None or value == "":
        return "0.00"
    try:
        return str(Decimal(str(value)).quantize(MONEY))
    except (InvalidOperation, ValueError) as unreadable:
        raise Gstr2bError(f"'{value}' is not a monetary amount") from unreadable


def _iso_date(value: str) -> str:
    """A GST date as ISO-8601.

    **The trap this connector exists to close.** GST returns dates as
    `DD-MM-YYYY`. Every finding rule compares dates as ISO strings, relying on
    lexicographic order being chronological order — which is what makes "which
    provision was in force" answerable at all, since the query engine has no
    date type in expressions. Passing `04-07-2026` through breaks that
    ordering *silently*: the string then sorts by day-of-month, the cap
    resolution picks the wrong provision, and the finding cites the wrong
    notification while looking entirely authoritative.

    ISO input is passed through rather than re-parsed, because reading
    `2026-07-04` as day-first would produce nonsense from a correct value.

    Anything else is refused. A date this cannot place is one the rules would
    mis-order, and a wrong citation is worse than a missing finding.
    """
    text = str(value).strip()
    for pattern in ("%Y-%m-%d", "%d-%m-%Y", "%d/%m/%Y"):
        try:
            return datetime.strptime(text, pattern).date().isoformat()
        except ValueError:
            continue
    raise Gstr2bError(
        f"'{text}' is not a date this connector can place — GST reports "
        f"DD-MM-YYYY and the rules need ISO-8601, and guessing at a date "
        f"would produce a finding citing the wrong provision"
    )


def _docdata(payload: dict) -> dict:
    """The `docdata` section, wherever the provider chose to wrap it.

    The published reference nests it at `data.data.docdata`; a provider that
    returns `docdata` at the top level is not wrong, only different. Finding
    it wherever it sits costs nothing and removes a per-provider adapter —
    which is the point, since the GSP is meant to be replaceable.
    """
    seen = payload
    for _ in range(4):
        if not isinstance(seen, dict):
            break
        if "docdata" in seen:
            found = seen["docdata"]
            if isinstance(found, dict):
                return found
            break
        seen = seen.get("data")  # type: ignore[assignment]
    raise Gstr2bError(
        "no 'docdata' section in this response — it is not a GSTR-2B payload, "
        "and reading it as an empty return would report every claimed invoice "
        "as unmatched"
    )


def normalize(payload: dict) -> list[Gstr2bInvoice]:
    """Every B2B invoice in a GSTR-2B response, in the pack's own terms.

    Pure: takes a `dict`, touches no network. All of this module's correctness
    risk lives here, which is why it is the part that is trivially testable.

    # Raises

    `Gstr2bError` if the payload is not a GSTR-2B response, or carries a date
    or amount that cannot be read.
    """
    suppliers = _docdata(payload).get("b2b", [])
    # An absent b2b section is a period in which nobody filed against this
    # taxpayer — legitimate, common, and exactly when a reconciliation is most
    # interesting, since everything claimed is then unmatched.
    if not isinstance(suppliers, list):
        raise Gstr2bError("'b2b' is present but is not a list of suppliers")

    invoices = []
    for supplier in suppliers:
        gstin = str(supplier.get("ctin", ""))
        name = str(supplier.get("trdnm", ""))
        for line in supplier.get("inv", []):
            igst, cgst = _money(line.get("igst")), _money(line.get("cgst"))
            sgst, cess = _money(line.get("sgst")), _money(line.get("cess"))
            invoices.append(
                Gstr2bInvoice(
                    supplier_gstin=gstin,
                    supplier_name=name,
                    invoice_number=str(line.get("inum", "")),
                    invoice_date=_iso_date(line.get("dt", "")),
                    taxable_value=_money(line.get("txval")),
                    igst=igst,
                    cgst=cgst,
                    sgst=sgst,
                    cess=cess,
                    # The register records one tax figure and the authority
                    # splits it four ways, so the total is what reconciles.
                    # The components are kept as evidence: an intra-state
                    # supply reported as inter-state is a real and common
                    # error that the total alone hides completely.
                    tax_amount=str(
                        sum(Decimal(part) for part in (igst, cgst, sgst, cess)).quantize(MONEY)
                    ),
                    itc_available=str(line.get("itcavl", "")),
                    reverse_charge=str(line.get("rev", "")),
                    invoice_type=str(line.get("typ", "")),
                    place_of_supply=str(line.get("pos", "")),
                )
            )
    return invoices


def _literal(value: str) -> str:
    """A Turtle string literal. Backslash first, or the escapes escape each
    other and one badly-named supplier corrupts every triple after it."""
    return value.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n")


def to_turtle(invoices: list[Gstr2bInvoice], namespace: str | None = None) -> str:
    """The same shape `packs/gst/fixtures/gstr2b.ttl` carries.

    Emitting the fixtures' exact vocabulary is what makes live and fixture
    data interchangeable — the finding rules must not be able to tell which
    produced a subject, or the whole "develop against fixtures, deploy against
    a GSP" split is a fiction.
    """
    iri = namespace or "https://graph-owl.dev/packs/gst#"
    lines = [
        "# GSTR-2B, as fetched from the authority. Generated — do not edit.",
        "#",
        "# The same vocabulary `packs/gst/fixtures/gstr2b.ttl` uses, so the",
        "# finding rules cannot tell a live line from a fixture one.",
        "",
        f"@prefix {PREFIX}: <{iri}> .",
        "@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .",
        "",
    ]
    for invoice in invoices:
        lines.append(f"{invoice.subject} rdf:type {PREFIX}:Gstr2bInvoice ;")
        fields = [
            ("supplierGstin", invoice.supplier_gstin),
            ("supplierName", invoice.supplier_name),
            ("invoiceNumber", invoice.invoice_number),
            ("invoiceDate", invoice.invoice_date),
            ("taxableValue", invoice.taxable_value),
            ("taxAmount", invoice.tax_amount),
            ("igst", invoice.igst),
            ("cgst", invoice.cgst),
            ("sgst", invoice.sgst),
            ("cess", invoice.cess),
            ("itcAvailable", invoice.itc_available),
            ("reverseCharge", invoice.reverse_charge),
            ("invoiceType", invoice.invoice_type),
            ("placeOfSupply", invoice.place_of_supply),
            ("period", invoice.period),
        ]
        # An absent value is omitted rather than written blank: "not reported"
        # and "reported as empty" are different facts, and a reconciliation is
        # mostly a set of questions about missing data.
        present = [(name, value) for name, value in fields if value != ""]
        for index, (name, value) in enumerate(present):
            terminator = " ." if index == len(present) - 1 else " ;"
            lines.append(f'    {PREFIX}:{name:<13} "{_literal(value)}"{terminator}')
        lines.append("")
    return "\n".join(lines)


def fetch(
    base_url: str,
    gstin: str,
    period: str,
    *,
    api_key: str | None = None,
    headers: dict[str, str] | None = None,
    timeout: float = 30.0,
) -> dict:
    """Fetch a GSTR-2B return from a GSP.

    **Deliberately thin, and deliberately not tested against a real provider.**
    Every GSP wraps the same returns behind its own auth, so this is the part
    that changes per provider and `normalize` is the part that does not. It
    takes a base URL and headers rather than naming a vendor, so pointing at a
    different GSP is configuration.

    `period` is `YYYY-MM` here and converted to the `MMYYYY` the returns use,
    so callers speak one date format throughout.

    # Raises

    `Gstr2bError` on any transport or protocol failure — never a partial or
    empty result, which would read as a clean reconciliation.
    """
    if len(period) != 7 or period[4] != "-":
        raise Gstr2bError(f"period '{period}' is not YYYY-MM")
    ret_period = f"{period[5:7]}{period[0:4]}"

    url = f"{base_url.rstrip('/')}/gstr2b?gstin={gstin}&ret_period={ret_period}"
    request = urllib.request.Request(url, method="GET")
    request.add_header("accept", "application/json")
    for name, value in (headers or {}).items():
        request.add_header(name, value)
    if api_key:
        request.add_header("authorization", f"Bearer {api_key}")

    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            return json.loads(response.read())
    except urllib.error.HTTPError as refused:
        detail = refused.read().decode("utf-8", errors="replace")[:400]
        raise Gstr2bError(f"GET {url} failed: HTTP {refused.code} {detail}") from refused
    except urllib.error.URLError as unreachable:
        raise Gstr2bError(f"GET {url} was unreachable: {unreachable.reason}") from unreachable
    except json.JSONDecodeError as not_json:
        raise Gstr2bError(f"GET {url} did not return JSON") from not_json


def from_file(path: Path) -> dict:
    """A saved GSTR-2B response.

    The mode that costs nothing, and the one CI uses. A response downloaded
    once by hand exercises the whole pipeline exactly as a live fetch does.
    """
    try:
        return json.loads(Path(path).read_text(encoding="utf-8"))
    except OSError as missing:
        raise Gstr2bError(f"cannot read {path}: {missing}") from missing
    except json.JSONDecodeError as not_json:
        raise Gstr2bError(f"{path} is not JSON") from not_json


__all__ = [
    "Gstr2bError",
    "Gstr2bInvoice",
    "normalize",
    "to_turtle",
    "fetch",
    "from_file",
]
