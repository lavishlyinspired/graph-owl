"""Triggering a pack's reconciliation — Epic 105 P5b.

**This module used to evaluate finding rules — n-gram similarity, date-span
arithmetic, SPARQL row-to-finding construction, the whole runtime.** That
was a Python matcher/scorer, and `.claude/docs/referencePlans/markdown/CA-GST/
25-graphowl-intelligence-platform.md` decision 4 is explicit that this
project does not have one: *"no Python matcher/scorer... Rust owns
deterministic graph intelligence, Python orchestrates."* The rule evaluator
is now native, in `graph-owl-resolution::rule_match` and
`Catalog::reconcile_pack` (`plans/105b-native-reconcile-engine.md`), reached
over `POST /packs/{pack}/reconcile`. `graph_owl_packs.loader` registers a
pack's rules there once, at install time (`_register_finding_rules`); this
module only ever asks the server to run them.

stdlib only, for the reason every other module here is: a pack runtime is
not a place to acquire an HTTP dependency.
"""

from __future__ import annotations

import json
from dataclasses import dataclass, field

from .loader import LoadError, _request


@dataclass
class ReconcileResult:
    """What one reconciliation run did."""

    pack_id: str
    evaluated: int
    found: int
    opened: int
    already_open: int
    #: What each rule concluded, and whether it could conclude at all —
    #: `{label, governedBy, status, found, unmet}` per rule, straight from the
    #: engine.
    #:
    #: `evaluated` above is a count, and a count cannot tell a rule that
    #: checked and was satisfied from one whose input data was absent. Both
    #: contribute zero findings, and a consumer showing only totals renders
    #: them identically as "no issues" — opposite claims. Defaults to empty so
    #: a server predating this field still parses.
    rules: list[dict] = field(default_factory=list)


def run_findings(
    pack_id: str,
    server: str,
    token: str | None = None,
    graphs: list[str] | None = None,
) -> ReconcileResult:
    """Ask the server to evaluate `pack_id`'s registered rules.

    Takes a pack **id**, not a directory — unlike `load_pack`, this reads
    nothing local. The rules were already registered by `load_pack`; this
    is the trigger, not the definition.

    `graphs` names the named graphs this run may read. **A caller that
    reconciles one slice of the estate must pass them.** Without a scope the
    rules read the whole store, and a rule will report a conclusion about a
    slice whose data was never supplied because a different slice supplied it.
    Omitted means the whole store, which is what every caller had before
    scoping existed.

    # Raises

    `LoadError` if the server refuses or is unreachable.
    """
    response = _request(
        f"{server.rstrip('/')}/packs/{pack_id}/reconcile",
        method="POST",
        token=token,
        body=json.dumps({"graphs": list(graphs)}).encode() if graphs else None,
    )
    if not isinstance(response, dict):
        raise LoadError(f"POST /packs/{pack_id}/reconcile returned an unexpected shape")

    return ReconcileResult(
        pack_id=str(response.get("pack", pack_id)),
        evaluated=int(response.get("evaluated", 0)),
        found=int(response.get("found", 0)),
        opened=int(response.get("opened", 0)),
        already_open=int(response.get("alreadyOpen", 0)),
        rules=list(response.get("rules") or []),
    )


__all__ = ["ReconcileResult", "run_findings"]
