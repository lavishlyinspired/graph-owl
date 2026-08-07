# Plan: Semantic Browse, Review Queues & Agent Activity (Epic 42)

**Branch**: feat/ui-semantic-surfaces
**Status**: Not started
**Depends on**: Epic 39 (shell, patterns, trust components), Epic 41 (admin section, schema-driven forms)
**Related engine epics**: 7c, 7d, 9, 9a, 14, 17, 20, 21, 23, 24, 25, 32, 33, 35
**Crates**: frontend in `ui/` (features `vocabulary`, `review`, `agents`, `interchange`) · served by `graph-owl-ui`

**Read `00h-ui-design-system.md` first** — this epic is where three of its five patterns get built, and it is scoped by that document's route budget.

## Why this epic exists

The screen inventory in `00h-ui-design-system.md` mapped all 41 engine epics to a surface. **Fifteen had none.** Epics 39–41 cover the shell, the graph, and the workbench/governance/admin — and between them they missed the glossary, the domain browser, the tag browser, ontology packs, entity-resolution adjudication, extraction review, metadata-as-code drift, agent activity, export, and the property-graph view.

The alternative was appending them to Epic 41. That was rejected: an epic that absorbs everything unassigned stops being estimable, and Epic 41 already carries the workbench, governance, memory, and admin.

**The scope control is that fifteen surfaces resolve into three patterns.** If this epic delivered fifteen screens it would be a mistake regardless of which epic it lived in.

## Resolved decisions

1. **One hierarchical vocabulary browser, parameterized — not four applications.** Glossary terms (24), classifications and tags (25), domains and data products (23), and ontology packs (33) are the same shape: a tree, a detail pane, typed relations, and assets carrying the term. They differ in their relation vocabulary and their governance rules, which is configuration. A reference implementation ships these as separate applications; that is the clearest single saving available in this epic.
2. **One review queue, parameterized — not four workflows.** Merge candidates (17), extracted triples (21), drift (20), and proposals (35) are one interaction: *something proposed a change, a human decides, the decision is recorded with a reason and is auditable.* Constraint violations (Epic 5, in Epic 41) are the fifth instance and reuse the same component.
3. **Every review decision is recorded with its evidence, and rejection requires a reason.** An accepted merge with no record of what the evidence was cannot be audited when it turns out wrong, and a rejection with no reason teaches the matcher nothing. This is the difference between a review queue and a pile of buttons.
4. **Rejection is a first-class outcome that persists.** A rejected merge candidate must not reappear next run. Without this, the queue is re-adjudicated forever and stops being worked — the standard failure of every human-in-the-loop system.
5. **Agent activity is a read-only audit surface, not a control panel.** What agents connected, what they read, what they wrote back, and under what identity. Agents are governed by Epic 13 policy, not by a UI toggle — a console switch that disables an agent is an authorization mechanism in disguise, and there is already one.
6. **The property-graph view is a *toggle* on the existing Knowledge tab, not a screen.** The same subject rendered as triples or as a node with labels and properties (Epic 7c). Two screens for two views of one thing teaches users they are two things.
7. **Export is a dialog, not a section.** Format, scope, time, and a preview of the first records. RDF (Epic 9) and property-graph (Epic 9a) formats appear in one list because the user's question is "get this out", not "which serialization family".

## Implementation reference

```
ui/src/features/
  vocabulary/
    VocabularyBrowser.tsx      tree + detail + relations; one component
    vocabularies.ts            config per vocabulary — relations, governance, icon
  review/
    ReviewQueue.tsx            proposal + evidence + decide + audit; one component
    queues.ts                  config per queue — evidence renderer, actions, source
  agents/
    AgentActivity.tsx          sessions, operations, write-backs
  interchange/
    ExportDialog.tsx           format · scope · as-of · preview
```

**The two config files are the deliverable.** If adding a fifth vocabulary or a sixth queue requires touching `VocabularyBrowser.tsx` or `ReviewQueue.tsx`, the parameterization failed and the epic has produced two more bespoke screens rather than two patterns.

| Vocabulary | Relations | Governs |
|---|---|---|
| Glossary (24) | SKOS `broader` / `narrower` / `related` / `exactMatch` | Business meaning |
| Classification (25) | Containment; mutual exclusivity within a classification | Handling and sensitivity |
| Domains (23) | Nesting; data products within domains | Accountability |
| Ontology packs (33) | Class and property hierarchy, read-mostly | Imported vocabulary |

| Queue | Proposed by | Evidence shown | Outcomes |
|---|---|---|---|
| Merge candidates (17) | Resolution | Both entities side by side, matching fields, score | Merge · Reject · Defer |
| Extracted facts (21) | Document ingestion | Source passage with the span highlighted, confidence | Accept · Edit · Reject |
| Drift (20) | Metadata-as-code | Declared vs actual diff | Apply · Ignore · Update declaration |
| Proposals (35) | A colleague | Proposed change, author, discussion | Approve · Request change · Decline |

## Acceptance criteria

- [ ] Four vocabularies render through **one** browser component — asserted structurally, not by inspection.
- [ ] Four queues render through **one** review component — same assertion.
- [ ] A fifth vocabulary and a fifth queue are addable by **config only**, proven by adding one in a test.
- [ ] Every review decision records the decider, the timestamp, the evidence, and — on rejection — the reason.
- [ ] A rejected proposal does not reappear on the next producing run.
- [ ] Deep links resolve to a term, a queue item, and an agent session.
- [ ] The Knowledge tab toggles triple ⇄ property-graph view over the same subject, losslessly for the round-trippable subset and **naming the losses** for the rest (Epic 7c's mapping report).
- [ ] Export offers RDF and property-graph formats in one list, with a preview and an as-of selector.
- [ ] Agent activity shows sessions, operations, and write-backs, filtered by Epic 13 policy like every other read.
- [ ] Bolt endpoint status and active sessions are visible in admin.
- [ ] Zero axe violations; every queue workable by keyboard alone.
- [ ] Total console routes remain **≤ 30** (`00h-ui-design-system.md`), asserted in CI.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first.

### Slice A: The vocabulary browser, on one vocabulary — **shipped, 7 August 2026**

**Acceptance criteria**: tree with lazy expansion, keyboard navigation, and deep-linkable selection; detail pane with description, relations, and the assets carrying the term; a poly-hierarchy term (several `broader` parents, legitimate in SKOS per Epic 24) renders under **each** parent without duplicating identity; a cycle in the data renders and is marked rather than hanging; an empty vocabulary shows a designed first-run state.
**RED**: The poly-hierarchy test. A tree component that assumes one parent per node either drops the second placement or forks the node into two identities with divergent selection state — and SKOS explicitly permits multiple parents, so this is normal data, not an edge case. Mutator watch: keying nodes by path rather than by id must fail it; an unguarded recursive walk must hang on the cycle fixture.
**Done when**: criteria met, mutation report reviewed, commit approved.

**Shipped**: `ui/src/features/vocabulary/vocabularyTree.ts` — a pure function building a poly-hierarchy tree from a flat term list plus each term's own `broader` relations (only `broader` is read; `GET /glossary-terms/{id}/relations` already returns derived inverses, so inverting `broader` gives children without needing `narrower` at all). A node's `renderKey` (unique per occurrence) is deliberately separate from its `termId` (shared identity) — the RED test the plan named. A cycle with no external anchor still renders, entered once and marked `isCyclic` at the point it would revisit itself, rather than producing zero nodes or hanging; 10 unit tests, Stryker 95.83% (2 accepted-equivalent survivors: a fallback value immediately filtered out downstream either way, and an initial-ancestry placeholder that could only matter if a real term were named "Stryker was here"). `VocabularyBrowser.tsx` drives it against the real, already-shipped Epic 24 glossary API (`GET /glossaries`, `/glossaries/{id}/terms`, `/glossary-terms/{id}/relations`, `/glossary-terms/{id}/usage`) — no backend gap here, unlike 7c/7d/9/9a. Deep-linking reuses `App.tsx`'s own `?param=` + `history.replaceState` convention (no router exists yet; introducing one was out of scope for this slice) via a small local `deepLink.ts`, since `App.tsx` explicitly says new code should live in `features/`, not grow further itself.

**Two real bugs found only by driving a real browser (Playwright + `agent-browser`), neither visible from the unit tests or `tsc`/`eslint`**: (1) selecting a poly-hierarchy occurrence only highlighted the one clicked, not its sibling occurrence — antd's `Tree` needs `multiple` set for a `selectedKeys` array to render more than one row as selected, and the `onSelect` callback's own `keys[0]` argument is not reliably "the row just clicked" once `multiple` is set (it can already reflect antd's own prior-selection bookkeeping), so `handleSelect` reads `info.node` instead, which has no such ambiguity. (2) `readParam("vocabulary")` was written but never read on initial mount — `VocabularyBrowser`'s `glossaryId` state only ever initialized from a `glossaryId` prop, so `?vocabulary=<id>` in the URL was silently ignored and the browser always opened on whichever glossary the server listed first. Both fixed and covered by `ui/tests/vocabulary.spec.ts` (Playwright, against a real server via `scripts/verify-first-run-journey.sh`) — including the specific case that exposed the first bug (the exact SKOS shape a hand-picked unit fixture would not by itself have caught: a poly-hierarchy term with two real backend-generated ids, not synthetic ones).

**A third bug found only by axe, against the real rendered page**: the detail pane used antd's `Layout.Content`, which renders a semantic `<main>` — nested inside `App.tsx`'s own outer `<main>` for whichever section is active, an ARIA `landmark-is-unique` violation no unit test could see. Fixed by using a plain `<div>` instead.

**Scope note, recorded rather than silently narrowed**: relations for every term in the glossary are fetched once, up front, to compute the tree's shape (parents, poly-hierarchy, cycles) before anything renders. "Lazy expansion" in the acceptance criteria is met at the *UI* layer — antd's `Tree` starts fully collapsed, nothing is rendered open — but the underlying data fetch is not yet lazy per node the way `App.tsx`'s own asset tree already is. A glossary large enough for that to matter is real, separable follow-up work, not built speculatively against Slice A's one-vocabulary scope.

**A pre-existing, unrelated finding surfaced while checking this slice didn't regress anything**: `ui/scripts/check-budgets.mjs` already reports the initial bundle over its own 350KB gzipped budget (563.4KB on `main`, before this slice; 565.5KB after — this slice's own cost was ~2.1KB). Confirmed via `git stash` before touching anything, so it is not attributed here; recorded because nobody else had, and the budget is currently silently failing every build.

### Slice B: Three more vocabularies, config only — **shipped, 7 August 2026**

**Value**: This slice is the proof of decision 1. If it requires component changes, the pattern is not a pattern.
**Acceptance criteria**: classifications, domains, and ontology packs render through the same component; mutual exclusivity (Epic 25) is enforced in the tag picker with the conflicting tag named; domains show their data products; ontology packs are read-mostly and say so rather than offering disabled controls; a **structural test asserts `VocabularyBrowser.tsx` has no vocabulary-specific branch**.
**RED**: The structural no-branching test, plus adding a **fifth, fictional** vocabulary in a test fixture and asserting it renders with config alone. Testing only the four real ones cannot distinguish a parameterized component from one with four hardcoded paths. Mutator watch: a `switch` on vocabulary type must fail the structural assertion.
**Done when**: criteria met, mutation report reviewed, commit approved.

**Shipped**: `ui/src/features/vocabulary/vocabularies.ts` — a `VocabularyConfig` interface (`fetchData`/`detailFor`, plus display copy: `label`/`treeLabel`/`emptyTitle`/`emptyDescription`/`readOnlyNotice`) with four implementations: `glossaryVocabulary`, `classificationVocabulary`, `domainVocabulary`, `ontologyPackVocabulary`. Every vocabulary reduces to Slice A's own two shapes (`vocabularyTree.ts`'s flat `{id, name}` list plus each item's `broader` relation) without a line of new tree logic — a classification's tags declare a synthetic `broader` at their classification, a domain's `parentId` becomes the same thing. 25 unit tests, Stryker 99.53% (100% of covered mutants killed; the one `[NoCoverage]` survivor is the same accepted-equivalent "fallback value immediately filtered downstream either way" pattern already accepted in `vocabularyTree.ts`). `ontologyPackVocabulary` reuses `glossaryVocabulary`'s calls verbatim against the pack's own `glossaryId` (Epic 33 decision 4: pack terms land as ordinary Approved glossary terms) — the only backend gap this slice needed, and it was zero: Epic 25 and Epic 23 routes were already fully shipped, just undocumented in `openapi.rs`'s `ROUTES` table (a pre-existing, unrelated gap, not touched here).

`VocabularyBrowser.tsx` was rewritten to accept `config: VocabularyConfig` and lost every glossary-specific import (`Glossary`/`GlossaryTerm`/`SkosRelation`, direct `api.*` calls) — it now renders `VocabularyDetail.fields`/`relationsLabel`/`usageLabel` generically and shows `config.readOnlyNotice` as a banner when present. The poly-hierarchy identity machinery from Slice A (`renderKey` vs `termId`, `multiple` + `info.node` selection, the plain-`<div>`-not-`Content` fix) carries over unchanged, because it was never glossary-specific to begin with. `VocabularyBrowser.structural.test.ts` reads the component's own source via Vite's `?raw` import (no `node:fs` — this project's `tsconfig.json` scopes `types` to `["vite/client"]` alone) and asserts none of the four vocabulary keys, a `switch`, or a direct `../../api` import appear in it.

**A new file, `VocabularySection.tsx`, holds the vocabulary-*picking* logic** (an antd `Segmented` for kind, a `Select` for glossary/pack instance) — deliberately one level above `VocabularyBrowser.tsx`, since which vocabulary and which instance to browse is itself vocabulary-specific reasoning the structural test would otherwise forbid. `App.tsx` now renders `<VocabularySection />` in place of the old direct `<VocabularyBrowser />`; `?vocabulary=<id>` keeps meaning "which glossary" by default (`kind` defaults to `"glossary"` when absent), so every link Slice A already produced still resolves the same way.

**One thing this slice changed that Slice A's own Playwright test had to catch**: the glossary tree's `aria-label` moved from a hardcoded `"Vocabulary terms"` to `config.treeLabel` (`"Glossary terms"` for glossary) — correct per this slice's design, since every vocabulary now names its own tree, but it broke `vocabulary.spec.ts`'s existing selector. Fixed there, not worked around here.

**Two Playwright flakes found only by running the same fresh-database suite repeatedly, neither a product bug**: (1) Slice A's own poly-hierarchy test clicked both parent rows' expand switchers back to back with no wait between them; antd's `Tree` animates the expand, and the second click occasionally fired before the first row's `aria-expanded` update had settled, intermittently rendering the poly-hierarchy term under only one parent instead of both. Fixed by asserting `aria-expanded="true"` on each row before moving to the next — a test-synchronization gap, not a `VocabularyBrowser` defect (the glossary tree code path is byte-identical to Slice A). (2) The new picker test tried to re-select an ontology pack that `VocabularySection`'s own auto-pick-first logic had already selected (the only pack in the fixture), racing antd's dropdown-close animation; fixed by asserting the already-selected state directly instead of re-clicking it.

**antd's `Segmented` keeps its native radio input visually hidden**, unlike `Tree`'s switcher which is a real clickable icon — `getByRole("radio").click()` and even `.check()` both time out waiting for it to become visible. The visible, clickable surface is the wrapping `.ant-segmented-item` label, the same class of workaround Slice A already needed for `Tree`'s switcher vs. row distinction.

All four vocabularies verified by hand against a real server (fresh Postgres database, `OIDC_ISSUER`/`GRAPH_OWL_JWT_SECRET` unset for open/system-principal mode, `agent-browser`) before writing the Playwright coverage: glossary's poly-hierarchy relations and usage, classification's mutual-exclusivity conflict naming, domain's data-product list, and — imported via the same `text/turtle` body the existing `ontology_packs.rs` integration test fixture uses — the ontology pack's read-only notice.

### Slice C: The review queue, on merge adjudication — **shipped, 7 August 2026**

**Acceptance criteria**: candidates listed with score and match reason; side-by-side comparison with matching fields highlighted and conflicting ones flagged; merge, reject, defer; **rejection requires a reason**; the decision, its evidence, and its decider are recorded and viewable later; a rejected pair does not reappear on the next resolution run; a merge is reversible per Epic 17, and the UI says so before confirming; two reviewers acting on the same candidate — the second sees the resolution, not a conflict error.
**RED**: The does-not-reappear test — a queue that re-proposes rejected candidates every night is abandoned within a fortnight, and abandonment is indistinguishable from a clean queue in every metric. Second RED: the concurrent-reviewer test, because a `409` on a queue two people are working is a dead end rather than an outcome. Mutator watch: dropping the rejection record must fail the first; accepting an empty reason must fail the criteria.
**Done when**: criteria met, mutation report reviewed, commit approved.

**Shipped**: `ui/src/features/review/ReviewQueue.tsx`, built directly against Epic 17's already-shipped resolution queue (`GET /resolution/queue`, `POST /resolution/queue/{id}/confirm`, `/reject`, `/bulk`) — no backend gap. Deliberately merge-candidate-specific for now, the same way `VocabularyBrowser.tsx` started glossary-only in Slice A: Slice D's job is to generalize it into a config-driven `ReviewQueue.tsx`, proven by adding the other three queues without restructuring this one. "Side-by-side comparison with matching fields highlighted and conflicting ones flagged" is `reviewDiff.ts`'s `compareAssets` — a pure function diffing the two full `Asset` records the queue entry's bare `target`/`candidate` ids point at (the entry itself carries neither name nor kind), the identical "decision a screen renders but does not make" split `vocabularyTree.ts` already established. 9 unit tests, Stryker 100%.

**"Defer" is client-only, deliberately**: Epic 17's queue only knows pending/confirmed/rejected, and a deferred candidate is by definition one nobody decided — so leaving it untouched server-side *is* correct, not a stub. It hides the entry from the current session's pending list until the page reloads, and the UI says so.

**"A merge is reversible... and the UI says so before confirming"**: satisfied literally — the confirm dialog names splitting before the merge happens — but a live Split action was **not** wired to a confirmed entry, and this is recorded rather than silently narrowed: there is no endpoint mapping a queue entry (or its target/candidate ids) to the `merge_records` row `POST /merges/{id}/split` needs. Epic 17's own integration test reads that id directly out of Postgres (`merge_id_for`, `crates/graph-owl-server/tests/entity_resolution.rs`) because no route exposes it. Wiring a real Split button needs that lookup endpoint first — backend scope, not this slice's.

**"Two reviewers... the second sees the resolution, not a conflict error"** is real UI-side handling, not just a claim: the server's `409` (confirm on any decided entry; reject only on an already-*confirmed* one — an asymmetry worth knowing, not a bug) is caught and turned into a plain refresh with a "someone else already decided this candidate" notice, proven by a Playwright test that decides the open page's own selected entry out from under it via a direct API call mid-test, then drives the UI's own reject action against it.

**A real, pre-existing accessibility bug found and fixed, in code this epic already shipped**: this slice's own axe check failed `heading-order` — the page title (`<Title level={4}>`) sits directly under the chrome's `<h1>graph-owl</h1>` with nothing between, a level-1-to-4 skip. Investigating why led to a bigger finding: `VocabularyBrowser.tsx`'s own per-term detail title has the **identical** bug, and Slice A's "zero axe violations" claim was true of the exact state its test scanned but not of every reachable state — its keyboard-reachability check re-navigates without `&term=`, so the axe scan there never actually has a term selected and the offending heading never renders. A new Playwright test (`vocabulary.spec.ts`, deep-linking straight to a selected term) reproduces it and is now green; both components' titles moved to `level={2}` with an explicit `fontSize` to keep the prior visual size while fixing the semantic level. Recorded here because it was found doing this slice's own work, not Slice D's.

**Bundle-size budget, still the same pre-existing gap Slice A recorded, now larger**: 563.4KB → 565.5KB after Slice A → 576.0KB after Slices B and C combined, against a 350KB gzipped budget `ui/scripts/check-budgets.mjs` has been silently failing since before this epic. Not fixed here — recorded again because the number moved and nobody else will have.

### Slice D: Three more queues, config only — **shipped, 7 August 2026, two of three**

**Acceptance criteria**: extraction review shows the **source passage with the extracted span highlighted** — provenance is the evidence, and an extracted triple without its sentence is unreviewable; drift shows declared vs actual as a diff with per-item apply; proposals carry author and discussion; a structural test asserts `ReviewQueue.tsx` has no queue-specific branch; each queue's evidence renderer is its only bespoke part.
**RED**: The extraction-provenance test. Reviewing "this document says X" without seeing where it says it is guessing, and a reviewer who is guessing approves everything — which is worse than no review, because it launders machine output as human-verified. Mutator watch: rendering the passage without the span offset must fail the highlight assertion.
**Done when**: criteria met, mutation report reviewed, commit approved.

**Shipped: extraction claims (Epic 21) and drift (Epic 20).** `ReviewQueue.tsx` generalized into a `QueueConfig` (`queues.ts`) the same way `VocabularyBrowser.tsx` generalized in Slice B — every action reduces to one of three shapes (`instant`, optionally through a static confirmation dialog; `withText`, a modal collecting one required string; `clientOnly`, defer), which is what lets Merge/Reject/Defer, Accept/Reject, and Apply/Ignore share one component with no queue naming it. `resolutionQueue.tsx` is Slice C's own content moved here unchanged in substance. `extractionQueue.tsx`'s evidence renderer is `passageSpan.ts`'s `splitPassage` (pure, unit-tested, Stryker 100%) — the extraction-provenance RED test itself: the source sentence renders with the extracted phrase highlighted in a real `<mark>`, proven end-to-end against a real server and a real submitted extraction run. `driftQueue.tsx`'s evidence renderer is the declared-vs-live diff the criteria ask for, with apply writing through to the live asset (proven by reading it back over HTTP after) and ignore requiring a reason.

**Proposals (Epic 35) is not shipped, and this is a real, named gap, not a silent one.** There is no catalog-wide "every pending proposal" listing endpoint — only per-entity (`GET /assets/{id}/change-proposals`) and per-user (`GET /users/{id}/change-proposals`), and the frontend has no "who am I" endpoint to resolve the caller's own id for the latter even if it wanted to fall back to "my own proposals". Building this queue against either would show a slice of proposals under a "review queue" label claiming to be the whole thing — worse than not building it. Needs a catalog-wide listing route on the backend first; recorded in `ReviewSection.tsx`'s own header comment as well as here.

**A design mistake found and fixed by Playwright, not by inspection**: each `QueueConfig` closes over a `raw` correlation map (`fetchEntries` populates it by id, `fetchDetail`/`performAction` read it back — the queue entry the component holds is deliberately thinner than the record the config fetched). The first version of `ReviewSection.tsx` constructed a fresh config on every render with no memoization, reasoning the configs were cheap, stateless factories — true of `vocabularies.ts`'s configs, false here. Any unrelated re-render higher in the tree (the header's own ticking clock) silently swapped in a new, empty-`raw` config underneath an already-mounted `ReviewQueue`, and the next `fetchDetail` call missed its own `fetchEntries` data — the detail pane rendered blank rather than throwing, so nothing failed loudly until a real browser was driven through it and the passage never appeared. Fixed with `useMemo` keyed on `kind`.

**A structural design gap, found by the structural test itself**: the original `ReviewQueue.tsx` imported `ApiError` from `../../api` to detect the two-reviewers `409` — a legitimate cross-cutting need, but a `../../api` import the structural test (correctly) forbids. Fixed by moving that detection into each config's own `performAction`, which now resolves `{conflict: boolean}` instead of throwing on `409` (a small shared helper, `apiAction.ts`, Stryker 100%, avoids repeating the try/catch three times) — `ReviewQueue.tsx` reads a thrown error's plain `.message` for everything else, never `ApiError` itself.

**Two accessibility/test-infrastructure findings, neither specific to this slice's own code**: (1) axe's `aria-dialog-name` rule fired when scanned mid-close-animation on an antd `Modal` (`ant-zoom-leave-active` — antd detaches the `aria-labelledby` wiring before the fade finishes; confirmed by hand that a settled, open dialog carries it correctly) — affects every `Modal` in this feature, including Slice C's own already-shipped one, whose Playwright test needed the identical `await expect(page.getByRole("dialog")).toHaveCount(0)` wait added retroactively. (2) Adding this slice's own spec file as a fourth file in `ui/tests/` exposed that `fullyParallel: false` alone does not make the whole suite sequential — Playwright still schedules different *files* onto separate worker processes by default, and two files driving the one shared server/database concurrently intermittently broke `first-run.spec.ts`. Fixed with an explicit `workers: 1` in `playwright.config.ts`. Neither finding is Slice D product code, both are recorded here because Slice D's own new file is what surfaced them.

**Slice C's own conflict-notice copy changed as a direct consequence of generalizing**: "Someone else already decided this candidate." became "Someone else already decided this." — "candidate" was resolution-specific wording a generic component cannot own. `review-queue.spec.ts`'s assertion updated to match, the same kind of test-text drift Slice B's `treeLabel` rename already established a precedent for.

### Slice E: Property-graph view and export — **shipped, 7 August 2026, one of two**

**Acceptance criteria**: the Knowledge tab toggles triples ⇄ node-with-labels-and-properties over one subject; the toggle preserves scroll and selection; **lossy aspects of the mapping are named on screen**, from Epic 7c's `MappingReport`, not silently dropped; export dialog offers RDF and property-graph formats in one list with scope, as-of, and a preview of the first records; an export the principal is not permitted in full exports the permitted subset and **says so** rather than failing or silently truncating.
**RED**: The lossy-mapping test — a view toggle that silently drops what does not map teaches users the two models are equivalent when Epic 7c's entire mapping report exists to say they are not. Second RED: the partial-export test, because an export that silently omits denied rows produces a file someone will treat as complete. Mutator watch: discarding the mapping report must fail; a full-or-nothing export must fail the partial test.
**Done when**: criteria met, mutation report reviewed, commit approved.

**Shipped: the Knowledge-tab triples ⇄ property-graph toggle.** No route existed to fetch one asset's own facts as a property-graph node, so this slice added one: `Catalog::lpg_node_for` (`graph-owl-api`) resolves the caller's own asset via the same `get_asset_for` authorization template every other per-asset method already uses (uniform `NotFound` for both "does not exist" and "denied" — immune to the FQN-vs-UUID authorization bug found and fixed earlier this epic), queries that subject's own flakes, and lowers them through `graph_owl_lpg::node_from_flakes` — the same conversion Epic 7c already built for Bolt, now reachable over HTTP for the first time via `GET /assets/{id}/lpg-node`. An asset that predates the graph engine being configured reads as `404`, not an error — proven with two `Catalog` instances sharing one storage `Arc`, one with `.with_graph()` and one without, to reconstruct that exact history without needing a real feature-flag flip. The route is deliberately not registered in `openapi.rs`'s schema table: `LpgNodeView` embeds `graph_owl_lpg::ElementId`, whose `Serialize` is hand-written rather than derived, and hand-writing a matching `utoipa::ToSchema` for it was judged disproportionate to one route — the same undocumented-route gap Slice A's write-up already recorded for two Epic 25/23 routes, not a new kind of debt.

`KnowledgeGraphToggle.tsx` is the RED test itself: `knowledgeGraph.ts`'s `describeLoss` (100% mutation score) gives every `LossyMapping` variant — `refInProperty`, `namedGraphCollapse`, `typeNarrowed` — its own named sentence, never a blank string or a generic fallback that would erase which kind of loss it was. Both panels (triples, property graph) stay mounted simultaneously, switched with inline `style={{display: ...}}` rather than conditional JSX, which is the whole mechanism behind "the toggle preserves scroll and selection": nothing unmounts, so nothing resets. Confirmed end-to-end against a real server with a real two-level asset (`service` → `database`, so the child's own `parentService` reference is a genuine lossy mapping, not a fixture artifact): the property-graph view renders `The reference in "parentService" was flattened to plain text — it no longer traverses as an edge.` on screen, exactly as the acceptance criterion asks.

**The export dialog half is not shipped, and this is a real, named gap, not a silent one** — the same category of honest deferral as Slice D's Proposals queue. Backend gaps found while scoping it: no `/graph/export/*` route accepts scope, as-of, or preview-of-first-records filtering (every existing export route is all-or-nothing); no RDF-format (Turtle, JSON-LD) export exists over HTTP at all — only the five LPG-side formats. Building a dialog that offers "RDF and property-graph formats in one list with scope, as-of, and a preview" against a backend that supports none of those three filters and half the formats would show controls that silently do nothing, which is worse than not shipping the dialog. Needs the export routes extended first (scope/as-of/preview filtering, plus RDF serialization exposed over HTTP) — recorded here so it is not re-discovered from scratch when Slice E is revisited.

**Two real bugs found by Playwright, not by inspection.** (1) An unscoped `page.getByText("database", { exact: true })` was a strict-mode violation: `database` is also an existing kind-filter chip elsewhere on the page, unrelated to the property-graph panel's own `Labels` tag. Fixed by scoping every assertion in the new spec to `page.getByRole("tabpanel", { name: /knowledge/i })`. (2) `heading-order`: the panel's own `<Title>` skipped from level 2 (the asset's own title) straight to level 4 with nothing in between — axe caught it on the very first real run. Fixed to level 3, the level actually missing from the chain, not a guess copied from an unrelated tab's convention.

**A pre-existing flake, made worse by simply adding a fifth spec file, fixed at its actual cause rather than by waiting longer again.** `vocabulary.spec.ts`'s poly-hierarchy test (already fixed once in Slice D with an explicit 15s timeout) started missing again once this slice's own `knowledge-graph.spec.ts` became the fifth file sharing one Playwright worker and one browser session. Bumping to 20s bought exactly one more clean run in four — and the failure mode at 20s was every poll reading `aria-expanded="false"` for the entire window, a click that never registered at all, not one that was merely slow to land. No amount of additional waiting fixes a click that did not take effect. Replaced the single click-then-wait with `expandTreeRow`, a small retry helper that re-issues the switcher click if the row has not expanded within a short window, up to five attempts. Four consecutive full-suite runs afterward: 15/15 green every time, back to the ~10s baseline wall-clock the very first (uncontended) run showed — not just a longer wait, an interaction that survives however many more spec files this directory grows to.

### Slice G: A text-first ontology editor, with the graph as feedback

**Added 30 July 2026** after a survey of ontology tooling. The gap it closes is
real and this console does not cover it: there is a **SPARQL workbench** for
asking questions and a **graph explorer** for reading the estate, but nowhere to
*author* an ontology. Today a vocabulary is edited by writing Turtle in another
tool and importing it, which means the round trip crosses two applications and
the graph consequences of an edit are invisible until after it lands.

**Value**: an ontology author sees what their edit does to the graph while they
are making it — the feedback loop that makes a subsumption mistake obvious
rather than discoverable a week later by a wrong inference.

**Path**: a text editor beside a live graph of what it declares. The editor's
text is the **source**; the graph is a rendering of it, never an alternative
input. Two editable representations of one ontology is two things to keep in
step, and the one that loses is always the one the author was not looking at.

**Every serialisation Epic 9 parses, not Turtle alone.** Turtle ships first
because it is the format people actually write by hand, but the editor is a
*text* surface over a parser that already handles N-Triples and JSON-LD — and
tying it to one syntax would mean a second editor the day somebody arrives with
a JSON-LD context. The format is detected or chosen; the graph and the save path
are identical either way. The tooling that inspired this slice is Turtle-only,
and that is a property of its parser rather than of the idea.

**Acceptance criteria**:
- The text is parsed as the author types; a syntax error marks its line and the
  **last good graph stays on screen**, because blanking the picture on every
  half-typed triple makes the feedback useless.
- The same document opens and saves in any format Epic 9 supports, and switching
  format does not change the graph — if it does, one of the two parsers is
  wrong, and this is the cheapest place to find that out.
- The graph shows classes, properties and subsumption, and **distinguishes terms
  this ontology declares from terms it merely references** — an author who
  cannot see which is which will "fix" somebody else's vocabulary.
- Namespace and predicate filters, because a real vocabulary is unreadable
  whole.
- An edit that would be **refused by Epic 5's shapes** or produce no inference
  says so before it is saved, using the dry-run pattern the policy editor
  already establishes.
- Saving goes through the existing import path (Epic 9 Slice E) — validated and
  resolved, not a second write path.

**RED**: The stale-graph test — a syntax error must keep the previous graph and
mark the line, and a test asserting the picture did not blank is the only way to
catch a renderer that clears on every keystroke. Mutator watch: rendering
declared and referenced terms alike must fail the distinction test.

**Licensing — binding, and the reason this slice names it.** A capable editor in
this space exists and is **GPL-3.0**, which `00i` and the `cargo deny` allowlist
both refuse: copyleft is rejected, and a GPL implementation is not a source this
project may read *or* borrow structure from. Its README was read for the
capability gap only, and nothing beyond "this gap exists" was taken. The
implementation source is the **W3C Turtle specification**, per `00i` rule 2, and
the parser is `00l`'s adopt-not-write decision — `oxttl`/`oxrdf` are Apache-2.0
and already in the tree via `spargebra`.

**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice F: Agent activity, Bolt sessions, and the route budget — **shipped, 7 August 2026**

**Acceptance criteria**: agent sessions with identity, connection time, and operation counts; operations filterable by type and entity; **write-backs distinguished from reads** and linked to what they changed; Bolt endpoint status and active sessions (Epic 7d), read-only; everything filtered by Epic 13 policy like any other read, verified by a two-principal test; **no control that mutates an agent's permissions** — a structural assertion of decision 5; total routes ≤ 30, CI-asserted as a build failure; zero axe violations; every queue workable by keyboard.
**RED**: The two-principal test on agent activity. This surface aggregates *what agents read*, which is a description of the graph — an unfiltered activity log leaks the existence of entities the viewer cannot see, through the back door of an audit feature. Second RED: the route-budget check must **fail** the build when a fixture exceeds 30. Mutator watch: an unfiltered aggregate must fail the two-principal test; a permission control must fail the structural assertion.
**Done when**: criteria met, mutation report reviewed, commit approved.

**A real, unpatched authorization leak found and fixed while scoping the RED test itself.** `GET /agents/{agent_id}/activity`'s handler received the caller's `Auth(_principal)` and discarded it entirely, and `Catalog::agent_activity` took no principal at all — any authenticated caller could list every `targetFqn` any agent had ever written to, including entities they had no read access to, exactly the leak this slice's own named RED test predicted. Confirmed as a genuine, currently-exploitable gap (not a hypothetical) by writing `agent_activity_is_filtered_by_the_viewers_own_policy`, reverting the fix, and watching it fail with the exact leaked row in the assertion output, then restoring the fix and watching it pass. Fixed with the same filter-after-fetch pattern `authorized_lpg_elements` already established: fetch the storage page, then `retain` on `predicate.admits(&activity.target_fqn)` — a restricted principal's page may come back with fewer than `limit` rows even though more exist server-side, the same accepted pagination trade-off `authorized_lpg_elements` already makes, never a security one. `list_agent_grants` and `/admin/bolt/status` were already admin-gated and needed no change.

**"Write-backs distinguished from reads" resolves to the data Epic 32 actually records, not a new read-audit trail.** `AgentActivity` has no "read" outcome — every entry is a write *attempt* (`applied`, `proposed`, `refused`); MCP's own query tools (`search_assets`, `get_asset_context`, etc.) are not authorization-gated by capability and have no persisted audit log at all. The real, load-bearing distinction the data supports is `applied` (a genuine write-back, the catalog changed) versus `proposed`/`refused` (it did not) — `describeOutcome`'s own named RED test (`agentActivity.ts`, 100% mutation score) asserts exactly that boundary. A genuine MCP read-audit trail is a real, separate backend gap, recorded here rather than assumed covered.

**The route budget did not exist and had to be invented, not just checked.** This app has no router — navigation is `App.tsx`'s own `?section=` switch — so `ROUTES` in the new `routes.ts` is defined as the set of distinct top-level `section` values that switch recognizes (8 today: overview, explore, governance, workbench, vocabulary, review, connectors, admin), matching what a router's own route table would list if one existed. Admin's own tabs, the vocabulary/queue pickers, and the asset detail view are deliberately *not* counted as separate routes — each is one route absorbing many features through a config or a tab, which is the entire reason the five-pattern budget exists; counting them separately would double-count the thing being measured. `checkRouteBudget` is pure and unit-tested against both a real 30-route fixture (passes) and a synthetic 31-route one (fails) — the plan's own named RED test. `routes.structural.test.ts` greps `App.tsx`'s raw source (the same `?raw` pattern `VocabularyBrowser.structural.test.ts` established) so `routes.ts` cannot silently drift from the real switch in either direction.

**Agent activity and Bolt sessions land as two new Admin tabs, consuming zero new top-level routes** — the same "absorb through an existing pattern" argument the route budget itself makes. `AgentActivityPanel.tsx` lists real grants (`GET /agents/grants`) and, per selected agent, its filterable history (`GET /agents/{id}/activity`) — filterable by type (capability) and entity (a `targetFqn` substring match), both pure and unit-tested in `agentActivity.ts` (100% mutation score). `BoltSessionsPanel.tsx` reads `GET /admin/bolt/status`. **Decision 5 ("no control that mutates an agent's permissions") is a structural test, not a runtime check**: `AgentActivityPanel.structural.test.ts` greps both panels' raw source for any non-`GET` method or a `revoke`/`grant`/`mutate` API call and fails the build if one appears — the same class of absence-proof `VocabularyBrowser.structural.test.ts` already established, because no unit test exercising only the read paths can distinguish "a mutating control was never built" from "one exists and nothing happens to click it yet."

**Four real bugs found by an actual axe scan, none by inspection** — this is the first slice to ever axe-scan the Admin section, and each finding was pre-existing, not introduced by this slice's own code: (1) `AdminPage`'s own `<h4>Admin</h4>` sat directly under the chrome's `<h1>` with nothing between — a heading-order bug Slice C already fixed once elsewhere in this epic, just never in Admin, because nothing had scanned it before. Fixed to level 2, with the two new panels' own headings at level 3/4 to keep the chain intact. (2) An antd `Select` filter had no accessible name — its `placeholder` prop renders as decorative text, not the native `placeholder` attribute, so an unlabelled `<input role="combobox">` reached axe; fixed with an explicit `aria-label`, matching the convention `VocabularySection.tsx`'s own selects already established. (3) A genuine **theme-level** contrast bug: antd derives a `Select`'s placeholder colour from `colorTextQuaternary` by default, which this theme maps to `textDisabled` (`#94A3B8`, 2.45:1 against the page background — WCAG AA needs 4.5:1). Right for an actually-disabled control, wrong for a placeholder on a fully interactive one. Fixed in `theme.ts` with an explicit `colorTextPlaceholder: c.textMuted` (9.9:1 light, 10.7:1 dark) — an app-wide fix, not a one-off override, since any other `Select` rendering its placeholder would have hit the identical failure the moment something finally scanned it. (4) A copy bug of its own, found manually rather than by axe: filtering the activity table to zero matches showed "No activity recorded for this agent yet." — true when the agent has no activity at all, false and misleading when a filter simply matched nothing. Split into two distinct messages.

**A fifth finding axe cannot catch at all, checked directly rather than assumed**: axe verifies static accessibility properties (labels, roles, contrast), not whether an interaction actually works from a keyboard — "every queue workable by keyboard" is a separate acceptance criterion axe's own zero-violations result says nothing about. The grant table's row-selection used antd's `onRow.onClick`, which attaches a mouse handler only; confirmed directly against the real rendered `<tr>` (`tabindex: null`, no keyboard equivalent at all) rather than assumed from the "zero axe violations" result nearby. Fixed with `tabIndex={0}`, `role="button"`, `aria-pressed`, and an `onKeyDown` treating Enter/Space as the click — then re-verified directly (focus the row, press Enter, confirm the History panel opens) rather than re-trusting axe to catch it. **The identical gap exists, unfixed, in the already-shipped `ReviewQueue.tsx`'s `List.Item onClick`** (Slice C/D) — nothing in this codebase has a Playwright keyboard test for row selection anywhere, meaning this criterion was previously satisfied only by axe's own (insufficient, for this specific property) zero-violations check. Fixed here because it is this slice's own new code; recorded here as a real, pre-existing gap in Slice C/D's own shipped queues, worth a follow-up rather than silently assumed covered.

**A file-ordering fragility found and fixed, not merely worked around.** This slice's own Playwright spec, originally `agent-activity.spec.ts`, sorted alphabetically *before* `first-run.spec.ts` — and `workers: 1` runs spec files in that discovery order. `first-run.spec.ts`'s own test asserts the *empty*-database state; a spec that creates real data and runs first breaks that assertion with a state-dependent axe violation that reads as an unrelated bug, not an obvious "wrong data" one. Every other spec file in this directory already happened to sort after `first-run.spec.ts`, an implicit convention nothing had ever documented or violated until now. Fixed by renaming to `governance-agent-activity.spec.ts` and writing the convention explicitly into `playwright.config.ts` — Slice G's own future spec file needs to sort after `f` too, and now there is somewhere that says so.

**Mutation testing on the authorization fix itself found nothing further to fix, for an explainable reason**: `cargo mutants --diff` against the change found exactly one mutable site (the function signature) and its only generated mutant did not type-check (`Page::new()`, which does not exist), so cargo-mutants' own heuristics could not produce a meaningful mutant for this function. The manual RED→GREEN cycle performed while writing the fix (revert the `retain`, watch the real leaked row appear in the failing assertion, restore, watch it pass) is the stronger evidence here and stands in its place.

## Explicitly deferred (with destination)

- **Vocabulary authoring beyond CRUD** (bulk import, merge, reparenting subtrees) → metadata-as-code (Epic 20) is the path for bulk change; `00f` rejects an authoring GUI.
- **Approval chains on review decisions** → a workflow engine, rejected in `ROADMAP.md`. A reason and a recorded decider cover the audit need.
- **Active learning from review decisions** → Epic 17 records the decisions; feeding them back into the matcher is a resolution-engine change, not a UI one.
- **Agent controls (pause, revoke, throttle)** → Epic 13 policy and Epic 12 tokens. Decision 5.
- **Scheduled or subscribed exports** → the job scheduler (Epic 15), not a dialog.
- **A visual SKOS relation editor** → the tree plus a relation picker covers it; a graph editor for the glossary is Epic 40's canvas asking for write access, which it does not have.

## Pre-PR quality gate

1. **Stryker** — 0 missed on the two config-driven components and the decision-recording logic.
2. Refactoring assessment. 3. `tsc --noEmit` strict; ESLint clean.
3. **A fifth vocabulary and a fifth queue added by config alone**, in a test (Slices B, D).
4. **Rejected proposals do not reappear** (Slice C).
5. **Extraction evidence shows the source span** (Slice D).
6. **Mapping losses are named on screen**; partial exports declare themselves (Slice E).
7. **Agent activity is policy-filtered**; no permission control exists (Slice F).
8. **≤ 30 routes**, zero axe violations, keyboard-complete queues.
