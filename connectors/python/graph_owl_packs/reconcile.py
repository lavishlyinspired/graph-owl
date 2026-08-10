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
    for row in rows:
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
                "label": str(rule.get("label", "")),
                "subject": subject,
                "summary": str(rule.get("summary", "")),
                "governedBy": str(rule.get("governed_by", "")),
                "evidence": evidence,
            }
        )
    return built


__all__ = ["ReconcileError", "ReconcileResult", "run_findings", "ManifestError"]
