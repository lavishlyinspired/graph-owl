# Plan: date-window traversal — P8's third remaining gap

**Branch**: main (committed directly, per `CLAUDE.md`)
**Status**: Shipped 11 August 2026, at the user's explicit direction to
complete the full list of P8/P9/F1/P12/connector/policy gaps recorded
after the Epic 105 completion review.
**Crates**: `graph-owl-traversal` (the `ValidityWindow` type),
`graph-owl-traversal-memory` (in-process filtering), `graph-owl-engine-postgres`
(the CTE), `graph-owl-api` (4 call sites updated for the new `EdgeFilter`
field, no behaviour change — every one passes `valid_at: None`).

## What `105i` left open, and why this is a separate slice

`plans/105i-temporal-validity-resolution.md` built
`graph_owl_resolution::temporal::{EffectivePeriod, in_force_at}` —
resolving *already-fetched* candidate rows — and named this explicitly as
what it did **not** attempt: "the graph walk itself filtered to entities
valid at a date... predicate names are pack-specific, not fixed, so this
cannot be a hardcoded traversal filter without breaking domain
neutrality, and threading pack-scoped predicate names through three
backend implementations is real, separate, multi-file work."

That is exactly this slice's scope, and the predicate-name problem is
solved the same way `EdgeFilter::relationship_types` already solves it
for relationship vocabulary: the caller supplies the predicates
(`ValidityWindow::{effective_from, effective_to}` are `Sid`s, not
strings), so nothing in `graph-owl-traversal` or either backend needs to
know a pack's own naming.

## What was built

- `graph_owl_traversal::ValidityWindow { effective_from: Sid, effective_to:
  Sid, at: NaiveDate }`, and `EdgeFilter::valid_at: Option<ValidityWindow>`.
  `graph-owl-traversal` gained a `chrono` dependency; it does **not**
  depend on `graph-owl-resolution` — the single containment check
  (`effective_from <= at && effective_to.is_none_or(|end| at < end)`) is
  small enough that owning it locally, in both backends, does not cost
  the coupling a shared dependency would (mirrors `00l`'s own "a five-line
  rule does not clear the bar for a dependency" judgment, applied to a
  crate edge rather than an external one).
- **In-process** (`graph-owl-traversal-memory`): `fetch_edges` already
  fetches every current flake in one call; `node_windows` builds a
  `HashMap<Sid, NodeWindow>` from the *same* fetched flakes (no second
  query), and `edges.retain(...)` excludes any edge whose `from` or `to`
  endpoint fails its own window — applied before the BFS runs, so an
  excluded node cannot be a pass-through to a further one.
- **Postgres** (`graph-owl-engine-postgres`): a new `invalid_nodes` CTE,
  grouping `live` by subject and computing each dated node's own
  `effective_from`/`effective_to` via `MAX(value_str) FILTER`, compared
  as **text** — matching `amount-mismatch.sparql`'s own established
  convention that ISO-8601 sorts correctly as text, not a date cast. The
  existing `filtered` CTE (previously only the `relationship_types`
  branch) was generalized to `WHERE TRUE AND ...` so both constraints
  compose, and extracted into its own `push_filtered` function once the
  combined logic pushed `push_logical_edges` over the 100-line pedantic
  threshold.
- **A node with only `effective_to` and no `effective_from` is left
  valid, in both backends** — a real dated entity always has a start, so
  this is malformed data rather than a real half-open window; failing
  open (not excluding it) avoids silently hiding a node over data no
  caller asserted correctly. In SQL this falls out of `NULL <= at` being
  unknown rather than false, so `HAVING NOT (unknown)` keeps no row.

## The RED tests

Identical fixtures and assertions added to both
`graph-owl-traversal-memory/tests/traversal.rs` and
`graph-owl-engine-postgres/tests/traversal.rs`, continuing this file
pair's own established differential-testing discipline ("two
implementations of one trait that are not differentially tested are two
behaviours, not one"): not-yet-valid excluded, no-longer-valid excluded,
inside-the-window reached, the inclusive/exclusive boundary (two nodes on
either side of the same date), an open-ended window valid arbitrarily far
in the future, an undated node reached regardless of the filter, and —
the mutation-relevant one — an excluded node cannot be a pass-through to
a node beyond it (proven by asserting the further node is *also*
unreached, not merely that the excluded one is).

**The Postgres side needed one extra step the memory side didn't**:
`effectiveFrom`/`effectiveTo` are not among `V3__predicate_registry.sql`'s
seeded DSC predicates (unlike `fromEntity`/`toEntity`/`relType`), so
writing them needs an explicit `define` first — the identical
`UnregisteredPredicate` requirement `105e`'s own
`register_pack_predicate` helper already established for a non-DSC
namespace, applied here to two DSC predicates that simply were never
shipped.

## Mutation report

**`graph-owl-traversal-memory/src/lib.rs`**, `--in-diff`: **16 mutants,
14 caught, 2 unviable, 0 missed.**

**`graph-owl-engine-postgres/src/traversal.rs`**, `--in-diff`, scoped to
the `traversal` integration binary at `--test-threads=1` (not `--lib`,
which this code has none of — every mutant here is only reachable through
the real `TraversalEngine` integration tests, the same
`--lib`-blindness gap `CLAUDE.md`'s own `observability.rs` finding
already documents): **3 mutants, 3 caught, 0 missed.** Fewer mutants than
the memory side because most of the Postgres change is SQL text pushed as
string literals, which cargo-mutants has nothing to mutate inside; the
real mutable surface (the `is_some()`/`if let Some` branches, the
`i32::from`/`.clone()` calls) is exactly what got caught.

## What this deliberately does not do

- **No pack-config surface wiring a specific rule's own dated concept
  into a traversal call automatically.** `ValidityWindow` is a primitive
  a caller constructs explicitly; nothing in this slice changes what
  `Catalog::traverse`/`finding_evidence_graph`/any other existing caller
  passes — every one still passes `valid_at: None`, unchanged behaviour.
  Wiring a specific console or MCP surface to *offer* date-window
  traversal is separate, future work once a real caller needs it —
  matching `105i`'s own precedent of not rewiring a passing query for
  cosmetic reasons.
- **Does not resolve "which of several candidate periods is in force"** —
  that is still `graph_owl_resolution::temporal::in_force_at`'s job, for
  already-fetched rows. This slice answers a different question: does
  *this one node's own* window cover the date, during a live walk.
