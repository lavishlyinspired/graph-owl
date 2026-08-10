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


@pytest.fixture
def similarity_pack(tmp_path: Path) -> Path:
    """A rule that keeps only rows whose two bindings are *nearly* equal.

    The domain-neutral shape of "one digit was transposed": a plain join finds
    candidate pairs, and a similarity threshold separates a typo from two
    genuinely different values that happen to collide on the other fields.
    """
    (tmp_path / "queries").mkdir()
    (tmp_path / "queries" / "pairs.sparql").write_text("SELECT ?a ?left ?right WHERE { ?a a x:P }\n")
    (tmp_path / "pack.toml").write_text(
        """
[pack]
id = "maritime"
namespace = "https://example.org/maritime#"
prefix = "mar"

[[queries]]
name = "pairs"
path = "queries/pairs.sparql"
label = "Candidate pairs"

[[findings]]
label = "mar:CallSignTransposition"
summary = "Two records whose call signs differ by what looks like a typo"
governed_by = "mar:IdentityPolicy"
query = "pairs"
subject = "a"
evidence = [
  { predicate = "mar:callSign", var = "left" },
  { predicate = "mar:reportedCallSign", var = "right" },
]

[findings.similarity]
strategy = "ngram"
n = 3
left = "left"
right = "right"
at_least = 0.4
at_most = 0.999
"""
    )
    return tmp_path


def test_only_near_identical_rows_become_findings(similarity_pack: Path) -> None:
    server = _Server(
        rows=[
            # A transposition: two characters swapped. Similar, not identical.
            {"a": "<x:1>", "left": '"GBRX1234ZZ"', "right": '"GBRX1234ZR"'},
            # Genuinely different values that collided on the join.
            {"a": "<x:2>", "left": '"GBRX1234ZZ"', "right": '"QQQQ9999AA"'},
        ],
        columns=["a", "left", "right"],
    )
    try:
        run_findings(similarity_pack, server.url)
    finally:
        server.close()

    assert [f["subject"] for f in server.recorded] == ["x:1"]


def test_an_exact_match_is_not_a_transposition(similarity_pack: Path) -> None:
    """**The upper bound is the load-bearing half.** Identical values are the
    matched pair, not a near-miss — without `at_most` every correctly matched
    record would be reported as a suspected typo, which is the finding that
    makes a reviewer stop trusting the queue."""
    server = _Server(
        rows=[{"a": "<x:1>", "left": '"GBRX1234ZZ"', "right": '"GBRX1234ZZ"'}],
        columns=["a", "left", "right"],
    )
    try:
        run_findings(similarity_pack, server.url)
    finally:
        server.close()

    assert server.recorded == []


def test_a_similarity_rule_naming_a_variable_the_query_does_not_bind_fails_loudly(
    similarity_pack: Path,
) -> None:
    server = _Server(rows=[{"a": "<x:1>", "left": '"AAA"'}], columns=["a", "left"])
    try:
        with pytest.raises(ReconcileError, match="right"):
            run_findings(similarity_pack, server.url)
    finally:
        server.close()


@pytest.fixture
def span_pack(tmp_path: Path) -> Path:
    """A rule about the time between two events.

    **Why this is a runtime filter and not a `FILTER` in the query.** Measured
    against the real engine: `xsd:date` subtraction, `date + duration`, and
    even `date > date` all evaluate to unbound — it has no date support in
    expressions. ISO-8601 *strings* compare correctly (lexicographic order is
    chronological order), which is enough for "which provision was in force"
    but not for "how many days apart". A day count needs real calendar
    arithmetic, so the runner does it.
    """
    (tmp_path / "queries").mkdir()
    (tmp_path / "queries" / "unpaid.sparql").write_text(
        "SELECT ?a ?start ?end WHERE { ?a a x:P }\n"
    )
    (tmp_path / "pack.toml").write_text(
        """
[pack]
id = "maritime"
namespace = "https://example.org/maritime#"
prefix = "mar"

[[queries]]
name = "unpaid"
path = "queries/unpaid.sparql"
label = "Overdue"

[[findings]]
label = "mar:CertificateLapsed"
summary = "More than 180 days between the two events"
governed_by = "mar:SafetyCode3"
query = "unpaid"
subject = "a"
evidence = [
  { predicate = "mar:issuedAt", var = "start" },
  { predicate = "mar:renewedAt", var = "end" },
]

[findings.span]
from = "start"
to = "end"
exceeds_days = 180
when_missing = "finding"
"""
    )
    return tmp_path


def test_a_span_longer_than_the_limit_is_a_finding(span_pack: Path) -> None:
    server = _Server(
        rows=[{"a": "<x:1>", "start": '"2026-07-15"', "end": '"2027-03-12"'}],
        columns=["a", "start", "end"],
    )
    try:
        run_findings(span_pack, server.url)
    finally:
        server.close()

    assert [f["subject"] for f in server.recorded] == ["x:1"]


def test_a_span_inside_the_limit_is_not(span_pack: Path) -> None:
    server = _Server(
        rows=[{"a": "<x:1>", "start": '"2026-07-04"', "end": '"2026-07-24"'}],
        columns=["a", "start", "end"],
    )
    try:
        run_findings(span_pack, server.url)
    finally:
        server.close()

    assert server.recorded == []


def test_exactly_the_limit_is_inside_it(span_pack: Path) -> None:
    """**A boundary a statute cares about.** "Within 180 days" includes the
    180th; a rule that fired on it would accuse a compliant taxpayer on the
    last day they were still compliant."""
    server = _Server(
        rows=[{"a": "<x:1>", "start": '"2026-01-01"', "end": '"2026-06-30"'}],
        columns=["a", "start", "end"],
    )
    try:
        run_findings(span_pack, server.url)
    finally:
        server.close()

    assert server.recorded == [], "2026-01-01 to 2026-06-30 is exactly 180 days"


def test_one_day_past_the_limit_is_outside_it(span_pack: Path) -> None:
    server = _Server(
        rows=[{"a": "<x:1>", "start": '"2026-01-01"', "end": '"2026-07-01"'}],
        columns=["a", "start", "end"],
    )
    try:
        run_findings(span_pack, server.url)
    finally:
        server.close()

    assert len(server.recorded) == 1, "181 days"


def test_a_missing_end_is_a_finding_when_the_rule_says_so(span_pack: Path) -> None:
    """**The case a "days elapsed" column cannot express at all.** There is no
    second event, so there is no delta to threshold — and an invoice nobody
    ever paid is precisely what the rule exists to catch."""
    server = _Server(rows=[{"a": "<x:1>", "start": '"2026-07-26"'}], columns=["a", "start", "end"])
    try:
        run_findings(span_pack, server.url)
    finally:
        server.close()

    assert len(server.recorded) == 1
    assert server.recorded[0]["evidence"] == [
        {"subject": "x:1", "predicate": "mar:issuedAt", "value": "2026-07-26"}
    ], "and it carries what it does have, so a reviewer sees which case it is"


def test_a_missing_end_is_ignored_when_the_rule_says_that_instead(tmp_path: Path) -> None:
    """`when_missing` is stated per rule rather than assumed, because both
    readings are legitimate: an absent second event can be the whole problem,
    or it can simply mean the process has not reached that step yet."""
    (tmp_path / "queries").mkdir()
    (tmp_path / "queries" / "unpaid.sparql").write_text("SELECT ?a ?start ?end WHERE { ?a a x:P }\n")
    (tmp_path / "pack.toml").write_text(
        """
[pack]
id = "maritime"
namespace = "https://example.org/maritime#"
prefix = "mar"

[[queries]]
name = "unpaid"
path = "queries/unpaid.sparql"
label = "Overdue"

[[findings]]
label = "mar:CertificateLapsed"
summary = "s"
governed_by = "mar:SafetyCode3"
query = "unpaid"
subject = "a"
evidence = [{ predicate = "mar:issuedAt", var = "start" }]

[findings.span]
from = "start"
to = "end"
exceeds_days = 180
when_missing = "ignore"
"""
    )
    server = _Server(rows=[{"a": "<x:1>", "start": '"2026-07-26"'}], columns=["a", "start", "end"])
    try:
        run_findings(tmp_path, server.url)
    finally:
        server.close()

    assert server.recorded == []


def test_a_date_the_runner_cannot_read_fails_loudly(span_pack: Path) -> None:
    """Not silently treated as missing — a malformed date that quietly became
    "no second event" would turn a data-quality problem into a fabricated
    compliance finding."""
    server = _Server(
        rows=[{"a": "<x:1>", "start": '"15/07/2026"', "end": '"2027-03-12"'}],
        columns=["a", "start", "end"],
    )
    try:
        with pytest.raises(ReconcileError, match="15/07/2026"):
            run_findings(span_pack, server.url)
    finally:
        server.close()


def _span_pack_with(tmp_path: Path, when_missing: str, as_of: str | None = None) -> Path:
    (tmp_path / "queries").mkdir(exist_ok=True)
    (tmp_path / "queries" / "unpaid.sparql").write_text("SELECT ?a ?start ?end WHERE { ?a a x:P }\n")
    as_of_line = f'as_of = "{as_of}"' if as_of else ""
    (tmp_path / "pack.toml").write_text(
        f"""
[pack]
id = "maritime"
namespace = "https://example.org/maritime#"
prefix = "mar"

[[queries]]
name = "unpaid"
path = "queries/unpaid.sparql"
label = "Overdue"

[[findings]]
label = "mar:CertificateLapsed"
summary = "s"
governed_by = "mar:SafetyCode3"
query = "unpaid"
subject = "a"
evidence = [{{ predicate = "mar:issuedAt", var = "start" }}]

[findings.span]
from = "start"
to = "end"
exceeds_days = 180
when_missing = "{when_missing}"
{as_of_line}
"""
    )
    return tmp_path


def test_an_absent_second_event_can_be_judged_on_elapsed_time(tmp_path: Path) -> None:
    """**The correction `when_missing = "finding"` needed.**

    Treating every missing second event as a finding flags an invoice issued
    yesterday as overdue — a false accusation on data that is simply not due
    yet, and the fastest way to fill a queue with noise. `elapsed` measures
    from the first event to the reconciliation date instead, so "not yet due"
    and "overdue" stop being the same answer.
    """
    pack = _span_pack_with(tmp_path, "elapsed", as_of="2026-08-01")
    server = _Server(
        rows=[
            {"a": "<x:old>", "start": '"2020-07-12"'},   # years past
            {"a": "<x:fresh>", "start": '"2026-07-26"'},  # six days
        ],
        columns=["a", "start", "end"],
    )
    try:
        run_findings(pack, server.url)
    finally:
        server.close()

    assert [f["subject"] for f in server.recorded] == ["x:old"]


def test_as_of_makes_an_elapsed_rule_deterministic(tmp_path: Path) -> None:
    """A statutory reconciliation is run *as of* a filing date, not "now".

    Without it a fixture silently changes meaning as the calendar moves — the
    "not yet due" case here becomes overdue in January 2027, which would turn
    a passing test into a failing one on a date nobody chose.
    """
    pack = _span_pack_with(tmp_path, "elapsed", as_of="2027-06-01")
    server = _Server(rows=[{"a": "<x:fresh>", "start": '"2026-07-26"'}], columns=["a", "start", "end"])
    try:
        run_findings(pack, server.url)
    finally:
        server.close()

    assert len(server.recorded) == 1, "the same row, judged from a later date, is overdue"
