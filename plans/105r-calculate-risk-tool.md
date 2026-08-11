# Plan: `calculate_risk()` — P10's eighth and last MCP intelligence tool

**Branch**: main (committed directly, per `CLAUDE.md`)
**Status**: Shipped 11 August 2026 — completes Epic 105 P10, "the eight
MCP intelligence tools," by direct user instruction ("complete p10
first").

**Crates**: `graph-owl-api` (the new `Catalog::calculate_risk` method,
narrowing the existing `obligation_calendar`), `graph-owl-mcp` (the tool),
`graph-owl-server` (a real-Postgres proof, reusing
`obligation_calendar.rs`'s own GST fixture).

## The scope question this had to answer first

The platform doc names `calculate_risk()` without specifying a shape, and
the only existing "risk" concept anywhere in this codebase is the
obligation calendar console route's `DUE_SOON_HORIZON_DAYS = 30` bucket
(`overdue`/`dueSoon`/`upcoming`) — and that constant's own comment already
says what it is: *"there is no pack-config field for it yet, so it is a
display threshold, not a business rule any finding depends on."* Building
`calculate_risk()` on top of it would hand an agent a UI convenience
dressed up as a computed fact.

**Resolved by refusing to invent a score.** The only non-invented signal
this system has for "how at-risk is this" is
[`Obligation::days_remaining`] — real calendar arithmetic (`due − today`),
already computed by `obligation_calendar` (P8/F4, shipped), negative once
overdue. A single numeric "risk score" would need a
severity/probability/impact weighting this system has no basis for
(`00i` rule 4: every magic number needs a stated reason, and none exists
for turning "12 days overdue" into "risk: 73"). `calculate_risk()`
narrows `obligation_calendar` from every open obligation a pack tracks to
the one subject an agent is asking about, and reports the real number —
nothing synthesized.

## What was built

- `Catalog::calculate_risk(principal, pack, subject)` — calls
  `obligation_calendar` and filters to one subject. No new query, no new
  rule-evaluation path; the entire method is a filter over an
  already-shipped computation.
- `ContextSource::calculate_risk` / `CALCULATE_RISK` / its
  `ToolDeclaration` / the dispatch arm — **open, not admin-gated**, unlike
  `reconcile`/`run_rule`: this only reads (`obligation_calendar` writes
  nothing), so it inherits the same open posture `search`/`resolve_entity`
  have rather than `reconcile`'s write-triggered gate.
- **No `Option` wrapper, no new wire type.** Reuses `Obligation` directly
  (already `Serialize`, camelCase, from P8) as `Outcome::RiskCalculated`'s
  bare `Vec` payload — the same "reuse a plain `Vec<T>`, no wrapper
  struct" shape `Outcome::Recalled` already has. No not-found: pack-domain
  subjects have no identity check to run (`obligation_calendar`'s own
  rows come from SPARQL bindings, never an asset lookup), so "nothing
  open" and "no such subject" are one real, empty answer — the same
  reading `resolve_entity`/`search` already give.
- **No `budget::Fits` impl**, matching `Outcome::Recalled`'s own
  precedent: a bare `Vec<T>` variant never goes through `budget::fit` in
  this dispatcher; one subject's own obligations is not a list this
  system has ever needed to truncate.

## The Catalog-layer RED test

`calculate_risk_tests` reuses the exact fixture shape `run_rule_tests`
(`105p`) already established for real-SPARQL-backed rule tests: real
asset rows (not hand-picked `Sid`s, because `Catalog::sparql` scopes every
scan to real storage rows), `cx: None` flakes (the default graph,
matching `Flake::assert`'s own convention — the same trap `105p`'s own
RED test found and documented). Two subjects seeded with a
`purchasedAt` far enough in the past (`2020-01-01`) and far enough in the
future (`2030-01-01`) that the test is deterministic regardless of when
it actually runs, proving both that the overdue subject's own risk is
negative and that the *other* subject's own answer excludes it — the
negative half of "reports only the named subject's own obligation."

## Mutation report

**`lib.rs`'s dispatch, wire declaration, and manifest list** — `--in-diff`,
`--lib` scoped: **4 mutants, 2 caught, 2 unviable, 0 missed**. Both
unviable mutants were on pre-existing generic dispatch machinery
(`tools()`/`call_within`'s own no-`Default` fallbacks) incidentally
inside the diff's line range, not `calculate_risk`-specific — the one
`calculate_risk`-specific mutant (deleting the whole `CALCULATE_RISK`
match arm) was caught.

**`Catalog::calculate_risk` itself** — `--in-diff`, `--lib` scoped: **3
mutants, 2 caught, 1 unviable, 0 missed**.

**`catalog.rs`'s production adapter** — scoped to a new real-Postgres
test reusing `obligation_calendar.rs`'s own GST fixture
(`seed_one_unpaid_purchase`/`register_payment_overdue_rule`, the same
query `GET /packs/{pack}/obligations`'s own test already proves): **2
mutants, 1 caught, 1 unviable, 0 missed.** The unviable mutant is the
whole-function `Ok(Vec::new())` fallback — viable in principle (`Vec`
always has a `Default`), but the new test's two assertions (the overdue
subject's own negative `days_remaining`, and the unrelated subject's real
empty answer) together leave no way for a constant-empty body to pass
both.

## What this deliberately does not do

- **No risk score.** See above — this is the whole design decision the
  slice rests on, not an omission.
- **Does not resolve subjects that are not already obligation-calendar
  subjects.** `calculate_risk` cannot tell an agent "this subject exists
  but has no obligations" apart from "this subject does not exist" —
  matching `resolve_entity`/`search`'s own posture, and for the identical
  reason: no identity check exists to run for a pack-domain subject.
- **Completes Epic 105 P10.** All eight platform-doc intelligence tools
  (`traverse`, `find_evidence`, `explain`, `reconcile`, `analytics`,
  `run_rule`, `resolve_entity`, `calculate_risk`) are now shipped,
  mutation-tested, and proven against a real adapter. What remains on the
  platform doc's own roadmap is P11 (the LangGraph agent) and P12 (the
  eval harness) — neither of which this slice touches.
