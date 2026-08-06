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

### Slice C: The review queue, on merge adjudication

**Acceptance criteria**: candidates listed with score and match reason; side-by-side comparison with matching fields highlighted and conflicting ones flagged; merge, reject, defer; **rejection requires a reason**; the decision, its evidence, and its decider are recorded and viewable later; a rejected pair does not reappear on the next resolution run; a merge is reversible per Epic 17, and the UI says so before confirming; two reviewers acting on the same candidate — the second sees the resolution, not a conflict error.
**RED**: The does-not-reappear test — a queue that re-proposes rejected candidates every night is abandoned within a fortnight, and abandonment is indistinguishable from a clean queue in every metric. Second RED: the concurrent-reviewer test, because a `409` on a queue two people are working is a dead end rather than an outcome. Mutator watch: dropping the rejection record must fail the first; accepting an empty reason must fail the criteria.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice D: Three more queues, config only

**Acceptance criteria**: extraction review shows the **source passage with the extracted span highlighted** — provenance is the evidence, and an extracted triple without its sentence is unreviewable; drift shows declared vs actual as a diff with per-item apply; proposals carry author and discussion; a structural test asserts `ReviewQueue.tsx` has no queue-specific branch; each queue's evidence renderer is its only bespoke part.
**RED**: The extraction-provenance test. Reviewing "this document says X" without seeing where it says it is guessing, and a reviewer who is guessing approves everything — which is worse than no review, because it launders machine output as human-verified. Mutator watch: rendering the passage without the span offset must fail the highlight assertion.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice E: Property-graph view and export

**Acceptance criteria**: the Knowledge tab toggles triples ⇄ node-with-labels-and-properties over one subject; the toggle preserves scroll and selection; **lossy aspects of the mapping are named on screen**, from Epic 7c's `MappingReport`, not silently dropped; export dialog offers RDF and property-graph formats in one list with scope, as-of, and a preview of the first records; an export the principal is not permitted in full exports the permitted subset and **says so** rather than failing or silently truncating.
**RED**: The lossy-mapping test — a view toggle that silently drops what does not map teaches users the two models are equivalent when Epic 7c's entire mapping report exists to say they are not. Second RED: the partial-export test, because an export that silently omits denied rows produces a file someone will treat as complete. Mutator watch: discarding the mapping report must fail; a full-or-nothing export must fail the partial test.
**Done when**: criteria met, mutation report reviewed, commit approved.

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

### Slice F: Agent activity, Bolt sessions, and the route budget

**Acceptance criteria**: agent sessions with identity, connection time, and operation counts; operations filterable by type and entity; **write-backs distinguished from reads** and linked to what they changed; Bolt endpoint status and active sessions (Epic 7d), read-only; everything filtered by Epic 13 policy like any other read, verified by a two-principal test; **no control that mutates an agent's permissions** — a structural assertion of decision 5; total routes ≤ 30, CI-asserted as a build failure; zero axe violations; every queue workable by keyboard.
**RED**: The two-principal test on agent activity. This surface aggregates *what agents read*, which is a description of the graph — an unfiltered activity log leaks the existence of entities the viewer cannot see, through the back door of an audit feature. Second RED: the route-budget check must **fail** the build when a fixture exceeds 30. Mutator watch: an unfiltered aggregate must fail the two-principal test; a permission control must fail the structural assertion.
**Done when**: criteria met, mutation report reviewed, commit approved.

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
