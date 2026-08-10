# Plan: Native reconcile engine — the platform plan's P5, actually in Rust (Epic 105 continuation)

**Branch**: main (committed directly, per `CLAUDE.md`)
**Status**: **Starting 11 August 2026.**
**Depends on**: Epic 105 DN-1/DN-2/DN-3 (shipped), P0/P0b (shipped), the findings storage+queue slice (`/findings`, `Catalog::record_findings`, `findingsQueue.tsx` — all shipped and confirmed live in `ReviewSection.tsx`)
**Crates**: `graph-owl-resolution` (pure comparison primitives) · `graph-owl-engine` (port) · `graph-owl-storage-postgres` (adapter) · `graph-owl-api` (orchestration) · `graph-owl-server` (route) — **no new crates**, placed by the same three-way rule DN-1/DN-2 already used.

## Why this exists

`connectors/python/graph_owl_packs/reconcile.py` is a working, well-tested rule evaluator — but it is Python, and `.claude/docs/referencePlans/markdown/CA-GST/25-graphowl-intelligence-platform.md` decision 4 states the platform invariant it violates: *"no Python matcher/scorer... no Python rule evaluator... Rust owns deterministic graph intelligence, Python orchestrates."* `plans/105-domain-neutrality.md`'s own "GST console surface" section names the gap directly: *"the findings runtime (the platform plan's P5) does not exist."* The findings **storage and review queue** shipped (confirmed by reading the code, not the task tracker — `record_findings`/`decide_finding`/`findingsQueue.tsx` are real and wired). What never got ported is the part that turns a query result into a finding: n-gram similarity banding, date-span arithmetic, and evidence construction from a manifest's `[[findings]]` rule.

Practically, this is also the fix for "I upload a file and nothing happens" — today, landing facts and producing findings are two disconnected steps, the second only reachable from a CLI a console user cannot run.

## Design

**Registry, not a parser.** Rust does not gain a TOML reader. Exactly the P0b pattern (`POST /namespaces`, `POST /predicates`): the Python pack loader reads `pack.toml`'s `[[findings]]` + `[[queries]]` at install time and registers each rule — SPARQL text inlined, not a file path — through a new endpoint. Rust never parses pack manifests; it only ever executes what was registered, the same posture `graph-owl-core` already holds toward namespaces and predicates.

**The comparison primitives are pure and placed in `graph-owl-resolution`.** N-gram Jaccard similarity and date-span-exceeds-days arithmetic are decision logic over already-retrieved values — no I/O — which is exactly what `00e` rule 4 reserves that crate for, and exactly the reasoning DN-1/DN-2 already applied. `graph-owl-resolution` already holds jaro-winkler string similarity for ER; n-gram similarity is a second, additive comparison strategy in the same family, not a new concern.

**The query itself runs in-process, not over loopback HTTP.** `reconcile.py` calls `POST /sparql` because Python has no other way in. Rust does: `Catalog::sparql(principal, query, as_of, budget)` is already the exact function `/sparql`'s own handler calls. The new orchestration calls it directly.

```
Python pack loader (unchanged 3-phase order, +1 phase)
  namespace → predicates → documents → finding rules
                                          │
                                          ▼
                         POST /packs/{id}/finding-rules  (admin, new)
                                          │
                                          ▼
                    finding_rules registry (Postgres, mirrors namespaces/predicates)
                                          │
                                          ▼
                     POST /packs/{id}/reconcile  (admin, new)
                       for each registered rule:
                         Catalog::sparql(...)              — existing, in-process
                         → graph_owl_resolution::rule_match — NEW, pure (Slice A)
                             ngram_similarity() / passes_span()
                         → Finding::new(...)                — existing
                       → Catalog::record_findings(...)      — existing
                       ⇒ { pack, evaluated, found, opened, alreadyOpen }
                                          │
                                          ▼
                    Console: "Run reconciliation" button (Slice E, config only)
                       → findingsQueue.tsx already renders the result — unchanged
```

**`reconcile.py`'s rule-evaluation logic is deleted, not kept as a fallback.** Two live implementations of the same decision is exactly the failure the platform doc's "no duplicated engines" section exists to prevent — keeping the Python version "just in case" is how that rule gets violated quietly. The CLI becomes a thin `POST /packs/{id}/reconcile` trigger.

## Slices

### Slice A — pure comparison primitives (`graph-owl-resolution::rule_match`)

- [ ] RED: `ngram_similarity("exact", a, a) == 1.0`; two values differing by one transposed pair score identically to `reconcile.py`'s own fixture numbers (0.619 / 0.065 from `pack.toml`'s own comment) — pinning the port to the same manifest thresholds already tuned against real data
- [ ] RED: `at_least`/`at_most` band is inclusive at both ends; unknown strategy is a named error, never a silent 0.0
- [ ] RED: `passes_span` — exact two-event case (`end - start > exceeds_days`, strictly greater, 180-day boundary itself does not fire); `when_missing: "elapsed"` measures from `as_of` when given, else "today" — but "today" cannot be asserted equal in a test, so assert only that an explicit `as_of` is used when present and that the function requires one or the other explicitly documented; `when_missing: "finding"` always passes; default `when_missing` (absent) does not fire
- [ ] RED: malformed ISO-8601 date is a named error, never treated as "no second event"
- [ ] Implement `ngram_similarity`, `passes_span_days`, error type
- [ ] MUTATE — `--file`, since this is a new file; expect boundary mutants on `>=`/`<=` and `>` in the day comparison, the padding length, and the `when_missing` match arms
- [ ] KILL MUTANTS

### Slice B — finding-rule registry (mirrors DN-1's namespace registry exactly)

- [ ] RED (port, in-memory double first): declaring a rule for a pack that has none returns it in `finding_rules(pack)`; re-declaring an identical rule is idempotent; a rule naming an unknown pack is not itself validated (packs are Python's concern, not this registry's)
- [ ] `FindingRuleRegistry` port trait in `graph-owl-engine` (`declare`, `for_pack`) — mirrors `NamespaceRegistry` shape exactly
- [ ] Migration `V61__finding_rules.sql`: `(pack, label)` unique, columns for `summary`, `governed_by`, `subject_var`, `query_text`, `evidence` (jsonb), `similarity` (nullable jsonb), `span` (nullable jsonb)
- [ ] Postgres adapter implementing the port
- [ ] `Catalog::declare_finding_rule` / `Catalog::finding_rules(pack)` — third optional field on `Catalog`, the same precedent `namespaces`/`traversal` already set
- [ ] `POST /packs/{id}/finding-rules` (admin) + `GET /packs/{id}/finding-rules` (admin — these are rule definitions, not data; same sensitivity tier as `POST /predicates`)
- [ ] Wired into all three composition roots (`main.rs`, stdio binary, test harness) — the P0b lesson: an unwired registry 500s with a message saying so, never silently `None`
- [ ] Integration tests against real Postgres (idempotent re-declare on reload; a rule persists across a fresh resolver build, i.e. survives what DN-3's finding 2 called "a restart would silently un-resolve every pack")
- [ ] Contract regenerated; mutation run on the port logic

### Slice C — `POST /packs/{id}/reconcile`

- [ ] RED (unit, `graph-owl-api`, fake `FindingRuleRegistry` + fake SPARQL): zero rules for a pack → `evaluated: 0, found: 0`, no write; a rule whose query returns rows with the similarity band failing produces no finding; a rule with a `subject` variable absent from a row is a named error (mirrors `reconcile.py`'s own `ReconcileError` for exactly this — a query edited to rename a variable must not fail silently); evidence entries whose `var` is unbound (OPTIONAL) are dropped, not the whole finding
- [ ] Orchestration function in `graph-owl-api`: run each registered rule's query via `Catalog::sparql`, filter via Slice A's primitives, build `Finding`s, call `record_findings`
- [ ] `POST /packs/{id}/reconcile` route (admin) in `graph-owl-server`, returning `{ pack, evaluated, found, opened, alreadyOpen }` — same shape `ReconcileResult` already has, so nothing downstream (the eval harness, if ever pointed at this) needs to change its expectations
- [ ] Integration test against real Postgres + the actual GST pack fixtures: run against the clean fixture data, assert the same six-finding baseline `verify-pack-load.sh` already asserts for the Python path — this is the parity proof, not a new behavior spec
- [ ] Mutation run

### Slice D — Python: register, don't evaluate

- [ ] Pack loader gains a fourth phase: after documents, read `[[findings]]` + `[[queries]]`, inline each named query's `.sparql` file, `POST /packs/{id}/finding-rules`
- [ ] `reconcile.py`'s `similarity`/`_passes_span`/`_rows_to_findings`/`run_findings` deleted; `graph-owl-load-pack reconcile <id>` becomes a thin `POST /packs/{id}/reconcile` call
- [ ] `scripts/demo.sh --gst` updated: pack install now also registers rules; the reconcile step becomes the HTTP trigger
- [ ] `graph_owl_packs` test suite updated — the deleted unit tests for `similarity`/`_passes_span` move to Slice A's Rust tests (same fixture numbers), not lost

### Slice E — console: click, and reconciliation happens

- [ ] "Run reconciliation" action added to `PackImportPanel.tsx` (or a small sibling in `features/packs/`) — POSTs `/packs/{id}/reconcile`, shows the `{evaluated, found, opened, alreadyOpen}` summary, no new route (§13)
- [ ] `api.ts` gains `reconcilePack(id)`
- [ ] Verify live: upload a file, click Run reconciliation, see the result summary, then see the same findings appear in Review → Findings without a page reload
- [ ] `routes.structural.test.ts` still green (no route added)

## Acceptance criteria

- [ ] Uploading a GST file through the console and clicking one button produces findings visible in Review → Findings, with zero CLI involvement.
- [ ] `reconcile.py` no longer contains similarity or span-arithmetic logic — grep for it comes back empty.
- [ ] The Rust engine reproduces the exact six-finding baseline the Python engine already produced against the same fixtures (parity, not a redesign).
- [ ] A rule with a malformed manifest (unknown similarity strategy, unbound subject variable) fails loudly with the rule's own label named, matching `reconcile.py`'s existing error discipline.

## Pre-PR quality gate

1. `cargo mutants` on `graph-owl-resolution::rule_match` (Slice A, `--file`) and the new registry/orchestration logic (`--in-diff`).
2. `cargo test/clippy/fmt` on touched crates only (`graph-owl-resolution`, `graph-owl-engine`, `graph-owl-storage-postgres`, `graph-owl-api`, `graph-owl-server`).
3. Live verification against the real demo server (upload → click → findings), not just unit tests — the whole point is the console path works end to end.
4. `packs/gst/` fixture parity check: the Rust path's findings count/labels match `verify-pack-load.sh`'s existing Python-derived assertions.
