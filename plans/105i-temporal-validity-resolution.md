# Plan: Temporal validity-period resolution — P8's §7 prerequisite, the real minimal form

**Branch**: main (committed directly, per `CLAUDE.md`)
**Status**: Shipped 11 August 2026, at the user's explicit direction to complete "P8's remainder (a real temporal graph engine, generalization beyond GST)."
**Crates**: `graph-owl-resolution` only (`temporal.rs`, pure, I/O-free — `00e` rule 4). No changes to `graph-owl-engine`, `graph-owl-engine-postgres`, `graph-owl-traversal` or `graph-owl-traversal-memory`.

## What was checked before writing anything

The platform doc's §7 in full: "An explicit temporal graph engine in
`graph-owl-engine`: `effectiveFrom`/`effectiveTo` on entities..., date-window
traversal, period arithmetic, and time-travel queries that already exist at
assertion-time (`as_of`) extended to entity validity."

Three claims, checked separately rather than assumed as one bundle:

1. **Period arithmetic is already Rust, and already shipped.**
   `plans/105h-obligation-calendar.md`'s `due = anchor + Duration::days(n)`
   is exactly this — `chrono::NaiveDate` addition, not a SPARQL expression.
   Nothing new needed here; noted rather than re-built.
2. **Entity-validity resolution — "which dated entity is in force" — did
   not exist as a Rust primitive.** `packs/gst/law/rule-36-4.ttl`'s own
   comment already names the workaround: `amount-mismatch.sparql` resolves
   "which provision was in force" by comparing `effectiveFrom` *strings*
   lexicographically inside the query, because ISO-8601 sorts correctly as
   text and the engine has no date arithmetic in expressions at all
   (`date > date` evaluates to unbound, measured and documented when that
   query was written). This is the real gap, and this plan closes it.
3. **Date-window *traversal*** — the graph walk itself filtered to entities
   valid at a date, as opposed to resolving a value once rows are already
   fetched — **is a materially larger, separate piece, not attempted here**.
   `EdgeFilter` already carries `as_of: Option<i64>` for assertion-time
   filtering, but extending it to entity-validity filtering would require
   the traversal engine (`graph-owl-traversal`, `graph-owl-traversal-memory`,
   `graph-owl-engine-postgres`) to know which predicates name
   `effectiveFrom`/`effectiveTo` for an arbitrary pack — predicate names are
   pack-specific, not fixed, so this cannot be a hardcoded traversal filter
   without breaking domain neutrality, and threading pack-scoped predicate
   names through three backend implementations is real, separate,
   multi-file work. Named here as an open gap rather than silently folded
   into "done."

## What was built

`graph_owl_resolution::temporal::{EffectivePeriod, in_force_at}` —
generic over the value a period stands for, so a statutory provision's cap
percentage and a price list's rate are the same shape at this level.
`in_force_at(periods, at)`: the latest-starting period whose window has
already opened and has not yet closed, matching exactly what
`amount-mismatch.sparql`'s string-comparison trick already computes for
GST, generalized to work for **any** dated-entity concept a pack names.

**Two proofs of generalization, not one**, matching the same discipline
`plans/105-domain-neutrality.md`'s hospitality proof-pack already applied
to blocking and matching:

1. Re-derives GST's own real fixture (`rule-36-4.ttl`'s four provisions)
   and gets the same answers `amount-mismatch.sparql` already produces —
   the 2020 invoice resolves to the 10% cap, the 2026 one to nil.
2. Resolves correctly for a domain with **no relationship to GST at all** —
   a freight rate card, open-ended and closed windows both — proving the
   primitive was not accidentally shaped around tax law's own fixture.

**Mutation testing found a real gap in the first test set.** The
`effective_to` exclusive-boundary test used a second period starting
exactly where the first ends, so `max_by_key`'s own tie-breaking picked the
right answer regardless of whether the boundary comparison was `<` or
`<=` — the mutant survived because the test's own design masked it. Fixed
by adding an isolated boundary test: a single closed window with nothing
superseding it, queried exactly on its end date, which only a genuinely
exclusive comparison resolves to `None`.

**Mutation report**: 10 mutants, 6 caught, 4 unviable, 0 missed after the
fix.

## What this deliberately does not do

- No pack-config surface for "declare a temporal-resolution rule." GST's
  `amount-mismatch.sparql` is not rewired to call this primitive — that
  query already works, is tested, and rewiring a passing rule for cosmetic
  reasons is not what "generalization" asked for. The primitive exists and
  is proven; wiring a specific pack rule through it is a future, separate,
  smaller change once a real second consumer needs it.
- No date-window traversal (see point 3 above) — a real, named, separate
  gap, not folded into this slice's "done."
- No entity-validity extension of `as_of`/time-travel at the storage layer.
  This resolves *already-fetched* candidate rows; it does not change how
  the graph engine answers a query.
