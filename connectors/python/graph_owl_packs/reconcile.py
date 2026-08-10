"""Running a pack's finding rules — Epic 105 P5.

**A finding rule is a query plus a binding, and this module is the whole of
it.** It reads `[[findings]]` from the manifest, runs the `[[queries]]` entry
each one names, and turns every result row into a finding posted to
`POST /findings`.

Nothing here mentions tax, invoices, guests or vessels. That is the property
`plans/105-domain-neutrality.md` is about: the GST reconciliation and a
hospitality duplicate-guest rule are the same code over different files, and
adding a seventh domain adds no branch to this file.

**Why the evidence mapping is written out in the manifest rather than inferred
from variable names.** A variable is named for whoever reads the query;
a predicate is named by the ontology. `?claimed` is not `gst:taxAmount`, and a
runtime that guessed would file evidence citing a predicate that does not
exist — evidence a reviewer cannot follow back into the graph, which is worse
than no evidence at all.

stdlib only, for the reason the loader beside it is: a pack runtime is not a
place to acquire an HTTP dependency.
"""

from __future__ import annotations

import json
from dataclasses import dataclass
from datetime import date
from pathlib import Path

from .loader import LoadError, _request
from .manifest import Manifest, ManifestError


class ReconcileError(RuntimeError):
    """A rule could not be evaluated. **Always raised, never skipped** — a rule
    that silently produced nothing is indistinguishable from a clean run, and
    an operator would file a return on it."""


@dataclass
class ReconcileResult:
    """What one reconciliation run did."""

    pack_id: str
    evaluated: int
    found: int
    opened: int
    already_open: int


def _term(value: str) -> str:
    """The plain value behind a SPARQL result term.

    Results come back in the query surface's own rendering — `<iri>` for an
    IRI and `"literal"` for a literal. A finding's subject must be the bare
    IRI, because that is what a console links and what a later query joins on;
    a subject carrying angle brackets matches nothing.
    """
    if len(value) >= 2 and value.startswith("<") and value.endswith(">"):
        return value[1:-1]
    if len(value) >= 2 and value.startswith('"') and value.endswith('"'):
        return value[1:-1]
    return value


def _trigrams(value: str, n: int) -> set[str]:
    """The n-grams of a value, padded so short strings still have some.

    Padding matters at this length: an 15-character identifier has 13 trigrams
    and a 3-character one has exactly 1, so without padding the shortest
    values compare as all-or-nothing.
    """
    padded = f"{'\x02' * (n - 1)}{value}{'\x03' * (n - 1)}"
    return {padded[i : i + n] for i in range(len(padded) - n + 1)}


def similarity(left: str, right: str, strategy: str, n: int) -> float:
    """How alike two values are, on 0.0–1.0.

    **Jaccard over n-grams, which is what makes a transposition visible.** Two
    identifiers with a pair of characters swapped share almost every n-gram
    but differ in a few, where an exact comparison sees only "not equal" and a
    prefix comparison sees only the part before the swap.

    `exact` is offered too, and is not pointless: it lets a rule express
    "these must be identical" through the same mechanism rather than through a
    second one.

    # Raises

    `ReconcileError` for a strategy this runtime does not implement — a
    misspelled strategy that silently scored 0.0 would drop every row and
    report a clean run.
    """
    if strategy == "exact":
        return 1.0 if left == right else 0.0
    if strategy != "ngram":
        raise ReconcileError(
            f"unknown similarity strategy '{strategy}' — a rule that silently "
            f"scored nothing would report a clean reconciliation"
        )
    if n < 1:
        raise ReconcileError("an n-gram size must be at least 1")
    if left == right:
        # Short-circuited so identical values are exactly 1.0 rather than
        # 1.0-within-floating-point, which an `at_most` bound would then
        # sometimes admit and sometimes not.
        return 1.0

    a, b = _trigrams(left, n), _trigrams(right, n)
    union = a | b
    return len(a & b) / len(union) if union else 0.0


def _passes_similarity(rule: dict, row: dict, label: str) -> bool:
    """Whether a row survives its rule's similarity band, if it has one."""
    band = rule.get("similarity")
    if not band:
        return True

    for side in ("left", "right"):
        variable = str(band.get(side, ""))
        if variable not in row:
            raise ReconcileError(
                f"rule '{label}' compares similarity on variable "
                f"'{variable}', which the query does not bind"
            )

    score = similarity(
        _term(str(row[str(band["left"])])),
        _term(str(row[str(band["right"])])),
        str(band.get("strategy", "ngram")),
        int(band.get("n", 3)),
    )
    # A band rather than a single threshold. The upper bound is the
    # load-bearing half: without it, every *correctly* matched pair scores 1.0
    # and is reported as a suspected typo, which is the finding that makes a
    # reviewer stop trusting the queue.
    return float(band.get("at_least", 0.0)) <= score <= float(band.get("at_most", 1.0))


def _as_date(value: str, label: str) -> date:
    """An ISO-8601 date, or a loud failure.

    **Never silently treated as absent.** A malformed date quietly becoming
    "no second event" would turn a data-quality problem into a fabricated
    compliance finding — the worst direction for this system to be wrong in,
    because the finding looks exactly like a real one.
    """
    try:
        return date.fromisoformat(value)
    except ValueError as unreadable:
        raise ReconcileError(
            f"rule '{label}' compares dates, and '{value}' is not an ISO-8601 "
            f"date — a date the runtime cannot read must not be guessed at"
        ) from unreadable


def _passes_span(rule: dict, row: dict, label: str) -> bool:
    """Whether a row survives its rule's time-span condition, if it has one.

    **Why this is here and not in the query.** Measured against the real
    engine: `xsd:date` subtraction, `date + duration`, and even `date > date`
    all evaluate to unbound — it has no date support in expressions. ISO-8601
    *strings* compare correctly, since their lexicographic order is their
    chronological order, which is enough for "which provision was in force"
    but not for "how many days apart". A day count needs calendar arithmetic.

    So the query does the join and the runtime does the arithmetic — the same
    split as `_passes_similarity`, for the same reason: each layer does what
    it is actually good at.
    """
    span = rule.get("span")
    if not span:
        return True

    start_var, end_var = str(span.get("from", "")), str(span.get("to", ""))
    if start_var not in row:
        raise ReconcileError(
            f"rule '{label}' measures a span from variable '{start_var}', "
            f"which the query does not bind"
        )

    if end_var not in row:
        # Both readings are legitimate, so the rule states which it means: an
        # absent second event can be the whole problem (an invoice nobody
        # paid), or it can mean the process has not reached that step yet.
        return str(span.get("when_missing", "ignore")) == "finding"

    start = _as_date(_term(str(row[start_var])), label)
    end = _as_date(_term(str(row[end_var])), label)
    # Strictly greater: "within 180 days" includes the 180th, and a rule that
    # fired on it would accuse a taxpayer on the last day they were compliant.
    return (end - start).days > int(span.get("exceeds_days", 0))


def _query_text(manifest: Manifest, name: str) -> str:
    for query in manifest.queries:
        if query.get("name") == name:
            path = manifest.directory / str(query.get("path", ""))
            try:
                return path.read_text(encoding="utf-8")
            except OSError as missing:
                raise ReconcileError(
                    f"rule query '{name}' names {path}, which cannot be read: {missing}"
                ) from missing
    raise ReconcileError(
        f"no [[queries]] entry named '{name}' — a rule pointing at a query "
        f"that does not exist would report a clean reconciliation"
    )


def run_findings(
    directory: Path,
    server: str,
    token: str | None = None,
) -> ReconcileResult:
    """Evaluate every `[[findings]]` rule and record what they conclude.

    # Raises

    `ManifestError` if the pack cannot be read, `ReconcileError` if a rule is
    unevaluable, and `LoadError` if the server refuses.
    """
    manifest = Manifest.load(directory)

    findings: list[dict] = []
    for rule in manifest.findings:
        query_name = rule.get("query")
        if not query_name:
            # A rule with no query is a declaration of intent — the label and
            # citation exist, the derivation does not. Skipped rather than
            # refused, because that is a legitimate half-built pack; the count
            # of *evaluated* rules is what reports it honestly.
            continue

        rows = _run_query(server, token, _query_text(manifest, str(query_name)))
        subject_var = str(rule.get("subject", ""))
        findings.extend(_rows_to_findings(manifest, rule, subject_var, rows))

    evaluated = sum(1 for rule in manifest.findings if rule.get("query"))
    if not findings:
        # Nothing posted rather than an empty batch: a no-op write in the
        # audit log of every scheduled run is how a log stops being read.
        return ReconcileResult(manifest.id, evaluated, 0, 0, 0)

    recorded = _request(
        f"{server.rstrip('/')}/findings",
        method="POST",
        token=token,
        body=json.dumps({"findings": findings}).encode("utf-8"),
    )
    if not isinstance(recorded, dict):
        raise LoadError("POST /findings returned an unexpected shape")
    return ReconcileResult(
        manifest.id,
        evaluated,
        len(findings),
        int(recorded.get("opened", 0)),
        int(recorded.get("alreadyOpen", 0)),
    )


def _run_query(server: str, token: str | None, query: str) -> list[dict]:
    answered = _request(
        f"{server.rstrip('/')}/sparql",
        method="POST",
        token=token,
        body=json.dumps({"query": query}).encode("utf-8"),
    )
    if not isinstance(answered, dict):
        raise LoadError("POST /sparql returned an unexpected shape")
    return list(answered.get("rows", []))


def _rows_to_findings(
    manifest: Manifest,
    rule: dict,
    subject_var: str,
    rows: list[dict],
) -> list[dict]:
    built = []
    label = str(rule.get("label", ""))
    for row in rows:
        if not _passes_similarity(rule, row, label):
            continue
        if not _passes_span(rule, row, label):
            continue
        if subject_var not in row:
            raise ReconcileError(
                f"rule '{rule.get('label')}' names subject variable "
                f"'{subject_var}', which the query does not bind — most often "
                f"a query edited to rename a variable, leaving the rule "
                f"pointing at nothing"
            )
        subject = _term(str(row[subject_var]))
        evidence = [
            {
                "subject": subject,
                "predicate": str(binding.get("predicate", "")),
                "value": _term(str(row[str(binding.get("var", ""))])),
            }
            for binding in rule.get("evidence", [])
            # An unbound OPTIONAL is not an error — it is often *why* the
            # finding exists. What must not happen is dropping the finding.
            if str(binding.get("var", "")) in row
        ]
        built.append(
            {
                "pack": manifest.id,
                "label": label,
                "subject": subject,
                "summary": str(rule.get("summary", "")),
                "governedBy": str(rule.get("governed_by", "")),
                "evidence": evidence,
            }
        )
    return built


__all__ = [
    "ReconcileError",
    "ReconcileResult",
    "run_findings",
    "similarity",
    "ManifestError",
]
