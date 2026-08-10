"""A discovering connector for Frappe/ERPNext — Epic 105 P2.

**This module knows Frappe's metadata protocol. It knows no domain at all.**
Point it at "Sales Invoice", "Item", "Employee" or a DocType somebody wrote
this morning, and it derives the vocabulary from what the instance reports —
no mapping table, no per-doctype branch, nothing named after accounting.

That is the point rather than a nicety. `packs/hospitality` and `packs/gst`
were written by this project, so they fit the platform by construction, which
is weak evidence for `plans/105-domain-neutrality.md`'s claim. A schema
**nobody designed for this platform**, discovered at run time, is the strongest
available test of it.

**Nothing from ERPNext is ever vendored** — no doctype definition, no schema
file, no fixture derived from one. ERPNext is GPL-3.0 and graph-owl speaks to
it over HTTP as a separate process; the surface actually used is Frappe's
(MIT). `plans/00l-build-vs-adopt.md` records the full position. Discovery at
run time is both the licence-safe path and the better engineering: a vendored
schema drifts from the instance it claims to describe, silently.

The three routes it drives are the ones Epic 105 built:

    DocType metadata  -> POST /namespaces        declare the vocabulary
    DocField list     -> POST /predicates        one per discovered field
    Records via REST  -> POST /graph/import/rdf  as Turtle
"""

from __future__ import annotations

import json
import re
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass

#: The vocabulary discovered DocTypes land in. One namespace for the whole
#: instance rather than one per DocType: they are one schema, and splitting
#: them would make a `Link` between two DocTypes a cross-vocabulary reference
#: for no reason.
DEFAULT_NAMESPACE = "https://graph-owl.dev/packs/erpnext#"

#: Frappe field types whose value is a reference to another record, not a
#: literal. Mapped to `value_type` 0 (Ref) so a `Link` becomes a real graph
#: edge rather than a string that happens to look like a name — which is the
#: difference between a graph you can traverse and one you can only read.
REFERENCE_FIELDTYPES = frozenset({"Link", "Dynamic Link"})

#: Field types carrying no data worth landing: layout and presentation only.
#: Excluded rather than imported-and-ignored, because a predicate defined for
#: a section break is a permanent entry in the registry that means nothing.
LAYOUT_FIELDTYPES = frozenset(
    {
        "Section Break",
        "Column Break",
        "Tab Break",
        "HTML",
        "Heading",
        "Button",
        "Fold",
        "Image",
    }
)

#: `graph_owl_core::flake::value_type`. Everything not a reference lands as a
#: string — deliberately. A currency parsed into a float at the graph boundary
#: loses the exactness a monetary figure needs, and a date parsed into an
#: instant invents a timezone the source never stated. The graph keeps what
#: was reported; interpretation belongs to whatever asks the question.
VALUE_TYPE_REF = 0
VALUE_TYPE_STRING = 1

_SAFE_LOCAL_NAME = re.compile(r"[^A-Za-z0-9_-]")


class ErpnextError(RuntimeError):
    """The instance could not be reached, or answered with something this
    connector does not recognise as Frappe's metadata shape."""


@dataclass(frozen=True)
class Field:
    """One discovered DocField."""

    fieldname: str
    label: str
    fieldtype: str

    @property
    def is_reference(self) -> bool:
        return self.fieldtype in REFERENCE_FIELDTYPES

    @property
    def value_type(self) -> int:
        return VALUE_TYPE_REF if self.is_reference else VALUE_TYPE_STRING


@dataclass(frozen=True)
class DocTypeSchema:
    """A DocType as the instance describes it."""

    name: str
    fields: tuple[Field, ...]


def local_name(value: str) -> str:
    """A graph-safe local name for an arbitrary DocType or record name.

    Frappe names contain spaces (`Sales Invoice`), and record names contain
    almost anything a user typed — slashes, hyphens, non-ASCII. A local name
    goes straight into an IRI, so anything outside the safe set becomes `_`.

    **Not a hash.** A hashed name is stable and unreadable, and the whole
    value of a discovered graph is that a human can recognise what they are
    looking at. Collisions between two names that differ only in punctuation
    are possible and acceptable here for the same reason a blocking key is
    allowed to be over-inclusive: the subject still carries its original name
    as a literal, so nothing is lost, only shared.
    """
    cleaned = _SAFE_LOCAL_NAME.sub("_", value.strip())
    return cleaned or "_"


def escape_literal(value: str) -> str:
    """Escape a string for a Turtle literal.

    Hand-rolled because this package has no runtime dependencies, and the
    escape set is small and specified. **Backslash first** — escaping it after
    the quotes would double-escape every backslash the quote-escaping
    introduced, which is the classic way this function is written wrong.
    """
    return (
        value.replace("\\", "\\\\")
        .replace('"', '\\"')
        .replace("\n", "\\n")
        .replace("\r", "\\r")
        .replace("\t", "\\t")
    )


class ErpnextClient:
    """Frappe's REST surface, over stdlib HTTP.

    Token auth (`Authorization: token key:secret`) rather than session
    cookies: a connector is a scheduled job, and a login flow that expires
    mid-run is a failure mode with no good handling.
    """

    def __init__(self, base_url: str, api_key: str | None = None, api_secret: str | None = None):
        self._base = base_url.rstrip("/")
        self._token = f"{api_key}:{api_secret}" if api_key and api_secret else None

    def _get(self, path: str, params: dict[str, str] | None = None) -> dict:
        url = f"{self._base}{path}"
        if params:
            url = f"{url}?{urllib.parse.urlencode(params)}"
        request = urllib.request.Request(url, method="GET")
        request.add_header("accept", "application/json")
        if self._token:
            request.add_header("authorization", f"token {self._token}")
        try:
            with urllib.request.urlopen(request) as response:
                return json.loads(response.read())
        except urllib.error.HTTPError as refused:
            detail = refused.read().decode("utf-8", errors="replace")[:400]
            raise ErpnextError(f"GET {url} failed: HTTP {refused.code} {detail}") from refused
        except urllib.error.URLError as unreachable:
            raise ErpnextError(f"GET {url} was unreachable: {unreachable.reason}") from unreachable
        except json.JSONDecodeError as not_json:
            raise ErpnextError(f"GET {url} returned a non-JSON response") from not_json

    def schema(self, doctype: str) -> DocTypeSchema:
        """Discover one DocType's fields.

        # Raises

        `ErpnextError` if the instance does not answer with a `data.fields`
        list — which means either the DocType does not exist or this is not a
        Frappe instance, and both are worth failing on rather than importing
        an empty vocabulary.
        """
        body = self._get(f"/api/resource/DocType/{urllib.parse.quote(doctype)}")
        data = body.get("data")
        if not isinstance(data, dict) or not isinstance(data.get("fields"), list):
            raise ErpnextError(
                f"`{doctype}` did not come back with a `data.fields` list — either "
                f"the DocType does not exist or this is not a Frappe instance"
            )

        fields = []
        for raw in data["fields"]:
            if not isinstance(raw, dict):
                continue
            fieldtype = raw.get("fieldtype", "")
            fieldname = raw.get("fieldname")
            if not fieldname or fieldtype in LAYOUT_FIELDTYPES:
                continue
            fields.append(
                Field(
                    fieldname=fieldname,
                    label=raw.get("label") or fieldname,
                    fieldtype=fieldtype,
                )
            )
        return DocTypeSchema(name=doctype, fields=tuple(fields))

    def records(self, schema: DocTypeSchema, limit: int = 100) -> list[dict]:
        """Fetch records, asking only for the fields discovery found.

        `fields=["*"]` would be simpler and is wrong: it returns child-table
        rows and internal columns this connector has no predicates for, so
        every record would carry values that could not be landed.
        """
        wanted = ["name"] + [f.fieldname for f in schema.fields]
        body = self._get(
            f"/api/resource/{urllib.parse.quote(schema.name)}",
            {"fields": json.dumps(wanted), "limit_page_length": str(limit)},
        )
        data = body.get("data")
        if not isinstance(data, list):
            raise ErpnextError(f"`{schema.name}` records did not come back as a list")
        return [row for row in data if isinstance(row, dict)]


def ontology_turtle(schema: DocTypeSchema, prefix: str, namespace: str) -> str:
    """The discovered schema, as the pack's own ontology.

    A DocType becomes a class and each field a property, both in the pack's
    namespace — so the vocabulary graph-owl reasons over is the one the
    instance actually has, not one somebody transcribed from a screenshot.
    """
    doctype = local_name(schema.name)
    lines = [
        f"@prefix {prefix}: <{namespace}> .",
        "@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .",
        "",
        f"# Discovered from a live instance, never vendored — see",
        f"# plans/00l-build-vs-adopt.md. Regenerating this against a changed",
        f"# instance is how the vocabulary stays true rather than drifting.",
        "",
        f'{prefix}:{doctype} rdf:type {prefix}:Class ; {prefix}:label "{escape_literal(schema.name)}" .',
        "",
    ]
    for field in schema.fields:
        lines.append(
            f'{prefix}:{local_name(field.fieldname)} rdf:type {prefix}:Property ; '
            f'{prefix}:label "{escape_literal(field.label)}" .'
        )
    return "\n".join(lines) + "\n"


def records_turtle(
    schema: DocTypeSchema, records: list[dict], prefix: str, namespace: str
) -> str:
    """Records as Turtle, one subject per record.

    A field whose value is empty or `None` is **omitted rather than written as
    an empty literal**: "not recorded" and "recorded as blank" are different
    facts, and a graph that cannot tell them apart cannot answer a question
    about missing data — which is most of what a reconciliation asks.
    """
    lines = [
        f"@prefix {prefix}: <{namespace}> .",
        "@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .",
        "",
    ]
    doctype = local_name(schema.name)
    by_name = {f.fieldname: f for f in schema.fields}

    for record in records:
        name = record.get("name")
        if not name:
            continue
        subject = f"{prefix}:{local_name(str(name))}"
        statements = [f"{subject} rdf:type {prefix}:{doctype}"]
        for key, value in record.items():
            field = by_name.get(key)
            if field is None or value is None or value == "":
                continue
            predicate = f"{prefix}:{local_name(field.fieldname)}"
            if field.is_reference:
                # A Link becomes a real edge into another subject, which is
                # what makes the discovered graph traversable rather than a
                # pile of strings.
                statements.append(f"{predicate} {prefix}:{local_name(str(value))}")
            else:
                statements.append(f'{predicate} "{escape_literal(str(value))}"')
        lines.append(" ;\n    ".join(statements) + " .")
        lines.append("")
    return "\n".join(lines) + "\n"


@dataclass
class SyncResult:
    """What one DocType sync did."""

    doctype: str
    namespace_code: int
    predicates: int
    landed: int
    skipped: int


def sync_doctype(
    client: ErpnextClient,
    doctype: str,
    server: str,
    token: str | None = None,
    limit: int = 100,
    prefix: str = "erpnext",
    namespace: str = DEFAULT_NAMESPACE,
    dry_run: bool = False,
) -> SyncResult:
    """Discover a DocType and land it, through graph-owl's public routes.

    **No step here knows what the DocType is.** The same call handles a sales
    invoice, an employee and a DocType invented this morning, because every
    domain-specific thing it needs — the vocabulary, the field list, the
    values — comes back from the instance.

    # Raises

    `ErpnextError` from discovery; `LoadError` from any graph-owl call.
    """
    # Imported here rather than at module scope so `erpnext.py` stays usable
    # for discovery alone, without a graph-owl server in the picture — which
    # is what makes `ontology_turtle` unit-testable with no network at all.
    from .loader import _request

    schema = client.schema(doctype)
    base = server.rstrip("/")

    declared = _request(
        f"{base}/namespaces",
        method="POST",
        token=token,
        body=json.dumps({"iri": namespace, "declaredBy": "connector:erpnext"}).encode("utf-8"),
    )
    if not isinstance(declared, dict) or "code" not in declared:
        raise ErpnextError(f"declaring `{namespace}` returned no code: {declared!r}")
    code = int(declared["code"])

    # **Every predicate before any document.** The import path refuses a flake
    # whose predicate is unregistered, so a document sent first is a document
    # entirely rejected — the same ordering the pack loader learned by running.
    #
    # `label` and `Class`/`Property` are the connector's own structural terms,
    # not fields: the ontology document asserts them, so they must be defined
    # like any other.
    for name in ["label"] + [f.fieldname for f in schema.fields]:
        field = next((f for f in schema.fields if f.fieldname == name), None)
        _request(
            f"{base}/predicates",
            method="POST",
            token=token,
            body=json.dumps(
                {
                    "namespace": code,
                    "name": local_name(name),
                    "valueType": field.value_type if field else VALUE_TYPE_STRING,
                    "many": False,
                }
            ).encode("utf-8"),
        )

    records = client.records(schema, limit=limit)
    documents = [
        (f"erpnext-{local_name(doctype).lower()}-ontology", ontology_turtle(schema, prefix, namespace)),
        (f"erpnext-{local_name(doctype).lower()}", records_turtle(schema, records, prefix, namespace)),
    ]

    landed = skipped = 0
    for source, turtle in documents:
        query = f"source={source}&format=turtle"
        if dry_run:
            query += "&dryRun=true"
        outcome = _request(
            f"{base}/graph/import/rdf?{query}",
            method="POST",
            token=token,
            body=turtle.encode("utf-8"),
            content_type="application/octet-stream",
        )
        if not isinstance(outcome, dict):
            raise ErpnextError(f"importing `{source}` returned {outcome!r}")
        landed += len(outcome.get("landed", []))
        skipped += len(outcome.get("skipped", []))

    return SyncResult(
        doctype=doctype,
        namespace_code=code,
        predicates=len(schema.fields) + 1,
        landed=landed,
        skipped=skipped,
    )
