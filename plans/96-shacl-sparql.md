# Plan: SHACL-SPARQL (Epic 96)

**Status**: **Slice A is ready.** Corrected 7 August 2026 — the previous
"blocked, full stop" status mis-cited which document gates it. Every term
Slice A's acceptance criteria need (`sh:SPARQLConstraint`,
`sh:SPARQLConstraintComponent`, `sh:SPARQLSelectValidator`,
`sh:SPARQLAskValidator`, `sh:sparql`, `sh:select`, `sh:ask`, `sh:parameter`,
`sh:labelTemplate`) is defined in **the 2017 SHACL Recommendation**
(`https://www.w3.org/TR/shacl/`, §5 "SPARQL-based Constraints" and §6
"SPARQL-based Constraint Components") — a W3C Recommendation, the highest
maturity tier, still the current in-force text and not superseded by
anything at Recommendation status. SHACL 1.2 SPARQL Extensions carries the
*same* vocabulary forward into a reorganised document, but that document is
a Working Draft and adds nothing this slice's acceptance criteria require —
so it is not what Slice A depends on. See "Status of the specification"
below for the verification.

Slice B (SHACL Rules) stays blocked, but not because a draft is immature —
verified directly against the 2017 REC's own scope statement, it defines
**no rule or derivation mechanism at all**: no `sh:rule`, no triple
production, nothing beyond validation. SHACL Rules has never had
Recommendation-track backing at any point; it lived in the non-normative
"SHACL Advanced Features" community note and is only now being formalised
into SHACL 1.2. So Slice B is blocked on spec existence, not spec
maturity, and additionally on the OWL-fixpoint composition question below.

**Depends on**: Epic 5 (SHACL Core), Epic 7 (SPARQL)
**Crates**: `graph-owl-constraint`, `graph-owl-query`

## Goal

An escape hatch for constraints the declarative shape language cannot express,
so an unusual rule stops being a feature request against Epic 5.

## Status of the specification, and what it means

**Two separate documents were being treated as one, and that was the error
in the earlier "blocked, full stop" status.** Checked directly, 7 August
2026:

- **`https://www.w3.org/TR/shacl/`** (2017) — **W3C Recommendation**, dated
  20 July 2017, still the current text (SHACL 1.2 has not reached
  Recommendation, so it has not superseded this document). §5 and §6 define
  SPARQL-based constraints and constraint components in full: property
  `sh:sparql` links a shape to a `sh:SPARQLConstraint` (whose `sh:select`
  or `sh:ask` carries the query); `sh:SPARQLConstraintComponent` plus
  `sh:parameter` and `sh:labelTemplate` define reusable, parameterised
  components with `sh:SPARQLSelectValidator`/`sh:SPARQLAskValidator` as the
  two validator shapes. Confirmed against the canonical machine-readable
  vocabulary (`https://www.w3.org/ns/shacl.ttl`), not just the prose, so
  there is no ambiguity about whether these are real, in-force terms.
- **`https://www.w3.org/TR/shacl12-sparql/`** (SHACL 1.2 SPARQL
  Extensions) — Working Draft, dated 6 August 2026, carries the *same*
  vocabulary forward plus genuinely new material (SPARQL node expressions,
  SPARQL-based rules). SHACL 1.2 Core, `https://www.w3.org/TR/shacl12-core/`,
  is also Working Draft, dated 3 August 2026.

Item 1 already fully answers what Slice A needs, at Recommendation maturity.
Item 2 is where the genuinely new, still-unstable material lives — and Slice
B (rules) is the only thing this plan wants from it. Building Slice A
against the 2017 REC is not "starting before Candidate Recommendation" — the
2017 document is *past* CR, at the terminal maturity stage. **The blocker on
Slice A was a citation error, not a specification gap.**

## What the specification carries

Three things, and this project wants them in descending order of urgency:

1. **`sh:SPARQLConstraint`** (2017 REC §5) — an arbitrary SPARQL SELECT as a
   constraint, where each returned solution is a violation. The escape
   hatch proper.
2. **SPARQL-based constraint components** (2017 REC §6) — parameterised,
   reusable validators (`sh:SPARQLSelectValidator`, `sh:SPARQLAskValidator`).
   Without these the escape hatch becomes a pile of one-off queries nobody
   can maintain, which is how a good escape hatch turns into technical debt.
3. **SPARQL-based inference rules** — shapes-driven derivation. Not in the
   2017 REC at all (verified: it explicitly scopes itself to validation,
   with no rule or triple-derivation mechanism of any kind). Only defined
   today in SHACL 1.2 SPARQL Extensions, a Working Draft — this is Slice
   B's actual dependency, and the reason it stays blocked.

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

Split by slice, because the two now have genuinely different bases. Slice A
is grounded in a Recommendation and has no open architectural question — a
constraint validates and writes nothing to the overlay. Slice B has no
Recommendation-track spec to build against at all, plus the unresolved
composition question below once one exists.

### Slice A — SPARQL constraints (ready)

- [ ] A `sh:SPARQLConstraint` returning solutions produces one violation per
      solution, with the focus node named.
- [ ] A constraint component is definable once and used with different
      parameters.
- [ ] A constraint query exceeding the budget is truncated and *reported as
      truncated*, never reported as "no violations found".
- [ ] A constraint cannot read what its author cannot — asserted with two
      principals, as Epic 13 does for search.

### Slice B — SHACL Rules (blocked)

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

### Resolved by deferral, 28 July 2026 — spec citation corrected 7 August 2026

**SHACL *rules* do not ship until a Recommendation-track specification
defines them.** SHACL *constraints* ship first (against the 2017 REC, not
the 1.2 draft) and raise no composition question at all, because they
**validate** rather than derive — they write nothing into the overlay, so there
is nothing for OWL's fixpoint to compose with. The question only becomes live
when rules ship, and rules are not the first thing this epic delivers.

The reason to wait on rules specifically is unchanged from 28 July: the only
document that defines them is a Working Draft, and starting before Candidate
Recommendation is a decision to accept rework. Here that cuts unusually
sharply — **W3C may specify the composition ordering itself**, in which case
implementing a choice now means implementing the wrong one and calling it a
decision.

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
