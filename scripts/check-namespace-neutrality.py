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



# ---------------------------------------------------------------- the console

# **The second half of the same rule, added after the console broke it three
# times.** `graph-owl-core` has been guarded since Epic 105; the *console* was
# not, and every GST constant that leaked into it was found by hand: a source
# list naming GSTR-2A and GSTR-2B, a thirteen-entry table of GST rule guidance,
# and a SPARQL builder with `PREFIX gst:` and nine `gst:` predicates hardcoded.
# Each would have rendered a healthcare or banking pack's data under GST's
# headings, or asked for GST's predicates against it and got nothing.
#
# **What is allowed, and why it is not a loophole.** The original `ui/`
# console's `features/packs/` was the sanctioned per-pack adapter registry
# — `packSurfaces.ts` stated the rule in its own doc comment ("nothing
# outside this object knows what GST is; adding a second domain is adding
# a second entry"). A file-format importer has to know its file's format.
# Everything *else* in the console must not.
#
# Plan 122a A11: `graphowl-app` replaced `ui/` as the live console (`ui/`
# is archived, see `_archived/README.md`) and has no equivalent adapter
# registry — its Packs screen (`src/routes/packs.tsx`) renders whatever an
# installed pack's id/description/term list says, with no per-pack branch
# anywhere, so there is nothing yet that needs the exemption. Kept as an
# empty tuple rather than deleted, so a future adapter registry has an
# obvious place to declare itself.
#
# Comments are exempt. Several of them exist precisely to record a domain term
# that used to be in the code and is not any more, and deleting that history to
# satisfy a grep would lose the reason.

CONSOLE_ROOT = ROOT / "graphowl-app" / "src"

# Where a pack's vocabulary is legitimately known — none yet, see above.
CONSOLE_EXEMPT_DIRS: tuple[str, ...] = ()

# A term is domain vocabulary if a pack declares it. Read from the packs
# themselves rather than listed here, so a new pack is covered the day it lands
# and this script never becomes a stale copy of somebody's ontology.
def pack_vocabulary() -> dict[str, set[str]]:
    """Each pack's prefix and the terms it registers, from `pack.toml`."""
    import tomllib

    vocab: dict[str, set[str]] = {}
    packs = ROOT / "packs"
    if not packs.is_dir():
        return vocab
    for manifest in sorted(packs.glob("*/pack.toml")):
        try:
            raw = tomllib.loads(manifest.read_text(encoding="utf-8"))
        except Exception:
            continue
        prefix = raw.get("pack", {}).get("prefix")
        if not prefix:
            continue
        # **At a token boundary, not as a substring.** `igst:`, `cgst:` and
        # `sgst:` in a TypeScript object literal all contain `gst:`, and the
        # first version of this check reported every one of them. A guard whose
        # failures are mostly false is one people learn to ignore.
        terms = {rf"(?<![A-Za-z0-9_]){re.escape(prefix)}:"}
        # Every finding label the pack registers — `gst:SupplierNotFiled` and
        # friends. These are the ones that were compiled into the console.
        for finding in raw.get("findings", []):
            label = finding.get("label")
            if label:
                terms.add(re.escape(label))
        vocab[prefix] = terms
    return vocab


def strip_comments(source: str) -> str:
    """Block and line comments removed, so a note *about* a term is not a use."""
    source = re.sub(r"/\*.*?\*/", "", source, flags=re.S)
    return re.sub(r"^\s*//.*$", "", source, flags=re.M)


def check_console() -> int:
    vocab = pack_vocabulary()
    if not vocab:
        # No packs on disk is not a pass — it means this half of the guard
        # matched nothing and would keep passing however much leaked.
        print(
            "no pack manifests found under packs/ — the console neutrality check\n"
            "matched nothing, which is not the same as finding nothing.",
            file=sys.stderr,
        )
        return 1

    if not CONSOLE_ROOT.is_dir():
        return 0

    violations: list[str] = []
    for path in sorted(CONSOLE_ROOT.rglob("*.ts")) + sorted(CONSOLE_ROOT.rglob("*.tsx")):
        rel = path.relative_to(ROOT).as_posix()
        if ".test." in path.name:
            continue
        if any(f"graphowl-app/src/{d}/" in rel for d in CONSOLE_EXEMPT_DIRS):
            continue
        body = strip_comments(path.read_text(encoding="utf-8"))
        for prefix, patterns in vocab.items():
            hits = sorted(
                {
                    match.group(0)
                    for pattern in patterns
                    for match in re.finditer(pattern, body)
                }
            )
            if hits:
                violations.append(f"    {rel}: {', '.join(hits)}")

    if violations:
        print(
            "A pack's vocabulary must not appear in console source.\n\n"
            + "\n".join(violations)
            + "\n\n"
            "The console renders whatever an installed pack declares; it must not\n"
            "know which pack that is. A term here means one domain's words would\n"
            "be rendered over another domain's data, or its predicates asked for\n"
            "and not found — which is what happened three times before this check\n"
            "existed.\n\n"
            "Declare it in `packs/<id>/pack.toml` (`[console.reconciliation]`,\n"
            "`[findings.guidance]`) and read it through `GET /packs/{pack}/console`.\n"
            "If the file really is a per-pack adapter, add its directory to\n"
            "CONSOLE_EXEMPT_DIRS in this script — graphowl-app has none yet.",
            file=sys.stderr,
        )
        return 1
    return 0


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

    # The console half of the same rule — see `check_console`.
    if check_console() != 0:
        return 1

    print(f"ok: {len(found)} namespace constants allowlisted, no pack vocabulary in the console")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
