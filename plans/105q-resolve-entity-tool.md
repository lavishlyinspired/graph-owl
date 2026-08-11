# Plan: `resolve_entity()` — P10's seventh MCP intelligence tool

**Branch**: main (committed directly, per `CLAUDE.md`)
**Status**: Shipped 11 August 2026, continuing "complete P10 first" by
direct user instruction.

**Crates**: `graph-owl-api` (the new `Catalog::resolve_entity` method,
reusing `search_assets_for` and `graph_owl_resolution::rule_match::similarity`
unmodified), `graph-owl-mcp` (the tool), `graph-owl-server` (a
real-Postgres proof).

## The design question this had to answer first

Unlike `analytics()` and `run_rule()` — both narrower cousins of an
already-shipped Catalog method — `resolve_entity()` had **no existing
Catalog capability to wrap**. The platform doc names it as P10's
entity-linking primitive and points at "existing full-text search" plus
DN-2's similarity function as the pieces to assemble, without specifying
the shape.

**The design question that had to be answered before any code**: full-text
search (`search_assets_for`, already wired as the `search_assets` tool)
already exists. What does `resolve_entity()` add that would not make it a
duplicate of `search`?

**Answer: a normalized similarity score, not a relevance rank.**
Full-text search answers "what mentions this text" — its own ranking is
whatever the storage engine's query planner produces, not a number
comparable across different query lengths or usable as a confidence
threshold. Entity resolution asks a different question: "how alike is
this asset's name to the string I have", which needs an actual metric an
agent can reason about ("is 0.3 close enough to treat these as the same
entity, or do I need to ask a human"). `resolve_entity()` retrieves
candidates through the identical policy-filtered `search_assets_for` call
`search` already uses — no second search index — then re-scores each hit
with `graph_owl_resolution::rule_match::similarity`, sorted descending.

## What was built

- `graph_owl_api::ResolvedEntity { asset: Asset, score: f64 }` and
  `Catalog::resolve_entity(principal, query, limit)` — candidates from
  `search_assets_for`, re-scored by trigram similarity
  (`SimilarityStrategy::NGram { n: 3 }`) against each candidate's
  `fully_qualified_name`, sorted descending.
- **`n = 3` is not a chosen-for-this-tool number.** Trigrams are the
  standard n-gram size for short-identifier fuzzy matching — the same
  default Postgres's own `pg_trgm` extension uses, on the storage engine
  this project already runs on — and the existing padding
  `graph_owl_resolution::rule_match::ngrams` applies (documented in its
  own doc comment) keeps short values from comparing all-or-nothing.
- `ContextSource::resolve_entity` / `RESOLVE_ENTITY` / its
  `ToolDeclaration` / the dispatch arm — **no `Option` wrapper on the
  trait method, matching `search`'s own signature exactly**: an empty
  result is a real, complete answer ("nothing resembles this"), and there
  is no per-asset visibility distinction to make since
  `search_assets_for` already filters by policy before this method ever
  sees a candidate.
- `ResolvedEntityContext` / `ResolvedCandidate` — a new wire type (one
  score field beyond what `SearchHit` carries; reusing `SearchHit` would
  have meant bolting a similarity score onto a type designed around
  relevance-ranked full-text hits and a trust summary, which entity
  resolution has no use for).
- `impl budget::Fits for ResolvedEntityContext` — `shorten_detail`/
  `shorten_relations` are both permanent `false` (every field is
  essential, there is no second tier below the candidate list itself,
  matching `SearchResults`'s identical shape for the identical reason);
  `drop_entities` pops from the tail, which — because the list is sorted
  descending — drops the **least** similar candidates first.
- `CatalogContext::resolve_entity` — the real production adapter.

## The RED test that paid for itself immediately

`resolve_entity_tests::ranks_the_closer_match_first_by_a_real_computed_score`
hand-derives the expected trigram similarity for two real candidates
against the query `"ord"` before running anything — `orders` shares 3 of
10 possible trigrams (`0.3`), `coordination` shares 1 of 18 (`1/18 ≈
0.0556`) — and asserts both exact floating-point values, plus that a third
seeded asset sharing no substring with the query (`customers`) does not
appear at all. All three assertions passed on the first run: the
hand-derived math was correct before the code that was supposed to
produce it ran, which is the property "write the answer key before the
code runs" (this project's own standing practice) exists to catch when it
is *not* true.

## Mutation report

**`lib.rs`'s dispatch, wire types, and `Fits` impl** — `--in-diff`, `--lib`
scoped: **9 mutants, 7 caught, 2 unviable, 0 missed**, after one round.
The first round found two MISSED — `ResolvedEntityContext::shorten_detail`
and `::shorten_relations`, both permanently-`false` levers whose flip to
`true` is absorbed by `budget::fit`'s own no-progress check — the
identical structural gap `TraversalContext::shorten_detail` and
`AnalyticsContext::shorten_detail` already established and closed the
same way: a direct test (`neither_lever_above_entities_ever_claims_progress`)
asserting both return `false`, not through the dispatcher, because the
mutant is provably unobservable through `fit()` by construction.

**`Catalog::resolve_entity` itself** — `--in-diff`, `--lib` scoped: **2
mutants, 1 caught, 1 unviable, 0 missed**. Caught: the whole-function
`Ok(vec![])` fallback, killed by the ranking test's `len() == 2`
assertion. Unviable: `Ok(vec![Default::default()])` — `ResolvedEntity`
derives no `Default`.

**`catalog.rs`'s production adapter** — scoped to a new real-Postgres test
(`cargo test -p graph-owl-server --test mcp_stdio -- resolve_entity`): **1
mutant, 1 caught, 0 missed.** Fewer candidate mutants than `run_rule`'s
own adapter because this one has no admin gate to mutate — `resolve_entity`
is open, matching `search`'s own posture. The single mutant
(`Ok(ResolvedEntityContext::default())` in place of the real body, viable
here because — unlike every other P10 wire type so far — `ResolvedEntityContext`
does derive `Default`) was caught by the new test's `candidates.len() == 1`
assertion.

## What this deliberately does not do

- **Does not resolve pack-domain entities** (a GST supplier, a hospitality
  guest) — scoped to catalog assets only, for the identical reason
  `traverse`/`analytics` are: a pack-domain subject has no policy model
  yet (`plans/105-domain-neutrality.md`'s recorded gap).
- **Does not weight by asset kind, trust, or recency** — the score is
  purely a string-similarity metric between the query and a candidate's
  own `fully_qualified_name`. An agent that needs to break a tie between
  equally-similar candidates has `search_assets`'s trust summary and
  `asset_context` available for that; folding a second signal into one
  score would make the number mean two different things depending on
  which candidates it is compared across.
- **Does not add a per-property or per-column resolution mode.** Scoped
  to whole-asset names, matching `search_assets_for`'s own scope exactly.
