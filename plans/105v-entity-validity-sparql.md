# Plan: entity-validity SPARQL time travel — P8's fourth and last named gap

**Branch**: main (committed directly, per `CLAUDE.md`)
**Status**: Shipped 11 August 2026, continuing the same completion pass as
`105u`.
**Crates**: `graph-owl-api` only (`Catalog::sparql_valid_at`, a new
private `sparql_scoped`, `entity_windows`/`entity_is_valid`). No changes
to `graph-owl-query`, `graph-owl-engine-postgres`, or `graph-owl-traversal`.

## What `105i` named and did not attempt, read precisely

`105i`'s own closing line: "No entity-validity extension of `as_of`/
time-travel at the storage layer. This resolves *already-fetched*
candidate rows; it does not change how the graph engine answers a
query." That is a different question from `105u`'s date-window
traversal (a graph *walk* filtered to valid entities) — this is SPARQL's
own visible-facts computation, the same layer `as_of` (transaction time)
already occupies, extended to a second temporal axis (valid time).

## Why a new method, not a new parameter on `Catalog::sparql`

`Catalog::sparql`'s signature has over fifty call sites — the bulk of
this crate's own SPARQL test suite, plus `graph-owl-mcp` and
`graph-owl-server`. Adding a parameter there means fifty mechanical
edits for a capability meant to be opt-in. Instead: `Catalog::sparql`
now delegates to a new private `sparql_scoped(..., valid_at: Option<&
ValidityWindow>, ...)`, and a new public `Catalog::sparql_valid_at`
delegates to the same method with `Some`. Every existing caller of
`sparql`, `execute_algebra`, and `scoped_facts` is unaffected —
mechanically, not merely by behaviour: `sparql`'s own public signature
never changed.

`ValidityWindow` is reused directly from `graph-owl-traversal` (`105u`)
rather than a second, parallel type — `graph-owl-api` already depends on
that crate for `EdgeFilter`, and the predicate-supplied-by-the-caller
design is identical: pack-scoped `effectiveFrom`/`effectiveTo` are never
hardcoded here either.

## The bug the RED test found before GREEN did

The first implementation filtered `all` — the flakes SPARQL's own
pushdown planner (`scans_for`) already fetched for the query pattern —
using a validity lookup built from that *same* set. It compiled, and two
of three RED tests passed immediately, which was the trap: a query like
`SELECT ?n WHERE { ?s dsc:name ?n }` never fetches
`effectiveFrom`/`effectiveTo` at all, because pushdown narrows to
exactly what the query names. A dated subject's own window was silently
absent from `all`, which `entity_is_valid`'s own "no window on record ⇒
always valid" rule (the correct behaviour for a genuinely undated
subject) then read as "this subject has no window," passing it through
regardless of whether it actually should have been excluded.

**Only the third test — a subject genuinely outside its window —
distinguished the bug from working code**, and it failed first: the row
came back anyway. Fixed with a dedicated fetch: when `valid_at` is
`Some`, two extra `query_pattern` calls (one per predicate, `TriplePattern
{ p: Some(predicate), as_of, .. }`) build the validity lookup
independently of what the caller's own query asked for — the same shape
`graph-owl-engine-postgres`'s `push_invalid_nodes` (`105u`) already uses:
querying broadly by predicate, not by the caller's own pattern.

This is exactly this project's own recurring mutation-testing lesson,
found here by a positive/negative pair before a mutation run was ever
needed: two tests that only exercise the "included" and "undated" cases
would have shipped this bug with 100% green.

## Mutation report

`--in-diff`, `--lib` (all RED tests are `graph-owl-api`'s own fast unit
tests, no Postgres). First run: **34 mutants, 16 caught, 15 unviable, 3
missed.** Two of the three missed were real gaps, closed by two more
tests:

- `entity_is_valid`'s `<` → `<=` boundary survived because every existing
  test checked a date well past the window, never *on* `effective_to`
  itself — fixed with
  `sparql_valid_at_excludes_a_subject_exactly_on_its_effective_to_date`,
  mirroring `105u`'s own boundary test.
- Deleting the validity fetch's `as_of` field survived because no test
  combined `valid_at` with a non-`None` `as_of` — fixed with
  `sparql_valid_at_judges_the_window_as_it_stood_at_as_of`, which found a
  real bug in the test itself first: retracting the original window at
  the *same* transaction time as its own assertion (rather than at the
  later time the replacement was asserted) ties against it under this
  test double's own "on a tie, the retraction wins" rule, hiding the
  original value even at the earlier `as_of` the test meant to prove was
  unaffected.

The third — deleting the `p` field from the validity fetch's own
`TriplePattern` — stayed missed after the fix, documented in-code (see
"A dedicated fetch" above) as a real but non-correctness distinction:
`entity_windows` re-filters by predicate identity regardless of what was
fetched, so no test can observe the difference without measuring fetch
volume on an estate larger than a unit test's own fixture.

**Second run, confirming the fix**: 34 mutants, 18 caught, 15 unviable,
**1 missed** — the one documented above, and no others.

## What this deliberately does not do

- **Filters by subject only, not by Ref-valued object.** A flake whose
  object references an invalid entity (rather than whose subject *is*
  one) is not excluded by this slice — a subtler question (should a
  *reference to* something invalid also disappear) left named rather
  than silently resolved either way.
- **No HTTP route.** `Catalog::sparql_valid_at` has no
  `GET /sparql?valid_at=...` counterpart yet; wiring a real caller (MCP,
  console, or an HTTP query param) is separate work once one needs it —
  the same posture `105j`'s `GraphContext` took toward its own missing
  route.
- **No pack-config surface** naming which predicates are "the" validity
  window for a pack's own concept, matching `105i`'s and `105u`'s
  identical, deliberate omission.
