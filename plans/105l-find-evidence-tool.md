# Plan: `find_evidence()` — P10's second MCP intelligence tool

**Branch**: main (committed directly, per `CLAUDE.md`)
**Status**: Shipped 11 August 2026, continuing "keep going tool by tool and
complete P10."
**Crates**: `graph-owl-mcp` (the tool), `graph-owl-server` (a real-Postgres
proof, reusing `evidence_graph.rs`'s own fixture rather than duplicating it).

## What was built

- `ContextSource::find_evidence(principal, finding_id, max_hops)` — wraps
  `Catalog::finding_evidence_graph` (Epic 105 P7, `105e`), the same
  walk-plus-provenance capability the pre-existing
  `GET /findings/{id}/evidence-graph` route already serves.
- `EvidenceContext` / `EvidenceNode` — a dedicated wire shape, not a reuse of
  `TraversalContext`/`TraversalNode`. A finding's subject is not necessarily
  a catalog asset, so a node here carries `iri` (resolved where one exists)
  and `sources` (Epic 105 P7's `105g` provenance work) — neither of which a
  catalog-asset node needs, and both of which the HTTP route this wraps
  already returns.
- `FIND_EVIDENCE` tool declaration, dispatch arm, `required_finding_id()`
  (a new UUID-parsing argument helper), reusing `traverse`'s existing
  `MAX_TRAVERSE_HOPS`/`DEFAULT_TRAVERSE_HOPS`/`traverse_hops()` — justified
  directly by those constants' own doc comment, which already cites this
  exact route's server-side cap as the reason for the number.
- `impl budget::Fits for EvidenceContext` — a real three-rung ladder, unlike
  `TraversalContext`'s permanently-`false` `shorten_detail`:
  `shorten_detail` clears every node's (and the near-miss's) `sources`,
  `shorten_relations` drops edges, `drop_entities` drops nodes and finally
  the near-miss candidate, cleaning up any edge left dangling.
- `CatalogContext::find_evidence` — the real production adapter. Resolves
  through `finding_evidence_graph`, assembles per-node provenance via
  `node_sources`, and folds in `near_miss_node` the same way the HTTP
  handler already does — this is a straight port of that handler's own
  assembly logic into the MCP adapter, not new logic.

## The authorization decision, stated once rather than re-litigated per tool

`Catalog::finding_evidence_graph` takes **no principal at all**. Unlike
`traverse` (which deliberately avoided `105j`'s unauthorized `graph_context`
in favour of the already-safe `asset_subgraph`), this is not a new gap:
the pre-existing HTTP route this wraps is **already** not visibility-checked
per finding, and its own doc comment states why directly — *"a finding is
queue data a reviewer needs to see to do the job, and this is a second view
onto the same finding, not a new privilege."* Wrapping it in an MCP tool
does not expose anything a caller could not already reach over HTTP with
the identical credential. `CatalogContext::find_evidence` still calls
`self.authenticated(principal)?` — authentication, not authorization — the
same distinction the HTTP route's own `Auth(_principal)` (extracted, then
discarded) already draws.

This is the general rule the next several tools should apply, not a
one-off: **a capability with no existing HTTP exposure needs its own
authorization analysis before it is wrapped (`traverse`'s situation); a
capability that already ships over HTTP inherits that route's posture
rather than getting a fresh one invented for the MCP surface.**

## Mutation report

**`lib.rs`'s dispatch, argument parsing, and `Fits` impl** — `--in-diff`,
`--lib` scoped: **18 mutants, 15 caught, 3 unviable, 0 missed**, after two
rounds. First round found 12 MISSED, all inside `impl budget::Fits for
EvidenceContext`. Closed by three dispatcher-level budget tests (detail →
relations → entities, in that order, each budget measured off the fixture
rather than hand-picked) plus two direct calls: `drop_entities`'s
dangling-edge/near-miss-last behaviour (unreachable through the real
dispatcher for the same reason `TraversalContext`'s equivalent survivor
was — `budget::fit`'s ladder always fully drains `edges` before touching
entities), and `shorten_detail`'s near-miss branch specifically (no fixture
anywhere in this file gives `find_evidence` a near-miss node, so that one
branch stayed dark even after the dispatcher-level detail test passed).

**`catalog.rs`'s production adapter** — scoped to two real-Postgres tests,
both reusing `evidence_graph.rs`'s own GST fixtures rather than duplicating
them into `mcp_stdio.rs`: the `PotentialMismatch` fixture
(`seed_invoice_with_a_real_supplier_node` / `register_and_run_missing_in_gstr2b`)
for the ordinary walk, and the `GstinTransposition` fixture
(`seed_transposition_scenario` / `register_and_run_gstin_transposition`) for
the near-miss path — the first test alone left `near_miss_node`'s `Some`
branch entirely dark, since `PotentialMismatch` has no `[findings.similarity]`
band and so never produces one. **2 of 4 candidate mutants specific to
`find_evidence` caught, 2 remain MISSED** — both honestly structural, not
addressed:

- Deleting the `max_hops` field from `Bounds` — the identical gap
  `traverse`'s own report (`105k`) already named: no fixture in this
  codebase has real multi-hop pack-domain data (the GST fixtures here are
  exactly one hop, invoice→supplier), so a hop-count bound has nothing to
  prove itself against.
- The near-miss exclusion guard (`!graph.nodes.contains(&sid)`) mutated to
  an unconditional `true`. Both real fixtures happen to have the near-miss
  candidate genuinely absent from the walk, so `contains(&sid)` is `false`
  and the guard's real value and the "always true" mutant agree by
  coincidence of the data, not because the guard is checked. Killing it
  needs a fixture where the near-miss candidate is *also* independently
  reachable by ordinary traversal — a real GST scenario that does not exist
  in this file and was not constructed for this slice, matching the same
  cost/benefit judgement `traverse`'s own report already made about
  fixture-shaped gaps.

## What this deliberately does not do

- **No new asset-relationship fixture built.** `traverse`'s own mutation
  report (`105k`) already named the gap — no path in this codebase creates
  a real asset-to-asset relationship — and this tool does not touch that
  path either; it walks pack-domain data via the traversal engine, which
  `evidence_graph.rs`'s existing RDF-import fixture already exercises for
  real.
- **`finding_evidence_graph`'s own HTTP handler is not migrated to reuse
  anything from this tool**, or vice versa — both call the same `Catalog`
  method independently, matching `105j`'s own stated precedent for not
  refactoring a shipped, working path inside an unrelated slice.
