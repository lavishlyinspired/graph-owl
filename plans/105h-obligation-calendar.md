# Plan: Obligation calendar — the real first slice of P8

**Branch**: main (committed directly, per `CLAUDE.md`)
**Status**: Slice 1 shipped 11 August 2026, at the user's explicit direction to continue through P8–P12 and the remaining console items without a further confirmation round.
**Depends on**: `plans/105b-native-reconcile-engine.md` (shipped — `[findings.span]`, `SpanRuleConfig`, `Catalog::finding_rules`). **No new pack config** — Slice 1 reuses the existing `[findings.span]` band exactly as written for `PaymentOverdue`.
**Crates**: `graph-owl-api` (`Obligation`, `obligations_from_rows`, `Catalog::obligation_calendar`), `graph-owl-server` (`GET /packs/{pack}/obligations`). No Rust changes to `graph-owl-engine`/`graph-owl-engine-postgres`.

## Why this instead of the platform doc's full P8

The doc's own words: "Event → Legal Rule → Obligation → Due Date → Risk," a
generic Rust runtime reading `rules/` and dated `law/` entities, backed by an
explicit temporal graph engine (§7 — `effectiveFrom`/`effectiveTo` as a
first-class entity-validity concept, "planned," not built), generalized
across GST, ROC, litigation and more.

Checked before writing any code, the same "prove the gap is real" discipline
`plans/00l` and this project's own `grill-me` practice require:

- **§7's temporal engine does not exist.** `gst:effectiveFrom` in
  `packs/gst/law/rule-36-4.ttl` is a plain data property, resolved by
  ordinary SPARQL string comparison (ISO-8601 sorts lexicographically) —
  not an entity-validity primitive, not `as_of`-style time travel extended
  to validity intervals. `amount-mismatch.sparql`'s own comment already
  says as much: the engine has no date arithmetic in expressions at all.
- **`[findings.span]` already *is* a working anchor-plus-period primitive**,
  just read backward. `passes_span` (`graph-owl-resolution::rule_match`)
  computes `(judged_on - start).num_days() > exceeds_days` — a boolean —
  and silently discards `start + exceeds_days`, which is the due date
  itself. Read forward instead of only checked backward, this is most of
  what an obligation calendar needs, for the one rule (`PaymentOverdue`)
  that is already shaped as an anchor/period obligation.

Building the full generic law/rules engine now, generalized to ROC and
litigation domains this project has no fixtures or law text for, would be
designing the general case from a sample size of one — exactly the trap
`plans/00l`'s spike discipline and this plan's own precedent (`105g`'s
near-miss slice) exist to catch. What is real and buildable today is the
due-date half of the one obligation GST already has.

## Slice 1 — Compute due dates for span-configured rules, expose them as a calendar

**Value**: A reviewer can see every open obligation a pack's rules track,
ordered by due date — including ones not yet overdue, which `PaymentOverdue`
findings alone never surface (a finding requires *already* crossing the
threshold).
**Path**: `GET /packs/{pack}/obligations` → `Catalog::obligation_calendar` →
runs every `[findings.span]`-configured rule's query (the exact call
`reconcile_pack` already makes) → `obligations_from_rows` computes
`due = anchor + exceeds_days` and `days_remaining = due - today` per open row
(a row whose `to` variable is already bound is discharged, excluded) → sorted
by due date.

**Acceptance criteria** (met):
- [x] An unpaid invoice's due date is its purchase date plus the rule's
  `exceedsDays` — proven against real Postgres with the real
  `payment-overdue.sparql` text, not a stand-in.
- [x] An obligation already past its due date reports negative
  `daysRemaining`; one not yet due reports positive.
- [x] A discharged obligation (payment event present) does not appear.
- [x] A rule with no `[findings.span]` band contributes nothing.
- [x] `GET /packs/{pack}/obligations` returns the calendar over HTTP,
  unauthenticated-by-admin (read-only, same reasoning `list_findings`
  already gives its own route).

**RED → GREEN → MUTATE**: `obligations_from_rows_tests` (8 unit tests,
mirroring `findings_from_rows_tests`'s own fixture pattern) then
`crates/graph-owl-server/tests/obligation_calendar.rs` (2 real-Postgres
integration tests). `scripts/mutants.sh graph-owl-api --diff <diff>
--cargo-test-arg --lib`: 8 mutants, 5 unviable, 2 caught, 1 **MISSED** —
`Catalog::obligation_calendar` itself replaced with `Ok(vec![])`.

**The MISSED mutant is a structural blind spot in the tooling, not a real
gap — recorded rather than hidden.** `obligation_calendar` calls
`self.sparql(...)`, so its only real coverage is the Postgres-backed
integration test in `graph-owl-server`, a *different crate* than the one
being mutated. `cargo mutants -p graph-owl-api` only ever runs
`graph-owl-api`'s own test command, and can never see a sibling crate's
`tests/` directory — there is no flag to point the mutation run at another
crate's suite while mutating this one's source. This is not new: the sibling
method `reconcile_pack` (same shape, same `self.sparql` call, same
integration-test-only coverage in `graph-owl-server/tests/reconcile.rs`) sits
in the identical blind spot and always has. Manually verified instead: if
`obligation_calendar` returned `Ok(vec![])`,
`an_unpaid_purchase_appears_with_its_computed_due_date`'s
`assert_eq!(obligations.len(), 1, ...)` would fail — the integration test
does catch it, cargo-mutants simply cannot observe that a different crate's
test would fail.

**Done**: acceptance criteria met, mutation report reviewed (the one survivor
explained above), real-Postgres integration test proving the computed due
date, `fmt`/`clippy` clean on touched crates.

## What this deliberately does not do

- No new pack-config table. `[findings.span]` already names anchor, period
  and discharge event; Slice 1 reads it, it does not extend it.
- No console surface yet (F4, "Obligation calendar," the one new route the
  platform doc names) — this slice is the data the calendar would read, not
  the calendar itself. F4 remains open.
- No generalization to ROC, litigation, or any domain without a real fixture
  — GST's `PaymentOverdue` is the only rule this was built and proven
  against.
- No §7 temporal graph engine. `obligations_from_rows` does its own date
  arithmetic in Rust (`chrono::NaiveDate` addition), the same way
  `passes_span` already does — not a graph-level primitive, and not claimed
  to be one.
