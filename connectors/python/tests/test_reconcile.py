"""The reconciliation runner — Epic 105 P5.

**A finding rule is a query plus a binding.** The runner owns no domain
knowledge at all: it reads a `[[findings]]` entry, runs the `[[queries]]` entry
it names, and turns each row into a finding. Nothing in it mentions tax,
invoices or any other subject matter, which is the whole point — the same code
runs a hospitality duplicate-guest rule.

The double here answers SPARQL and collects findings, so the tests can assert
what the runner *sends* rather than what a server chose to store.
"""

from __future__ import annotations

import json
import sys
import threading
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from graph_owl_packs.reconcile import ReconcileError, run_findings  # noqa: E402


class _Server:
    """A graph-owl double: answers `/sparql`, records `POST /findings`."""

    def __init__(self, rows: list[dict], columns: list[str]) -> None:
        self.rows = rows
        self.columns = columns
        self.recorded: list[dict] = []
        self.queries: list[str] = []
        outer = self

        class Handler(BaseHTTPRequestHandler):
            def do_POST(self) -> None:  # noqa: N802
                length = int(self.headers.get("content-length", 0))
                body = json.loads(self.rfile.read(length) or b"{}")
                if self.path == "/sparql":
                    outer.queries.append(body["query"])
                    payload = {"columns": outer.columns, "rows": outer.rows}
                else:
                    outer.recorded.extend(body["findings"])
                    payload = {"opened": len(body["findings"]), "alreadyOpen": 0}
                raw = json.dumps(payload).encode()
                self.send_response(200)
                self.send_header("content-type", "application/json")
                self.send_header("content-length", str(len(raw)))
                self.end_headers()
                self.wfile.write(raw)

            def log_message(self, *args: object) -> None:
                pass

        self._http = HTTPServer(("127.0.0.1", 0), Handler)
        threading.Thread(target=self._http.serve_forever, daemon=True).start()
        self.url = f"http://127.0.0.1:{self._http.server_port}"

    def close(self) -> None:
        self._http.shutdown()


@pytest.fixture
def pack(tmp_path: Path) -> Path:
    """A pack with one finding rule, in a domain nobody would build for.

    Deliberately not GST: if the runner works for lifeboat inspections it is
    reading the manifest rather than recognising a domain.
    """
    (tmp_path / "queries").mkdir()
    (tmp_path / "queries" / "overdue.sparql").write_text(
        "SELECT ?vessel ?name ?due WHERE { ?vessel a x:Vessel }\n"
    )
    (tmp_path / "pack.toml").write_text(
        """
[pack]
id = "maritime"
namespace = "https://example.org/maritime#"
prefix = "mar"

[[queries]]
name = "overdue"
path = "queries/overdue.sparql"
label = "Inspections past due"

[[findings]]
label = "mar:InspectionOverdue"
summary = "A vessel whose safety inspection lapsed"
governed_by = "mar:SafetyCode3"
query = "overdue"
subject = "vessel"
evidence = [
  { predicate = "mar:name", var = "name" },
  { predicate = "mar:dueDate", var = "due" },
]
"""
    )
    return tmp_path


def test_each_result_row_becomes_one_finding(pack: Path) -> None:
    server = _Server(
        rows=[
            {"vessel": "<https://example.org/maritime#MV-1>", "name": '"Resolute"', "due": '"2026-01-04"'},
            {"vessel": "<https://example.org/maritime#MV-2>", "name": '"Tenacious"', "due": '"2026-02-11"'},
        ],
        columns=["vessel", "name", "due"],
    )
    try:
        result = run_findings(pack, server.url)
    finally:
        server.close()

    assert result.evaluated == 1
    assert len(server.recorded) == 2
    assert {f["subject"] for f in server.recorded} == {
        "https://example.org/maritime#MV-1",
        "https://example.org/maritime#MV-2",
    }


def test_a_finding_carries_the_citation_and_the_predicates_its_evidence_came_from(
    pack: Path,
) -> None:
    """The property that makes a finding reviewable rather than an assertion."""
    server = _Server(
        rows=[{"vessel": "<https://example.org/maritime#MV-1>", "name": '"Resolute"', "due": '"2026-01-04"'}],
        columns=["vessel", "name", "due"],
    )
    try:
        run_findings(pack, server.url)
    finally:
        server.close()

    finding = server.recorded[0]
    assert finding["governedBy"] == "mar:SafetyCode3"
    assert finding["pack"] == "maritime"
    assert finding["evidence"] == [
        {"subject": "https://example.org/maritime#MV-1", "predicate": "mar:name", "value": "Resolute"},
        {"subject": "https://example.org/maritime#MV-1", "predicate": "mar:dueDate", "value": "2026-01-04"},
    ]


def test_a_clean_run_records_nothing_rather_than_posting_an_empty_batch(pack: Path) -> None:
    """A reconciliation that finds nothing is the outcome everyone wants.

    Posting an empty batch would put a no-op write in the audit log of every
    scheduled run, which is how a log stops being read.
    """
    server = _Server(rows=[], columns=["vessel", "name", "due"])
    try:
        result = run_findings(pack, server.url)
    finally:
        server.close()

    assert result.evaluated == 1
    assert result.found == 0
    assert server.recorded == []


def test_a_rule_naming_a_query_that_does_not_exist_fails_loudly(tmp_path: Path) -> None:
    """**Not silently skipped.** A typo'd query name that produced no findings
    reads exactly like a clean reconciliation, and an operator would file a
    return on it."""
    (tmp_path / "pack.toml").write_text(
        """
[pack]
id = "maritime"
namespace = "https://example.org/maritime#"
prefix = "mar"

[[findings]]
label = "mar:InspectionOverdue"
summary = "s"
governed_by = "mar:SafetyCode3"
query = "overdeu"
subject = "vessel"
evidence = [{ predicate = "mar:name", var = "name" }]
"""
    )
    with pytest.raises(ReconcileError, match="overdeu"):
        run_findings(tmp_path, "http://127.0.0.1:1")


def test_a_rule_whose_subject_variable_is_not_in_the_results_fails_loudly(pack: Path) -> None:
    """The same class of failure one level down, and the more likely one: a
    query edited to rename a variable leaves the rule pointing at nothing."""
    server = _Server(rows=[{"name": '"Resolute"'}], columns=["name"])
    try:
        with pytest.raises(ReconcileError, match="vessel"):
            run_findings(pack, server.url)
    finally:
        server.close()


def test_a_row_missing_an_optional_evidence_binding_still_yields_a_finding(pack: Path) -> None:
    """An unbound `OPTIONAL` is not an error — it is often *why* the finding
    exists. What must not happen is the finding being dropped for it."""
    server = _Server(
        rows=[{"vessel": "<https://example.org/maritime#MV-1>", "name": '"Resolute"'}],
        columns=["vessel", "name", "due"],
    )
    try:
        run_findings(pack, server.url)
    finally:
        server.close()

    assert len(server.recorded) == 1
    assert [e["predicate"] for e in server.recorded[0]["evidence"]] == ["mar:name"]


def test_a_pack_with_no_finding_rules_is_a_clean_no_op(tmp_path: Path) -> None:
    (tmp_path / "pack.toml").write_text(
        """
[pack]
id = "maritime"
namespace = "https://example.org/maritime#"
prefix = "mar"
"""
    )
    result = run_findings(tmp_path, "http://127.0.0.1:1")

    assert result.evaluated == 0
    assert result.found == 0
