# Plan: OWL 2 RL Completion (Epic 95)

**Status**: Not started
**Depends on**: Epic 6 (the eight rules and the fixpoint that runs them)
**Unblocks**: Epic 17 (identity from keys), Epic 29 (lineage rollup)
**Crates**: `graph-owl-reasoning`, `graph-owl-ontology`

## Goal

Add the OWL 2 RL rules that Epics 17 and 29 would otherwise hand-code, and
leave out the ones a metadata catalog does not want.

## Why this is a separate epic from 6

Epic 6 builds the machinery: the rule model, semi-naive fixpoint, budgets,
explainability, the overlay. Adding a rule to a working reasoner is a bounded
change; building the reasoner is not. Splitting them means Epic 6 ships with
eight rules that demonstrably work rather than eighty that mostly do.

The eight in Epic 6 are also **the right eight to start with** — hierarchy,
transitivity, symmetry, inversion, domain, range, identity. A metadata ontology
is mostly taxonomy and mostly shallow, so they cover close to all the value.

## The decision this epic exists to apply

**Rules that derive facts belong in reasoning; rules that detect contradictions
belong in Epic 5.** OWL treats both as entailment. A catalog must not, because
a user who declares two disjoint classes and an asset in both wants to be
*told*, not to have the graph quietly become inconsistent and start deriving
everything.

That single line decides the scope below.

## In scope

| Rule | Buys | Who needs it |
|---|---|---|
| `owl:propertyChainAxiom` | Compose relationships — "column feeds column, column belongs to table ⟹ table feeds table" | **Epic 29.** This is lineage rollup, and without the rule it is hand-written traversal that drifts from the ontology |
| `owl:InverseFunctionalProperty` | Two subjects sharing an IFP value are the same thing | **Epic 17.** Identity from a shared identifier, without a bespoke matcher |
| `owl:FunctionalProperty` | At most one value — combined with `sameAs`, merges objects | Epic 17 |
| `owl:hasKey` | Key-based identity across sources, the multi-property IFP | Epic 17 |

Property chains are the one with teeth. They are also the one most likely to
explode: a chain over a transitive property on a wide estate derives a lot, and
Epic 6's budget is what stops that being discovered in production.

## Out of scope, with reasons

| Rule | Why not |
|---|---|
| `owl:disjointWith` | Detects a contradiction. **Epic 5**, as a violation |
| `owl:minCardinality` / `owl:maxCardinality` | Users want these *reported*, not materialised. **Epic 5** |
| `owl:someValuesFrom` / `owl:allValuesFrom` | Existential and universal restrictions. Rare in metadata ontologies, and the rules most likely to derive facts that surprise the person who wrote the axiom |
| The remaining ~60 RL rules | Datatype entailment, list axioms, and the machinery for OWL constructs no metadata ontology uses. Enumerated in the spec; deliberately unimplemented here |

## Acceptance criteria

- [ ] Each rule derives correctly, one test per rule with a positive and a
      negative case, as Epic 6 slice A established.
- [ ] A property chain over a transitive property terminates and respects the
      budget — the explosion case, tested rather than hoped for.
- [ ] An IFP derivation produces `sameAs`, and Epic 17 consumes it rather than
      re-deriving identity its own way. Asserted structurally.
- [ ] Every derived fact still carries its derivation chain (Epic 6 slice D).
- [ ] No rule derives anything when its triggering axiom triple is absent.

## Explicitly deferred

- **OWL 2 EL** → polynomial classification for very large taxonomies. Metadata
  ontologies are thousands of classes, not the hundreds of thousands EL exists
  for. Trigger: an ontology past ~50k classes.
- **OWL 2 QL** → query rewriting instead of materialisation. Epic 6 materialises
  deliberately, because explainability is a requirement and rewriting hides the
  derivation.
- **OWL 2 DL** → permanently not planned (Epic 6 decision 1).
