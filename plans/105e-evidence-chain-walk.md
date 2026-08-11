# Plan: Evidence-chain walk — the platform doc's P7, Slice 1

**Branch**: main (committed directly, per `CLAUDE.md`)
**Status**: **Starting 11 August 2026**, at the user's explicit "continue to finish it" following `plans/105c-gst-causal-graph.md` Slice 1.
**Depends on**: `graph-owl-traversal`'s `subgraph` (exists, Epic 7a), the findings store (exists, Epic 105 P5), DN-1's runtime `Sid::from_iri` resolution (exists — confirmed it already falls through to the process-wide namespace table, no new resolver plumbing needed).
**Crates**: `graph-owl-storage` (port, new trait method) · `graph-owl-storage-postgres` (adapter) · `graph-owl-api` (orchestration) · `graph-owl-server` (route) — **no new crates**.

## Honest scope statement

The platform doc's P7 is "a bounded graph computation (candidate evidence retrieval, **missing-hop determination**) ... Python interprets what a gap *means* under which assertion." Missing-hop determination requires knowing, per finding label, which hop is *expected* — that is pack-config-driven interpretation this project has no shape for yet (`findings.yaml`'s "assertion mappings" from the platform doc do not exist in this project's `pack.toml`). Inventing that shape unilaterally in one slice would be exactly the kind of design decision the user's own earlier answer said needs its own scoping pass, not a decision folded into an unrelated slice.

**So this slice is P7's traversal half only**: given a finding, walk the real graph around its subject and return what is actually connected — nodes and edges, traversal-derived, not the flat rule-authored evidence list `pack.toml` already provides. This is a genuine, verifiable step past "flat list of facts named at rule-authoring time" toward "a walk computed at answer time," and it is honestly *not* the full Entity→Evidence→Source→Provenance→Assertion→Finding semantic chain with gap interpretation — that half stays explicitly out of scope, named here rather than silently dropped.

## Design

`TraversalEngine::subgraph` already exists and is already wired to `Catalog` (`asset_subgraph` is the precedent, scoped to `dsc:`-namespaced catalog assets). The only genuinely new step is resolving a **finding's** subject — which can be in *any* pack's namespace, not just `dsc:` — into a `Sid`, and `Sid::from_iri` already handles that: it falls through to the process-wide runtime namespace table `DN-1` built, so no new resolution machinery is needed.

```
GET /findings/{id}/evidence-graph?hops=&maxNodes=&direction=
  → FindingStore::get_finding(id)         — NEW: no existing single-fetch method
  → Sid::from_iri(&finding.subject)       — existing, resolves any registered namespace
  → TraversalEngine::subgraph([sid], ...) — existing, already wired to Catalog via `traversal`
  ⇒ Subgraph { nodes, edges, truncated }
```

## Slices

### Slice 1 — `FindingStore::get_finding`, `Catalog::finding_evidence_graph`, the route

- [x] RED: `FindingStore::get_finding` — in-memory fake, found/not-found
- [x] Postgres adapter + integration test against a real finding
- [x] `Catalog::finding_evidence_graph(id, direction, bounds)` — RED with a fake store + fake traversal engine: finding not found → `NotFound`; finding found, subject resolves, traversal called with the right seed; a subject that fails to resolve (unregistered namespace) → a named error, not a panic
- [x] `GET /findings/{id}/evidence-graph` route, not admin-gated (same visibility as `GET /findings`), bounds capped server-side matching `asset_graph`'s own caps
- [x] Integration test against the real GST pack: a `PotentialMismatch` finding's evidence graph actually contains the `Supplier` node its `issuedBy` edge points to — proof this is a real traversal, not a restatement of the flat evidence list
- [x] OpenAPI contract regenerated
- [x] Mutation run

**Found and fixed along the way — the traversal engine itself was DSC-only.**
The real-GST-pack integration test above initially failed: the evidence
graph came back with the seed alone, no `Supplier`. Root cause, in
`graph-owl-engine-postgres/src/traversal.rs` (Epic 7a's original code):
every `neighbours`/`shortest_path`/`all_paths`/`detect_cycles`/`subgraph`
reconstructed nodes as `Sid::new(namespace::DSC, id)` unconditionally, and
the "direct reference" edge branch only ever recognised
`namespace_p = DSC`. Both were invisible until now because every existing
caller (`asset_subgraph`) only ever fed the engine `dsc:`-scoped catalog
assets — Epic 105e's `finding_evidence_graph` is the first caller to seed a
walk from a domain pack's own node.

Fixed by carrying `namespace_code` through the recursive CTE as a
`"ns:id"` composite text key (`composite_id`/`decode_composite` in
`traversal.rs`), and broadening the direct-reference predicate filter from
an allowlist of one (`DSC`) to a denylist of four ontology/schema
namespaces (RDF, RDFS, OWL, SHACL) — so a domain pack's own Ref-valued
predicate (`gst:issuedBy`) is walkable, while `rdf:type` and friends stay
excluded on purpose rather than by accident. Four new tests added to
`crates/graph-owl-engine-postgres/tests/traversal.rs` cover: a non-DSC
direct reference is an edge; a reconstructed node keeps its real namespace;
two namespaces sharing a local id stay distinct; `rdf:type` still does not
become an edge. All 28 pre-existing DSC-only tests in that file pass
unchanged.

**Known follow-up, not fixed here**: `graph-owl-traversal-memory`
(Epic 103's in-process petgraph adapter) has the same
`namespace_p = DSC`-only restriction on direct references (no data-loss bug
there, since it carries real `Sid`s natively) — it is not wired into any
production HTTP path today, only used as a test double / equivalence
fixture in `graph-owl-api`'s own tests, so this was left as a known gap
rather than expanding this slice's scope further.

### Slice 2 — console (if time permits within this session) — ✅ shipped

- [x] A section in `findingsQueue.tsx`'s detail pane rendering the evidence graph (nodes + edges as a structured list — not a full interactive graph visualization, which is separate scope)
- [x] Verified live — against the demo's real accumulated GST data (9 findings), `pr-INV-1003`'s evidence graph correctly showed `issuedBy` to its `Supplier` plus two `onInvoice` edges the flat evidence list never named

## What this explicitly does not do

- Missing-hop determination (i.e. "we expected a Gstr2bInvoice from this supplier and there is none") — needs a pack-config shape this project doesn't have yet.
- The MCP `find_evidence`/`explain` tools (P10) or the agent (P11) — both depend on this existing first, per the platform doc's own dependency order, and are weeks-sized work needing their own scoping pass.
