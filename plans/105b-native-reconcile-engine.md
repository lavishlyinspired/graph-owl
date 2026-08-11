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

### Slice A — pure comparison primitives (`graph-owl-resolution::rule_match`) — ✅ done, committed `f5ed990`

- [x] RED: `ngram_similarity("exact", a, a) == 1.0`; two values differing by one transposed pair score identically to `reconcile.py`'s own fixture numbers (0.619 confirmed exactly against the pack's real GSTINs) — pinning the port to the same manifest thresholds already tuned against real data
- [x] RED: `at_least`/`at_most` band is inclusive at both ends; unknown strategy is a named error, never a silent 0.0
- [x] RED: `passes_span` — exact two-event case, strictly greater, 180-day boundary does not fire; `when_missing: elapsed`/`finding`/`ignore` all covered
- [x] RED: malformed ISO-8601 date is a named error, never treated as "no second event"
- [x] Implement `similarity`, `passes_similarity`, `passes_span`, `RuleMatchError`
- [x] MUTATE — `--file`; 46/46 mutants (38 caught, 7 unviable, 0 missed) after one fix round
- [x] KILL MUTANTS — Display-renders-nothing, and two off-by-one guards (n=1 boundary, ngrams underflow guard)

### Slice B — finding-rule registry (mirrors DN-1's namespace registry exactly) — ✅ done, uncommitted

- [x] RED (Catalog level, in-memory `FakeFindingRuleRegistry` double): declaring a rule for a pack that has none returns it in `finding_rules(pack)`; a rule is scoped to its own pack; redeclaring the same `(pack, label)` replaces rather than duplicates; a catalog with no registry configured says so
- [x] `FindingRuleRegistry` port trait in `graph-owl-engine` (`declare`, `for_pack`) — mirrors `NamespaceRegistry` shape; `FindingRuleDef`/`EvidenceBinding` types, `similarity`/`span` kept as opaque `serde_json::Value` (typed only where `graph_owl_resolution::rule_match` actually evaluates them)
- [x] Migration `V15__finding_rules.sql` (in `graph-owl-engine-postgres`, not V61 — that number was `graph-owl-storage-postgres`'s own separate sequence): `(pack, label)` primary key, `evidence`/`similarity`/`span` as jsonb
- [x] Postgres adapter (`crates/graph-owl-engine-postgres/src/registry.rs`) — **upsert on conflict, deliberately not idempotent-or-reject**: unlike a namespace code or predicate value-type, a rule's query text has nothing stored against it that a change would invalidate, so redeclaring must replace
- [x] `Catalog::declare_finding_rule` / `Catalog::finding_rules(pack)` — sixth optional field on `Catalog`, same precedent `namespaces`/`predicates` already set
- [x] Wired into all three composition roots (`main.rs`, `bin/graph-owl-mcp-stdio.rs`, `tests/common/mod.rs`)
- [x] 6 integration tests against real Postgres: read-back, per-pack scoping, upsert-replaces (not duplicates), empty pack, similarity/span JSON round-trip, `None` bands stay `None` not JSON `null`
- [ ] `POST /packs/{id}/finding-rules` (admin) HTTP route + OpenAPI contract regen — **deferred to Slice C**, since the route is only needed once the Python loader (Slice D) has something to call; the registry itself is fully usable via `Catalog` today
- [ ] Mutation run on the adapter/orchestration logic — pending, done together with Slice C's orchestration

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
