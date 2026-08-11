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
from dataclasses import dataclass

from .loader import LoadError, _request


@dataclass
class ReconcileResult:
    """What one reconciliation run did."""

    pack_id: str
    evaluated: int
    found: int
    opened: int
    already_open: int


def run_findings(pack_id: str, server: str, token: str | None = None) -> ReconcileResult:
    """Ask the server to evaluate `pack_id`'s registered rules.

    Takes a pack **id**, not a directory — unlike `load_pack`, this reads
    nothing local. The rules were already registered by `load_pack`; this
    is the trigger, not the definition.

    # Raises

    `LoadError` if the server refuses or is unreachable.
    """
    response = _request(
        f"{server.rstrip('/')}/packs/{pack_id}/reconcile",
        method="POST",
        token=token,
    )
    if not isinstance(response, dict):
        raise LoadError(f"POST /packs/{pack_id}/reconcile returned an unexpected shape")

    return ReconcileResult(
        pack_id=str(response.get("pack", pack_id)),
        evaluated=int(response.get("evaluated", 0)),
        found=int(response.get("found", 0)),
        opened=int(response.get("opened", 0)),
        already_open=int(response.get("alreadyOpen", 0)),
    )


__all__ = ["ReconcileResult", "run_findings"]
