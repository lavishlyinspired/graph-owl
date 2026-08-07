# Plan: SHACL-SPARQL (Epic 96)

**Status**: Not started — **blocked on the specification stabilising**
**Depends on**: Epic 5 (SHACL Core), Epic 7 (SPARQL)
**Crates**: `graph-owl-constraint`, `graph-owl-query`

## Goal

An escape hatch for constraints the declarative shape language cannot express,
so an unusual rule stops being a feature request against Epic 5.

## Status of the specification, and what it means

**SHACL 1.2 SPARQL Extensions is a W3C Working Draft.** Re-checked 7 August
2026 against `https://www.w3.org/TR/shacl12-sparql/`: still Working Draft,
now dated 6 August 2026 (was 24 July 2026 at the previous check — the
document has republished as a Working Draft again, not advanced track
stage). SHACL 1.2 Core is also a Working Draft, re-checked the same day
against `https://www.w3.org/TR/shacl12-core/`: Working Draft, dated 3
August 2026.

A Working Draft may change in ways that break an implementation. This epic is
therefore *planned but not scheduled*: the design below is stable enough to
reason about, and starting it before the spec reaches Candidate Recommendation
is a decision to accept rework. **The blocker is maturity, not capability.**

## What the specification carries

Three things, and this project wants them in descending order of urgency:

1. **`sh:SPARQLConstraint`** — an arbitrary SPARQL SELECT as a constraint,
   where each returned solution is a violation. The escape hatch proper.
2. **SPARQL-based constraint components** — parameterised, reusable validators
   (`sh:SPARQLSelectValidator`, `sh:SPARQLAskValidator`). Without these the
   escape hatch becomes a pile of one-off queries nobody can maintain, which is
   how a good escape hatch turns into technical debt.
3. **SPARQL-based inference rules** — shapes-driven derivation.

## The overlap that must be decided before any code

Item 3 derives triples. So does Epic 6. Both will produce overlapping facts on
a real catalog, and `00k-standards-conformance.md` decision 4 records the rule:

> **Derived facts carry which engine derived them**, and the reasoning overlay
> (`graph:reasoning`) holds both.

Epic 6's explainability requirement then covers both, because a derivation
chain that cannot say which engine produced a step is not an explanation — it
is a list.

**The reason this is decided here rather than discovered later**: if the two
engines ship without provenance on derived facts, retrofitting it means
re-deriving everything, and the person debugging a wrong inference has no way
to know which system to look at.

## Resolved decisions

1. **Constraint components before bare constraints in the API surface.** Both
   ship, but the documentation leads with components, because the shape of the
   examples decides what people write.
2. **A SHACL-SPARQL constraint runs under the same budget as everything else.**
   An arbitrary user-supplied query inside a validation pass is an unbounded
   query inside a bounded operation; Epic 7's `Tracker` applies.
3. **Authorization is not optional inside a user-supplied query.** Epic 13's
   predicate compiles into the constraint's SPARQL exactly as it does into a
   user query. A constraint that could read what its author cannot is a
   read-anything primitive wearing a validation costume.

## Acceptance criteria

- [ ] A `sh:SPARQLConstraint` returning solutions produces one violation per
      solution, with the focus node named.
- [ ] A constraint component is definable once and used with different
      parameters.
- [ ] A constraint query exceeding the budget is truncated and *reported as
      truncated*, never reported as "no violations found".
- [ ] A constraint cannot read what its author cannot — asserted with two
      principals, as Epic 13 does for search.
- [ ] A SHACL rule's derived facts record that SHACL derived them.

## Open: do SHACL-derived facts feed the OWL fixpoint?

**Raised 28 July 2026, unresolved.** This epic adds a **second inference
mechanism** producing facts into the same overlay as Epic 6's OWL rules, and the
plans do not say how the two compose. Provenance is handled — a SHACL rule's
output records that SHACL derived it — but provenance answers *where a fact came
from*, not *what the fact set is*.

Three orderings, giving three different answers:

1. **OWL to fixpoint, then SHACL once** — SHACL sees every OWL inference; OWL
   never sees SHACL's.
2. **SHACL once, then OWL to fixpoint** — the reverse.
3. **Both in one fixpoint** — the only order-independent option, and the only
   one where a SHACL rule can trigger an OWL rule that re-triggers the SHACL
   rule. It is also the one that can fail to terminate, since SHACL rules are
   not restricted the way OWL 2 RL is.

### Resolved by deferral, 28 July 2026

**SHACL *rules* do not ship until the specification stabilises.** SHACL
*constraints* ship first and raise no composition question at all, because they
**validate** rather than derive — they write nothing into the overlay, so there
is nothing for OWL's fixpoint to compose with. The question only becomes live
when rules ship, and rules are not the first thing this epic delivers.

The reason to wait is already in this plan: the specification is a Working Draft,
and starting before Candidate Recommendation is a decision to accept rework.
Here that cuts unusually sharply — **W3C may specify the composition ordering
itself**, in which case implementing a choice now means implementing the wrong
one and calling it a decision.

**If forced to choose before then: OWL to fixpoint, then SHACL once.** OWL 2 RL
is decidable in polynomial time and terminates; SHACL rules carry no such
guarantee, so putting them inside the fixpoint puts a non-terminating construct
inside a loop. It also gets the useful direction for free — a SHACL rule can
validate and act on derived facts, whereas OWL axioms have no reason to depend on
what a shape produced. The cost is order-dependence, which is acceptable *stated*
and unacceptable *discovered*.

**Revisit trigger**: the specification reaching Candidate Recommendation. At that
point (1) check whether the spec fixes the ordering, (2) if not, implement OWL
then SHACL, (3) record the choice here **and** in `06-engine-reasoning.md`, since
it is a property of the combined inference set rather than of either engine.

**Confirm before the trigger fires**: which W3C document actually owns rules.
This epic is scoped to SPARQL Extensions, and rule constructs have historically
lived in SHACL Advanced Features — a separate document on a separate track. A
trigger watching the wrong document would fire late, or never.

Option 3 is the semantically satisfying one and the one that needs a
termination argument before it is chosen. Options 1 and 2 are defensible if
**stated** — an order-dependent inference set that nobody documented is a system
where the same data yields different conclusions depending on an implementation
detail, and the explainability contract cannot survive that.

## Explicitly deferred

- **SHACL Advanced Features beyond these three** → node expressions, target
  types. Revisit when something concrete needs them.
- **SHACL-JS** → running JavaScript inside the validator. No.
