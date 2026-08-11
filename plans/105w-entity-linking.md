# Plan: entity linking — P9's first remaining gap

**Branch**: main (committed directly, per `CLAUDE.md`)
**Status**: Shipped 11 August 2026, continuing the same completion pass as
`105u`/`105v`.
**Crates**: `graph-owl-api` only (`LinkedEntity`, `Catalog::link_entity`).
No changes to `graph-owl-query`, `graph-owl-resolution`, or any search
crate.

## The gap, read precisely from `105j`

`105j`'s own closing line: "No entity linking. Resolving free text ('the
July invoices') to a seed `Sid` is a real, separate, string-matching-shaped
problem... that deserves its own scoping pass, not a hasty addition here.
`graph_context` takes an already-resolved `Sid`." This slice is that
scoping pass, sized to what `graph_context` actually needs: a mention in,
a candidate `Sid` out.

## Why this is not `resolve_entity` again

`Catalog::resolve_entity` (P10) already resolves free text to a candidate
— but to a **catalog asset**, scored against `fully_qualified_name`. An
invoice, a statutory provision, a guest — anything a pack's own data
names — is not a catalog asset and has no FQN. `LinkedEntity` resolves to
*any* graph subject, scored against whichever of its own literal
properties best matches, which is the shape `graph_context`'s own `seed:
Sid` parameter actually needs filled.

## What was built

- `LinkedEntity { subject: Sid, matched_value: String, score: f64 }`.
- `Catalog::link_entity(principal, query, limit)` — runs
  `SELECT ?s ?v WHERE { ?s ?p ?v . FILTER(isLiteral(?v)) }` through
  [`Self::sparql`] (not a new storage-layer scan), re-scores every literal
  with the same `rule_match::similarity`/`NGram { n: 3 }` `resolve_entity`
  already uses, then keeps the **best-scoring literal per subject** — a
  caller wants one seed candidate per entity, not one row per matching
  property.
- **Reuses `Catalog::sparql` deliberately**, the same "reuse an existing
  retrieval path, re-score" shape `resolve_entity` already established for
  catalog assets: authorization scoping (`scoped_facts`) and the query
  budget (`max_facts` truncation) both already live there, so a full
  literal scan — which has no predicate to push down, since every
  property is a candidate by design — is bounded the same way any other
  unbounded `Catalog::sparql` call already is, not by inventing a second
  budget.
- `Sid::from_iri(bare_term(...))` round-trips a bound `?s` term back to a
  graph `Sid` — the same established pair `finding_evidence_graph`'s own
  seed resolution already uses, not a new parsing path.

## The RED test

Two tests, both against `graph-owl-api`'s own `RecordingGraph` double (no
Postgres): the closer of two assets' own `dsc:name` literals ranks first
for a partial-text query, and a subject with two matching literal
properties appears once, at its best score, not once per property.

## Mutation report

`--in-diff`, `--lib`: **5 mutants, 3 caught, 1 unviable, 1 missed.** The
missed mutant (`>` → `>=` in the best-per-subject tie-break) is
documented in-code rather than force-tested: on an exact score tie
between two different literal values, the returned `score` is identical
regardless of which comparison wins, so the only thing a test could
observe is *which literal string* survives — arbitrary by design, not a
correctness question, and constructing a genuine trigram-similarity tie
between two different strings to prove it would test the similarity
formula's own symmetry, not this method's logic.

## What this deliberately does not do

- **No hybrid-search fusion.** `link_entity` is lexical/similarity
  matching only — combining it with Epic 31's embeddings and full-text
  ranking into one fused score is `105j`'s *other* named gap
  (`plans/105x-hybrid-search.md`), not this one.
- **No HTTP route or MCP tool.** Wiring a real caller is separate work
  once one needs it, the same posture `graph_context` itself already
  took toward its own missing route.
- **Does not dedupe near-duplicate subjects across packs** (e.g. the same
  invoice number appearing under two suppliers) — every literal match is
  a real, distinct graph subject; disambiguating "which one did the
  caller mean" is a downstream, caller-side judgment this method does not
  make on their behalf.
