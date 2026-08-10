"""The discovering connector, against a real local double of Frappe's API.

**The DocType used here is deliberately not an invoice.** A test written
around "Sales Invoice" would prove the connector handles the one shape
somebody had in mind; `Rescue Mission` proves it handles a schema nobody
designed for — which is the actual claim.

No ERPNext data appears anywhere in this file: the double answers with a
made-up DocType in Frappe's *shape*. That shape is Frappe's MIT-licensed
interface, not ERPNext's GPL-3.0 content — see `plans/00l-build-vs-adopt.md`.
"""

from __future__ import annotations

import json
import threading
from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, HTTPServer
from urllib.parse import parse_qs, urlparse

import pytest
from graph_owl_packs.erpnext import (
    DocTypeSchema,
    ErpnextClient,
    ErpnextError,
    Field,
    escape_literal,
    local_name,
    ontology_turtle,
    records_turtle,
    sync_doctype,
)

# A DocType this platform was certainly not designed around, in Frappe's shape.
SCHEMA = {
    "data": {
        "name": "Rescue Mission",
        "fields": [
            {"fieldname": "mission_code", "label": "Mission Code", "fieldtype": "Data"},
            {"fieldname": "layout_only", "label": "Details", "fieldtype": "Section Break"},
            {"fieldname": "vessel", "label": "Vessel", "fieldtype": "Link"},
            {"fieldname": "souls_aboard", "label": "Souls Aboard", "fieldtype": "Int"},
            {"fieldname": "notes", "label": "Notes", "fieldtype": "Text"},
        ],
    }
}

RECORDS = {
    "data": [
        {
            "name": "RM-0001",
            "mission_code": "ALPHA",
            "vessel": "MV Resolute",
            "souls_aboard": 14,
            "notes": 'He said "north", then nothing',
        },
        {
            "name": "RM-0002",
            "mission_code": "BRAVO",
            "vessel": None,
            "souls_aboard": "",
            "notes": "line one\nline two",
        },
    ]
}


def _handler(received: list[dict], schema_body, records_body):
    class Scripted(BaseHTTPRequestHandler):
        def _respond(self, body, status=200):
            encoded = json.dumps(body).encode("utf-8")
            self.send_response(status)
            self.send_header("content-type", "application/json")
            self.end_headers()
            self.wfile.write(encoded)

        def do_GET(self):  # noqa: N802
            parsed = urlparse(self.path)
            received.append(
                {
                    "method": "GET",
                    "path": parsed.path,
                    "query": {k: v[0] for k, v in parse_qs(parsed.query).items()},
                    "auth": self.headers.get("authorization"),
                }
            )
            if parsed.path.startswith("/api/resource/DocType/"):
                if schema_body is None:
                    self._respond({"data": {"nope": True}})
                else:
                    self._respond(schema_body)
            else:
                self._respond(records_body)

        def do_POST(self):  # noqa: N802
            parsed = urlparse(self.path)
            length = int(self.headers.get("content-length", 0))
            raw = self.rfile.read(length) if length else b""
            received.append(
                {
                    "method": "POST",
                    "path": parsed.path,
                    "query": {k: v[0] for k, v in parse_qs(parsed.query).items()},
                    "raw": raw,
                }
            )
            if parsed.path == "/namespaces":
                self._respond({"code": 1026, "iri": "x", "declaredBy": "y"})
            elif parsed.path == "/predicates":
                self._respond({})
            else:
                self._respond({"landed": ["erpnext:RM-0001"], "skipped": [], "rejected": []})

        def log_message(self, *args):
            pass

    return Scripted


@contextmanager
def instance(schema_body=SCHEMA, records_body=RECORDS):
    received: list[dict] = []
    server = HTTPServer(("127.0.0.1", 0), _handler(received, schema_body, records_body))
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_port}", received
    finally:
        server.shutdown()
        thread.join()


# ── discovery ────────────────────────────────────────────────────────────


def test_a_doctype_nobody_designed_for_is_discovered_whole():
    with instance() as (url, _):
        schema = ErpnextClient(url).schema("Rescue Mission")

    assert schema.name == "Rescue Mission"
    assert [f.fieldname for f in schema.fields] == [
        "mission_code",
        "vessel",
        "souls_aboard",
        "notes",
    ]


def test_layout_fields_are_dropped_rather_than_imported_and_ignored():
    # A predicate defined for a section break is a permanent registry entry
    # that means nothing — and the registry has no removal path.
    with instance() as (url, _):
        schema = ErpnextClient(url).schema("Rescue Mission")

    assert all(f.fieldtype != "Section Break" for f in schema.fields)


def test_a_link_field_becomes_a_reference_and_everything_else_a_string():
    # The difference between a graph you can traverse and one you can only
    # read: a `Link` has to become a real edge, not a string that happens to
    # look like a name.
    with instance() as (url, _):
        schema = ErpnextClient(url).schema("Rescue Mission")

    by_name = {f.fieldname: f for f in schema.fields}
    assert by_name["vessel"].value_type == 0, "a Link is a Ref"
    assert by_name["souls_aboard"].value_type == 1
    assert by_name["mission_code"].value_type == 1


def test_a_response_that_is_not_frappe_shaped_is_a_typed_error():
    # Better to fail than to import an empty vocabulary that looks like a
    # DocType with no fields.
    with instance(schema_body=None) as (url, _):
        with pytest.raises(ErpnextError, match="data.fields"):
            ErpnextClient(url).schema("Rescue Mission")


def test_an_unreachable_instance_names_the_instance():
    with pytest.raises(ErpnextError, match="unreachable"):
        ErpnextClient("http://127.0.0.1:1").schema("Rescue Mission")


def test_a_token_is_sent_when_configured_and_omitted_when_not():
    with instance() as (url, received):
        ErpnextClient(url, "key", "secret").schema("Rescue Mission")
    assert received[0]["auth"] == "token key:secret"

    with instance() as (url, received):
        ErpnextClient(url).schema("Rescue Mission")
    assert received[0]["auth"] is None


def test_records_are_fetched_by_the_discovered_field_list_not_a_wildcard():
    # `fields=["*"]` returns child-table rows and internal columns this
    # connector has no predicates for, so every record would carry values
    # that could not be landed.
    with instance() as (url, received):
        client = ErpnextClient(url)
        client.records(client.schema("Rescue Mission"))

    asked = json.loads(received[-1]["query"]["fields"])
    assert asked[0] == "name", "the record's own identity is always needed"
    assert "mission_code" in asked
    assert "*" not in asked


# ── local names and escaping ─────────────────────────────────────────────


def test_a_name_with_spaces_or_punctuation_becomes_iri_safe():
    assert local_name("Sales Invoice") == "Sales_Invoice"
    assert local_name("ACC-SINV-2026-00001") == "ACC-SINV-2026-00001"
    assert local_name("a/b c") == "a_b_c"


def test_an_empty_or_whitespace_name_still_yields_something_usable():
    # An empty local name would produce a bare-prefix IRI, which is a
    # different subject from every record and silently merges them.
    assert local_name("   ") == "_"
    assert local_name("") == "_"


def test_a_backslash_is_escaped_once_not_twice():
    # **The classic way this function is written wrong**: escaping quotes
    # before backslashes double-escapes every backslash the quote-escaping
    # just introduced.
    assert escape_literal(r"a\b") == r"a\\b"
    assert escape_literal('say "hi"') == 'say \\"hi\\"'
    assert escape_literal("one\ntwo") == "one\\ntwo"


# ── emission ─────────────────────────────────────────────────────────────


def schema_object() -> DocTypeSchema:
    return DocTypeSchema(
        name="Rescue Mission",
        fields=(
            Field("mission_code", "Mission Code", "Data"),
            Field("vessel", "Vessel", "Link"),
            Field("souls_aboard", "Souls Aboard", "Int"),
            Field("notes", "Notes", "Text"),
        ),
    )


def test_the_ontology_names_the_doctype_and_every_field():
    turtle = ontology_turtle(schema_object(), "erpnext", "https://example.org/erp#")

    assert "erpnext:Rescue_Mission rdf:type erpnext:Class" in turtle
    for field in ("mission_code", "vessel", "souls_aboard", "notes"):
        assert f"erpnext:{field} rdf:type erpnext:Property" in turtle


def test_a_record_becomes_a_typed_subject_with_its_values():
    turtle = records_turtle(
        schema_object(), RECORDS["data"], "erpnext", "https://example.org/erp#"
    )

    assert "erpnext:RM-0001 rdf:type erpnext:Rescue_Mission" in turtle
    assert 'erpnext:mission_code "ALPHA"' in turtle
    assert "erpnext:vessel erpnext:MV_Resolute" in turtle, "a Link is an edge, not a literal"
    assert 'erpnext:souls_aboard "14"' in turtle


def test_an_absent_or_empty_value_is_omitted_rather_than_written_blank():
    # "not recorded" and "recorded as blank" are different facts, and a graph
    # that cannot tell them apart cannot answer a question about missing data
    # — which is most of what a reconciliation asks.
    turtle = records_turtle(
        schema_object(), RECORDS["data"], "erpnext", "https://example.org/erp#"
    )

    second = turtle.split("erpnext:RM-0002")[1]
    assert "erpnext:vessel" not in second, "a None Link must not be written"
    assert "erpnext:souls_aboard" not in second, "an empty string must not be written"
    assert 'erpnext:mission_code "BRAVO"' in second, "and the present ones still land"


def test_emitted_turtle_escapes_values_that_would_otherwise_break_it():
    # A quote or newline in user-entered text would end the literal early and
    # produce a document the importer rejects wholesale — one bad note losing
    # every record in the batch.
    turtle = records_turtle(
        schema_object(), RECORDS["data"], "erpnext", "https://example.org/erp#"
    )

    assert '\\"north\\"' in turtle
    assert "line one\\nline two" in turtle
    assert "\n" not in turtle.split('erpnext:notes "')[1].split('"')[0]


def test_a_record_with_no_name_is_skipped_rather_than_given_a_blank_subject():
    turtle = records_turtle(
        schema_object(), [{"mission_code": "GHOST"}], "erpnext", "https://example.org/erp#"
    )

    assert "GHOST" not in turtle


# ── the sync, end to end against both doubles ────────────────────────────


def test_sync_declares_then_defines_then_imports():
    # The ordering the pack loader learned by running: a document imported
    # before its predicates exist is a document entirely rejected.
    with instance() as (erp_url, erp_calls):
        with instance() as (owl_url, owl_calls):
            result = sync_doctype(ErpnextClient(erp_url), "Rescue Mission", owl_url)

    posts = [c["path"] for c in owl_calls if c["method"] == "POST"]
    assert posts[0] == "/namespaces"
    first_import = posts.index("/graph/import/rdf")
    assert all(p == "/predicates" for p in posts[1:first_import])
    assert result.namespace_code == 1026
    assert result.doctype == "Rescue Mission"
    assert erp_calls, "the instance was actually queried"


def test_sync_defines_a_predicate_for_every_field_plus_label():
    with instance() as (erp_url, _):
        with instance() as (owl_url, owl_calls):
            result = sync_doctype(ErpnextClient(erp_url), "Rescue Mission", owl_url)

    defined = [
        json.loads(c["raw"])["name"] for c in owl_calls if c["path"] == "/predicates"
    ]
    assert set(defined) == {"label", "mission_code", "vessel", "souls_aboard", "notes"}
    assert result.predicates == len(defined)


def test_sync_sends_the_link_field_as_a_reference_predicate():
    with instance() as (erp_url, _):
        with instance() as (owl_url, owl_calls):
            sync_doctype(ErpnextClient(erp_url), "Rescue Mission", owl_url)

    by_name = {
        json.loads(c["raw"])["name"]: json.loads(c["raw"])
        for c in owl_calls
        if c["path"] == "/predicates"
    }
    assert by_name["vessel"]["valueType"] == 0
    assert by_name["mission_code"]["valueType"] == 1


def test_sync_lands_the_ontology_and_the_records_under_separate_sources():
    # Separate import graphs so the schema can be reloaded without touching
    # the data, and either can be dropped alone.
    with instance() as (erp_url, _):
        with instance() as (owl_url, owl_calls):
            sync_doctype(ErpnextClient(erp_url), "Rescue Mission", owl_url)

    sources = [
        c["query"]["source"] for c in owl_calls if c["path"] == "/graph/import/rdf"
    ]
    assert len(sources) == 2
    assert len(set(sources)) == 2
    assert any("ontology" in s for s in sources)


def test_a_dry_run_asks_the_server_not_to_write():
    with instance() as (erp_url, _):
        with instance() as (owl_url, owl_calls):
            sync_doctype(ErpnextClient(erp_url), "Rescue Mission", owl_url, dry_run=True)

    imports = [c for c in owl_calls if c["path"] == "/graph/import/rdf"]
    assert imports and all(c["query"].get("dryRun") == "true" for c in imports)
