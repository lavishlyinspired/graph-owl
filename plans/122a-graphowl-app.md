# Plan 122a — GraphOWL Console rebuild (`graphowl-app/`)

**Parent**: `plans/122-frontend-rebuild.md` — read it first. Decisions D1–D4,
the nine standing constraints, the bundle-budget position and the archival
rules all live there and are not repeated here.

**Status**: planned, 17 August 2026. Not started.
**Path**: `graphowl-app/` — replaces `ui/`, embedded in the server binary.

---

## Goal

Implement the 24-destination console described by
`samples/GraphOWL and Reco Now UI Mockups3/GraphOWL Console.dc.html`, on the
existing API where it exists and on new API where it does not, inside the
CI budgets `ui/` currently breaches.

## Why here

`00f-ui-architecture.md` §"Positioning change": the API and MCP surfaces are
the product; the console is a client that proves the capabilities are
reachable by a human. Today ~15 of the console's 24 intended surfaces are
already reachable in the API and only 13 have any UI at all. The gap is not
engine capability — it is that capability stops at the API with no human
surface, which `00f` names as the thing to avoid.

---

## 1. Capability matrix

Every mockup surface against `openapi.json` (157 paths, regenerated
16 Aug 2026). This is the authority for which slices carry a `.api`
sub-slice.

### 1.1 Backed today — UI work only

| Surface | Endpoints |
|---|---|
| Explore (canvas, expand, filter) | `POST /graph/context` · `GET /assets/{id}/graph` · `POST /graph/context/analytics` |
| Entity: facts, state, confidence | `GET /assets/{id}` · `GET /assets/{id}/children` · `/ancestors` |
| Entity: contradictions, accept A/B | `GET /assets/{id}/contradictions` · `POST /contradictions/reviews` |
| Entity: history, time travel | `GET /assets/{id}/versions` · `asof` (10 occurrences) · `GET /context/{version}` |
| Trace · Lineage | `GET /lineage/asset/{id}` · `POST /lineage` · `DELETE /lineage/{id}` |
| Trace · Paths | `POST /graph/paths` |
| Trace · Evidence | `GET /findings/{id}/evidence-graph` · `GET /findings` |
| Govern · Validation | `GET /validation/report` · `POST /validation/runs` · `/waivers` · `/assignments` · `/shapes/seed` |
| Govern · Contradictions | as Entity above |
| Govern · Resolution | `GET /resolution/queue` · `/{id}/confirm` · `/reject` · `/bulk` · `POST /merges/{id}/split` |
| Govern · Drift | `GET /drift` · `POST /drift/reports` · `/{id}/apply` · `/{id}/ignore` |
| Govern · Governance | `GET,POST /policies` · `POST /policies/dry-run` · `DELETE /policies/{name}` |
| Ingest · Sources / Connectors | `/connectors/configs` · `/connectors/runs` · `/connectors/{c}/schema` · `/{c}/test` · `/ingest` · `/ingest/batch` · `/ingest/jobs/{id}` |
| Knowledge | `/memories` · `/assets/{id}/memories` · `/memories/{id}/retract` · `/supersede` · `/mentions` |
| Workbench | `POST /sparql` · `POST /cypher` |
| Packs | `/ontology-packs*` · `/packs/{pack}/finding-rules` · `/packs/{pack}/candidates` · `/ontology/profile` |
| MCP | `POST /mcp` |
| Admin | `/users/*` · `/teams*` · `/webhooks/*` · `/admin/outbound-webhooks` · `/auth/config` · `/me` · `/health` · `/ready` · `/metrics` |
| Studio · Build tree + concept detail | `/glossaries` · `/glossaries/{id}/terms` · `/glossary-terms/{id}` · `/glossary-terms/{id}/relations` · `/glossary-terms/search` |
| Studio · Proposals | `/proposals` · `/{id}/accept` · `/{id}/reject` · `/change-proposals` · `/glossary-terms/{id}/transitions` · `/reviewers` |
| Studio · Mappings to other systems | `POST /alignments` · `GET /alignments/review` |
| Studio · SPARQL tab (run) | `POST /sparql` |

### 1.2 Partially backed — a named, bounded `.api` addition

| Surface | Have | Missing |
|---|---|---|
| Overview | `GET /overview`, `GET /assets/stats` | Graph-health percentages (coverage/validation/confidence/freshness/governance) as one payload; **"Consumers of this graph"** — per-consumer call counts over 24h |
| Source mapping (`pipeline`) | `GET /connectors/{c}/schema` returns columns | Proposed column → ontology-property mapping with per-mapping confidence; persisted mapping template; explicit "unmapped columns are kept, not discarded" contract |
| Studio · labels | prefLabel, altLabel | `hiddenLabel`; per-label language tags; SKOS-XL toggle |
| Studio · Export | `GET /graph/export/rdf` (Turtle, and 5 other formats) | Vocabulary-*scoped* export; `skos:ConceptScheme` + Dublin Core scheme metadata; audit log as CSV |
| Studio · Validate | `POST /validation/runs`, `/shapes/seed` | qSKOS check set (S-codes), severity, affected count, offered fix; `skos-shapes.ttl` |
| Studio · Glossary (candidates) | `POST /packs/{pack}/candidates` | Candidate staging bucket with source/count/suggested placement/match score; promote-as-altLabel vs promote-as-concept, both audited |
| Agents | `/agents/grants` · `/agents/{id}/grant` · `/agents/{id}/activity` · `/mcp` | Per-agent trigger, runs-in-24h, **grounding %**, pipeline-stage membership |
| Studio · SPARQL tab (generate) | run works | Natural-language → SPARQL generation |
| Global · search/ask | `/assets/search` · `/glossary-terms/search` · `/business-metrics/search` | One federated search endpoint; the "ask" path |
| Global · inbox | `/proposals` · `/change-proposals` · `/resolution/queue` · `/findings` · `/extraction/queue` | One aggregated "waiting on you" feed across all five queues |

### 1.3 No API at all — the four real gaps

| Surface | What it needs | Sizing |
|---|---|---|
| **Agent runs** | Persisted run records: id, agent, kind, status, trigger, input, output, cited fact ids, tools called, tokens, latency, destination. Plus regenerate + batch. | Its own plan, referenced from A8. This is a storage-and-lifecycle feature, not an endpoint. |
| **Analytics** | Graph growth by fact state over time · source-row → certified-fact funnel · confidence decay by predicate by month · most-traversed relationships · model spend vs graph work. | Its own plan, referenced from A9. Needs a time-series or rollup story the engine does not have. |
| **Token / model usage** | Tokens by model, spend, budget, per-run attribution, guardrails. | Folds into the Agent-runs plan — same records, different aggregation. |
| **Workspaces** | Isolated graph + packs + audit trail per workspace. Verified: the 21 "workspace" hits in `crates/` are **cargo workspace**, not tenancy. No tenancy exists. | **Largest gap in the plan and out of scope for a frontend rebuild.** See A1. |

> **A1 decision, taken here so no slice is blocked on it:** the workspace
> switcher renders **read-only against a single implicit workspace** until
> tenancy is planned separately. The control appears (it is load-bearing in
> the mockup's information architecture) and honestly reports one workspace
> rather than faking a list. Multi-tenancy is a storage, authorization and
> migration epic — it does not belong inside a UI plan, and pretending
> otherwise is how a UI plan becomes a year.

---

## 2. Acceptance criteria — epic level

1. All 24 destinations reachable, each rendering real API data, with
   loading / empty / error states. No panel ships that cannot be populated.
2. `ROUTES` ≤ 30, asserted by a ported `routes.structural.test.ts` that
   greps the real router.
3. `npm run check:budgets` green: **initial JS ≤ 350KB gzipped**, route
   chunk ≤ 100KB, runtime dependencies ≤ 40.
4. Zero axe violations across all 24 routes.
5. Derived facts visually distinct from asserted, everywhere, with
   derivation reachable in ≤ 1 interaction.
6. `crates/graph-owl-ui` serves `graphowl-app/dist`; `ui/` archived.
7. `openapi.json` regenerated and diffed on every slice that adds a route.
8. No GST noun anywhere in `graphowl-app/` or in any endpoint it added.

---

## 3. Slices

Each is one commit. Value · Path · AC · RED (incl. mutator watch).
`.api` sub-slices are separate commits landing **before** their UI slice.

---

### A0 · Shell, tokens, budgets, embed

**Value** — the new app boots, is served by the real binary, and every
budget that governs it is measured from the first commit rather than
discovered at the end.

**Path** — `graphowl-app/` (Vite 6, React 19, TS strict, Tailwind 4, Radix).
`crates/graph-owl-ui/build.rs` gains a second embed at `/next/`, keeping
`ui/` at `/`.

- Tokens from the mockup's `:root` / `[data-theme="light"]` blocks as
  Tailwind `@theme` CSS variables — two-tier semantic, per `00h`.
- Chrome: top bar, collapsible grouped rail, banner + undo, drawer.
- A real router with route-level `React.lazy`.
- Ported: `routes.ts` + `routes.structural.test.ts`,
  `scripts/check-budgets.mjs`, `eslint-rules/`, `stryker.config.mjs`,
  Playwright + axe harness.
- `generated/api.d.ts` regenerated from `openapi.json` via
  `openapi-typescript`, with the round-trip test.

**AC** — `cargo run -p graph-owl-server` serves the new shell at `/next/`
and the old console still works at `/`. Theme toggle switches both token
sets. Rail collapses. `check:budgets` green. axe clean. Binary still
under the 50MB budget **with both consoles embedded** — measured, recorded.

**RED** — route-budget check fails on a 31-route fixture. Structural test
fails when `ROUTES` and the router disagree. Budget script fails on a
fixture bundle over 350KB. Theme test asserts both token sets resolve.
*Mutators to watch*: `≤` → `<` in the budget comparison; the theme toggle
inverted (a light/dark swap that still "works" needs an assertion on a
specific token value, not on "a class changed").

---

### A1 · Global chrome with real data

**Value** — search, inbox and time travel are the three controls present on
every screen; wiring them once makes all 24 destinations feel finished.

**`A1.api`** — `GET /inbox`: one aggregated "waiting on you" feed across
`/proposals`, `/change-proposals`, `/resolution/queue`, `/findings` and
`/extraction/queue`, each item carrying its source queue, subject, who
raised it and the actions available. `GET /search` federating the three
existing search endpoints. Both are read-only compositions over existing
authorized reads — **no new authorization surface**, and a test must prove
the composed result is filtered exactly as its constituents are.

**Path** — `graphowl-app/src/chrome/`, plus the two Rust routes.

**AC** — ⌘K opens search, returns assets + glossary terms + metrics,
keyboard-navigable, respects authorization. Inbox badge counts real
pending items; approve/reject round-trips to the owning queue. AS-OF chip
sets `asof` on every subsequent read. Workspace switcher renders the single
implicit workspace read-only (§1.3).

**RED** — inbox aggregation returns nothing a principal may not see (seed
two principals, assert disjoint feeds — the *negative* assertion, per the
project's standing "a surviving mutant is a missing negative test" finding).
Approving from the inbox mutates the correct underlying queue and not a
sibling. AS-OF actually changes a result, and clearing it changes it back.
*Mutators*: dropping one of the five queues from the union still returns a
plausible feed — assert per-queue presence explicitly, not just a count.

---

### A2 · Overview

**Value** — the landing screen, and the cheapest proof that the generic
KPI + panel layout works.

**`A2.api`** — extend `GET /overview` with the five graph-health
percentages and a `consumers` block (per-consumer call counts, 24h window).
Consumer counts come from existing request telemetry; if that telemetry
does not distinguish consumers, **say so and drop the panel from A2** rather
than inventing an attribution.

**AC** — eight stat tiles, five health bars, activity feed, consumers
panel, all from one request. Every number traces to a field in the
response; none is computed client-side from a different number.

**RED** — health percentages are derived from real counts (assert a seeded
graph produces a stated percentage, and that changing one fact moves it).
*Mutators*: a percentage numerator/denominator swap that still yields a
plausible number — assert an exact value on a fixture, not a range.

---

### A3 · Explore + Entity

**Value** — the two bespoke screens the product is actually judged on, and
the ones with the heaviest renderer.

**Path** — G6 canvas behind a dynamic import, harvested from
`ui/src/graph/`. Entity detail as a full route (it is deep-linkable and
carries tabs), not a drawer.

**AC** — Explore: expand/collapse, filter chips, legend, zoom/fit, pin to
investigation (via `/memories`, asset-scoped, which Plan 120 shipped).
Selecting an edge shows confidence, why-believed reasoning steps, evidence
list, and a trace-path action. Entity: facts with state + confidence,
contradiction A/B with three outcomes (accept A, accept B, keep
unresolved — *"neither is hidden until you decide"*), history, impact.
Derived facts visually distinct. G6 loads only on these routes.

**RED** — graph tests **assert the model, not the picture** (`00f`): node X
upstream of Y, a derived edge carries derived treatment, a low-confidence
edge is marked. No screenshot assertions on the canvas. "Keep unresolved"
leaves *both* values readable — the negative test for the contradiction UI.
*Mutators*: accept-A applying B's value; the derived flag inverted.

---

### A4 · TRACE — Lineage · Paths · History · Evidence

**Value** — four destinations from one pattern in four configurations. This
is the slice that proves the generic template, on the group where every
endpoint already exists.

**AC** — all four render real data through one shared component with four
configs, not four components. Each deep-links. Route chunk stays ≤ 100KB.

**RED** — a config-driven test table drives all four surfaces through the
same assertions. *Mutators*: a config field ignored by the renderer — assert
each config's distinguishing element is actually present, or the template
silently collapses into one screen four times.

---

### A5 · GOVERN — Validation · Contradictions · Resolution · Drift · Governance

**Value** — five destinations, all backed, and the product's governance
argument made visible. Includes the review-queue pattern (`00h`).

**AC** — validation report with waivers and assignments; resolution queue
with confirm/reject/bulk and merge-split; drift with apply/ignore; policy
list with dry-run. Every decision is recorded with an author and a reason
and is visible in the audit trail. Bulk actions show exactly what they will
affect **before** they run.

**RED** — a bulk action on a filtered selection affects the filtered set and
nothing else (the negative assertion). Dry-run mutates nothing — assert
state is byte-identical after. *Mutators*: bulk applying to the unfiltered
set; ignore behaving as apply.

---

### A6 · INGEST — Sources · Connectors · Source mapping

**Value** — the five-step mapping screen is where a human tells the engine
what a column means. The mockup is explicit that everything downstream
inherits it.

**Shipped, 2026-08-18**: Sources and Connectors, both against real data —
no new backend needed. Sources has no dedicated entity in the API; it is a
pure client-side rollup (`graphowl-app/src/lib/sources.ts`) over
`GET /connectors/runs` history, grouped by `serviceName` (objects =
`created - deleted` summed across every run for that service, health
derived from the most recent run's `failed` count and a 7-day staleness
window — the mockup's own stated definition, not an invented number).
Connectors lists the one real registered type (`postgres` — the only
connector wired to `/connectors/{connector}/schema` and `/test`; the
"100 connectors, one crate" convention this stays honest to), with a form
that calls `POST /connectors/postgres/test` and `POST /connectors/postgres/runs`
directly. Live-verified end to end against a real Postgres connection.

**`A6.api` — NOT shipped, needs its own plan** (same treatment as A8/A9).
Checked against the real backend before starting the frontend and found
three real gaps, not just missing UI:
1. **No predicate listing.** `POST /predicates` defines one; there is no
   `GET /predicates` or equivalent anywhere in `graph-owl-api`. A mapping
   proposal needs something to propose *against* — this does not exist.
2. **No mapping-template persistence.** No storage table, no facade method,
   no route for a per-source saved column→property template.
3. **No `Source` entity.** Only raw `ConnectorRun` history exists; a
   five-step wizard's "per-dataset confirm" and "one unconfirmed dataset
   blocks the build" need a persisted draft state that outlives a single
   run, which nothing today models.

A confidence-scoring approach also needs to be designed and justified
per `00i` rule 4 (every number needs a stated reason) before any of this
is implemented — Resolution's `Evidence` enum (`graph-owl-core/src/resolution.rs`)
is the nearest real precedent for "named reasons, not a bare score" and is
worth reusing the shape of, not the numbers.

**Original AC/RED, preserved for when the sub-plan is written**:
five-step progress, per-dataset tabs, sample row, raw-row toggle, mapping
table with confidence, unmapped-columns list, per-dataset confirm. One
unconfirmed dataset blocks the build. Re-uploading uses the saved
template. RED: an unmapped column is still retrievable after ingest (the
whole point, and the assertion nobody writes). One unconfirmed dataset
blocks; all confirmed unblocks. *Mutators*: the block condition inverted —
needs both the positive and negative case.

---

### A7 · Vocabulary Studio

**Value** — the largest single surface: eight tabs behind one route. It is
also the clearest instance of `00h`'s claim that separate Glossary,
Classification and Domain applications are one tree-plus-detail pattern.

**Shipped, 2026-08-18**: 5 of 8 tabs, fully real, against the substantial
glossary/term backend that already existed (glossary + term CRUD, SKOS
relations broader/narrower/related/exactMatch/closeMatch with real
add/delete, the real `TermStatus` workflow draft→inReview→approved→
deprecated via `/glossary-terms/{id}/transitions`, reviewers, usage, and
`/sparql`) — none of it needed the SKOS-completion backend below.
- **Build**: tree + detail, `vocabularyTree.ts` and its 10-test suite
  ported verbatim from `ui/src/features/vocabulary/` (poly-hierarchy,
  cycle-guarded); relations fetched per-term (no bulk endpoint exists) to
  populate it. Add/remove relation, create/delete term, all real.
- **Glossary**: "candidates → promote" maps onto the real term lifecycle,
  not a new concept — draft/inReview terms with a "submit for review" /
  "promote to approved" action. Found live: approval genuinely requires an
  assigned reviewer first (`set_term_reviewers` is a real precondition,
  not a bug) — added a reviewer-assignment control and real error surfacing
  after hitting the 400 in manual verification.
- **Business view**: approved terms only, name + definition, nothing else.
- **Graph**: real bubble layout (`vocabularyGraph.ts`, 8 tests) — a plain
  SVG radial layout, not a new graph library (G6 is already the one
  bundle-budget exception; a second heavy dependency for a term-relation
  view isn't justified). "Connect two concepts" calls the real add-relation
  endpoint.
- **SPARQL**: query box + results table against the real `/sparql`
  endpoint. NL-to-SPARQL generation is out of scope per the AC itself
  (deferred to A7b).

All live-verified against a real Postgres-backed server: created a
glossary and two terms, built a real poly-hierarchy relation, watched the
tree nest correctly, ran the full submit-for-review → assign-reviewer →
promote cycle, saw the bubble graph render the real edge, and ran a real
SPARQL query.

**`A7.api` — NOT shipped, needs its own plan** (same treatment as A6/A8/A9).
Three tabs stay honestly-labeled placeholders because the SKOS completion
they depend on doesn't exist: **Proposals** (candidate staging — a
system-suggested altLabel/concept with a match score, a different concept
from the real term-status workflow Glossary already uses), **Validate**
(qSKOS checks — no check set seeded, no `skos-shapes.ttl`), **Export**
(RDF serialization — Turtle/JSON-LD/RDF-XML/N-Triples). Items 1, 2 and 6
below (`hiddenLabel`, `ConceptScheme`, SKOS-XL) are smaller, additive gaps
on the term model that block no tab — worth doing whenever the sub-plan is
written, not gating anything shipped today.

**`A7.api` original scope, preserved for the sub-plan — the SKOS
completion, in this order:**
1. `hiddenLabel` + per-label language tags on glossary terms.
2. `skos:ConceptScheme` with Dublin Core scheme metadata.
3. Vocabulary-scoped export (Turtle / JSON-LD / RDF-XML / N-Triples) with
   optional Dublin Core, plus audit log as CSV.
4. qSKOS check set with severity, affected count and an offered fix;
   `skos-shapes.ttl` seeded through the existing `/validation/shapes/seed`.
5. Candidate staging: source, count, suggested placement, match score;
   promote-as-altLabel and promote-as-concept, both audited.
6. SKOS-XL toggle.

**Run the build-vs-adopt check before writing any of it** —
`plans/00l-build-vs-adopt.md` gets a row. qSKOS in particular is a published
check set with existing implementations; the specification is the source
(`00i` rule 2), and a permissively licensed implementation is preferred
over writing one.

**AC** — Build (tree + concept detail with labels, documentation, semantic
relations, mappings, qSKOS inline), Glossary (candidates → promote),
Business view (no RDF vocabulary visible; read-only share), Proposals
(accept-as-altLabel / approve-as-concept / reject), Graph (bubble layout,
connect two concepts), Validate (qSKOS table, manual + automatic, fixes
offered never applied), SPARQL (run; generation deferred), Export (live
Turtle preview + download). A failing check blocks *publishing a version*,
never *editing*.

**RED** — promoting as altLabel merges into the existing concept; promoting
as concept creates a child under the chosen parent; **both write an audit
row naming the author** — assert the audit row, not just the outcome. A
failing qSKOS check blocks publish and does not block edit (both
assertions). Export round-trips: parse the emitted Turtle back and assert
the same concepts. *Mutators*: broader/narrower inverted — assert the
**swap fails**, per the standing `domain`/`range` discipline; a promote that
silently creates a duplicate rather than merging.

**Defers** — NL → SPARQL generation to A7b (needs a model call, a grounding
story, and a "not enough evidence" path; it is not a vocabulary feature).

---

### A8 · Agents · MCP · Agent runs

**Value** — the mockup's strongest argument: *"Stages hand each other fact
ids, not prose. If a stage cannot cite the graph the pipeline halts there
and the case stays unexplained rather than guessed."*

**`A8.api`** — **needs its own plan.** Persisted agent-run records (input,
output, cited fact ids, tools called, tokens, latency, trigger,
destination), the grounding metric, and token/model/spend aggregation. This
is storage and lifecycle, not an endpoint, and `plans/106-agent-trace-hygiene.md`
(shipped) covers adjacent ground worth reading first. **Do not attempt it
inside A8.**

**AC** — Agents: pipeline stages, agent table with trigger/runs/grounding,
run trace with tools and citations, tokens by model, spend against budget,
guardrails, MCP tools exposed. Runs: list, filter, detail with output +
cited facts + tools. An uncited sentence is dropped before storage — and
the UI states that, because it is true.

**RED** — a run whose output cites no fact is rejected at write, not
filtered at read (assert storage refuses it). Read-only agents cannot reach
the inbox; a run needing a graph change files a proposal instead — the
central product promise, and the negative test that proves it.
*Mutators*: the grounding ratio inverted; the read-only guard removed.

---

### A9 · Analytics

**Value** — growth by fact state, the source-row → certified-fact funnel,
confidence decay by predicate, traversal hotspots.

**`A9.api`** — **needs its own plan.** Requires a rollup or time-series
story the engine does not have. `POST /graph/context/analytics` and
`/metrics` cover none of these four. Size it honestly before A9 starts; if
it is an epic, A9 waits and the console ships without it rather than
shipping a fake one.

**AC** — four visualizations, each cell/bar traceable to the facts behind
it ("Show the facts"). The period narrative cites its fact count.

**RED** — clicking a cell opens exactly the facts that produced it (assert
the set, not the count). *Mutators*: an off-by-one bucket boundary — assert
a value exactly on a boundary lands in the stated bucket.

**Build-vs-adopt** — charting library. Check licence first; the current app
has none, and the dependency budget has 11 slots left.

---

### A10 · PLATFORM — Workbench · Packs · Admin

**Value** — three destinations, all backed, closing the inventory.

**AC** — Workbench: SPARQL + Cypher with results, timing, error surfacing,
editor behind a dynamic import. Packs: install, terms, overrides, upgrade,
profile. Admin: users, teams, roles, webhooks, health, budgets.

**RED** — a query error renders as an RFC 9457 problem body, not a blank
panel. Pack upgrade is previewed before applied. *Mutators*: the upgrade
preview and apply paths swapped.

---

### A11 · Cutover and archive

**Value** — one console again.

**Path** — `build.rs` serves `graphowl-app/dist` at `/`; the `/next/` embed
is removed; `ui/` → `_archived/ui/` with a line in `_archived/README.md`.

**AC** — all 24 routes reachable at `/`. Full epic gate green: `fmt`,
`clippy`, `cargo test` on touched crates, frontend suite, Playwright + axe,
`check:budgets`. Binary under 50MB with **one** console. `openapi.json`
regenerated and diffed. `DEMOS.md` updated and `EPIC-STATUS.md`
regenerated.

**RED** — a structural test asserts `build.rs` watches `graphowl-app/dist`
and that no path resolves into `ui/`.

---

## 4. Explicitly deferred

| Deferred | Destination |
|---|---|
| Multi-tenant workspaces (isolated graph + packs + audit per workspace) | Its own epic — storage, authz, migration. A1 renders one workspace read-only until then. |
| Agent-run persistence, grounding metric, token/spend aggregation | Its own plan, prerequisite to A8. |
| Analytics rollups / time series | Its own plan, prerequisite to A9. |
| NL → SPARQL generation | A7b. |
| Business-view public share links | A7b — needs an unauthenticated-read story, which is a security decision. |
| Responsive / sub-1440px layout | Post-cutover. The mockups specify 1440px only; do not invent breakpoints. |
| Mobile | Not planned. `00f` does not claim it. |

---

## 5. Pre-PR quality gate

Per slice: 0 missed mutants on new decision logic (`scripts/mutants.sh`,
`--in-diff` for edits, `--file` for new files); `clippy` and `fmt` clean on
touched crates; the touched crates' own tests; the frontend suite;
`check:budgets`; axe clean on touched routes; `plans/00l-build-vs-adopt.md`
row added if the slice considered a dependency; `openapi.json` regenerated
if the slice added a route.

Per epic (not per slice): `scripts/gate.sh` scoped to changed crates.
`--full` only when the user asks or several epics have accumulated.
