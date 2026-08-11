# Plan: P9's remaining two-thirds — query planner (characterized) and GraphRAG retrieval (a real slice)

**Branch**: main (committed directly, per `CLAUDE.md`)
**Status**: Shipped 11 August 2026, at the user's explicit direction to complete "P9's other two-thirds (Rust query planner, GraphRAG retrieval)."
**Crates**: `graph-owl-api` only (`GraphContext`, `GraphContextNode`, `Catalog::graph_context`). No new crates, no changes to `graph-owl-query`, `graph-owl-traversal`, `graph-owl-engine-postgres`.

## Part 1 — The Rust query planner (§8): characterized, not rebuilt

The platform doc's own words: "The agent pipeline is `agent → LLM →
intent/tool request → Rust query planner → graph execution → structured
result → LLM explanation`... extending `graph-owl-query`'s existing pushdown
planner: it takes a tool request, plans the graph execution
(SPARQL/Cypher/traversal/analytics), and returns a structured result."

Checked against the real code before writing anything, the same "prove the
gap is real" discipline `plans/00l` and this session's own `105g`/`105h`
precedent require:

1. **The pushdown planner already exists and already does its job.**
   `graph_owl_query::pushdown::scans_for` turns a parsed SPARQL query into
   the narrowest set of flake scans that can answer it — exactly "plans the
   graph execution" for the SPARQL front end. Nothing to add here.
2. **SPARQL and Cypher are not two planners to route between — they already
   share one.** Epic 7b/7c's Cypher front end (`graph_owl_query::cypher::lower`)
   lowers `MATCH`/`RETURN`/aggregates to the *same*
   `spargebra::algebra::GraphPattern` the SPARQL front end produces, stated
   explicitly in that module's own header: "One engine, two front ends...
   the planner, the evaluator and the authorization path are shared." A
   "query planner routing between SPARQL and Cypher" would be solving an
   already-solved problem — there is one algebra, one pushdown pass, one
   evaluator, regardless of which surface language asked.
3. **Traversal and analytics already have their own dedicated entry
   points**, deliberately not folded into the SPARQL/Cypher path:
   `Catalog::asset_subgraph`/`finding_evidence_graph` (bounded walks, Epic
   7a) and `graph-owl-analytics` (Epic 38, whole-graph computation,
   correctly kept off the synchronous request path per that epic's own
   Slice F deferral). `Catalog::sparql`, `Catalog::cypher`,
   `asset_subgraph`, and analytics already coexist as separate, callable
   entry points on the same `Catalog`.
4. **What is genuinely missing is the *dispatcher*** — something that takes
   an abstract tool request (`resolve_entity()`, `traverse()`,
   `find_evidence()`, ...) and decides which of the four entry points above
   answers it. The platform doc's own text says this dispatch **is** the
   tool surface: "the agent's intended tool surface... is designed to map
   one-to-one onto Rust core capabilities — that mapping is unbuilt, and
   building it is a step of its own" (§12, tracked separately as P10's 8
   MCP tools). Building a "planner" with no tool requests to route —
   because the tool definitions do not exist yet — would be routing logic
   with nothing to route, designed from a sample size of zero rather than
   the "sample size of one" trap `plans/00l` already warns against; here
   there is no sample at all.

**Conclusion, matching P6's own precedent exactly**: the query planner the
doc describes is **already built**, distributed across the pushdown pass,
the shared Cypher/SPARQL algebra, and `Catalog`'s existing entry points. The
only remaining piece — routing a tool request to one of them — is
definitionally P10's work (the tools themselves), not a separate module to
build in isolation. Recorded here rather than silently assumed, so the next
person reading `105` does not re-propose a planner that would duplicate
`pushdown.rs`.

## Part 2 — GraphRAG retrieval (§10): the real assembly gap, closed for one seed shape

The doc: "Entity linking, candidate retrieval, k-hop expansion, path
retrieval, subgraph filtering, ranking, provenance preservation and context
construction are Rust, assembling the existing traversal/query/search
primitives."

Checked piece by piece against what exists:

| Sub-capability | Status |
|---|---|
| k-hop expansion | **Exists** — `Bounds{max_hops}`, used throughout `105e`/`105g` |
| Path retrieval | **Exists** — `TraversalEngine::shortest_path`/`all_paths`, implemented in all three backends (`graph-owl-traversal-memory`, `graph-owl-engine-postgres`, the trait itself) |
| Subgraph filtering | **Exists** — `EdgeFilter`, `asset_subgraph`/`finding_evidence_graph` |
| Ranking | **Exists** — Epic 31's cosine-similarity embedding search |
| Provenance preservation | **Exists** — `Catalog::node_sources` (`105g` Slice 1) |
| Entity linking | **Missing** — resolving free text to a graph `Sid` has no Rust primitive yet |
| Context construction | **Missing until this slice** — no type named what an LLM-facing retrieval result looks like; the closest precedent, `finding_evidence_graph`, returns a bare `Subgraph` and leaves provenance assembly to the HTTP handler that happens to need it |

**What was built**: `Catalog::graph_context(seed, direction, bounds) ->
GraphContext`. Generalizes `finding_evidence_graph`'s own walk-plus-provenance
shape beyond a finding's subject to *any* seed `Sid` — an asset, an entity a
future connector lands, anything already in the graph — and moves the
per-node provenance assembly (currently a loop inside
`graph-owl-server`'s HTTP handler) into `Catalog` itself, which is where the
doc says "provenance preservation... are Rust" belongs. Proven against a
non-finding fixture (a warehouse asset with an ERP-sourced label, not a GST
subject) to show the generalization is real, not cosmetic.

**Mutation testing found nothing to test, and that turned out to be a real
finding rather than a tooling gap.** `--in-diff` (twice, with default and
`-U50` context) and a `--list`-scoped check of the exact line range both
agree: `graph_context` has exactly **one** candidate mutant —
`Ok(Default::default())` in place of the whole function body — and it is
unviable, because `GraphContext` deliberately has no `Default` impl,
so the mutant does not compile. No field-deletion mutants exist for
`EdgeFilter { relationship_types: None, as_of: None }` or
`GraphContextNode { id: id.clone(), sources }` either, for the identical
reason: cargo-mutants' field-deletion strategy needs the struct's `Default`
to fall back to, and neither struct is given one. Cross-checked against
every earlier "MISSED" survivor this session found (`TriplePattern`,
`Flake`, `Asset`, `AssetUpdate`) — every one of those types *does* derive
`Default`, which is what made the field-deletion mutation possible in the
first place. The four new tests (arbitrary seed, per-node provenance, an
empty-provenance node, truncation survives assembly) still exist and still
pass; there is simply no meaningless fallback value left in this function
for a mutant to hide behind.

## What this deliberately does not do

- **No entity linking.** Resolving free text ("the July invoices") to a
  seed `Sid` is a real, separate, string-matching-shaped problem — fuzzy
  matching against labels, full-text search integration, or both — that
  deserves its own scoping pass, not a hasty addition here. `graph_context`
  takes an already-resolved `Sid`, the same precondition
  `finding_evidence_graph` already has.
- **No HTTP route.** `Sid`/`EdgeRef` have no `Serialize` impl by design —
  every existing graph-rendering route builds its own `json!()` by hand at
  the boundary rather than deriving one. A wire shape for `GraphContext`
  belongs with whichever real caller (an MCP tool, P10) first needs one.
- **`finding_evidence_graph`'s own HTTP handler is not migrated to reuse
  `graph_context`.** It already works, is tested, and is verified live in
  the browser (`105e`/`105g`); refactoring a shipped, working path to
  remove duplication is a legitimate future cleanup, not a change to make
  in the same slice that adds the new capability.
- **No hybrid search (§11) fusion/ranking layer.** Ranking exists (Epic
  31's embeddings); combining it with lexical and graph signals into one
  fused ranking is real, separate work this slice does not attempt.
