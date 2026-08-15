"""Loading a pack, against a real local HTTP double.

No mock: a real `http.server` on a real loopback port, the same "a real
server, not a placeholder" discipline the OCR worker's endpoint client and CLI
tests already use. What these pin is the *sequence* — declare the namespace,
then import each document in order, with the right query parameters — because
that sequence is the whole loader and every part of it is observable on the
wire.

`scripts/verify-pack-load.sh` runs the same packs against a real graph-owl.
These run in milliseconds and catch the wiring; that one catches everything
the wiring cannot.
"""

from __future__ import annotations

import json
import threading
from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlparse

import pytest
from graph_owl_packs import LoadError, load_pack

PACKS = Path(__file__).resolve().parents[3] / "packs"


def _handler(received: list[dict], fail_on: str | None):
    class Scripted(BaseHTTPRequestHandler):
        def do_POST(self):  # noqa: N802
            parsed = urlparse(self.path)
            length = int(self.headers.get("content-length", 0))
            raw = self.rfile.read(length) if length else b""
            received.append(
                {
                    "path": parsed.path,
                    "query": {k: v[0] for k, v in parse_qs(parsed.query).items()},
                    "raw": raw,
                    "auth": self.headers.get("authorization"),
                }
            )

            if fail_on and fail_on in parsed.path:
                self.send_response(500)
                self.end_headers()
                self.wfile.write(b'{"detail":"deliberate"}')
                return

            if parsed.path == "/namespaces":
                body = {"code": 1024, "iri": "x", "declaredBy": "y"}
            elif parsed.path.endswith("/finding-rules") or parsed.path.endswith("/queries"):
                body = {}
            elif parsed.path == "/ontology-packs":
                self.send_response(201)
                self.send_header("content-type", "application/json")
                self.end_headers()
                self.wfile.write(
                    json.dumps({"id": "9d1f...", "packId": "gst", "version": "1"}).encode(
                        "utf-8"
                    )
                )
                return
            else:
                # One subject per document, so `landed` counts are checkable.
                body = {"landed": ["gst:thing"], "skipped": [], "rejected": []}
            encoded = json.dumps(body).encode("utf-8")
            self.send_response(200)
            self.send_header("content-type", "application/json")
            self.end_headers()
            self.wfile.write(encoded)

        def log_message(self, *args):
            pass

    return Scripted


@contextmanager
def scripted_server(fail_on: str | None = None):
    received: list[dict] = []
    server = HTTPServer(("127.0.0.1", 0), _handler(received, fail_on))
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_port}", received
    finally:
        server.shutdown()
        thread.join()


@pytest.mark.parametrize("pack", ["hospitality", "gst"])
def test_a_pack_declares_its_namespace_before_importing_anything(pack):
    # **Order is the property, not an implementation detail.** A document
    # importing terms in a namespace the server has not been told about
    # resolves to nothing — the exact failure Epic 105 was written to fix.
    with scripted_server() as (url, received):
        load_pack(PACKS / pack, url)

    # **Six phases, and the order of all of them is load-bearing.** A
    # document imported before its namespace is declared resolves to nothing;
    # a document imported before its predicates are defined is rejected
    # wholesale by `reject_unregistered_predicates`; a finding rule or a
    # named query declared before its documents load would point at a graph
    # that was never populated. Found by running this against a real
    # server, not by reading the code. The glossary import (Epic 33
    # `OntologyPack`, present only for a pack that declares one) is
    # independent of the flake-import predicate registry, so it is placed
    # with the rest of vocabulary setup — after predicates, before any
    # document lands. `/packs/{pack}/queries` is the sixth phase (Epic 105
    # P106 Slice 4a) — every `[[queries]]` entry, registered after finding
    # rules for the identical reason finding rules come after documents:
    # nothing downstream depends on the order between the two, but a query
    # registered before its own documents load would name a graph never
    # populated.
    paths = [r["path"] for r in received]
    assert paths[0] == "/namespaces", f"the vocabulary must be declared first, got {paths}"

    first_import = paths.index("/graph/import/rdf")
    vocabulary_calls = paths[1:first_import]
    assert all(p in ("/predicates", "/ontology-packs") for p in vocabulary_calls), (
        f"predicates and the glossary must all be defined before the first import: {paths}"
    )
    assert vocabulary_calls.count("/ontology-packs") <= 1, paths

    imports = [p for p in paths[first_import:] if p == "/graph/import/rdf"]
    after_imports = paths[first_import + len(imports) :]
    assert paths[first_import : first_import + len(imports)] == imports
    possible_tails = [
        [],
        [f"/packs/{pack}/finding-rules"],
        [f"/packs/{pack}/queries"],
        [f"/packs/{pack}/finding-rules", f"/packs/{pack}/queries"],
    ]
    assert after_imports in possible_tails, (
        f"only finding-rules and/or queries, in that order, may follow the last import: {paths}"
    )


def test_a_pack_with_a_glossary_registers_it_as_an_ontology_pack():
    # Epic 33 already owns pack vocabulary lifecycle (versioning, licence,
    # overrides) — a domain pack's glossary is an `OntologyPack`, not a
    # parallel mechanism (the platform doc's decision 10). Without this call
    # the pack's terms are flakes a reviewer can query but never see in the
    # console's Vocabulary browser, which reads `GET /ontology-packs`.
    with scripted_server() as (url, received):
        load_pack(PACKS / "gst", url)

    calls = [r for r in received if r["path"] == "/ontology-packs"]
    assert len(calls) == 1, f"exactly one glossary import, got {received}"
    assert calls[0]["query"]["packId"] == "gst"
    assert calls[0]["query"]["licenceKind"] == "permissive"
    assert b"skos:prefLabel" in calls[0]["raw"], (
        "the glossary file itself, as the request body — not a path to it"
    )


def test_a_pack_with_no_glossary_table_registers_none():
    # Hospitality has no `[glossary]` — optional, not a silent no-op that
    # would be indistinguishable from a bug if every pack were expected to
    # have one.
    with scripted_server() as (url, received):
        load_pack(PACKS / "hospitality", url)

    assert not any(r["path"] == "/ontology-packs" for r in received), received


@pytest.mark.parametrize("pack", ["hospitality", "gst"])
def test_include_documents_false_skips_every_document_import(pack):
    # A consumer composing with a pack's *vocabulary* (namespace,
    # predicates, ontology) without wanting its demo/reference fixture
    # data — ext-apps/Reco/backend/app/main.py loading packs/gst is the
    # motivating case: reco-now's own reconciliation must not be
    # cross-contaminated by packs/gst's planted INV-1001..INV-2002
    # scenarios, since the native reconcile engine has no per-source
    # isolation (plans/119-architecture-audit.md's Slice-2 investigation).
    with scripted_server() as (url, received):
        result = load_pack(PACKS / pack, url, include_documents=False)

    assert not any(r["path"] == "/graph/import/rdf" for r in received), received
    assert result.documents == []
    assert result.landed == 0
    assert result.skipped == 0
    # Namespace and predicates are still real requests — the whole point is
    # vocabulary without data, not nothing at all.
    assert any(r["path"] == "/namespaces" for r in received), received
    assert any(r["path"] == "/predicates" for r in received), received


def test_include_documents_defaults_to_true_unchanged_from_before(pack="gst"):
    # The new parameter must not change any existing caller's behaviour by
    # default — every current call site (demo.sh, verify-pack-load.sh,
    # the CLI) calls load_pack with no opinion on this at all.
    with scripted_server() as (url, received):
        load_pack(PACKS / pack, url)

    assert any(r["path"] == "/graph/import/rdf" for r in received), received


@pytest.mark.parametrize("pack", ["hospitality", "gst"])
def test_every_document_is_imported_under_its_own_source(pack):
    with scripted_server() as (url, received):
        result = load_pack(PACKS / pack, url)

    sources = [
        r["query"]["source"] for r in received if r["path"] == "/graph/import/rdf"
    ]
    assert len(sources) == len(set(sources)), "two documents sharing a source overwrite"
    assert result.landed == len(sources)


def test_the_declared_namespace_is_the_packs_own():
    with scripted_server() as (url, received):
        load_pack(PACKS / "gst", url)

    declared = json.loads(received[0]["raw"])
    assert declared["iri"] == "https://graph-owl.dev/packs/gst#"
    assert declared["declaredBy"] == "pack:gst", (
        "provenance names the pack, so an operator can later tell where a "
        "stray prefix came from"
    )


def test_the_two_packs_declare_different_namespaces():
    # The neutrality property, observed on the wire rather than read out of
    # the manifests.
    with scripted_server() as (url, hospitality_calls):
        load_pack(PACKS / "hospitality", url)
    with scripted_server() as (url, gst_calls):
        load_pack(PACKS / "gst", url)

    assert (
        json.loads(hospitality_calls[0]["raw"])["iri"]
        != json.loads(gst_calls[0]["raw"])["iri"]
    )


def test_gst_registers_all_thirteen_finding_rules_in_one_call():
    # Epic 105 P5b: the native reconcile engine reads rules from the
    # registry, never from `pack.toml` — so every rule with a `query` must
    # reach the server, in one batch rather than one call per rule.
    #
    # **Stale at six since before Plan 108, fixed here rather than carried
    # forward.** `pack.toml` grew from six findings to thirteen across Plan
    # 108 (GSTR-1 as a third evidence source, five findings) and Plan 109
    # (SupplierPanMismatch) — this assertion was never updated for either.
    # Not something Plan 109 Slice 2 introduced, but it blocks the pre-PR
    # `pytest connectors/python/tests/` gate that slice's own acceptance
    # criteria names, so it is fixed alongside this slice.
    with scripted_server() as (url, received):
        load_pack(PACKS / "gst", url)

    calls = [r for r in received if r["path"] == "/packs/gst/finding-rules"]
    assert len(calls) == 1, f"one batched call, not one per rule: {received}"

    rules = json.loads(calls[0]["raw"])["rules"]
    assert len(rules) == 13, f"pack.toml declares thirteen findings, all with a query: {rules}"
    assert {r["label"] for r in rules} == {
        "gst:PotentialMismatch",
        "gst:AmountMismatch",
        "gst:ITCNotAvailable",
        "gst:Reversed",
        "gst:GstinTransposition",
        "gst:PaymentOverdue",
        "gst:SupplierNotFiled",
        "gst:Gstr1NotIn2b",
        "gst:MissingInBooks",
        "gst:BooksGstr1Mismatch",
        "gst:GoodsReceiptTiming",
        "gst:TaxHeadMismatch",
        "gst:SupplierPanMismatch",
    }


def test_gst_registers_its_query_not_referenced_by_any_finding_rule():
    # Epic 105 P106 Slice 4a: `provision-in-force` has no `[[findings]]`
    # rule pointing at it — it exists to be invoked directly, by name, with
    # a caller-supplied binding (`run_pack_query`). Without this call it
    # would never reach the server at all, since `_register_finding_rules`
    # only ever registers a query as a side effect of a rule referencing it.
    with scripted_server() as (url, received):
        load_pack(PACKS / "gst", url)

    calls = [r for r in received if r["path"] == "/packs/gst/queries"]
    assert len(calls) == 1, f"one batched call, not one per query: {received}"

    queries = json.loads(calls[0]["raw"])["queries"]
    names = {q["name"] for q in queries}
    assert "provision-in-force" in names, (
        f"the one query with no referencing finding rule: {names}"
    )
    provision_in_force = next(q for q in queries if q["name"] == "provision-in-force")
    assert "PREFIX gst:" in provision_in_force["query"]
    assert "{{invoice}}" in provision_in_force["query"], (
        "the real query text from provision-in-force.sparql, not a stand-in"
    )


def test_hospitality_registers_no_queries():
    # The negative that makes the positive case above about *this* pack's
    # own `[[queries]]`, not about every pack always sending the call.
    with scripted_server() as (url, received):
        load_pack(PACKS / "hospitality", url)

    assert not any(r["path"].endswith("/queries") for r in received), received


def test_the_query_text_is_inlined_not_a_path():
    # The whole point of registering rules server-side: the native engine
    # never reads a `.sparql` file, so the loader must have read it already.
    with scripted_server() as (url, received):
        load_pack(PACKS / "gst", url)

    rules = json.loads(
        next(r for r in received if r["path"] == "/packs/gst/finding-rules")["raw"]
    )["rules"]
    potential_mismatch = next(r for r in rules if r["label"] == "gst:PotentialMismatch")
    assert "PREFIX gst:" in potential_mismatch["query"]
    assert "gst:PurchaseInvoice" in potential_mismatch["query"], (
        "the real query text from missing-in-gstr2b.sparql, not a stand-in"
    )
    assert potential_mismatch["subjectVar"] == "invoice"
    assert potential_mismatch["governedBy"] == "gst:Section16-2-aa"


def test_similarity_and_span_bands_are_translated_to_camel_case():
    # `pack.toml` is snake_case (`at_least`, `when_missing`); the wire is
    # camelCase, matching every other route in this project. A loader that
    # forwarded the TOML keys verbatim would post a rule the server's
    # `#[serde(rename_all = "camelCase")]` deserializer silently ignored,
    # since an unrecognised key defaults to absent rather than erroring the
    # way a typo'd required field would.
    with scripted_server() as (url, received):
        load_pack(PACKS / "gst", url)

    rules = json.loads(
        next(r for r in received if r["path"] == "/packs/gst/finding-rules")["raw"]
    )["rules"]

    transposition = next(r for r in rules if r["label"] == "gst:GstinTransposition")
    assert transposition["similarity"]["atLeast"] == pytest.approx(0.40)
    assert transposition["similarity"]["atMost"] == pytest.approx(0.999)
    assert "at_least" not in transposition["similarity"]
    # Epic 105 P7's near-miss half (`plans/105g-...`) — the one field in this
    # band that resolves a subject rather than merely comparing a string, so
    # it carries a full IRI rather than the pack's usual curie shorthand.
    assert transposition["similarity"]["resolveBy"] == (
        "https://graph-owl.dev/packs/gst#supplierGstin"
    )

    overdue = next(r for r in rules if r["label"] == "gst:PaymentOverdue")
    assert overdue["span"]["exceedsDays"] == 180
    assert overdue["span"]["whenMissing"] == "elapsed"
    assert "exceeds_days" not in overdue["span"]

    # A rule with neither band round-trips with both keys present and null,
    # not simply absent — the server's own `#[serde(default)]` accepts
    # either, but an explicit `null` is what this loader actually sends.
    reversed_rule = next(r for r in rules if r["label"] == "gst:Reversed")
    assert reversed_rule["similarity"] is None
    assert reversed_rule["span"] is None


def test_hospitality_registers_no_finding_rules() -> None:
    # Its one finding has no `query` — a declaration of intent, the same
    # legitimate half-built state the runtime always read it as. A rule
    # with nothing to evaluate must not reach the server at all.
    with scripted_server() as (url, received):
        load_pack(PACKS / "hospitality", url)

    assert not any(r["path"].endswith("/finding-rules") for r in received), received


def test_finding_rules_are_registered_only_after_every_document_lands():
    with scripted_server() as (url, received):
        load_pack(PACKS / "gst", url)

    paths = [r["path"] for r in received]
    rule_call = paths.index("/packs/gst/finding-rules")
    assert all(p != "/packs/gst/finding-rules" for p in paths[:rule_call]), paths
    # `/packs/gst/queries` (Epic 105 P106 Slice 4a) is the only call that may
    # follow — every `[[queries]]` entry, registered after finding rules,
    # not before any document.
    assert paths[rule_call + 1 :] == ["/packs/gst/queries"], (
        f"only queries may follow finding-rules: {paths}"
    )


def test_a_dry_run_asks_the_server_not_to_write():
    with scripted_server() as (url, received):
        load_pack(PACKS / "gst", url, dry_run=True)

    imports = [r for r in received if r["path"] == "/graph/import/rdf"]
    assert imports, "there were no imports to check"
    assert all(r["query"].get("dryRun") == "true" for r in imports)


def test_a_real_run_does_not_ask_for_a_dry_one():
    # The negative: a loader that always sent `dryRun` would pass every test
    # above and never write anything.
    with scripted_server() as (url, received):
        load_pack(PACKS / "gst", url)

    assert all(
        "dryRun" not in r["query"]
        for r in received
        if r["path"] == "/graph/import/rdf"
    )


def test_a_token_reaches_every_call():
    # A pack load is admin-only on both surfaces. A token that reached the
    # namespace call but not the imports would half-load and fail confusingly.
    with scripted_server() as (url, received):
        load_pack(PACKS / "gst", url, token="secret-token")

    assert received, "no calls were made"
    assert all(r["auth"] == "Bearer secret-token" for r in received)


def test_a_failing_namespace_declaration_stops_the_load():
    # Nothing should be imported into a vocabulary the server does not know.
    with scripted_server(fail_on="/namespaces") as (url, received):
        with pytest.raises(LoadError, match="500"):
            load_pack(PACKS / "gst", url)

    assert len(received) == 1, "the load must stop at the failed declaration"


def test_a_failing_import_is_an_error_rather_than_a_short_result():
    # A partial load reported as success is the worst outcome available: the
    # operator sees a result and believes the pack is installed.
    with scripted_server(fail_on="/graph/import/rdf") as (url, _received):
        with pytest.raises(LoadError, match="500"):
            load_pack(PACKS / "gst", url)


def test_a_missing_document_is_named_before_any_http_call_for_it(tmp_path):
    directory = tmp_path / "broken"
    directory.mkdir()
    (directory / "pack.toml").write_text(
        """
[pack]
id = "broken"
namespace = "https://example.org/ns/broken#"
prefix = "broken"

[[documents]]
path = "absent.ttl"
source = "broken-absent"
""",
        encoding="utf-8",
    )

    with scripted_server() as (url, received):
        with pytest.raises(LoadError, match="absent.ttl"):
            load_pack(directory, url)

    assert len(received) == 1, "only the namespace declaration should have happened"


def test_an_unreachable_server_names_the_server():
    # The message an operator sees when they typo'd `--server`, which is the
    # single most likely failure of this command.
    with pytest.raises(LoadError, match="unreachable"):
        load_pack(PACKS / "gst", "http://127.0.0.1:1")
