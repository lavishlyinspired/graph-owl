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

Both are idempotent, so **loading a pack twice is a no-op rather than an
error**. That is the normal case: a demo script runs repeatedly, and a loader
that failed the second time would make a reload a failure.

stdlib only (`urllib`), for the same reason the OCR worker's endpoint client
is: a loader is not a place to acquire an HTTP dependency, and the reference
applications that drive it may import nothing else.
"""

from __future__ import annotations

import json
import urllib.error
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
        with urllib.request.urlopen(request) as response:
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
    except json.JSONDecodeError as not_json:
        raise LoadError(f"{method} {url} returned a non-JSON response") from not_json


def load_pack(
    directory: Path,
    server: str,
    token: str | None = None,
    dry_run: bool = False,
) -> LoadResult:
    """Declare the pack's namespace, then import every document it lists.

    `dry_run` reaches the server — it parses, validates against the live
    shapes graph and reports what *would* land, writing nothing. That is a
    genuinely different check from validating the manifest locally, and it is
    the one worth having: a pack whose ontology violates a shape is a pack
    that will half-load, and finding that out before it does is the point.

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

    results: list[DocumentResult] = []
    for document in manifest.documents:
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

    return LoadResult(
        pack_id=manifest.id,
        namespace_code=int(declared["code"]),
        documents=results,
    )
