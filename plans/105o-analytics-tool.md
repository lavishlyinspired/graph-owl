# Plan: `analytics()` — P10's fifth MCP intelligence tool

**Branch**: main (committed directly, per `CLAUDE.md`)
**Status**: Shipped 11 August 2026, continuing "complete P10 first" by
direct user instruction — the tool was *not* deferred, unlike the earlier
plan for it.

**Crates**: `graph-owl-api` (the `Catalog` method and its projection
assembly), `graph-owl-analytics` (Epic 38, reused unmodified), `graph-owl-mcp`
(the tool), `graph-owl-server` (a real-Postgres proof).

## The instruction this resolves

The user's own words: *"for the analytics tool do not defer it. see how it
can incorporated as it is needed for traversal and analysis."* This
overrides the earlier working assumption (stated while planning P10) that
`analytics()` should be left explicitly deferred, on the grounds that Epic
38's own decision 2 says analytics must never run live over the whole graph
per request.

**Both constraints are satisfiable at once, and `00e`'s own text already
says how.** `plans/00e-crate-architecture.md`'s "purity boundary" entry for
`graph-owl-analytics` distinguishes "a bounded walk" from "an unbounded
whole-graph computation" as "a real operational distinction" — meaning
Epic 38 never forbade analytics over a *bounded* neighbourhood, only over
the whole graph on a synchronous request. `analytics()` is scoped to the
exact same bounded, already-authorized neighbourhood `traverse()`/
`asset_subgraph` already walks, never wider. That satisfies Epic 38's
decision 2 (never whole-graph, never live-unbounded) and the user's framing
("needed for traversal and analysis") simultaneously: it is a structural
companion to `traverse`, not an independent whole-graph operation.

## What was built

- `graph_owl_api::AssetAnalytics` — a new wire-adjacent (not `Serialize`,
  matching `Sid`/`EdgeRef` convention) result type: `nodes: Vec<Sid>`,
  `in_degree`/`out_degree: Vec<f64>` (index-aligned to `nodes`), `orphans:
  Vec<Sid>`, `edge_types: Vec<Sid>`, `truncated: bool`.
- `Catalog::asset_analytics(principal, id, direction, bounds)` — walks via
  the existing `asset_subgraph` (so visibility is checked exactly once,
  the same way it already is for `traverse`), fetches the walked nodes'
  own flakes (one `query_pattern` per node, the same bounded per-node
  fan-out `node_sources` already established), derives `edge_types` as
  every predicate whose object is a `FlakeValue::Ref` among those flakes
  (domain-neutral — no pack-specific relationship name hard-coded), builds
  a `graph_owl_analytics::AnalyticsBudget` whose `max_edges` is
  `bounds.max_nodes.saturating_mul(bounds.max_nodes)` (the mathematical
  ceiling on directed edges among a node set that size — `00i` rule 4:
  every magic number needs a stated reason, and "the reference used this"
  is never one), and runs `graph_owl_analytics::project` /
  `degree_centrality` (`In`, `Out`) / `connected_components` over it.
- **`PageRank` is deliberately excluded.** Its meaning depends on
  whole-graph scope (Epic 38 decision 6 already has it "on probation"
  pending a bake-off that never ran); computing it over an arbitrary
  bounded neighbourhood would produce a number shaped like PageRank
  without meaning what PageRank means. Degree centrality and connected
  components do not have that problem — both answer meaningful questions
  about a neighbourhood on its own terms.
- `ContextSource::analytics(principal, fqn, direction, max_hops)` — the MCP
  trait method, mirroring `traverse`'s own signature exactly (same fqn
  resolution, same `Direction`/`max_hops` shape) since both tools answer
  about the same kind of walk.
- `AnalyticsContext`/`NodeAnalytics` — the wire type. **Each node
  self-describes its own degree** (`{id, inDegree, outDegree}`) rather than
  three parallel arrays index-aligned by position the way
  `AssetAnalytics` stores them internally: an agent reading JSON should
  never have to zip lists back together to answer "how connected is this
  one node."
- `Outcome::Analyzed`, the `ANALYTICS` tool constant, its `ToolDeclaration`,
  and the dispatch arm — all following the identical shape `TRAVERSE`
  already uses (`required_fqn`, `direction_of`, `traverse_hops` are reused
  verbatim, not duplicated).
- `impl budget::Fits for AnalyticsContext` — `edgeTypes` then `orphans`
  shrink first (metadata about the walk, not the per-node answer itself),
  then nodes drop last-first, removing the dropped node's own orphan flag
  too (the same dangling-reference invariant
  `TraversalContext::drop_entities` enforces for edges).
- `CatalogContext::analytics` — the real production adapter, mirroring
  `CatalogContext::traverse`'s fqn-resolution and `Bounds` mapping, calling
  `asset_analytics` instead of `asset_subgraph`.

## The RED test that found a real gap, not a fabricated one

The Catalog-layer unit test (`asset_analytics_tests` in `graph-owl-api`)
originally scoped the per-node flake fetch with a graph double
(`RecordingGraph`) seeded with only the walked nodes' own flakes. That
passed trivially — it could not distinguish a correctly-scoped `s:
Some(sid)` query from an unscoped `s: None` one, because with so few
flakes in the double, an unscoped scan produces the identical deduplicated
result via the `BTreeSet` collection downstream. **A mutation run caught
this**: deleting the `s: Some(sid.clone())` field from the per-node
`TriplePattern` survived. Fixed by seeding a fourth flake belonging to a
node *outside* the walked set (`asset-outsider`) — under correct scoping
it is never fetched; under the mutant it leaks into `edge_types` and adds
two extra nodes to the projection, which the test's exact `nodes` and
`edge_types` assertions now catch.

## A second real gap, found by the real-Postgres integration test

The first draft of `CatalogContext::analytics` (and its test) assumed a
walked catalog-asset node's wire `id` would render as its FQN. It does
not: `TraversalNode.id` (the pre-existing, already-shipped precedent this
mirrors) is `sid.id.clone()` — the graph subject's own raw local id, which
for a catalog asset (seeded as `Sid::new(DSC, uuid.to_string())`) is the
UUID, not the FQN, whatever `TraversalNode`'s own doc comment claims about
rendering "a full IRI." The test's first run against a real seeded asset
failed on exactly this, asserting a UUID where an FQN was expected —
caught before it shipped rather than discovered by an agent later.

**Resolved by matching `traverse`'s actual (not documented) behaviour**,
deliberately, rather than diverging: if `analytics()` rendered FQNs while
`traverse()` renders raw subject ids for the identical underlying nodes,
an agent could never correlate degree data back to the traversal graph it
came from. Consistency between the two tools' node identities is the
correct design here, independent of whatever `TraversalNode`'s doc comment
gets wrong about IRIs — a pre-existing discrepancy this slice did not
introduce and left unfixed as out of scope for a slice about a different
tool.

## Mutation report

**`lib.rs`'s dispatch, wire types, and `Fits` impl** — `--in-diff`, `--lib`
scoped: **11 mutants — 9 caught, 2 unviable, 0 missed**, after one round.
The first round found one MISSED (`AnalyticsContext::shorten_detail`
replaced with a hardcoded `true`), closed the same way
`TraversalContext::shorten_detail_never_claims_progress` already closed
the identical structural gap: a *direct* call asserting `!shorten_detail()`
rather than through `budget::fit`, because `fit`'s own no-progress
shrink-check absorbs a no-op lever regardless of its return value — the
mutant is provably unobservable through the dispatcher by construction,
not a gap in the dispatcher-level tests.

**`catalog.rs`'s production adapter** — `--in-diff`, scoped to
`cargo test -p graph-owl-server --test mcp_stdio -- analytics` (the new
real-Postgres test; `-p graph-owl-mcp`'s own default test scope never
reaches it, since the exercising test lives in a different crate): **3
mutants, 2 caught, 1 missed**. Caught: the whole-function `Ok(None)` and
`Ok(Some(Default::default()))` fallbacks — both killed by the new test's
assertion that a real, visible asset's own analytics are never empty and
never default. **Missed, for the identical structural reason `105k`
already recorded and did not fix for `traverse`'s own adapter**: deleting
`max_hops` from the `Bounds` passed to `asset_analytics`. Killing it needs
a multi-hop fixture, and there is still no asset-to-asset
relationship-creation path in this codebase to build one with — the exact
same gap, inherited rather than reintroduced. Not closed here for the same
reason it was not closed in `105k`: closing it means building the missing
write path first, which is separate work out of scope for wiring one read
tool.

## What this deliberately does not do

- **No `PageRank`.** See above — whole-graph-relative by construction, and
  computing it over a bounded neighbourhood would not mean what it claims
  to mean.
- **Does not fix `TraversalContext`'s "full IRI" doc-comment inaccuracy.**
  Discovered while building this tool; belongs to `traverse`, not to this
  slice.
- **Does not close `asset_subgraph`'s pre-existing multi-hop test gap.**
  The same structural gap `105k` already recorded: there is no
  asset-to-asset relationship-creation path in this codebase today, so
  every real-Postgres proof of a walk-shaped tool (`traverse`, now
  `analytics`) is limited to a lone, edgeless asset. Building that missing
  write path is separate work.
