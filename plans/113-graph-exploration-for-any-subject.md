# Plan 113 — Graph exploration for any subject, not only catalog assets

**Status**: draft, executing. **Branch**: main. **Trigger**: the user pushed
back on Plan 112's "GST invoices aren't catalog assets, so the relationship
filter and connectivity panel don't apply" — correctly. Investigated rather
than accepted, and the investigation found the fix already half-built.

## The map, verified by reading the code rather than assumed

| Capability | Constraint found | GST-reachable |
|---|---|---|
| Path finding (`find_paths`) | `Sid`-generic since Plan 111 Slice A; `asset_id_of` falls through for non-catalog subjects | ✅ |
| Per-entity reasoning (`/reasoning/derived`) | `parse_sid`, no UUID requirement | ✅ (UI hardcodes `1:${assetId}`) |
| Global OWL EL classify/explain | Ontology-wide, not asset-scoped | ✅ |
| Evidence graph + blocking candidates | `Sid`-generic since Plan 111 Slice F | ✅ |
| SPARQL/Cypher Workbench | Fully generic | ✅ |
| **Relationship-filtered neighbourhood, connectivity panel** | `asset_subgraph`/`asset_analytics` call `get_asset_for`, which requires a relational `assets` row | ❌ |
| Lineage tab | Same relational requirement, and the *concept* is data-pipeline lineage between catalog assets — not the provenance question GST actually has | ❌, and arguably not the right capability to force |
| Memory / Knowledge / Disagreements | `Memory` storage is keyed by asset UUID (`memories_about(subject: Uuid, ...)`) | ❌, by design — GST's equivalent (candidates, evidence) is a different, already-served mechanism |

## The primitive that already exists and has no caller

`Catalog::graph_context(seed: Sid, direction, bounds) -> GraphContext`
(`crates/graph-owl-api/src/lib.rs:3709`) walks from **any** `Sid`, no
relational asset required, and resolves per-node provenance (`node_sources`)
inline. `grep graph_context crates/graph-owl-server crates/graph-owl-mcp`
returns nothing. Same defect as `shortest_path`, `blocking_strategy`,
`assetAnalytics` before it — the fourth instance of "a capability that stops
before any caller can reach it" in this project's own history.

It is missing, relative to `asset_subgraph`:
1. A `relationship_types` filter (Plan 112 Slice A's own addition).
2. `as_of` support.
3. **Any authorization at all.** Currently unchecked. Needs the same posture
   `find_paths` already established: asset-checked via `graph_subject_visible`
   if the seed resolves to a catalog asset, pass-through otherwise — the
   documented, stated limitation, not a new decision.
4. An HTTP route.
5. An analytics counterpart, mirroring how `asset_analytics` computes over
   `asset_subgraph` — a `graph_context_analytics` computing degree/orphans/edge
   types over `graph_context`'s walk instead.
6. A UI surface reachable from something other than a catalog asset page.

## Slices

### Slice A — `graph_context` reaches HTTP, with the filter and authorization ✅ shipped

- `Catalog::graph_context` gains `relationship_types: Option<Vec<String>>` and
  `as_of: Option<DateTime<Utc>>`, threaded into the `EdgeFilter` exactly as
  Slice A of Plan 112 did for `asset_subgraph`.
- Authorization: for each node in the returned subgraph, `graph_subject_visible`
  filters out anything the principal may not see — asset nodes checked for
  real, pack nodes passed through (the stated limitation, applied consistently
  rather than left as an inconsistency between routes).
- `POST /graph/context` — `{seed, direction, hops, maxNodes, relationshipTypes?, asOf?}`.
  `POST` for the same reason `/graph/paths` is: the seed is an identity, not a
  URL-safe token in general (a pack subject's local id can contain characters a
  query string mangles).
- **RED**: a GST subject's neighbourhood walk returns real nodes with real
  provenance; filtering to one relationship type narrows it; an asset seed
  still authorizes as an asset.

**Retrospective.** `Catalog::graph_context` was a shipped Epic 105 primitive
with **zero callers** — the fourth instance in two plans of a capability that
stops one layer short of anything that could reach it (`shortest_path`,
`blocking_strategy`, `assetAnalytics` were the others). The walk and the
provenance resolution were split into `walk_authorized` and
`assemble_graph_context` so Slice B could reuse the first half without
re-deriving it.

`parse_node_id` gained a third accepted shape: a full IRI. **The ordering is
load-bearing** — the `http(s)://` check must precede the general `contains(':')`
check, because an IRI contains a colon too and would otherwise be handed to
`parse_sid`, which tries to parse `"https"` as a numeric namespace code and
fails with an error naming the wrong thing.

### Slice B — Connectivity, generalized ✅ shipped

- A pure `graph_context_analytics(seed: Sid, direction, bounds) -> AssetAnalytics`
  (reusing the existing `AssetAnalytics` shape — the numbers mean the same
  thing regardless of what kind of node they describe) built on `graph_context`
  the same way `asset_analytics` is built on `asset_subgraph`.
- `POST /graph/context/analytics`.
- **RED**: degree counts over a GST invoice's real neighbourhood; truncation
  stated; `PageRank` still absent (Epic 38's purity boundary is about scope,
  not about which kind of node).

**Retrospective — a real bug, found by a test written before the code
shipped.** `analytics_over_subgraph` derived its `edge_types` from the raw
flakes, *independently of* the `relationship_types` filter applied to the
walk. So a filtered neighbourhood picture and its filtered connectivity
numbers would have disagreed: the picture narrowed, the numbers did not. The
filter is now threaded into the edge-type derivation, and
`the_filter_narrows_what_analytics_measures_too` pins it. Verified live in the
browser afterwards — filtering a real GST invoice to `type` narrows the
picture and the connectivity panel in lockstep (2 nodes in both).

One fixture quirk, recorded so it is not re-diagnosed as a product gap: the
**in-memory traversal double** only walks direct-reference flakes into edges
when they are DSC-namespaced, while `graph_owl_analytics::project` reads only
direct-reference flakes for degree. Unit fixtures therefore seed *both* a
reified relationship and direct triples. The HTTP integration tests need no
such workaround — the real Postgres engine reads direct triples exactly as
imported GST data does, which is itself evidence the integration test is the
realistic case and the double is the contrived one.

### Slice C — A subject explorer the console can link to from anywhere ✅ shipped

- A route-free "Subject" panel (reuses `routes.ts`'s pattern: one surface
  absorbing many entry points, not a new top-level route) that takes a raw
  `namespace:local` identifier instead of an asset UUID, and renders the same
  three things Plan 112 built for assets: the neighbourhood picture with the
  relationship filter, the connectivity panel, and the path finder — all three
  already `Sid`-generic underneath, now given a mount point that is not gated
  on being a catalog asset.
- Linked from: evidence-graph nodes (findings), reconciliation statement rows,
  blocking candidates — everywhere this console already renders a bare subject
  id as a label and does nothing with it on click.

**Retrospective.** Shipped as `SubjectExplorer` (the panel), `ClickableSubject`
(a link owning its own `Drawer`) and `subjectContext.ts` (the pure
node/edge/summary logic, 100% mutation score, 32/32 mutants killed). Wired into
the findings queue's near-miss and candidate rows, which required carrying
`iri` through `evidenceNearMiss` and `evidenceCandidates` — those rows had only
a bare local `id`, which resolves to nothing.

`ConnectivityPanel` was refactored from `assetId`/`hops` props to
`cacheKey` + a caller-supplied `load` callback. It hardcoded
`api.assetAnalytics`, and a GST invoice has no asset id to call that with —
the panel would have been unusable for exactly the subjects this plan exists
to reach.

**Deliberately list-based, not a canvas.** `GraphCanvas`'s `Picture` type
requires catalog-shaped `GraphNode`s with a `kind` field that a pack subject
does not have. `00f` already requires an accessible equivalent for any
picture; this builds that half first rather than after.

Two mechanical traps: `SubjectExplorer.tsx` and a pure module originally named
`subjectExplorer.ts` **collided on a case-insensitive filesystem** and broke
module resolution (renamed to `subjectContext.ts`); and `react/no-array-index-key`
is not in this project's eslint config, so the disable comment for it was
itself an error — edge rows are keyed on the triple content instead.

**Verified in the browser against real GST data**, not a synthetic fixture:
opening a `gst:SupplierPanMismatch` finding's `2b-INV-1001` candidate walks 37
nodes and 36 relationships, correctly identifies `supplier-27AABCU9603R1ZM` as
the hub at 36 incoming edges, and populates the relationship filter from the
subject's own real edge types.

### Slice D — Per-entity reasoning stops assuming the catalog namespace ✅ shipped

- `ReasoningView` (currently `api.derivedAbout(\`1:${assetId}\`)`) accepts a raw
  Sid string, reachable from the same Subject panel Slice C builds.

**Retrospective.** Two halves, and the server half was the one that was
actually load-bearing.

`GET /reasoning/derived` parsed its `subject` with `parse_sid`, which accepts
only `namespace:local`. `parse_node_id` — extended in Slice C to resolve full
IRIs — already handled all three shapes, so the server fix was a one-word
swap. It is still a real fix: the console's evidence graph, near-misses and
blocking candidates carry an **IRI**, never a `namespace:local` string, so
before this the reasoning route could not be called with the identity the UI
actually holds.

The component half cost more than expected, for a reason worth recording.
`ReasoningView` lived in `App.tsx`, and `SubjectExplorer` is imported *by*
`App.tsx` (through the findings queue's `ClickableSubject`) — so
`SubjectExplorer` importing `ReasoningView` from `App.tsx` would have closed
an import cycle. `ReasoningView`, `DerivationChain` and `triple` were
therefore lifted into `ui/src/graph/ReasoningView.tsx`. **That move was free
of risk precisely because none of their dependencies lived in `App.tsx`** —
they come from `api`, `theme`, `trust/TrustComponents` and `governance/*` —
so both callers now import downward. A component that needs to be reachable
from two levels of the tree does not belong in the composition root.

Two smaller findings:

- **Moving code into a new file applies lint rules the old file was
  grandfathered under.** Eight `local/no-raw-jsx-text` errors appeared with no
  wording changed at all. Externalized into `COPY` verbatim; the rule was
  right and `App.tsx` is simply exempt by age.
- **`cargo mutants` could not produce a single viable mutant for
  `derived_about`.** All four candidates failed to compile (`Json<Value>` has
  no `new`/`from_iter`). That is an honest result for a thin imperative shell
  — parse, delegate, serialize — and the branching that matters is in
  `parse_node_id`, which mutates cleanly under `--lib` (1 caught, 1 unviable,
  0 survivors). Where a handler has no viable mutants, the mutation question
  belongs to the function it delegates to.

## Explicitly not pursued, and why

- **Lineage** stays asset-only. It encodes data-pipeline provenance
  (`derivedFrom` edges recorded by connectors between catalog assets); GST's
  actual need — "which document said this" — is already served by
  `node_sources` and the evidence graph, and forcing lineage semantics onto tax
  documents would be modeling the wrong relationship.
- **Memory / Knowledge / Disagreements** stay asset-only. Memory is
  institutional commentary attached to a catalog entity by a human; GST's
  equivalent question — "is this finding still valid, has someone looked at
  it" — is already the Review queue's job, not a second annotation system.
  Extending Memory to arbitrary graph subjects is a real feature and a real
  schema change (dropping the UUID foreign key), not a wiring fix, and nothing
  in this session's evidence says GST needs it yet.

## The test every slice must pass

Same as Plan 112: works for hospitality, the pack declares nothing the
console assumes, an empty result is an honest empty state — plus, new for
this plan: **works for a subject that has no catalog asset at all**, proven
by testing against a GST invoice, not a table, as the primary fixture.
