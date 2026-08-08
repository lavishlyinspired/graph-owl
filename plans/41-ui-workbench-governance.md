# Plan: Query Workbench, Governance & Admin (Epic 41)

**Branch**: feat/ui-workbench
**Status**: Shipped — Slices A–F, 5 August 2026 (DEMOS.md's Epic 41 section is the authority on what shipped and when; this line had gone stale). Slice G (standards-depth surfaces for Epics 96/97/100/101/102) remains, gated on those epics.
**Depends on**: Epic 39 (shell, trust components), Epic 40 (graph model and renderers), Epic 5 (constraints), Epic 7 (SPARQL), Epic 7b (Cypher), Epic 13 (authz), Epic 15 (connectors), Epic 31 (memory)
**Crates**: frontend in **`ui/`** (features `workbench`, `governance`, `memory`, `admin`) · served by **`graph-owl-ui`** · consumes Epic 7/7b query, Epic 5 validation, Epic 31 memory, and Epic 13 policy APIs

## Goal

The three surfaces that turn the engine from a thing you query into a thing you operate: **ask it anything, see what is broken, run it.**

Epic 39 is the shell and Epic 40 is the picture. This epic is where the remaining differentiators become visible — dual-language query, constraint validation as a governance workflow, and agent memory as an inspectable, editable store rather than an opaque one.

## Resolved decisions

1. **One workbench, two languages.** SPARQL and openCypher (Epic 7b) in the same editor, with the language chosen explicitly and remembered per tab. Two separate query screens would double the surface and teach users that the two languages address different data — which is the opposite of true, since both lower onto the same plan over the same flakes.
2. **Results render as a table *and* as a graph, from the same result set.** A result containing nodes and edges is a graph; a result of scalars is a table. The toggle is available whenever the shape permits it and disabled, with a reason, when it does not. This reuses Epic 40's `GraphModel` — the workbench does not get a third renderer.
3. **The editor is a client of the query API's own capabilities.** Syntax highlighting, completion from the live predicate registry and label set, `EXPLAIN`-style plan display, and errors with source positions all come from the server. A UI that maintains its own grammar drifts from the parser and produces confidently wrong squiggles.
4. **Every query runs as the signed-in principal, under Epic 13's policy.** No elevated console query path. The three-way authorization equivalence (HTTP, Bolt, SPARQL) from `07d-engine-bolt.md` extends to the workbench: the same principal, the same compiled predicate, the same visible subset.
5. **Validation is a workflow, not a report.** A constraint violation (Epic 5) has an owner, a state, and a history. Rendering violations as a list to be admired is how a quality dashboard becomes wallpaper within a quarter. Assign, acknowledge, waive-with-reason, resolve — with the waiver visible on the asset.
6. **Memory (Epic 31) is inspectable and editable by humans.** Anything an agent wrote can be read, corrected, or retracted by a person, with provenance for who or what wrote it and when. An agent memory nobody can audit is a liability, and `00a-product-position.md` sells explainability.
7. **Admin is a section, not a second application.** Policies, principals, connectors, jobs, and schedules live under one section of the same console, sharing its shell, auth, and design language.
8. **Connector configuration forms are generated from JSON Schema.** Epic 1 already emits it and Epic 15 already defines per-connector config. Hand-writing a form per connector across 100+ connectors is the single largest avoidable cost in the console, and a generated form cannot drift from the schema it validates against.

## Implementation reference

```
ui/src/features/
  workbench/
    Editor.tsx         CodeMirror; language mode per tab
    Results.tsx        table ⇄ graph toggle, sharing Epic 40's GraphModel
    Plan.tsx           EXPLAIN output from the query API
    History.tsx        per-principal, local; saved queries are server-side
  governance/
    Violations.tsx     Epic 5 results as a workflow queue
    Certification.tsx  lifecycle transitions (Epic 26)
    Ownership.tsx      gaps and bulk assignment (Epic 11)
  memory/
    MemoryPanel.tsx    entity-scoped, on the Knowledge tab
    MemoryAdmin.tsx    cross-entity search, provenance, retraction
  admin/
    Policies.tsx  Principals.tsx  Connectors.tsx  Jobs.tsx
    SchemaForm.tsx     JSON Schema → form; the only form renderer in admin
```

**One `SchemaForm`, used by every connector, every custom property (Epic 22), and every policy input.** A second bespoke form in this section is a review-blocking finding.

### Query result shapes

| Result contains | Table | Graph |
|---|---|---|
| Scalars / literals only | ✔ | disabled, with the reason shown |
| Nodes and edges | ✔ | ✔ |
| Paths | ✔ (one row per path) | ✔ (union of paths) |
| Mixed | ✔ | ✔, graph-shaped subset only, **marked as partial** |

The mixed case is where a naive implementation quietly drops rows. Marking it partial is a criterion, not a nicety.

## Acceptance criteria

- [ ] One editor runs both SPARQL and openCypher; the language is explicit and per-tab.
- [ ] Completion and highlighting derive from the **live** registry and grammar, not a UI-local copy.
- [ ] Query errors show the source position the parser reported.
- [ ] Results toggle table ⇄ graph where the shape permits; the graph reuses Epic 40's model and renderers.
- [ ] Long-running queries stream or page; the UI never blocks and cancellation actually cancels server-side.
- [ ] Every query executes as the signed-in principal under Epic 13 policy — asserted by a two-principal test.
- [ ] Violations are a workflow with assignment, acknowledgement, waiver-with-reason, and resolution; waivers are visible on the asset.
- [ ] Memories are searchable, attributable, correctable, and retractable by a human.
- [ ] Connector, custom-property, and policy forms are **generated from JSON Schema** — one renderer.
- [ ] Job and ingestion runs show status, duration, records, and failures with actionable errors.
- [ ] Zero axe violations; keyboard-only completion of the workbench and violation-triage journeys.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first.

### Slice A: The editor and query execution

**Acceptance criteria**: CodeMirror with a SPARQL mode and a Cypher mode; explicit per-tab language selection; execution against Epic 7/7b with results, timing, and row counts; a syntax error renders at the reported source position; a query the principal may not run returns the denied state, not an empty result; cancellation cancels server-side, verified by the server observing it; query text and language live in the URL for sharing.
**RED**: The empty-versus-denied test. A permission failure rendered as zero rows teaches the user their data does not exist, and they will act on that. Second RED: cancellation must be observed **at the server**, because a UI that merely stops listening leaves the query running and the connection pool degrades under an impatient analyst. Mutator watch: dropping the source position must fail; a client-only cancel must fail the server-observation assertion.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice B: Results as table and as graph

**Acceptance criteria**: tabular results with typed columns, sorting, and CSV/JSON export; a graph toggle enabled only when the result contains nodes or edges, disabled **with a stated reason** otherwise; the graph view reuses Epic 40's `GraphModel` and renderers with no third renderer introduced; a mixed result renders the graph subset and is **marked partial**; a 100k-row result pages without freezing the browser; nulls and empty strings are distinguishable.
**RED**: The mixed-result test. Rendering only the graph-shaped rows without saying so silently deletes data from the user's answer — the most dangerous class of bug on a query screen. Second RED: the structural single-renderer assertion, protecting decision 2 and `00f-ui-architecture.md`'s two-renderer commitment. Mutator watch: dropping the partial marker must fail; null rendered as empty string must fail.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice C: Query plans, completion, and saved queries

**Acceptance criteria**: an `EXPLAIN`-style plan view showing the operators and index choices from Epic 7; completion for predicates, labels, and entity types from the live registry; saved queries stored server-side with sharing governed by Epic 13; per-principal local history; a plan rendered for both languages, since both lower onto the same plan structure.
**RED**: The both-languages plan test — if the plan view only works for SPARQL, decision 1's claim that both lower onto one plan is unverified in the one place a user could see it. Second RED: completion sourced from the live registry, asserted by adding a predicate and seeing it offered without a UI release. Mutator watch: a hardcoded predicate list must fail that test.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice D: Constraint violations as a workflow

**Acceptance criteria**: violations from Epic 5 grouped by shape, by asset, and by owner; each carries state (open / acknowledged / waived / resolved), an assignee, and a history; waiving requires a reason and an expiry; a waived violation is **visible on the asset page**, not hidden; resolution is verified by re-validation rather than asserted by a human click; trend over time; a filter by domain (Epic 23) and by severity.
**RED**: The waiver-visibility test. A waiver that hides a violation from the asset page is how a governance tool becomes a way to make problems disappear — the violation must remain visible with its reason and its expiry. Second RED: resolution-by-revalidation, because a "resolved" state a human can set without the constraint passing makes the whole dashboard advisory fiction. Mutator watch: accepting a waiver with an empty reason must fail; treating an expired waiver as active must fail a boundary test at the expiry instant.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice E: Memory panel and memory administration — **shipped 5 August 2026**

**Acceptance criteria**: entity-scoped memories on the Epic 39 Knowledge tab; each shows content, confidence, provenance (which agent or human, when, from what), and the retrieval context that produced it; a human can add, correct, or retract a memory; retraction is a retraction (`op = false`), never a delete, so the history survives; cross-entity search over memories with filters by author, confidence, and age; a memory below the ignore band (<0.5) is not surfaced on the entity page but **is** findable in administration.
**Delivered**: all of the above — see the Epic 41 write-up in `DEMOS.md` for the full account, including `V45`'s `retracted_at`/`retraction_reason` columns, `Storage::retract_memory`/`search_memories`, and the two new HTTP endpoints. **The `0.8` boundary named below was not found to exist for memory confidence** — `00c-domain-model.md` states only the `<0.5` ignore band this slice's own acceptance criteria line states; `0.8` matches Epic 21 extraction's *assert* band (`Disposition::for_confidence`), a different feature. Tested at `0.5` only, which is the boundary the acceptance criteria and the domain model both actually name; recorded here rather than silently dropped in case `0.8` was meant to name something not yet written down.
**RED**: The retraction-preserves-history test — a memory deleted rather than retracted destroys the audit trail of what an agent believed and when, which is the entire reason decision 6 exists. Second RED: the confidence-band boundary test at exactly `0.5`. Mutator watch: `>=` becoming `>` at the band edge must fail — covered by `a memory exactly at the band is shown` in `ui/src/memory/memory.test.ts`.
**Done when**: criteria met, mutation report reviewed, commit approved. Met.

### Slice F: Admin — policies, principals, connectors, jobs

**Acceptance criteria**: policy authoring against Epic 13 with a **dry-run** showing what a given principal would see before saving; principal and group management; connector configuration via `SchemaForm` generated from each connector's JSON Schema, with connection test before save; secrets are write-only — never rendered back, never in a response; job and schedule management with run history, duration, record counts, and failures carrying actionable errors; a structural test asserting `SchemaForm` is the only form renderer in admin.
**RED**: The secret round-trip test — asserting a configured credential never appears in any subsequent API response or DOM node. Second RED: the policy dry-run, because a policy saved without preview is a production access change made blind, and this is the one screen where a mistake is a security incident. Mutator watch: a masked-but-present secret in the response must fail; a dry-run evaluating as the admin rather than the target principal must fail a two-principal test.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice G: The standards-depth surfaces (Epics 96, 97, 100, 101, 102)

**Added 28 July 2026.** `00h-ui-design-system.md`'s screen inventory stopped at Epic 38, so every epic in the standards-depth and full-semantics phases was unassigned — the exact condition its completeness requirement exists to catch. Most extend surfaces this epic already owns, which is the five-pattern design working; they are gathered here rather than scattered so the work is estimable.

**Acceptance criteria**:

- **Reasoner routing is visible wherever reasoned facts are** (Epic 100) — the detected profile as a badge, and the reasoner that produced a derivation named in the explanation panel. Epics 98 and 99 add reasoners with *different completeness guarantees*, so an unlabelled conclusion is one whose strength cannot be assessed.
- **An out-of-profile ontology, and an override-permitted partial result, are marked** — not by colour alone.
- **Remote rows are attributed to their endpoint** in the result grid (Epic 101), and a `SILENT` failure is visible *in the result*. An empty region of a grid reads as "no such data" rather than "we could not ask" — the same absence-versus-omission confusion `40-ui-graph-explorer.md` calls its most damaging bug.
- **The federation allow-list is an admin surface with dry-run**, like Slice F's policy editor. Adding an endpoint grants the query engine permission to make outbound calls carrying the caller's bindings; that is a policy decision in a configuration costume.
- **The constraint editor accepts SPARQL** (Epic 96) — the violations workflow in Slice D is unchanged, but authoring gains a second language and therefore a second editor mode.
- **Overlay staleness is shown** (Epic 97) — a derived fact whose age is invisible is a derived fact nobody can weigh. Incremental reasoning makes "when was this last recomputed" a real question with a real answer. **Shipped 9 August 2026**: the Governance page's "Run reasoning" result now names the technique, counts, and raw `maintainedTo` watermark (`ui/src/governance/reasoningRun.ts`), with a partial-run warning naming what was ignored.
- **Partition health and replication lag** (Epic 102) in admin. Partition health shipped 8 August 2026 (`PartitionHealthPanel.tsx`); replication lag not attempted — this deployment has no read replica to measure lag against.

**RED**: The unattributed-remote-row test — a federated result rendered indistinguishably from a local one, which is Epic 101's own named danger arriving on a screen. Second RED: the silenced-failure test, since `SILENT` plus an empty grid region is the strongest available way to make a user conclude something false. Mutator watch: dropping the endpoint attribution must fail; rendering a partial reasoning result identically to a complete one must fail; a profile badge derived from the *requested* profile rather than the *detected* one must fail a mismatch fixture.
**Done when**: criteria met, mutation report reviewed, commit approved.

## Explicitly deferred (with destination)

- **Visual query builder** → a drag-and-drop query builder is a large product that produces worse queries than typing. Completion and plan display close most of the gap.
- **Notebook features (cells, narrative, charts)** → Bolt (Epic 7d) makes real notebook tools work against this engine; reimplementing them loses.
- **Scheduled queries / alerting on results** → belongs with the job scheduler (Epic 15), not the workbench.
- **Ontology and shape authoring GUI** → not doing (`00f-ui-architecture.md`); metadata-as-code (Epic 20) is the path, and review workflow is the reason.
- **Approval workflows for waivers** → the waiver reason and expiry cover the immediate need; an approval chain needs a workflow engine.
- **In-console SDK/token issuance UI** → CLI (Epic 20) issues tokens; a browser is a poor place to display a long-lived credential.

## Pre-PR quality gate

1. **Stryker** — 0 missed on result shaping, violation state transitions, and confidence-band logic.
2. Refactoring assessment. 3. `tsc --noEmit` strict; ESLint clean.
4. **Denied ≠ empty** on every query and result path (Slice A).
5. **A mixed result is marked partial**; no third renderer introduced (Slice B).
6. **A waived violation remains visible on its asset**; resolution requires re-validation (Slice D).
7. **Retraction preserves history**; band boundaries tested at 0.5 and 0.8 exactly (Slice E).
8. **Secrets never round-trip**; policy dry-run evaluates as the target principal (Slice F).
9. Zero axe violations; keyboard-only workbench and triage journeys.
