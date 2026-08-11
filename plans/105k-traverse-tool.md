# Plan: `traverse()` — the first of P10's eight MCP intelligence tools

**Branch**: main (committed directly, per `CLAUDE.md`)
**Status**: Shipped 11 August 2026, at the user's explicit direction to complete
"whats pending including the p10 to p12."
**Crates**: `graph-owl-mcp` (the tool itself), `graph-owl-server` (a real-Postgres
proof of the production adapter). No new crates.

## What P10 actually asks for

The platform doc names eight MCP tools an agent needs beyond Epic 14's original
seven read tools and Epic 32's six write tools: `resolve_entity()`,
`traverse()`, `reconcile()`, `find_evidence()`, `run_rule()`,
`calculate_risk()`, `analytics()`, `explain()`. Zero existed before this slice.
This slice builds **one** — `traverse()` — completely, rather than stubbing
eight. The other seven remain open; see "What this deliberately does not do."

## Why `traverse`, and why it wraps `asset_subgraph` rather than `graph_context`

`105j` built `Catalog::graph_context(seed: Sid, ...)` for GraphRAG assembly.
It takes a bare `Sid` and **has no principal parameter — no authorization
check at all**, because pack-domain data (a GST invoice, a hospitality guest)
has no policy model yet (a pre-existing, documented gap:
`plans/105-domain-neutrality.md`). Wiring `traverse` around `graph_context`
would hand an MCP client — a surface explicitly designed for an autonomous
agent — unauthorized read access to the whole graph.

`Catalog::asset_subgraph` (pre-existing, Epic 7a) already does the right
thing: it calls `self.get_asset_for(principal, id)` before walking (`00b`
decision 7), so a caller only ever reaches what they may already see. **This
tool wraps `asset_subgraph`, not `graph_context`, and is scoped to catalog
assets only** — a pack-domain subject is out of reach through this tool
until a policy model exists for one, which is not this slice's job to build.

## What was built

- `ContextSource::traverse(principal, fqn, direction, max_hops)` — the port
  method, alongside the other seven read capabilities (`graph-owl-mcp/src/lib.rs`).
- `TraversalContext` / `TraversalNode` / `TraversalEdge` — wire types. Unlike
  `GraphContext` (105j), these **do** derive `Serialize`, because this tool
  has a real caller (the MCP dispatcher) from the moment it exists.
- `TRAVERSE` tool declaration, dispatch arm in `call_within`, and
  `traverse_hops()` argument parsing — capped at 6 hops, defaulted to 2,
  refused at 0 — mirroring `search_limit`'s existing "capped, not refused"
  posture exactly.
- `impl budget::Fits for TraversalContext` — nodes drop before edges before
  detail, matching every other tool's fitting ladder.
- `CatalogContext::traverse` (`graph-owl-mcp/src/catalog.rs`) — the real
  production adapter: resolves `fqn` to an asset, maps MCP's two-value
  `Direction` (`Upstream`/`Downstream`) onto `graph_owl_traversal::Direction`
  (`Incoming`/`Outgoing`), calls `asset_subgraph`, and converts the returned
  `Subgraph` into the wire shape. A `CatalogError::NotFound` — covering both
  "no such asset" and "not visible to this principal" — becomes `Ok(None)`,
  preserving the "denied and absent are the same answer" property every
  other tool on this trait already holds.
- Stub `traverse` implementations on the crate's three test doubles
  (`jsonrpc.rs`'s `Fixture`, `lib.rs`'s own `Fixture`, `graph-owl-server`'s
  `stdio.rs` `Fixture`) and the `graph-owl-mcp/tests/thesis.rs` `Seeded`
  fixture, so the trait continues to compile everywhere it is implemented.
- Ten new unit tests (`lib.rs`'s `the_traversal_tool` module): the success
  path, default-argument propagation, explicit direction/hop-count
  propagation, hop-count clamping, hop-count-zero refusal, denied-equals-absent,
  and two direct calls onto `TraversalContext`'s own `Fits` methods (see the
  mutation report below for why those two are direct calls rather than
  dispatcher calls). Two more in `the_token_budget` module prove the
  edges-before-nodes budget ladder through the real dispatcher, with the
  budget measured off the fixture rather than hand-picked (matching
  `budget.rs`'s own test convention). `TRAVERSE` was also added to the three
  existing shared-property loops
  (`a_denied_asset_is_not_found_on_every_tool`,
  `an_unauthenticated_caller_learns_nothing_from_any_new_tool`,
  `an_unreachable_catalog_is_never_reported_as_absence`) and to the manifest
  contract-drift guards (`every_declared_tool_is_one_that_can_be_called`,
  `a_read_only_server_declares_no_write_tools`,
  `a_writable_server_declares_both_halves`), which now expect eight read
  tools rather than seven.
- One real-Postgres integration test,
  `traverse_reaches_the_real_catalog_through_the_real_adapter`
  (`graph-owl-server/tests/mcp_stdio.rs`) — proves `CatalogContext::traverse`
  against a real `Catalog`, not the unit-level `Fixture` double.

## The mutation report, and the one gap it found that this slice does not close

Two separate runs, because the new code splits across two crates with two
different affordable test scopes.

**`lib.rs`'s dispatch, argument parsing, and `Fits` impl** — `--in-diff`
against the `lib.rs` diff, `--lib` scoped (all logic here is exercised by
unit tests against the `Fixture` double, no Postgres needed): **16 mutants,
14 caught, 2 unviable, 0 missed** — after two rounds. The first round found
9 MISSED mutants, all inside `impl budget::Fits for TraversalContext`:
`shorten_detail`/`shorten_relations`/`drop_entities` each replaced with a
hardcoded `true`/`false`, `render` replaced with `Default::default()`, and
two boolean-flip mutants inside `drop_entities`'s dangling-edge cleanup.
Closed by two more dispatcher-level tests (edges-before-nodes ordering, the
same "detail before entities" shape `SEARCH_ASSETS`'s existing tests already
prove, with the budget measured off the fixture) plus two **direct** calls
onto `TraversalContext::shorten_detail`/`drop_entities` — direct rather than
through the dispatcher because `budget::fit`'s ladder always fully drains
`edges` via `shorten_relations` before it ever calls `drop_entities`, so
`drop_entities`'s own edge-retention logic can never run against a
non-empty edge list through a real dispatcher call; and `shorten_detail`'s
mutant is absorbed by `drain`'s own shrink-check (a no-op lever produces the
same observable state whether it claims `true` or `false`), making it
provably unobservable through `fit()` regardless of which test calls it.
Both are documented in the tests themselves, not just here.

**`catalog.rs`'s production adapter** — `--re "traverse"`, scoped to the new
real-Postgres test (`--lib` doesn't reach this method at all, since it
exercises no `Fixture` double): **2 of 3 candidate mutants caught**
(`Ok(None)` and `Ok(Some(Default::default()))` in place of the real body —
both killed by the new integration test's assertion that a real, visible
asset's own walk is never empty). **One remains MISSED**: deleting the
`max_hops` field from the `Bounds` passed to `asset_subgraph`.

**This was not fixed, and the reason is structural, not an oversight.**
Killing it needs a fixture with at least two hops of real, Postgres-projected
relationship data — and there is no path in this codebase today to create an
asset-to-asset relationship that a traversal engine can walk:

- `POST /tables/{id}/relationships` (`Catalog::create_relationship`)
  operates on the legacy `Table`/`Relationship` entity pair, a separate id
  space from `Asset` — the exact trap `CLAUDE.md`'s Epic 31 gotcha already
  documents ("tables and assets are different relations").
- `LINK_LINEAGE` (`graph-owl-mcp::write`) is a *declared* write capability
  whose own existing test,
  `linking_lineage_asks_for_the_capability_that_cannot_apply`, proves it is
  not actually wired to write anything yet.

So `asset_subgraph`'s own multi-hop behaviour has never had a real-Postgres
test anywhere in this codebase — `GET /assets/{id}/graph`, the pre-existing
HTTP route wrapping the same method, has zero integration tests of its own,
checked directly (`grep` across `graph-owl-server/tests/` for the route
found nothing). This `traverse` slice inherits that gap rather than
introducing it, and closing it means building the missing asset-relationship
write path first — real, separate work, not a test this slice can add
without it.

**The lone-asset test that was written proves the real path (fqn resolution,
authorization, `Subgraph`→`TraversalContext` conversion) is genuine**, since
a BFS walk always includes its own seed at depth zero — a lone asset with no
relationships still returns exactly one node, which is what distinguishes a
real call from both `Default::default()` and `None`.

## What this deliberately does not do

- **Seven of P10's eight tools remain unbuilt**: `resolve_entity()`,
  `reconcile()`, `find_evidence()`, `run_rule()`, `calculate_risk()`,
  `analytics()`, `explain()`. Several of these likely wrap existing
  `Catalog` methods the same way `traverse` wraps `asset_subgraph` (e.g.
  `find_evidence()` is a strong candidate to wrap
  `Catalog::finding_evidence_graph`, already shipped in `105e`), but each
  needs its own authorization-scoping check before being wired — the same
  question this slice had to answer for `traverse` vs `graph_context`.
- **No asset-to-asset relationship write path.** The mutation gap above is
  named, not closed. A future slice building real inter-asset lineage
  writes should close it and re-run this file's mutation report.
- **P11 (the LangGraph agent) and P12 (the eval harness) are untouched.**
  Both depend on more of P10's tool surface existing than one tool provides.
