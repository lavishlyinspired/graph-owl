# Epic completion plan

> **Generated 8 August 2026** from a full dependency + code audit of every epic
> `plans/EPIC-STATUS.md` lists as "In progress" — not from re-reading the status
> table's prose, but from checking each claim against the running code: grep
> for the route, read the handler, read the struct. Six parallel research
> passes did the checking; this document is the synthesis, organized for
> execution rather than by epic number.
>
> **The single biggest finding**: a recurring pattern across six epics (96, 97,
> 98, 99, 100, 104) where real, tested backend logic exists at the
> `graph-owl-api::Catalog` method level — and is checked off as shipped in
> `DEMOS.md` — but was **never wired to an HTTP route**. The epic's own RED
> tests call the Catalog method directly, never the HTTP layer, so the gap is
> invisible to the project's own test discipline. These epics are not
> reachable in a running deployment today, despite reading as done. This is
> Phase 1 below because it is high-value and mostly mechanical.
>
> **Scale**: ~45 real, verified work items across 27 epics, ranging from
> 10-minute fixes to multi-day features requiring their own design pass. Nine
> items are flagged **NEEDS DECISION** — a product or architecture choice only
> a human should make, not sized until decided. Given this project's own
> non-negotiable TDD discipline (RED→GREEN→MUTATE→KILL MUTANTS→REFACTOR per
> item, `scripts/gate.sh` batched per epic), this is not a single sitting — see
> "Sequencing and pacing" at the end for a realistic accounting and the
> decisions needed before starting.

## How to read this document

- **Phase 0** — pure documentation corrections. No code changes; `DEMOS.md`/
  `EPIC-STATUS.md` currently show something as pending that the code already
  does. Fix these first — they're free, and they shrink the "In progress"
  count immediately.
- **Phase 1** — the HTTP-wiring gap. Backend logic exists and is tested;
  routes don't. Small, mechanical, high-value, no blocking dependencies.
- **Phase 2** — small well-scoped additions (filters, facets, small fixes) with
  no blocking dependency and no open design question.
- **Phase 3** — medium items: real feature work, unblocked, but each needs its
  own RED→GREEN→MUTATE cycle and is sized in days, not hours.
- **Phase 4** — items that need a human decision before sizing is even
  meaningful. Flagged with the exact question.
- **Phase 5** — large, multi-day undertakings, several of which are themselves
  gated on a Phase 4 decision.
- **Epic ledger** — every epic cross-referenced to which phase(s) closes it,
  so `EPIC-STATUS.md` can be regenerated accurately once work lands.

---

## Phase 0 — Documentation corrections (no code)

These are epics where the tracked "pending" item is fully satisfied by code
that shipped elsewhere, and only the checkbox is stale. Flip these in
`DEMOS.md` first; `scripts/epic-status.py` regenerates `EPIC-STATUS.md` from
that.

| Epic | Stale claim | Why it's actually done | Evidence |
|---|---|---|---|
| 2 | "Non-database services deferred to Epic 34" | Epic 34 shipped all five families (dashboards, pipelines, topics, ML models, storage) with full CRUD/envelope/search/tags/ownership/lineage/authz | `plans/34-entity-expansion.md` status line; `plans/DEMOS.md:27` |
| 4 | "`rdf:reifies` + triple terms → Epic 94" | Shipped: `FlakeValue::TripleTerm` (discriminant 10), export serialization, query-surface synthesis | `crates/graph-owl-core/src/flake.rs:250`; Epic 94 Slices A/B/D |
| 4 | "Language-tag side table → Epic 94" | Shipped via a deliberate design pivot — a `FlakeValue::LangString{text,language,direction}` variant (discriminant 11) instead of the originally-planned side table | `crates/graph-owl-core/src/flake.rs:258-272`; Epic 94 Slice C |
| 29 | "Column-level lineage" unchecked *(Demo-3-pulled-forward duplicate)* | Shipped, Slice D, 3 August 2026 — a later DEMOS.md section ("Epic 29 — Lineage, the column half") already correctly marks this `[x]`; the earlier duplicate line was never updated | `crates/graph-owl-storage-postgres/migrations/V43__column_lineage.sql`; `plans/DEMOS.md:810` |
| 29 | "Connector-asserted lineage reconciles with curated edges" unchecked *(same duplicate)* | Shipped, Slice E — `Catalog::reconcile_lineage` exists and is called | `crates/graph-owl-api/src/lib.rs:3628-3637`; `plans/DEMOS.md:812` |
| 29 | "Lineage survives entity deletion... a tombstone..." unchecked *(same duplicate)* | Describes the *pre*-Slice-F state; Slice F shipped exactly this (soft delete retains edges, hard delete purges by design) | `plans/DEMOS.md:814` |

**Correction, not a Phase 0 item**: Epic 7b's DEMOS.md line is already correctly
marked `[~]` (two genuine items remain: TCK Slice A2, the unconstrained-node
binding bug — see Phase 3 note under the epic ledger). Only
`EPIC-STATUS.md`'s summary-table "Depends on" text ("ships after 7c") reads as
stale; that's cosmetic prose from the plan header, not a tracking error, and
needs no action.

**Epic 37c** — the stale claim ("Slice F stays blocked on Epic 34") lives in
`plans/37c-embeddable.md`'s own status line, not a DEMOS.md checkbox (its
single `[~]` DEMOS.md bullet correctly stays partial: Slices E and F are both
still genuinely unstarted). Fix the sentence in the plan file directly, not
DEMOS.md.

**Action**: update the five lines above in `DEMOS.md`, fix `37c-embeddable.md`'s
stale status sentence, regenerate `EPIC-STATUS.md` via
`python3 scripts/epic-status.py`. Zero risk, immediate.

---

## Phase 1 — Wire built logic to HTTP (the cross-cutting finding)

Every item below is the same shape: a `Catalog` method exists, is unit- and
integration-tested, and does the right thing — but no route calls it, or (one
case) the route calls it and drops fields from the response. Fixing each is
mechanical: add or fix one handler, following an existing sibling route's
pattern (`/reasoning/runs`, `/validation/runs`, `/admin/export` are all
precedent). Still needs a RED test at the HTTP layer for each — that's the
actual gap in test discipline that let this go unnoticed, and skipping it
here would repeat the mistake.

| # | Epic | Gap | Fix | Size | Evidence |
|---|---|---|---|---|---|
| 1.1 | 96 | `POST /validation/runs` calls `catalog.run_validation()`, not `run_validation_as(&principal, budget)` — the only method that evaluates `Constraint::Sparql`. A `sh:sparql` shape produces **zero violations today**, always. | Swap the handler to call `run_validation_as` | trivial (<1hr) | `crates/graph-owl-server/src/lib.rs:2781-2789` vs. `crates/graph-owl-api/src/lib.rs:12054-12145` |
| 1.2 | 99 | `query_outcome_json` hand-builds the `/sparql` response JSON and omits `outcome.ql_rewrite` and `outcome.refused_axioms` — both exist, both populated server-side, neither ever serialized. Two checked acceptance criteria are false at the wire. | Add `qlRewrite`/`refusedAxioms` to the JSON literal; confirm `Serialize` derives | trivial (<1hr) | `crates/graph-owl-server/src/lib.rs:2568-2594` vs. `crates/graph-owl-api/src/lib.rs:531-541` |
| 1.3 | 98 | `Catalog::classify_ontology()` and `explain_subsumption()` have no route at all. EL classification is unreachable in production. | Two new routes: `POST /reasoning/el/classify` (admin-gated, spawns the `whelk` sidecar), `GET /reasoning/el/explain?subclass=&superclass=` | small (<1 day) | `crates/graph-owl-api/src/lib.rs:1606-1710`; no hits in `graph-owl-server` |
| 1.4 | 100 | `detect_ontology_profiles`/`route_ontology_reasoning`/`force_ontology_reasoning` are never called from `run_reasoning`'s handler. An out-of-every-profile ontology loaded today silently runs RL and **does not refuse** — the exact failure this epic exists to prevent is still live. | Wire detection+routing into `run_reasoning`'s call path; refuse unless `force`/override; surface `partial`/ignored-axioms on `ReasoningReport`. Also add `GET /ontology/profile` (asking without running reasoning) — no route exists for that either. | medium (1-3 days) — real behavior change to a live path, not additive | `crates/graph-owl-api/src/lib.rs:1823-1870` (methods) vs. `29460+` (only callers, tests) |
| 1.5 | 102 | `PostgresEngine::compact()` has **zero callers outside its own tests** — no Catalog wrapper, no route, no scheduled task. In production, `flakes_delta` grows forever and is never folded into `flakes_main`; the whole point of the split degrades to nothing. | `Catalog::compact(batch_size)` wrapper + admin-gated `POST /admin/compact`. Automatic scheduling is a separate, larger question (Phase 4). | small (<1 day) for the manual trigger | `crates/graph-owl-engine-postgres/src/lib.rs:296`; no caller in `graph-owl-api` |
| 1.6 | 102 | No partition-health metric exists at the API level at all (not a UI gap as `EPIC-STATUS.md`'s "(UI → Epic 41 Slice G)" annotation implies — there's nothing yet to surface). | A Prometheus gauge or small admin JSON endpoint: delta-table row count, oldest un-compacted transaction time | small (<1 day) | `crates/graph-owl-server/src/observability.rs:379-405` — no partition metric |
| 1.7 | 104 | `Catalog::upsert_alignment()`/`pending_alignment_review()` have no HTTP route. No way to write or read an alignment via the real API. | `POST /alignments`, `GET /alignments/review` — DTO shape is itself an open question the plan flags (Phase 4 item 4.7) | medium (1-3 days), mostly the DTO design | `crates/graph-owl-api/src/lib.rs:1849-1870`, `~11940`; no hits in `graph-owl-server` |
| 1.8 | 94 | `serialize_turtle`/`RdfSerializer::serialize` (with the Slice B `rdf:reifies` logic) has zero callers outside its own crate's tests. No RDF export exists over HTTP — only 5 LPG-side formats. | `Catalog::export_rdf(principal, format, scope, as_of)` + `GET /graph/export/rdf?format=turtle`, following the existing `serve_temp_file` export pattern | small (<1 day) — serializer and export-route pattern both exist already | `crates/graph-owl-rdf-io/src/lib.rs:157-205`; `crates/graph-owl-server/src/lib.rs:80-84` (5 routes, none RDF) |
| 1.9 | 97 | `run_reasoning_incremental(retracted, budget)` has zero callers outside tests — `/reasoning/runs` always does a full run. DRed never executes against a live deployment. | **NEEDS DECISION first** (Phase 4.4) — how retractions reach the endpoint (server-tracked watermark vs. explicit caller-supplied list). Wiring itself is medium once decided. | medium, gated on 4.4 | `crates/graph-owl-server/src/lib.rs:6628-6641`; `crates/graph-owl-api/src/lib.rs:11694` |

**Recommended order within Phase 1**: 1.1 and 1.2 first (trivial, zero risk,
immediate correctness fixes to a *security-relevant* constraint mechanism in
1.1's case). Then 1.3, 1.5, 1.6, 1.8 (small, independent). Then 1.4 and 1.7
(medium, each with a small design question worth 15 minutes of thought, not a
blocking decision). 1.9 waits on Phase 4.4.

---

## Phase 2 — Small, well-scoped, unblocked

No open design question, no dependency on anything not already shipped. Each
follows an existing pattern elsewhere in the codebase closely enough that the
pattern itself is the spec.

| # | Epic | Item | Size | Pattern to follow |
|---|---|---|---|---|
| 2.1 | 25 | `?tags=` filter on `GET /assets`/`/assets/search` — table-level match against `tag_labels`, mirroring the existing `?domain=` join pattern; column-level (prefix match on `target_fqn`) folded into the same slice per the plan's own framing | small (<1 day) | `AssetFilter` domain-filter SQL, `crates/graph-owl-storage-postgres/src/lib.rs:4698` |
| 2.2 | 26 | `?lifecycle=` filter — column + partial index already exist, just no query-param wiring | trivial–small | same `AssetFilter` pattern |
| 2.3 | 26 | `?certification=` filter — needs a join/subquery against "latest non-superseded certification vs. now()", not a plain equality | small (<1 day) | `certifications_by_expiry` index already there |
| 2.4 | 8 | Snippets — `ts_headline(description, to_tsquery(...))` as one more SELECT expression, threaded through the response DTO | small (<1 day) | existing rank computation call sites, `crates/graph-owl-storage-postgres/src/lib.rs:3803/4728/5124` |
| 2.5 | 5 | `RelationshipShape`/`EnvelopeShape` seed shapes — the blocking dependency (relationship projection, envelope projection) shipped with Epic 4 Slice E; this is simply unbuilt, not blocked | small (<1 day) | two more entries in `shapes.rs`'s `definitions` array, constraints already worked out in the plan |
| 2.6 | 4 | Predicate-registry cardinality/datatype enforcement on write — the registry already carries the data, nothing reads it at assert time | small (<1 day) | one write-path check in `graph-owl-engine`/`graph-owl-engine-postgres` |
| 2.7 | 16 | Invalid-`kind`-string per-item fix — move `kind` parsing from the pre-loop into the per-item loop, matching how the neighboring parent-not-found case is already handled | small (<1 day) | `crates/graph-owl-server/src/lib.rs:3034-3043` vs. `~3105-3115` (existing per-item pattern) |
| 2.8 | 19 | Pulsar lag reporting — currently hardcoded `None`; needs an admin-REST HTTP call to the Pulsar backlog API | small (<1 day) | `StreamConsumer::lag`, `crates/graph-owl-server/src/streaming.rs:230-238` (already has the `None` branch to fill) |
| 2.9 | 37c | Slice E — crate publishability: `description`/`repository`/`keywords` on every crate `Cargo.toml`, `publish = false` on adapter/server crates, path→version+path deps, a `cargo publish --dry-run` CI step | small–medium | none — greenfield within the crate, mechanical across ~10 `Cargo.toml`s |
| 2.10 | 37c | Slice F — surface survives expansion: extend `crates/graph-owl-api/examples/embedded.rs` with a second entity family, add a `cargo public-api` snapshot test | small (<1 day) | existing `embedded.rs` (60 lines, one entity kind) |
| 2.11 | 39 | `AssetDetail`'s ancestors/children fetches swallow errors into an empty array — replicate the `*Failed` boolean pattern already used for search | trivial (<1hr) | `ui/src/App.tsx:1660-1662` next to the already-fixed search case |
| 2.12 | 42 | Heading-order bug on Governance and Connectors pages (`<Title level={4}>` with nothing at 2/3) — the identical bug already fixed on 5 other pages | trivial | one-line change per page, `ui/src/App.tsx:3037` |
| 2.13 | 42 | Keyboard-selection gap on `ReviewQueue.tsx`'s `List.Item` — the identical pattern already fixed in `AgentActivityPanel`'s grant table, left unfixed here on purpose ("a follow-up") | trivial | `tabIndex`/`role`/`onKeyDown` pattern already in `AgentActivityPanel.tsx` |
| 2.14 | 42 | Agent-session deep link (`?agent=<id>`) on `AgentActivityPanel` — the one surface in this epic missing the `readParam`/`writeParam` convention every sibling surface uses | trivial | `readParam`/`writeParam` used elsewhere in the same epic |

---

## Phase 3 — Medium items, unblocked but genuinely multi-step

Each needs its own RED→GREEN→MUTATE→KILL MUTANTS cycle. Ordered roughly by
value/risk, not epic number.

| # | Epic | Item | Size | Notes |
|---|---|---|---|---|
| 3.1 | 35 | ~~**Activity feed has no `AccessPredicate` enforcement**~~ **DONE 8 Aug 2026** — `entity_activity` now checks `predicate.admits(&asset.fully_qualified_name)` (`ViewBasic`) and returns `NotFound` when denied. 5 new unit tests, mutation-tested (2 caught, 1 unviable, 0 survivors). | small (<1 day) but **treat as priority**, not cosmetic — it's an authorization hole | Thread the existing `AccessPredicate` pattern (already used in Epic 9a's `project_incremental`) into the feed's merge query |
| 3.2 | 35 | Proposals (Epic 35) not wired into the review queue — no catalog-wide `GET /change-proposals` listing endpoint, no "who am I" endpoint for the frontend's per-user fallback | small backend (<1 day) + small-medium UI (a 4th `QueueConfig`, the pattern is proven by 3 existing instances) | `ui/src/features/review/ReviewSection.tsx:8-16` already documents exactly what's missing |
| 3.3 | 2 | Cascade-on-rename for `Asset` — no `name` field on `AssetUpdate`, no rename endpoint at all. Named explicitly by Epic 34's own write-up as unbuilt machinery its dependency line assumed existed. | medium (1-3 days) | core DTO + facade + storage-postgres cascade query + handler + tests |
| 3.4 | 6 | Ownership-inherits-down-`contains` and Domain-inherits-down-`contains` as reasoning rules, with a differential test against Epics 11/23's existing query-time walks (now buildable — both epics are shipped, so there's a reference behavior to test against for the first time) | medium (1-3 days) | mirror the 13 existing rule functions in `crates/graph-owl-reasoning/src/lib.rs:387-846` |
| 3.5 | 28 | Popularity/usage term in search ranking — the ranking key today is purely lexical (`ts_rank_cd`), no reference to `usage_rollups` anywhere | medium (1-3 days) | RED test must prove "weight at zero reproduces prior ordering exactly" — a real before/after correctness proof, not just adding a term |
| 3.6 | 32 | Wire the 6 remaining `AgentCapability` variants into `apply_proposed_change` (currently only `ApplyDescription` actually applies on accept; the rest 400 telling the human to do it by hand — meaning `record_memory`/`record_investigation` never behave as the plan describes) | medium (1-3 days) | facade methods to call already exist from their own epics; this is per-variant wiring + tests |
| 3.7 | 8 | Fuzzy matching + column-name search + tag search as additional ranking components, working toward the 7-tier Decision 5 ordering | medium (1-3 days) for the additions; large if pursuing the full literal ordering with a dedicated test corpus | no `pg_trgm`/`similarity`/`levenshtein` anywhere yet |
| 3.8 | 29 | Column rename/drop propagation — `lineage_column_mappings` stores plain-TEXT FQNs with no FK; a column rename leaves the mapping stale | medium (1-3 days) | hook into Epic 2's containment-rename cascade (3.3) rather than a second parallel traversal — **sequence after 3.3** |
| 3.9 | 33 | Import a real BFSI ontology pack (FIBO named by the plan) via the already-generic `POST /ontology-packs` — needs sourcing FIBO's Turtle release (or converting from its RDF/XML distribution) and possibly extending the SKOS importer to accept `rdfs:label`/`rdfs:subClassOf` as aliases, since it currently requires `skos:prefLabel` | medium (1-3 days) if a usable release is found; see Phase 4.6 for the fork in the road | `crates/graph-owl-rdf-io/src/skos.rs:52-108` is Turtle-only and `skos:prefLabel`-gated |
| 3.10 | 96 | SPARQL-based constraint components (`sh:SPARQLConstraintComponent`, `sh:parameter`, `sh:labelTemplate`) — a real, separate mechanism (component registry + parameter substitution + message templating), not a generalization of the bare constraint | medium, possibly large if message templating is done properly | zero implementation exists; sequence after 1.1 |
| 3.11 | 97 | `maintained_to` freshness stamp on `ReasoningReport` — track the base transaction-time watermark the last run computed against | small (<1 day) once 1.9/4.4 lands | Epic 98's EL cache already has an identical watermark pattern to copy |
| 3.12 | 15 | Generic connector-run governance surface (open/close a run, report FQN extent for deletion reconciliation) exposed as endpoints an out-of-process Python worker can call — currently entangled entirely inside `run_postgres_connector` | large (>3 days) — see Phase 5, this is the meat of Epic 15's remaining scope | `crates/graph-owl-server/src/lib.rs:7061` |
| 3.13 | 104 | UMLS ingestion delivery mechanism — `ingest_mrconso` has no caller anywhere (no HTTP endpoint, no CLI, no job registration). Files are typically GBs; needs progress persisted by whatever calls it. | medium (1-3 days) for a basic CLI/admin-triggered runner; see Phase 4.8 | `crates/graph-owl-connectors/src/umls.rs` |
| 3.14 | 42 | Review queue Epic 104 alignment as a 5th `QueueConfig`, once 1.7's DTO shape settles | small, once 1.7 lands | proven-generic pattern, 4 instances already exist |
| 3.15 | 42 | Export dialog (`ExportDialog.tsx`) — does not exist at all (zero files). Needs scope/as-of/preview filtering added to the existing 5 LPG export routes plus the new RDF route (1.8), and the dialog UI itself. | medium-large | shared scope with Epic 94's RDF-export gap (1.8) — plan together |

---

## Phase 4 — Needs a human decision before sizing

Each of these blocks real implementation until someone (you) makes a call.
I've written the concrete question for each; answering these unblocks Phase 3
items 3.9/3.13 and the Phase 5 items below.

**4.1 — Hard delete + erasure (Epic 3).** Nothing hard-deletes an asset today;
`EventKind::HardDeleted` exists but has no producer. Separately, `00g-operations.md`
sketches "crypto-shredding at the identity boundary" for `User` PII erasure as
one paragraph of intent, not a spec. **Question**: do you want (a) a real
generic hard-delete for ops/test-data purge, (b) only `User`-specific
crypto-shredding for GDPR-style erasure, (c) both, or (d) leave both deferred
for now? This also touches Epic 12 (can a soft-deleted-in-the-future user
still authenticate?).

**4.2 — EventSink / webhook sender (Epics 3 + 14 — the same gap).** All the
hard logic (HMAC signing, canonicalization, SSRF admission, backoff) is built
and tested in `graph-owl-events::webhook`, with zero callers. No production
`EventSink` is ever wired into `Catalog` at all — `Catalog::announce()` is a
no-op in the running server today, for *every* change, not just webhooks.
**Question**: is outbound webhook delivery the first real consumer to justify
wiring `EventSink` into production, or is there another planned consumer
(e.g. search reindex-on-change) that should be designed in at the same time
rather than risk a second wiring pass later?

**4.3 — Ingestion partial-success scope (Epic 16).** Duplicate-FQN and
containment-cycle within a batch are deliberately whole-batch `400`s today
("a duplicate FQN states two intents; nothing can know which is meant" — a
reasoned choice, not an oversight). **Question**: keep this as the permanent
design (only the invalid-kind case, 2.7, gets fixed to per-item), or do you
want per-item partial success for duplicate/cycle too? The latter requires
restructuring `apply_order` from an all-or-nothing `Result` into something
that isolates just the offending indices — a real algorithmic change to an
already mutation-tested pure function.

**4.4 — Incremental reasoning retraction tracking (Epic 97).** DRed exists
and is correct, but nothing supplies it with `retracted` flakes automatically.
**Question**: should the server track retractions itself between reasoning
runs (a durable retraction log/watermark — more infrastructure, fully
automatic), or should an explicit endpoint/body parameter accept a caller-
supplied retraction list (less infrastructure, requires every caller to know
what changed)?

**4.5 — Health/certification filtering infrastructure (Epics 26 + 30, shared
question).** Both want to filter/facet on a *computed* status (quality health,
certification currency) that is not a stored column — today computed
on-read from `test_cases`/`test_results` or the `certifications` table. No
async work queue or denormalized-column-refresh mechanism exists anywhere in
this codebase. **Question**: accept per-row computation cost for the filter
(cheapest, might not scale), build a narrow denormalized-column-plus-refresh-
on-write for just these two cases, or build a general async-refresh
mechanism reusable by future "filter on a computed thing" needs? This is one
decision that unblocks two epics at once.

**4.6 — BFSI ontology pack scope (Epic 33).** FIBO's production distribution
is RDF/XML with OWL-native `rdfs:label`, not the `skos:prefLabel` the shipped
importer requires. **Question**: extend the SKOS importer to accept OWL-native
annotation predicates as aliases (benefits future packs too), or write a
one-off FIBO-specific conversion step? Also: the plan's narrative about
per-axiom RL-subset-drop reporting is aspirational text, not a tracked
acceptance criterion — should it become one, or is flat concept+relation
import (what's actually built) sufficient for a first BFSI pack?

**4.7 — Alignment review DTO shape (Epic 104, blocks 1.7 and 3.14).**
`pending_alignment_review`'s return type is described in the plan itself as
"a deliberately minimal backend contract, not a finished API." **Question**:
should I design the response DTO now (needed to build 1.7/3.14), or do you
want to see/approve the shape before it's wired into a real endpoint?

**4.8 — UMLS ingestion delivery mechanism (Epic 104, blocks 3.13).**
**Question**: CLI binary, admin-HTTP-triggered background job, or wired into
the generic `/connectors/*` job framework? RRF files are typically
gigabytes, which argues against a synchronous HTTP call.

**4.9 — Automatic partition compaction scheduling (Epic 102, follows 1.5).**
Once a manual `POST /admin/compact` exists, **question**: is manual-trigger
sufficient, or do you want automatic scheduling (size- or age-triggered,
in-process timer vs. external job)? This is deliberately separated from 1.5
so the manual endpoint isn't blocked on the scheduling design.

---

## Phase 5 — Large, multi-day undertakings

These are sized honestly as genuinely large, several gated on a Phase 4
decision. Recommend treating each as its own sub-plan (this project's own
convention — `plans/15-connectors.md:223` says exactly this: "each its own
small plan, not a deferral of this epic").

| # | Epic | Item | Size | Gate |
|---|---|---|---|---|
| 5.1 | 15 | Python connector protocol: generic run-governance HTTP surface (3.12) + a registry mechanism (replacing the current `if connector != "postgres" { 404 }` hardcoding) + a Python-side SDK helper + one real non-Postgres connector (e.g. MySQL) proven against a real database | large (>3 days) | none — architecture already decided (out-of-process, HTTP callback, per plan decision 1); this is execution |
| 5.2 | 14 | Full outbound webhook feature: migration + `Storage` methods for registrations/delivery/dead-letter state, admin HTTP endpoints (naming needs care — `/webhooks/*` is claimed by Epic 18's *inbound* receivers), a background sender task, `EventSink` production wiring | medium–large (2-4 days) | 4.2 |
| 5.3 | 27 | SLA evaluation: `Freshness`/`QualityPassRate` can map to existing Epic 28/30 signals now (medium once mapped); `Completeness`/`Availability` have no existing signal source in this codebase at all | medium for 2 of 4 SLA types; **needs decision** for the other 2 (is a new signal type in scope?) | partial — Freshness/QualityPassRate unblocked, Completeness/Availability need a source decision |
| 5.4 | 27 | ODCS (Open Data Contract Standard) interop | large, and genuinely unresearched — no `00l-build-vs-adopt.md` entry exists | **needs a spike/decision first**, per this project's own standing rule for any standard-shaped component |
| 5.5 | 31 | Semantic ranking term for memory retrieval | large — this is not "add a term," it's "stand up vector search from nothing." `graph-owl-search-hnsw` is a 6-line placeholder; no embedding pipeline, no vector index exists anywhere. Epic 8's own plan explicitly defers semantic search to "after lexical relevance plateaus" and it isn't even in Epic 8's current unchecked items. | **needs a product decision**: greenlight Epic 8's deferred vector-search scope, or leave Epic 31 at its current (already real, 6-of-7-term) ranking |
| 5.6 | 35 | Thread UI (list threads on an asset, post/reply, resolve/reopen, pagination) — no console surface exists at all | medium-large (comparable to or larger than Epic 42's ~1300-line review-queue feature, since a comment-thread UI isn't a drop-in reuse of the decide/reject queue pattern) | none, but should follow 3.1/3.2 (backend correctness + the queue-wiring precedent) |
| 5.7 | 38 | Slice E (PageRank-vs-usage bake-off — needs a real corpus and a human rating a blind sample, explicitly not an unattended task) + Slice F (scheduling, caching, HTTP surfacing, wiring onto Epic 40's asset page and Epic 14's `TrustSummary`) | Slice E needs human involvement directly; Slice F is large (>3 days), spans `graph-owl-api`, `graph-owl-server`, a new storage table, Epic 15's scheduler, two UI surfaces | Slice E needs you; Slice F unblocked but big |
| 5.8 | 39 | Bundle-budget fix — 576.9KB gzip against a 350KB limit, caused by zero code-splitting across `@xyflow/react`/`cytoscape`/`d3-dag` in three call sites inside a ~4500-line `App.tsx` | medium-large (1-3+ days) — the CI-wiring half is small; the actual refactor is the real cost | none, but genuinely a separately-scoped refactor per the plan's own framing |

---

## Cross-cutting correctness note (surfaced during the audit, not asked for)

**Epic 26 / Epic 30 — `TrustSummary` reads the wrong data source.**
`graph_owl_mcp::catalog::observe()` (`crates/graph-owl-mcp/src/catalog.rs:121-137`)
resolves lifecycle, certification, and test-health for the MCP trust surface
by reading `asset.properties` — a free-form, connector-writable JSON bag
(`text(asset, "lifecycle")`, `flag(asset, "testsPassing")`) — not the real
`assets.lifecycle` column, the `certifications` table (Epic 26), or
`health_of()` (Epic 30). An agent asking "is this safe to build on?" through
MCP today gets whatever a connector happened to write into a loosely-typed
property bag, not the structured governance data these two epics actually
built. This wasn't on anyone's checklist because it's a disconnect between
two already-shipped epics, not a gap within either one's own acceptance
criteria. **Recommend folding into Phase 3** alongside 2.2/2.3 (Epic 26) and
the health-filter work (4.5/Epic 30) since it touches the same data —
medium (1-3 days), no blocking dependency.

---

## Epic ledger — what closes each epic

| Epic | Closed by | Epic | Closed by |
|---|---|---|---|
| 2 | Phase 0 (checkbox) + 3.3 (real gap) | 27 | 5.3 + 5.4 (needs decisions) |
| 3 | 4.1 + 4.2 (needs decisions) | 28 | 3.5 |
| 4 | Phase 0 (checkbox) + 2.6 | 29 | Phase 0 (3 checkboxes) + 3.8 |
| 5 | 2.5 | 30 | 4.5 (needs decision) + TrustSummary note |
| 6 | 3.4 | 31 | 5.5 (needs decision) |
| 7 | no action — deliberate deferral (`sparopt`, gated on Epic 37a measurement) | 32 | 3.6 |
| 7b | already correctly tracked as partial — TCK Slice A2 + unconstrained-node fix (see note below) | 33 | 3.9, gated on 4.6 |
| 8 | 2.4 + 3.7 | 35 | 3.1 (priority) + 3.2 + 5.6 |
| 11 | 4.1 (soft-delete decision, shared with Epic 3) | 37c | 2.9 + 2.10 |
| 14 | 5.2, gated on 4.2 | 38 | 5.7 (part needs you directly) |
| 15 | 5.1 | 39 | 2.11 + 5.8 |
| 16 | 2.7 + 4.3 (needs decision for full scope) | 42 | 3.2 + 3.14 + 3.15 + 2.12–2.14 |
| 19 | 2.8 | 94 | 1.8 |
| 24 | small remaining items not itemized above (Metric-as-lineage-endpoint needs a schema decision; event emission on term transitions needs a shape decision) — see note | 96 | 1.1 + 3.10 |
| 25 | 2.1 | 97 | 1.9 + 3.11, gated on 4.4 |
| 26 | 2.2 + 2.3 + TrustSummary note | 98 | 1.3 |
| — | — | 99 | 1.2 |
| — | — | 100 | 1.4 |
| — | — | 102 | 1.5 + 1.6, optionally 4.9 |
| — | — | 103 | no action — confirmed zero remaining work |
| — | — | 104 | 1.7 + 3.13, gated on 4.7 + 4.8 |

**Notes on items not fully itemized in the phases above** (found during the
audit but small enough to fold into whichever phase they land in, not
significant enough for their own row):

- **Epic 24**: Metric lineage as a real graph-traversable edge needs a schema
  decision (give `Metric` an `AssetKind`, or widen `lineage_edges`' endpoint
  typing) before the medium-sized implementation. Event emission on
  glossary-term transitions needs a shape decision (widen `EventSubject` vs.
  a parallel term-event type) before a small implementation. Both are
  genuinely "needs decision, then small-medium" — treat as Phase 4 items if
  you want them sequenced; omitted from the Phase 4 table above only to avoid
  a 20-item decision list, not because they're unimportant.
- **Epic 7b**: two small remaining items — the openCypher TCK conformance
  oracle (Slice A2, medium, scoped starting point already identified: `Given
  an empty graph` scenarios) and `MATCH (n) RETURN n` binding nothing (small,
  needs a one-line design choice: wildcard triple pattern vs. explicit
  refusal).

---

## Sequencing and pacing

This is not a single-session undertaking under this project's own TDD
discipline — every item above, even the "trivial" ones, gets a RED test
first, per `CLAUDE.md`'s non-negotiable rule, and the "small"/"medium" items
each want their own mutation-testing pass before being called done. A
realistic accounting:

- **Phase 0**: minutes. Pure doc correction.
- **Phase 1** (9 items, mostly small/trivial): the highest-value phase per
  hour spent — it's the difference between "tested" and "actually shippable."
  Recommend starting here immediately, in the order given.
- **Phase 2** (14 items, all small/trivial): straightforward, low-risk,
  can proceed in parallel with Phase 1 once it's underway.
- **Phase 3** (15 items, small–medium, one flagged priority for correctness):
  each is a real slice with its own RED→GREEN→MUTATE cycle. 3.1 (the
  authorization hole in Epic 35's activity feed) should not wait for its
  epic's "turn" — it's a real correctness gap, not a feature gap.
  3.3 → 3.8 has a real ordering dependency (rename cascade before lineage
  rename-propagation).
- **Phase 4**: nine decisions, none of which block Phase 1/2/most of Phase 3.
  Answering these unblocks Phase 5 and a few Phase 3 items (3.9, 3.13).
- **Phase 5** (8 items): genuinely multi-day each, two need direct human
  involvement (5.4's spike, 5.7's blind-rating). These are the epics that
  will make "every epic shipped" take real calendar time regardless of how
  the rest goes — 15, 14, 27 (ODCS half), 31, 38, 39 in particular.

**What I'd suggest, concretely**: work Phases 0–2 to completion first (fast,
compounds — Phase 0 alone shrinks the in-progress count for free), then start
Phase 3 in the dependency order noted, surfacing Phase 4 questions as they're
reached rather than all nine at once up front. Phase 5 items are worth
scoping as their own `plans/NN-*.md` sub-plans when their turn comes, per this
project's own standing convention for large undertakings — not attempted
inline here.

I have **not** started implementation yet. This document is Step 1 of what
was asked for; confirming the phase order (or redirecting it) is Step 2,
before code changes begin.
