# Plan: SHACL-SPARQL (Epic 96)

**Status**: Not started — **blocked on the specification stabilising**
**Depends on**: Epic 5 (SHACL Core), Epic 7 (SPARQL)
**Crates**: `graph-owl-constraint`, `graph-owl-query`

## Goal

An escape hatch for constraints the declarative shape language cannot express,
so an unusual rule stops being a feature request against Epic 5.

## Status of the specification, and what it means

**SHACL 1.2 SPARQL Extensions is a W3C Working Draft, 24 July 2026.** SHACL 1.2
Core is also a Working Draft.

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

## Explicitly deferred

- **SHACL Advanced Features beyond these three** → node expressions, target
  types. Revisit when something concrete needs them.
- **SHACL-JS** → running JavaScript inside the validator. No.
