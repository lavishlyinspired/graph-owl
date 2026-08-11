# Plan: `reconcile()` — P10's fourth MCP intelligence tool

**Branch**: main (committed directly, per `CLAUDE.md`)
**Status**: Shipped 11 August 2026, continuing "keep going tool by tool and
complete P10."
**Crates**: `graph-owl-mcp` (the tool), `graph-owl-server` (a real-Postgres
proof, reusing `reconcile.rs`'s own GST fixture).

## What was built

- `ContextSource::reconcile(principal, pack)` — wraps
  `Catalog::reconcile_pack` (Epic 105 P5b, `105b`), the same computation
  `POST /packs/{pack}/reconcile` already serves (the console's own "Run
  reconciliation" button).
- No new wire type: `graph_owl_api::ReconcileOutcome` already derives
  `Serialize` with `camelCase` renaming (Epic 105 P5b's own choice, stated
  in its own doc comment as matching `reconcile.py`'s pre-existing Python
  shape) — reused directly as `Outcome::Reconciled`'s payload.
- **No `budget::Fits` impl** — matching `Outcome::Wrote`'s own precedent
  (`write.rs` never calls `budget::fit` either). `ReconcileOutcome` is five
  scalar fields; there is nothing to shrink and no entity list for a
  truncation flag to describe.
- `CatalogContext::reconcile` — the real production adapter.

## The authorization decision — the one tool on this trait that needed a new check

Every prior P10 tool either needed its own fresh authorization analysis
(`traverse`, because `graph_context` had no HTTP precedent) or inherited an
already-open HTTP posture (`find_evidence`, `explain`). `reconcile` is
neither: **it writes**. Evaluating a pack's rules and recording what they
conclude puts new rows in the findings/review queue as a side effect, and
the HTTP route this wraps is admin-gated because of that
(`if !principal.is_admin { return Err(AppError::NotFound); }`) — the
identical gate `GET /packs/{pack}/finding-rules`, immediately above it in
`graph-owl-server`, applies for the identical reason, confirming this is an
established convention rather than a one-off judgement call for this
route specifically.

`ContextSource::reconcile` therefore checks `principal.is_admin` itself and
returns `Ok(None)` for a non-admin — the same "absent and denied are
indistinguishable" property every other tool on this trait already holds,
now protecting a write rather than a read. This does **not** mean
`reconcile` belongs in `WriteSink` instead: every existing write tool
(`propose_metadata_change`, `record_investigation`, …) is shaped around one
human-narrated assertion with a required rationale and confidence, subject
to review; `reconcile` is a bulk, deterministic, pack-rule-driven
computation with neither, and does not fit that shape. It stays a
`ContextSource` method with its own authorization check layered on top —
the same shape the HTTP route itself already uses.

## Mutation report

**`lib.rs`'s dispatch and argument parsing** — `--in-diff`, `--lib`
scoped: **4 mutants, 2 caught, 2 unviable, 0 missed**, clean on the first
pass. The unviable pair is `Ok(Some(Default::default()))`-shaped: like
`TraversalContext`/`EvidenceContext`/`FactExplanation` before it,
`ReconcileOutcome` derives no `Default` (a deliberate choice already made
in `105b`, not something this slice added), so cargo-mutants' fallback
mutation does not compile.

**`catalog.rs`'s production adapter** — scoped to a new real-Postgres test
reusing `reconcile.rs`'s own GST fixture
(`seed_gst_vocabulary_and_one_unmatched_invoice` /
`register_missing_in_gstr2b_rule`), proving both halves of the admin gate
for real: `Principal::system()` (`is_admin: true`) gets a genuine
`ReconcileOutcome` with real counts, and a hand-built non-admin `Principal`
(`is_admin: false`) gets `Ok(None)` — the property stated in the trait doc
comment, and the one property no earlier test in this codebase could
exercise, since every existing HTTP test resolves to `Principal::system()`
in open mode regardless of which subject the bearer token claims. **Clean
result**: 2 caught (the whole-function `Ok(None)` fallback, and the
`is_admin` gate condition itself — both meaningfully exercised because the
new test asserts on *both* the admin-success and non-admin-refusal paths),
1 unviable (`Ok(Some(Default::default()))`, no `Default` on
`ReconcileOutcome`), 0 open gaps specific to `reconcile`. The run's other 4
MISSED lines are pre-existing and unrelated — `search`'s `AssetFilter.kind`,
`traverse`'s and `find_evidence`'s own already-documented `max_hops` gaps,
and `apply_directly`'s `description` field — an artefact of `--re` matching
against the whole file rather than filtering mutant generation, the same
quirk `105k`/`105l` already recorded.

## What this deliberately does not do

- **No `run_rule()` yet.** `reconcile` always evaluates every rule a pack
  has registered; running exactly one named rule is P10's next tool
  (`105o`), and needs a small new `Catalog` method this slice does not add.
- **`reconcile_pack`'s own HTTP route is not migrated to reuse this tool**,
  or vice versa — both call the identical `Catalog` method independently,
  matching the same "do not refactor a shipped path inside an unrelated
  slice" precedent `105j`/`105m` already established.
