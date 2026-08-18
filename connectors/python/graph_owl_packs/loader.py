"""Loading a domain pack into a running graph-owl — the platform's P1.

**The loader knows nothing about any domain**, and that is the whole property
under test. It declares whatever namespace the manifest names, imports
whatever documents it lists, and reports what happened. Hospitality, tax and
automotive differ only in the bytes it reads.

Two HTTP surfaces, both built for this:

- `POST /namespaces` — the pack's own vocabulary becomes real graph terms.
  Before Epic 105 the only way to have one was adding a constant to
  `graph-owl-core`.
- `POST /graph/import/rdf` — the pack's ontology, shapes and fixtures land in
  named import graphs, one per `source`, each independently removable.
- `POST /packs/{id}/finding-rules` — Epic 105 P5b
  (`plans/105b-native-reconcile-engine.md`): a pack's `[[findings]]` rules,
  registered so the *native* reconcile engine can evaluate them without ever
  parsing a manifest itself. This loader inlines each named query's SPARQL
  text; nothing downstream of this call ever reads a `.sparql` file again.

All three are idempotent, so **loading a pack twice is a no-op rather than an
error**. That is the normal case: a demo script runs repeatedly, and a loader
that failed the second time would make a reload a failure.

stdlib only (`urllib`), for the same reason the OCR worker's endpoint client
is: a loader is not a place to acquire an HTTP dependency, and the reference
applications that drive it may import nothing else.
"""

from __future__ import annotations

import json
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass
from pathlib import Path

from .manifest import Manifest


class LoadError(RuntimeError):
    """A pack could not be loaded. Always names the step and the server."""


@dataclass
class DocumentResult:
    """What one document's import did."""

    source: str
    landed: int
    skipped: int
    rejected: list[tuple[str, str]]


@dataclass
class LoadResult:
    """What loading a whole pack did."""

    pack_id: str
    namespace_code: int
    documents: list[DocumentResult]

    @property
    def landed(self) -> int:
        return sum(d.landed for d in self.documents)

    @property
    def skipped(self) -> int:
        return sum(d.skipped for d in self.documents)

    @property
    def rejected(self) -> list[tuple[str, str]]:
        return [r for d in self.documents for r in d.rejected]


def _request(
    url: str,
    *,
    method: str,
    token: str | None,
    body: bytes | None = None,
    content_type: str = "application/json",
) -> dict | list:
    request = urllib.request.Request(url, data=body, method=method)
    if body is not None:
        request.add_header("content-type", content_type)
    if token:
        request.add_header("authorization", f"Bearer {token}")
    try:
        with urllib.request.urlopen(request, timeout=5) as response:
            raw = response.read()
            return json.loads(raw) if raw else {}
    except urllib.error.HTTPError as refused:
        # The server speaks RFC 9457 problem+json, so the *detail* is worth
        # surfacing — "1 field failed validation" alone sends the reader to a
        # debugger, where the field name sends them to their manifest.
        detail = refused.read().decode("utf-8", errors="replace")
        raise LoadError(f"{method} {url} failed: HTTP {refused.code} {detail}") from refused
    except urllib.error.URLError as unreachable:
        raise LoadError(f"{method} {url} was unreachable: {unreachable.reason}") from unreachable
    except TimeoutError as timeout:
        raise LoadError(f"{method} {url} timed out") from timeout
    except json.JSONDecodeError as not_json:
        raise LoadError(f"{method} {url} returned a non-JSON response") from not_json


def load_pack(
    directory: Path,
    server: str,
    token: str | None = None,
    dry_run: bool = False,
    include_documents: bool = True,
) -> LoadResult:
    """Declare the pack's namespace, then import every document it lists.

    `dry_run` reaches the server — it parses, validates against the live
    shapes graph and reports what *would* land, writing nothing. That is a
    genuinely different check from validating the manifest locally, and it is
    the one worth having: a pack whose ontology violates a shape is a pack
    that will half-load, and finding that out before it does is the point.

    `include_documents=False` declares the namespace, predicates and glossary
    — the pack's *vocabulary* — without importing any `[[documents]]`. For a
    consumer composing with a pack rather than running its own copy of it
    (reco-now extending packs/gst, plans/119-architecture-audit.md): the
    native reconcile engine has no per-source data isolation, so a pack's own
    demo/reference fixtures loaded alongside a real consumer's data would be
    evaluated together and cross-contaminate that consumer's findings.

    # Raises

    `LoadError` if any step fails. A partial load is reported as a failure,
    not as a success with a short list.
    """
    manifest = Manifest.load(directory)
    base = server.rstrip("/")

    declared = _request(
        f"{base}/namespaces",
        method="POST",
        token=token,
        body=json.dumps(
            {"iri": manifest.namespace, "declaredBy": f"pack:{manifest.id}"}
        ).encode("utf-8"),
    )
    if not isinstance(declared, dict) or "code" not in declared:
        raise LoadError(
            f"declaring `{manifest.namespace}` returned no namespace code: {declared!r}"
        )

    # **Predicates before documents, and the order is not cosmetic.** The
    # import path refuses a flake whose predicate is not in the registry
    # (`reject_unregistered_predicates`), so a document imported first is a
    # document entirely rejected — found by running this against a real
    # server, not by reading the code.
    code = int(declared["code"])
    for predicate in manifest.predicates:
        _request(
            f"{base}/predicates",
            method="POST",
            token=token,
            body=json.dumps(
                {
                    "namespace": code,
                    "name": predicate.name,
                    "valueType": predicate.value_type,
                    "many": predicate.many,
                }
            ).encode("utf-8"),
        )

    # **The glossary, if the pack has one, before any document lands.**
    # `POST /ontology-packs` registers the pack's SKOS terms as an Epic 33
    # `OntologyPack` — the mechanism the console's Vocabulary browser already
    # reads (`GET /ontology-packs`). It is independent of the predicate
    # registry above (Epic 33's storage is its own tables), so ordering
    # relative to it does not matter for correctness; it is placed here
    # because it is still vocabulary setup, not data.
    if manifest.glossary and not dry_run:
        _register_glossary(base, manifest, token)

    results: list[DocumentResult] = []
    for document in manifest.documents if include_documents else ():
        path = directory / document.path
        if not path.is_file():
            raise LoadError(
                f"{manifest.id}: `{document.path}` is listed in pack.toml but does not exist"
            )
        query = f"source={document.source}&format={document.format}"
        if dry_run:
            query += "&dryRun=true"
        outcome = _request(
            f"{base}/graph/import/rdf?{query}",
            method="POST",
            token=token,
            body=path.read_bytes(),
            content_type="application/octet-stream",
        )
        if not isinstance(outcome, dict):
            raise LoadError(f"importing `{document.path}` returned {outcome!r}")
        results.append(
            DocumentResult(
                source=document.source,
                landed=len(outcome.get("landed", [])),
                skipped=len(outcome.get("skipped", [])),
                rejected=[tuple(r) for r in outcome.get("rejected", [])],
            )
        )

    # **Finding rules after documents, for the reason that order matters
    # everywhere else in this function: nothing downstream depends on it,
    # but registering a rule for a pack whose documents failed to load would
    # leave a rule pointing at a graph that was never populated.** SPARQL
    # text is inlined here, once — the native reconcile engine (Epic 105
    # P5b) never parses `pack.toml` or reads a `.sparql` file itself.
    if manifest.findings:
        _register_finding_rules(base, manifest, token)

    # **Every [[queries]] entry, not only the ones a [[findings]] rule
    # references.** `_register_finding_rules` above only ever registers a
    # query as a side effect of the rule pointing at it — a query meant to
    # be invoked directly (`provision-in-force`, bound to a caller-supplied
    # subject rather than run unconditionally) has no rule pointing at it
    # at all, so it would otherwise never reach the server. Epic 105 P106
    # Slice 4a.
    if manifest.queries:
        _register_pack_queries(base, manifest, token)

    return LoadResult(
        pack_id=manifest.id,
        namespace_code=int(declared["code"]),
        documents=results,
    )


def _register_glossary(base: str, manifest: Manifest, token: str | None) -> None:
    glossary = manifest.glossary
    assert glossary is not None  # checked by the caller

    path = manifest.directory / str(glossary["path"])
    if not path.is_file():
        raise LoadError(
            f"{manifest.id}: `glossary.path` names `{glossary['path']}`, which does not exist"
        )

    params = {
        "packId": manifest.id,
        "version": str(glossary["version"]),
        "sourceUrl": str(glossary["source_url"]),
        "licenceKind": str(glossary["licence_kind"]),
        "licenceName": str(glossary["licence_name"]),
    }
    if "licence_notice" in glossary:
        params["licenceNotice"] = str(glossary["licence_notice"])
    if "licence_contact" in glossary:
        params["licenceContact"] = str(glossary["licence_contact"])
    if glossary.get("acknowledge_licence"):
        params["acknowledgeLicence"] = "true"

    _request(
        f"{base}/ontology-packs?{urllib.parse.urlencode(params)}",
        method="POST",
        token=token,
        body=path.read_bytes(),
        content_type="text/turtle",
    )


def _query_text(manifest: Manifest, name: str) -> str:
    for query in manifest.queries:
        if query.get("name") == name:
            path = manifest.directory / str(query.get("path", ""))
            try:
                return path.read_text(encoding="utf-8")
            except OSError as missing:
                raise LoadError(
                    f"rule query '{name}' names {path}, which cannot be read: {missing}"
                ) from missing
    raise LoadError(
        f"no [[queries]] entry named '{name}' — a rule pointing at a query "
        f"that does not exist would silently never fire"
    )


def _camel_case_band(band: dict | None, keys: dict[str, str]) -> dict | None:
    """A `[findings.similarity]`/`[findings.span]` table, translated from
    the manifest's snake_case to the wire's camelCase.

    **Explicit key-by-key, not a generic snake→camel converter.** A few
    fixed keys are known and expected; a converter would silently pass
    through a typo'd key instead of it being caught the same way an
    unrecognised field anywhere else in this loader would be.
    """
    if not band:
        return None
    return {wire: band[toml] for toml, wire in keys.items() if toml in band}


def _register_finding_rules(base: str, manifest: Manifest, token: str | None) -> None:
    rules = []
    for rule in manifest.findings:
        query_name = rule.get("query")
        if not query_name:
            # A rule with no query is a declaration of intent, the same
            # legitimate half-built state `graph_owl_packs.reconcile` used
            # to read it as — skipped, not refused.
            continue
        rules.append(
            {
                "label": str(rule.get("label", "")),
                "summary": str(rule.get("summary", "")),
                "governedBy": str(rule.get("governed_by", "")),
                "query": _query_text(manifest, str(query_name)),
                "subjectVar": str(rule.get("subject", "")),
                "evidence": [
                    {"predicate": str(b.get("predicate", "")), "var": str(b.get("var", ""))}
                    for b in rule.get("evidence", [])
                ],
                "similarity": _camel_case_band(
                    rule.get("similarity"),
                    {
                        "strategy": "strategy",
                        "n": "n",
                        "left": "left",
                        "right": "right",
                        "at_least": "atLeast",
                        "at_most": "atMost",
                        "resolve_by": "resolveBy",
                    },
                ),
                "span": _camel_case_band(
                    rule.get("span"),
                    {
                        "from": "from",
                        "to": "to",
                        "exceeds_days": "exceedsDays",
                        "when_missing": "whenMissing",
                        "as_of": "asOf",
                    },
                ),
                # Epic 105 P10 (plans/119-architecture-audit.md §10): how
                # this rule ranks against a pack's other rules when more
                # than one fires on the same subject. `None` (sent as an
                # explicit `null`, matching similarity/span's own
                # convention above) for a rule that declares none.
                "priority": rule.get("priority"),
            }
        )

    if not rules:
        return

    _request(
        f"{base}/packs/{manifest.id}/finding-rules",
        method="POST",
        token=token,
        body=json.dumps({"rules": rules}).encode("utf-8"),
    )


def _register_pack_queries(base: str, manifest: Manifest, token: str | None) -> None:
    queries = [
        {"name": str(query["name"]), "query": _query_text(manifest, str(query["name"]))}
        for query in manifest.queries
    ]
    if not queries:
        return

    _request(
        f"{base}/packs/{manifest.id}/queries",
        method="POST",
        token=token,
        body=json.dumps({"queries": queries}).encode("utf-8"),
    )
