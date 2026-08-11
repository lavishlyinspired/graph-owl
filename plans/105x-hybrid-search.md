# Plan: hybrid-search fusion — P9's second remaining gap

**Branch**: main (committed directly, per `CLAUDE.md`)
**Status**: Shipped 11 August 2026, continuing the same completion pass as
`105u`/`105v`/`105w`.
**Crates**: `graph-owl-core` (`hybrid.rs`, pure — `00e` rule 4),
`graph-owl-api` (`Catalog::link_entity` now fuses; `LinkedEntity`'s shape
changed accordingly). No changes to `graph-owl-search`,
`graph-owl-analytics`, or `graph-owl-traversal`.

## The gap, and what turned out to already exist

`105j`'s own closing line: "No hybrid search (§11) fusion/ranking layer.
Ranking exists (Epic 31's embeddings); combining it with lexical and graph
signals into one fused ranking is real, separate work." Checked before
writing anything: `graph_owl_core::recall::rank` (Epic 31 Slice C)
**already does exactly this fusion** — anchor (a graph signal: an explicit
link to the subject), lexical, semantic (Epic 31's embedding), staleness,
recency, authorship, confidence, all weighted and summed. That fusion is
real and already shipped; it is scoped to ranking **recalled memories**
against one asset, not to searching **entities** in general.

The actual remaining gap is that `link_entity` (`105w`, shipped
immediately before this slice) and `resolve_entity` (P10) both rank by
lexical similarity **alone** — no graph signal, and no semantic signal,
because no graph subject in general has an embedding (only memories do,
per Epic 31's own scope). This slice closes that gap for `link_entity`.

## Why a new, generic primitive rather than reusing `recall::rank`

`recall::rank` is memory-shaped: its `Candidate` carries a `&Memory`, its
terms include staleness/recency/authorship/confidence, none of which
apply to a graph subject in general. Building a **new**, smaller, generic
fusion function — `graph_owl_core::hybrid::fuse(lexical, semantic, graph,
weights) -> HybridScore` — follows `recall`'s own architecture exactly
(pure, no I/O, a `Weights`-and-`Score` pair, `#[must_use]`) without
forcing an entity-linking candidate to carry memory-only fields it does
not have.

## What was built

- `graph_owl_core::hybrid`: `HybridWeights` (equal weight by default —
  matching `recall::Weights`'s own "no evidence to distinguish them"
  reasoning for terms in one tier), `HybridScore` (decomposed —
  `lexical`/`semantic`/`graph`/`total`), `fuse(...)`.
  **Both `semantic` and `graph` are `Option<f64>`**, not a bare `f64`
  defaulting to zero — the identical honesty
  `recall::Candidate::semantic` already established: a missing addend and
  a zero addend reach the same total arithmetically, but only `None`
  lets a reader tell "not similar"/"not connected" from "never measured."
- `Catalog::link_entity` now fuses: the same lexical similarity as before,
  plus a **graph-connectivity term** computed via
  `TraversalEngine::neighbours(subject, Both, {max_hops: 1, max_nodes:
  200}, EdgeFilter::default())` — the fraction of the one-hop node budget
  actually reached, excluding the seed itself. `semantic` stays honestly
  `None` throughout: no graph subject in general has an embedding today
  (only memories do), and inventing one here would misreport "not
  measured" as "measured, and dissimilar."
- **Degrades gracefully, never fails the call**: no traversal engine
  configured, or a traversal error, both read as `graph: None` — the same
  posture `node_sources`'s own per-node lookup already takes toward a
  failure that should degrade ranking quality, not availability.
- `LinkedEntity`'s shape changed: `score: f64` (bare lexical similarity)
  became `lexical: f64`, `graph: Option<f64>`, `score: f64` (now the
  fused total) — a real, deliberate breaking change to a method shipped
  minutes earlier in the same session, not hidden: `105w`'s own plan doc
  already named this as `105j`'s *other* gap, not yet attempted.

## The RED test

A third test, alongside `105w`'s original two: two subjects carrying the
**identical** literal value (so lexical similarity cannot distinguish
them by construction), one connected to a real edge and one isolated,
against the real `graph_owl_traversal_memory::InMemoryTraversalEngine` —
not a faked traversal result, matching this file's own established
precedent (`the_real_in_memory_adapter_still_respects_the_access_predicate_per_principal`)
for proving a real adapter rather than assuming its shape. Asserts the
connected subject ranks first *and* that its own `graph` term is
`Some(> 0.0)` while the isolated one reads `Some(0.0)` — measured and
disconnected, not unmeasured.

`105w`'s original two tests needed no changes: neither configures a
traversal engine, so `graph` stays `None` throughout and the fused total
reduces to the bare lexical term — the same numbers those tests already
asserted.

## Mutation report

**`graph-owl-core/src/hybrid.rs`**, `--file`, `--lib`: **11 mutants, 10
caught, 1 unviable, 0 missed.**

**`graph-owl-api/src/lib.rs`**, `--in-diff`, `--lib`: first run **13
mutants, 8 caught, 1 unviable, 4 missed** — all four in
`link_entity_graph_signal`'s own arithmetic (`others / (max_nodes - 1)`):
the ranking test only distinguished zero from non-zero, so `/` swapped
for `%`/`*`, and `-` swapped for `+`/`/`, all left it passing. Fixed with
a fourth test pinning the exact value for a known reached-count (3
neighbours out of a 200-node budget, asserting `3.0 / 199.0` to `1e-9`),
hand-derived before the test was written. **Second run: 13 mutants, 11
caught, 1 unviable, 1 missed** — the one pre-existing, already-documented
tie-break (`>` vs `>=` in the best-per-subject loop, `105w`'s own doc
comment on it, unchanged by this slice).

## What this deliberately does not do

- **Does not give graph subjects embeddings.** Building an embedding
  pipeline for arbitrary entities (not only memories) is a materially
  larger, separate undertaking than fusing an existing signal — `semantic`
  stays `None` for `link_entity` until that exists.
- **Does not apply fusion to `resolve_entity`** (catalog-asset search).
  `link_entity` was the natural, newest consumer; extending
  `resolve_entity` the same way is a small, separate follow-up once a
  caller needs it.
- **The graph term is a bounded one-hop reach fraction, not a real
  centrality measure** (PageRank, betweenness). `graph-owl-analytics`'s
  own `degree_centrality` computes exactly that, but over a whole
  pre-fetched `GraphProjection` — appropriate for a batch analytics job,
  not a per-candidate call inside a ranking loop. `TraversalEngine::
  neighbours`, already budget-bounded and designed for exactly this "how
  connected is this one node" question, is the right tool at this cost
  point; a truer centrality measure is real, separate work if a caller's
  ranking quality ever needs it.
