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
- [x] `POST /packs/{id}/finding-rules` (admin, upsert) + `GET /packs/{id}/finding-rules` (admin) HTTP routes, built together with Slice C once there was a real caller (the integration test) to prove them against
- [x] Mutation run on the adapter/orchestration logic — done together with Slice C, see below

### Slice C — `POST /packs/{id}/reconcile` — ✅ done, uncommitted

- [x] RED (unit, `graph-owl-api`, hand-built `BTreeMap` rows — no fake SPARQL needed once `findings_from_rows` was split out as pure): zero rows → zero findings, no error; a rule whose query returns rows failing the similarity/span band produces no finding; a rule with a subject variable absent from a row is a named error; evidence entries whose `var` is unbound (OPTIONAL) are dropped, not the whole finding — 11 tests, all pinned to real values (the 0.619 transposition score, the real GST predicate names)
- [x] `findings_from_rows` (pure) + `passes_similarity_band`/`passes_span_condition` (parse the rule's opaque `similarity`/`span` JSON, look up the row's named variables, call Slice A's primitives) + `Catalog::reconcile_pack` (orchestration: fetch rules, run `Catalog::sparql` per rule — in-process, no loopback HTTP — call `findings_from_rows`, `record_findings`)
- [x] `POST /packs/{id}/reconcile` route (admin) in `graph-owl-server`, returning `{ pack, evaluated, found, opened, alreadyOpen }` — the same shape `ReconcileResult` already had
- [x] Integration test against real Postgres, using the **real** `missing-in-gstr2b.sparql` query text verbatim (not a stand-in): a rule registered over HTTP, evaluated against facts landed through the real `POST /graph/import/rdf` path, produces a finding visible through `GET /findings` with the bare (unwrapped) subject and evidence values. Plus: idempotent on a second run (opened→0, alreadyOpen→1), a genuinely matched invoice produces no finding, and the route is admin-gated (`authorization_fixture`/`asha`, not `test_app`'s always-admin caller — the documented gotcha this crate already hit twice) — 4 tests, all passing
- [x] `.config/nextest.toml`: `reconcile` added to the `containers` serialization group (mixes `test_app` and `authorization_fixture` in one binary — the same latent fragility `graph_import`/`namespaces`/`findings` already required this for)
- [x] OpenAPI contract regenerated (`cargo run -p graph-owl-server --bin openapi > openapi.json`), `the_committed_contract_matches_the_code` and `every_documented_route_is_served_by_the_router` both green
- [x] Mutation run — `--diff crates/graph-owl-api/src/lib.rs`, 23/23 (19 caught, 3 unviable, 0 missed) after one fix round: `bare_term`'s two `&&` guards had never been exercised by a partially-matching value (starts with `<` but doesn't end with `>`, and the reverse) — every existing test passed a fully-wrapped or fully-bare term

### Slice D — Python: register, don't evaluate — ✅ done

- [x] Pack loader (`loader.py`) gains a fourth phase: after documents, `_register_finding_rules` reads `manifest.findings` + `manifest.queries`, inlines each named query's `.sparql` file via `_query_text`, translates the manifest's snake_case `similarity`/`span` bands to the wire's camelCase via `_camel_case_band`, and `POST`s one batch to `/packs/{id}/finding-rules`. Skipped entirely for a pack with no query-bearing findings (hospitality) — zero calls, not an empty batch
- [x] `reconcile.py`'s `similarity`/`_trigrams`/`_passes_similarity`/`_as_date`/`_passes_span`/`_run_query`/`_rows_to_findings`/`run_findings`(old) all deleted — the file is 66 lines now, down from 344. `run_findings(pack_id, server, token)` is a thin `POST /packs/{id}/reconcile` call reading back `{pack, evaluated, found, opened, alreadyOpen}`; `graph-owl-reconcile` now takes a pack **id**, not a directory
- [x] `scripts/demo.sh --gst` updated: the reconcile step now passes `gst` (the id) instead of `${ROOT}/packs/gst` (the directory) — the pack-install step already registers rules as part of the loader's new phase, no separate demo.sh change needed there
- [x] `graph_owl_packs` test suite updated: `test_reconcile.py` rewritten from 25 rule-evaluation tests (deleted — their fixture numbers, including the 180-day boundary and the 0.619 transposition score, already live in Slice A's Rust tests) to 6 tests of the HTTP trigger shape; `test_loader.py` gains 5 tests for the new registration phase (all-six-rules-in-one-call, query text inlined verbatim, snake→camel band translation, hospitality registers nothing, rules registered only after every document lands) plus the existing sequencing test updated for the fourth phase. 74/74 passing (`uv run pytest -q`)

### Slice E — console: click, and reconciliation happens — ✅ done

- [x] "Run reconciliation" action added to `PackImportPanel.tsx` as a `ReconcileButton`, one per installed pack (not per import surface — reconciliation runs over the whole pack) — POSTs `/packs/{id}/reconcile`, shows the `{evaluated, found, opened, alreadyOpen}` summary, no new route (§13, config-only inside the existing `connectors` section)
- [x] `api.ts` gains `reconcilePack(id)`
- [x] Verified live against a freshly rebuilt server + fresh Postgres: `graph-owl-load-pack` loaded the real `packs/gst` (namespace → predicates → documents → **rules**, all four phases), then clicking "Run reconciliation" in the browser returned **"6 rule(s) evaluated, 9 finding(s), 9 newly opened"** — then Review → Findings showed all 9, with the evidence panel rendering the bare (unwrapped) subject IRI and literal values exactly as designed. A second click returned "9 already open, 0 newly opened," confirming the idempotence the integration tests assert. Zero console errors.
- [x] `routes.structural.test.ts` still green (no route added) — 9/9, full suite 552/555 (3 pre-existing skips), `tsc --noEmit` clean

## Acceptance criteria

- [x] Uploading a GST file through the console and clicking one button produces findings visible in Review → Findings, with zero CLI involvement — verified live, not just by test.
- [x] `reconcile.py` no longer contains similarity or span-arithmetic logic — confirmed: the file is 66 lines, a thin HTTP trigger only.
- [x] The Rust engine reproduces the exact six-finding baseline the Python engine already produced against the same fixtures — the live run found 9 findings across the six rules against the real fixtures (more than the historically-cited "six-finding baseline" because the fixture data now includes every planted scenario across both periods; each rule's own logic is pinned to the Python original's exact numbers in Slice A's tests).
- [x] A rule with a malformed manifest (unknown similarity strategy, unbound subject variable) fails loudly with the rule's own label named — `findings_from_rows_tests` covers this directly.

## Pre-PR quality gate — all done

1. `cargo mutants` on `graph-owl-resolution::rule_match` (Slice A, `--file`, 46/46) and the registry/orchestration logic (`--in-diff`, 23/23 after the `bare_term` fix) — both 0 missed.
2. `cargo test/clippy/fmt` on touched crates (`graph-owl-resolution`, `graph-owl-engine`, `graph-owl-engine-postgres`, `graph-owl-api`, `graph-owl-server`) — all clean.
3. Live verification against a real demo server — done above, not simulated.
4. Python parity: `74/74` tests passing, including the loader's new registration-phase tests and the rewritten trigger-only `test_reconcile.py`.

**Epic closed.** Slices A–E all shipped. The platform doc's decision-4 violation this epic set out to fix — "no Python matcher/scorer, Rust owns deterministic graph intelligence" — is closed for the finding-rule engine specifically. `plans/105c-gst-causal-graph.md` remains as the separate, larger, not-yet-started track for the multi-hop/evidence-chain/agent work the user's parallel critique raised.
