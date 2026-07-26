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

### Slice A: The vocabulary browser, on one vocabulary

**Acceptance criteria**: tree with lazy expansion, keyboard navigation, and deep-linkable selection; detail pane with description, relations, and the assets carrying the term; a poly-hierarchy term (several `broader` parents, legitimate in SKOS per Epic 24) renders under **each** parent without duplicating identity; a cycle in the data renders and is marked rather than hanging; an empty vocabulary shows a designed first-run state.
**RED**: The poly-hierarchy test. A tree component that assumes one parent per node either drops the second placement or forks the node into two identities with divergent selection state — and SKOS explicitly permits multiple parents, so this is normal data, not an edge case. Mutator watch: keying nodes by path rather than by id must fail it; an unguarded recursive walk must hang on the cycle fixture.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice B: Three more vocabularies, config only

**Value**: This slice is the proof of decision 1. If it requires component changes, the pattern is not a pattern.
**Acceptance criteria**: classifications, domains, and ontology packs render through the same component; mutual exclusivity (Epic 25) is enforced in the tag picker with the conflicting tag named; domains show their data products; ontology packs are read-mostly and say so rather than offering disabled controls; a **structural test asserts `VocabularyBrowser.tsx` has no vocabulary-specific branch**.
**RED**: The structural no-branching test, plus adding a **fifth, fictional** vocabulary in a test fixture and asserting it renders with config alone. Testing only the four real ones cannot distinguish a parameterized component from one with four hardcoded paths. Mutator watch: a `switch` on vocabulary type must fail the structural assertion.
**Done when**: criteria met, mutation report reviewed, commit approved.

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
