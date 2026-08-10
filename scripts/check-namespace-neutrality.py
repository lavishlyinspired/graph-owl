#!/usr/bin/env python3
"""Fail the build if a *domain* namespace is added to `graph-owl-core`.

Epic 105 DN-1, checklist item X.1 — the regression guard for a failure this
project has already shipped once.

`namespace::CUI`, `SNOMED_CT` and `RXNORM` were added to
`crates/graph-owl-core/src/flake.rs` as Rust constants so one domain's
ingestion work could use its own vocabulary. That was the only way to do it at
the time: `Sid::from_iri` scanned a fixed compile-time array, so an IRI in an
unregistered namespace resolved to nothing. It is no longer the only way — a
namespace is declared at runtime and stored in the `namespaces` table — and
the next domain that adds a constant here would be reintroducing per-domain
code into the one crate that must never have any.

**What this checks, and what it deliberately does not.** It is not a ban on
touching `namespace`: a genuinely *general* vocabulary — a new W3C standard,
a new RDF serialization's terms — belongs in the binary alongside `rdf:` and
`owl:`, and adding one is a legitimate change. What it bans is growth of the
allowlist without a human saying so. The allowlist below is the shipped set as
of Epic 105; adding to it is a deliberate, reviewable act, which is precisely
the property that was missing when three medical namespaces arrived.

Run from `scripts/gate.sh` and CI. Exits 1 on a violation, naming the constant.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FLAKE = ROOT / "crates" / "graph-owl-core" / "src" / "flake.rs"

# The shipped set as of Epic 105 (10 August 2026). Each is either graph-owl's
# own vocabulary or a general standard that any domain might use — none is
# specific to one industry.
#
# CUI, SNOMED_CT and RXNORM are here because they already shipped, not because
# they were the right call: they are the medical-domain constants whose arrival
# motivated this guard. They are grandfathered, not endorsed, and a new domain
# must use the runtime registry instead.
ALLOWED = {
    "UNSET",
    "DSC",
    "RDF",
    "RDFS",
    "XSD",
    "OWL",
    "SHACL",
    "SCHEMA",
    "DCTERMS",
    "DCAT",
    "PROV",
    "FOAF",
    "SKOS",
    # Grandfathered — see above.
    "CUI",
    "SNOMED_CT",
    "RXNORM",
    # Range markers, not vocabularies.
    "RUNTIME_START",
    "NOT_FOUND",
}

CONSTANT = re.compile(r"^\s*pub const ([A-Z][A-Z0-9_]*)\s*:\s*u16\s*=", re.M)


def main() -> int:
    if not FLAKE.exists():
        print(f"cannot find {FLAKE}", file=sys.stderr)
        return 1

    source = FLAKE.read_text(encoding="utf-8")
    # Only the `namespace` module's own constants, not every u16 in the file.
    start = source.find("pub mod namespace {")
    if start == -1:
        print("could not find `pub mod namespace` — has flake.rs been restructured?", file=sys.stderr)
        return 1
    end = source.find("\n}", start)
    block = source[start:end]

    found = set(CONSTANT.findall(block))
    added = sorted(found - ALLOWED)

    if added:
        print(
            "Domain namespaces must not be added to `graph-owl-core`.\n\n"
            f"  New namespace constant(s) in {FLAKE.relative_to(ROOT)}:\n"
            + "".join(f"    namespace::{name}\n" for name in added)
            + "\n"
            "A namespace for a domain is declared at runtime and stored in the\n"
            "`namespaces` table (`NamespaceRegistry::declare`), so a pack brings\n"
            "its own vocabulary with no code change. Adding a constant here puts\n"
            "one domain into the crate every domain shares — the exact failure\n"
            "`plans/105-domain-neutrality.md` was written to end.\n\n"
            "If this really is a *general* vocabulary (a new W3C standard, not an\n"
            "industry's), add it to ALLOWED in this script in the same commit, so\n"
            "the decision is reviewed rather than assumed.",
            file=sys.stderr,
        )
        return 1

    # The negative half: a guard that cannot fail is not a guard. If the
    # allowlist has drifted ahead of the source, the check is silently
    # matching nothing and would pass no matter what was added.
    missing = sorted(ALLOWED - found - {"UNSET", "RUNTIME_START", "NOT_FOUND"})
    if missing:
        print(
            "This check is not looking at what it thinks it is: the allowlist names\n"
            f"constants that no longer exist in {FLAKE.relative_to(ROOT)}:\n"
            + "".join(f"    namespace::{name}\n" for name in missing)
            + "\nRemove them from ALLOWED, or fix the parser.",
            file=sys.stderr,
        )
        return 1

    print(f"ok: {len(found)} namespace constants, all allowlisted")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
