# Plan: `run_rule()` — P10's sixth MCP intelligence tool

**Branch**: main (committed directly, per `CLAUDE.md`)
**Status**: Shipped 11 August 2026, continuing "complete P10 first" by
direct user instruction.

**Crates**: `graph-owl-api` (the new `Catalog::run_rule` method),
`graph-owl-mcp` (the tool), `graph-owl-server` (a real-Postgres proof,
reusing `reconcile.rs`'s own GST fixture).

## What was built

- `Catalog::run_rule(principal, pack, label)` — the single-rule
  counterpart to `reconcile_pack`, evaluating exactly one of a pack's
  registered rules and recording what it concludes. **Reuses
  `finding_rules` rather than adding a registry lookup method**:
  `FindingRuleRegistry` has no per-label fetch, and a pack's rule count is
  small enough (six for GST) that filtering the already-fetched list costs
  nothing a new registry method would save. `NotFound` when the pack has
  no rule with that label — a rule's key is `(pack, label)`, not `label`
  alone, so the identical label registered under a different pack does
  not match.
- `ContextSource::run_rule` / `RUN_RULE` / its `ToolDeclaration` / the
  dispatch arm — mirroring `reconcile`'s shape throughout, including
  **admin-gating for the identical reason**: this call writes (a matched
  rule's finding lands in the review queue), the same side effect
  `reconcile` has, just narrower in scope. There is no HTTP route this
  wraps (unlike `reconcile`, which mirrors `POST /packs/{pack}/reconcile`'s
  existing gate) — the posture is carried over from `reconcile` rather
  than re-derived, because it is the same side effect.
- **No new wire type.** `graph_owl_api::ReconcileOutcome` is reused
  directly as `Outcome::Reconciled`'s payload, the same variant
  `reconcile()` already produces — `evaluated` (1 vs. the whole pack's
  rule count) is what tells the two calls apart on the wire, not the
  `Outcome` variant.
- **No `budget::Fits` impl**, matching `reconcile`'s own precedent: five
  scalar fields, nothing to shrink.

## The Catalog-layer RED test, and what it found about this codebase's own SPARQL scoping

The first draft of `run_rule_tests::evaluates_only_the_named_rule_not_every_rule_in_the_pack`
seeded two rules in one pack, asserted a flake matching each, and expected
one match. It found zero, twice, for two different real reasons — not
invented ones:

1. **`Catalog::sparql` scopes every scan to `visible`, built from real
   asset rows in storage** (`scoped_facts`), not from whatever the graph
   happens to hold. A raw flake asserted under a hand-picked `Sid::dsc(...)`
   subject with no matching `Asset` row is invisible to every query — the
   same trap `asset_analytics_tests` (`105o`) already had to route around
   by seeding through `upsert_asset` rather than a bare `Sid`. Fixed by
   registering real assets and using their real `id` as the flake's
   subject.
2. **`Flake::assert` (the constructor `upsert_asset`'s own projection
   uses) sets `cx: None` — the default graph.** The test's hand-built
   flakes used `cx: Some(...)`, a *named* graph, which a `GRAPH`-less
   `SELECT` never sees. `facts_scanned` in the debug trace showed the one
   intended fact passing the visibility filter, with the SPARQL evaluator
   itself still producing zero solutions — the signal that pointed at
   named-graph exclusion rather than a visibility bug. Fixed by using
   `cx: None`, matching this system's own convention for asserted (not
   imported) facts.

Both are genuine properties of `Catalog::sparql`'s existing scoping
behaviour, not bugs introduced by this slice — they were simply never hit
by a `graph-owl-api`-level unit test before, because `reconcile_pack`
itself (the method `run_rule` is the single-rule counterpart to) has **no**
`graph-owl-api`-level unit test of its own; its only prior coverage is
`graph-owl-server/tests/reconcile.rs`'s HTTP-level proof. `run_rule` is
the first test at this layer to exercise real SPARQL execution against a
`RecordingGraph`-backed catalog for a finding-rule-shaped query, and the
two failures above are exactly what that new exposure found.

## Mutation report

**`lib.rs`'s dispatch, wire declaration, and manifest list** — `--in-diff`,
`--lib` scoped: **4 mutants, 2 caught, 2 unviable, 0 missed**. The unviable
pair matches the now-familiar `Ok(Some(Default::default()))`-shaped gap:
`ReconcileOutcome` derives no `Default` (`105b`'s own deliberate choice),
so the fallback mutation does not compile.

**`Catalog::run_rule` itself** — `--in-diff`, `--lib` scoped: **2 mutants,
1 caught, 1 unviable, 0 missed**. Caught: `r.label == label` flipped to
`!=` inside the rule lookup, killed by
`evaluates_only_the_named_rule_not_every_rule_in_the_pack` selecting the
*other* declared rule and asserting the wrong subject would have been
recorded. Unviable: the same `Ok(Default::default())` whole-function
fallback.

**`catalog.rs`'s production adapter** — scoped to
`cargo test -p graph-owl-server --test reconcile -- run_rule` (the new
real-Postgres test, reusing `reconcile.rs`'s own GST fixture and the exact
query text `register_missing_in_gstr2b_rule` already registers): **3
mutants, 2 caught, 1 unviable, 0 missed**. Caught: the `who.is_admin` gate
condition and the whole-function `Ok(None)` fallback — both meaningfully
exercised because the new test asserts on the admin-success, non-admin-
refusal, *and* unknown-rule paths together. Unviable: the same
`Ok(Some(Default::default()))` shape every P10 tool so far has hit,
because `ReconcileOutcome` derives no `Default`.

## What this deliberately does not do

- **Does not prove the `(pack, label)` composite-key scoping against a
  real Postgres-backed finding-rule registry** — only against the
  `graph-owl-api` unit-level `FakeFindingRuleRegistry`. The registry
  adapter itself (`graph-owl-storage-postgres`) is unmodified and was not
  re-verified here; `run_rule` calls the same `finding_rules` method
  `reconcile_pack` already exercises against Postgres in
  `reconcile.rs`'s HTTP-level tests.
- **Does not add a per-label registry fetch.** See `Catalog::run_rule`'s
  own doc comment for why filtering the already-fetched per-pack list is
  the right size for this, not a new `FindingRuleRegistry` method.
