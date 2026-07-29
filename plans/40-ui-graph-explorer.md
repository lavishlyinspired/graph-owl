# Plan: Graph Explorer, Lineage & Time Travel (Epic 40) ★

**Branch**: feat/ui-explorer
**Status**: In progress — **differentiator**. Slices B and D shipped, and the
canvas is now Cytoscape with a deterministic layout and WebGL above 256 nodes
(29 Jul 2026). Slice A's model exists but not its `Sid`-derived identity; the
lineage DAG waits on Epic 29, which is what would declare lineage; E and F not
started
**Depends on**: Epic 39 (console shell, trust components), Epic 7a (traversal), Epic 4 (flakes, time travel), Epic 6 (reasoning overlay), Epic 29 (lineage), **Epic 7c** (`Sid`-derived element ids — Slice A's node identity comes from the LPG projection, not from fetch order)
**Crates**: frontend in **`ui/`** (features `explorer`, `lineage`, `graph`) · served by **`graph-owl-ui`** · consumes the Epic 7a traversal, Epic 29 lineage, and Epic 4 as-of APIs — **no new endpoint is private to the console**

## The renderer swap, as built (29 July 2026)

The hand-drawn SVG is replaced by Cytoscape. `00f`'s revision picked it over
Sigma because Cytoscape ships **deterministic layouts** and Sigma has none — with
Sigma the layout is code this project writes, owns and tests, in order to use a
library whose WebGL advantage expired when Cytoscape 3.31 shipped one.

**Everything decidable lives in `ui/src/graph/cytoscape.ts`**, not in the
component: which elements exist, what classes each carries, whether the layout
is deterministic, and whether WebGL is worth switching on. `00f` requires graph
tests to assert the model rather than the picture, and that is only possible if
the picture is thin. The component is mount, feed, listen.

**The model did not change.** `GraphView` was renderer-agnostic from Slice A, so
`model.ts`, `diff.ts` and their mutation-tested assertions survived the swap
untouched. That was the claim the design made two slices ago and this is the
first time it was actually tested.

Three decisions worth keeping:

- **WebGL above 256 nodes, chosen once at creation.** Not because Canvas is
  inadequate below that — it is *better*, because WebGL has a fixed
  context-creation cost and a texture atlas that only pays for itself once there
  is enough to draw. `00f` rejects a hybrid that swaps renderers mid-session:
  the swap discards the layout at the moment a reader most needs it, since their
  mental map of where things are is the main thing keeping a large graph
  legible. 256 is an order of magnitude below the 10,000-node interactivity
  budget, so the budget is never what this threshold is testing.
- **Elements are replaced in place, not remounted.** A remount loses the
  reader's pan and zoom on every expand — the one thing they were using to keep
  their place.
- **Removed nodes are marked by shape and opacity, never colour alone.** A
  deletion shown only in red is invisible to a reader who cannot see red, and
  this screen exists to show deletions.

A mutation run over `src/graph` found the assertion style was hiding a real
defect: `classes.join(" ")` mutated to `join("")` produced one garbage class
name that broke every style rule, and every `toContain("seed")` assertion passed
straight through it. Classes are now asserted as a split list. `src/graph` is at
**100%**, 166 mutants.

## Why this epic is starred

`00a-product-position.md` names time travel, inference, and validation as the differentiators. All three are things a picture communicates instantly and JSON communicates badly:

- **Time travel** is invisible in a payload and self-evident on a slider. `op = false` is a retraction, not a delete (`00b-architecture.md`), so this graph can render *what the estate looked like on any past date* — and comparable catalogs cannot, because they overwrite.
- **Inference** is indistinguishable from assertion in a response body unless you read the provenance field. On a canvas it is a different edge, visibly.
- **Blast radius** is a number in an API and a shape on a screen, and the shape is what makes someone cancel a deployment.

This is also the screen that decides evaluations. A graph engine with no graph view loses to products with worse graphs and better pictures — that is the whole argument in `00f-ui-architecture.md`'s positioning section.

## Resolved decisions

1. **Two renderers, chosen by graph *shape*, not by feature.** Lineage is a DAG read left-to-right at modest node counts → **React Flow (`@xyflow/react`) + d3-dag layered layout**. Exploration is an arbitrary cyclic graph at 10k+ nodes → **Cytoscape.js, WebGL renderer**. Force-directed layout on a DAG is actively wrong; a DOM renderer on 10k nodes stops being interactive an order of magnitude too early. Behind **one internal interface**, so a third shape is a decision rather than an accident.

   *Revised 28 Jul 2026* — this decision previously named Sigma.js and ELK. Both changed; the *rule* (two renderers, one per shape) did not. Sigma was chosen for being the WebGL option and Cytoscape has since shipped a WebGL renderer, which removes the deciding difference and leaves Cytoscape's built-in deterministic layouts as the tiebreak — the thing decision 6 of `00f`'s testing rules requires and Sigma would have us hand-write. `elkjs` is EPL-2.0, which `00i-licensing.md` rejects by default, and the console is embedded in the shipped binary; MIT-licensed d3-dag does the same layered layout without needing a documented licence exception. Full reasoning in `00f-ui-architecture.md`, revision of 28 Jul 2026; decision log entries 17–20 in `00b-architecture.md`.

   **A node-count-threshold hybrid (Cytoscape below 10k, Sigma above) was proposed and rejected.** Two libraries for one graph shape is precisely the accretion `00f` consequence 2 forbids; a mid-session swap throws away the layout at the moment the user's mental map is the main thing keeping the picture legible; and the 10,000 switch point had no derivation — it was `00f`'s *interactivity budget* reused as if it were a measured Cytoscape limit. Per `00i` rule 4, a magic number needs a stated reason, and "the budget is also 10k" is not one.
2. **Expand-on-click, never load-the-graph.** The canvas opens on a seed and grows by explicit expansion. There is no "show everything" — on a real estate it is a hang, and on a demo estate it is a hairball that teaches nothing.
3. **Every fetch is budgeted server-side.** Expansion calls Epic 7a's traversal with an explicit `NodeBudget`. A truncated result is rendered **as truncated**, with the count omitted and a way to continue. Silent truncation is the failure mode that makes a lineage view untrustworthy.
4. **Derived edges are visually distinct, always, and not by colour alone.** Uses Epic 39 Slice E's shared components. `00b-architecture.md` says the reasoning overlay is never persisted; the canvas must never let a user mistake an inferred edge for an asserted one — including in a screenshot pasted into a ticket.
5. **Time travel is a first-class control on the canvas, not a settings page.** A date control that re-queries as-of `t` and re-renders. Diff mode (added / removed / changed between two times) is the same control with two values.
6. **Column-level lineage is a zoom level of the same view, not a separate screen.** Table-level and column-level are the same DAG at different granularity; splitting them into two screens forces the user to hold the mapping in their head.
7. **The canvas has a non-visual equivalent, in this epic, not later.** A navigable tree/list of the same nodes and edges, keyboard-operable, screen-reader-labelled, deep-linkable. A WebGL canvas is otherwise entirely unusable to a screen reader, and "it's a graph" is not an exemption (`00f-ui-architecture.md` non-negotiable 7).

8. **The WebGL renderer is the *last* thing Slice B needs, not the first.**

   Decision 1 commits to a WebGL renderer for exploration and that stands — but it is a
   commitment about which renderer handles which *shape*, not about build order.
   The valuable half of Slice B is expand-on-click: seeding from an entity,
   fetching neighbours, growing the picture. That is API and view-state work, and
   it lands on the SVG canvas that already exists.

   **No renderer makes a large graph legible.** At ten thousand visible nodes the
   picture is unreadable whether it is SVG, WebGL, or anything else; what makes a
   large graph readable is clustering and progressive disclosure, which are layout
   and interaction problems. Swapping the renderer first buys the ability to draw
   an illegible picture faster.

   So the honest order inside this epic is: expansion and budgeting on the
   existing canvas → the lineage DAG (a *different* problem — layered
   left-to-right, not force-directed, and the one users ask for by name) → the
   WebGL swap, when a real graph is measurably slow rather than theoretically
   large. The current SVG is honest at demo scale and will not survive 10k nodes;
   that is a real limit, and it is the third one to fix.

   *Update, 28 Jul 2026* — this order held. Slices B (expansion) and D (diff)
   both shipped **on the SVG canvas**, and neither needed a renderer to do it,
   which is the evidence for the claim rather than a restatement of it. What
   the swap now has to buy is scale alone — every interaction it was once
   expected to bring already exists and is tested at the model layer, so the
   swap is a rendering change with no behaviour attached.


## Implementation reference

```
ui/src/graph/
  GraphView.tsx        one interface, two implementations          [not yet]
  renderers/
    dag.ts             React Flow + d3-dag — lineage, impact       [not yet]
    canvas.ts          Cytoscape.js WebGL — exploration            [not yet]
  model.ts             nodes, edges, truncation — renderer-agnostic  [SHIPPED]
                       seed() and expand(); budgeted expansion
                       against Epic 7a traversal
  diff.ts              two GraphModels at t1 and t2                  [SHIPPED]
                       → added / removed / changed / unchanged
```

**What shipped differs from what this block originally predicted, and the difference is worth keeping.** `model/GraphModel.ts` + `model/expand.ts` landed as one `model.ts`: expansion is not a separate concern from the model it expands, and splitting them would have produced a file whose only export took the other file's type and returned it. The `renderers/` directory does not exist yet because the SVG canvas lives in `App.tsx` and there is exactly one renderer — inventing the interface before the second implementation exists would be guessing at the seam. It gets extracted when the second renderer arrives, which is the point at which its shape is known rather than assumed.

**Dependencies, when the renderers land.** Package names verified 28 Jul 2026 — the older ones are traps:

| Purpose | Package | Licence | Note |
|---|---|---|---|
| Exploration canvas | `cytoscape` | MIT | WebGL renderer is in-core from v3.31; no separate extension |
| Lineage DAG | `@xyflow/react` | MIT | **Not `reactflow`** — renamed at v12; the old name installs a frozen v11 |
| Lineage layout | `d3-dag` | MIT | Layered Sugiyama. **Not `elkjs`** — EPL-2.0, rejected by `00i` |

Two libraries, three packages, no extensions. Cytoscape's `expand-collapse` extension is **not** wanted: it collapses *compound* (parent/child) nodes, which is a different problem from fetching more neighbours from the server, and adopting it would mean adopting the wrong abstraction for the interaction this epic already has working.

**The model is renderer-agnostic and is what the tests assert.** `00f-ui-architecture.md`: *graph tests assert the model, not the picture*. With a seeded deterministic layout, assert that X is upstream of Y, that a derived edge carries the derived treatment, that a 0.55-confidence edge is marked — never that a pixel sits at a coordinate. Screenshot tests on a graph canvas fail on every renderer upgrade and catch almost nothing.

### What the two views are for

| View | Renderer | Shape | Question it answers |
|---|---|---|---|
| Exploration | Cytoscape/WebGL | Arbitrary, cyclic, large | "What is this connected to, and how?" |
| Lineage | React Flow/d3-dag | DAG, layered | "Where did this come from and where does it go?" |
| Column lineage | React Flow/d3-dag | DAG, finer granularity | "Which upstream *column* feeds this one?" |
| Impact | React Flow/d3-dag, downstream-only | DAG subtree | "What breaks if I change this?" |
| Diff | Either, overlay | Two models | "What changed between these two dates?" |

## Acceptance criteria

- [ ] Exploration canvas interactive at **10,000 nodes**; lineage at **1,000** (`00f-ui-architecture.md` budgets), CI-enforced.
- [ ] Expansion is explicit and budgeted; truncation is visible, never silent.
- [ ] Derived edges are distinguishable from asserted ones, everywhere, without relying on colour.
- [ ] Confidence bands render via Epic 39's shared components — no explorer-local styling.
- [ ] A time control re-queries as-of `t` and re-renders; diff mode shows added / removed / changed.
- [ ] Column-level lineage is reachable as a zoom level of the table-level view.
- [ ] Impact analysis reports the downstream set with counts, exportable.
- [ ] Every canvas state — seed, expansions, filters, time, zoom level — is in the URL and restores exactly.
- [ ] A **non-visual equivalent** of every canvas is keyboard-navigable and screen-reader-labelled.
- [ ] The explorer renderer is lazily loaded and absent from the initial bundle.
- [ ] The explorer and lineage route chunks each stay inside `00f`'s **100KB route-chunk budget, measured gzipped** — the number CI enforces. Published library sizes are quoted *minified* and are roughly three times their gzipped weight, so a renderer advertised at "~200KB" is not automatically over budget and must not be waved through or rejected on that figure. Measure the built chunk; do not estimate it.
- [ ] No endpoint exists solely for the console (`00f-ui-architecture.md` non-negotiable 1), asserted against the OpenAPI document.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first.

### Slice A: The graph model and one interface

**Acceptance criteria**: a renderer-agnostic `GraphModel` of nodes, edges, provenance, confidence, and truncation state; one `GraphView` interface both renderers satisfy; a fixture graph builds a model deterministically; node and edge identity comes from `Sid`-derived element ids (Epic 7c), so the same entity is the same node across views and across sessions; the model is serializable and is the shared test fixture.
**RED**: The identity test across two views — the same entity reached via exploration and via lineage must be one node with one id, or selection, deep links, and diff all silently break. Use the **shared JSON Graph fixture from Epic 9a Slice F**, so the console and the interchange format cannot drift apart. Mutator watch: an identity derived from array position or fetch order must fail it.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice B: Exploration canvas — expand, filter, budget

**Acceptance criteria**: seeds from an entity page or search result; click-to-expand calls Epic 7a traversal with an explicit budget; filters by relationship type and by confidence band; a truncated expansion renders a visible truncation marker with a continue action; expanding an already-expanded node is a no-op, not a duplicate; 10k nodes stay interactive; the renderer is lazily loaded; every action updates the URL.
**RED**: The silent-truncation test — a budget-limited expansion that renders as if complete is the single most damaging bug on this screen, because the user draws a conclusion ("nothing else depends on this") from an absence the system created. Second RED: the double-expand test, since duplicated nodes make degree counts wrong and degree is what Epic 38 surfaces as blast radius. Mutator watch: dropping the truncation flag must fail; a set-union that becomes a concat must fail.
**Done when**: criteria met, mutation report reviewed, commit approved.

**Shipped 28 Jul 2026, partially.** `graph/model.ts`: seed, expand, dedup by node id and by `(from, to, relationship)`, truncation that is **sticky** (a complete later walk does not make an earlier omission complete) and **per-node** (`truncatedAt`, so the marker sits on the node hiding something rather than only on the canvas). Expansions replay from `?expand=`, so a pasted link restores the sender's picture. Stryker: 105 mutants, 0 survivors across `model.ts` and `diff.ts` — pre-PR gate 1 met.

Still open in this slice: filters by relationship type and by confidence band; the 10k interactivity budget (SVG will not meet it — that is the renderer swap); lazy loading of the renderer (nothing to lazily load yet).

### Slice C: Lineage and column-level lineage

**Acceptance criteria**: layered DAG, upstream and downstream, from Epic 29; depth control; a cycle in the data renders without hanging and is **marked as a cycle**; column-level lineage as a zoom level, preserving the table context; a column with no upstream renders as a designed terminal, not an error; lineage from a connector, a query parse, and a manual assertion are visually distinguished by provenance; layout is deterministic under a seed.
**RED**: The cycle test. Lineage *should* be acyclic and in practice is not — a circular dependency introduced by a bad parse will hang a naive layered layout, and the correct behaviour is to render it and call it out, since a cycle in lineage is itself a finding. Mutator watch: removing the visited-set guard must hang or fail; conflating the three provenance sources must fail the distinguishability assertion.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice D: Time travel and diff ★

**Value**: This is the screen nothing comparable can show. It exists only because `op = false` is a retraction rather than a delete.
**Acceptance criteria**: a time control sets an as-of `t` and re-queries; the canvas re-renders at that time; entities that did not yet exist are absent, not blank; entities retracted after `t` are **present**; a diff mode takes two times and marks added, removed, and changed nodes and edges; the time is in the URL; a time before the first flake renders the designed empty state; the current time is visually distinct from a historical one, so nobody mistakes a past view for the present.
**RED**: The retraction test — an entity deleted last week must appear when viewing last month. If the query path treats a retraction as a delete, this returns nothing and the differentiator silently becomes an ordinary graph view. Second RED: the historical-marker test, because a user acting on a past view believing it is current is the worst outcome this screen can produce. Mutator watch: `t <= as_of` becoming `t < as_of` must fail a boundary test at exactly the transaction time.
**Done when**: criteria met, mutation report reviewed, commit approved.

**Shipped 28 Jul 2026, partially.** `graph/diff.ts` compares two instants. A node present before and absent after is **removed and still listed** — dropping it renders past and present identically, which is this screen's differentiator failing silently. A rename is `changed`, not unchanged-because-the-id-matched. An edge is identified by both endpoints *and* its relationship, so re-typing one is a removal plus an addition and there is no `changed` case for edges. Either side truncated marks the comparison `partial`, because a partial comparison shown as complete **invents deletions that never happened**. Change is signalled by sigil (`+` / `−` / `~`), strikethrough, and dash pattern — not colour alone (decision 4), so it survives greyscale and a screenshot in a ticket. `?compareTo=` carries the second instant.

**Known-loose, and named rather than hidden**: the diff runs over the *seed* walk at each instant, not the expanded model — expansions are not replayed against the baseline, so a user who expands while comparing sees new nodes marked as added-at-the-later-instant regardless of when they actually arrived. This is the one place on the screen that can currently assert a change that did not happen, which makes it the next RED in this slice rather than a deferral.

Still open: the historical-marker requirement (a past view must be visually distinct from the present one), and the designed empty state for a time before the first flake.

### Slice E: Impact analysis and derived-edge treatment

**Acceptance criteria**: from any node, the downstream set with counts by type and by depth; exportable as CSV and as the Epic 9a JSON Graph format; derived edges (Epic 6) rendered distinctly, with the derivation rule and its inputs reachable from the edge; confidence rendered via Epic 39's shared components; a structural test asserts the canvas uses **no local confidence or derivation styling**; Epic 38's degree centrality surfaced as a blast-radius count where available, stamped with `computed_at_t`.
**RED**: The derivation-reachability test — a derived edge whose rule cannot be inspected is indistinguishable from an unexplained assertion, and `00a-product-position.md` sells explainability. Second RED: the structural no-local-styling test, protecting Epic 39 Slice E's single source. Mutator watch: rendering derived and asserted identically must fail; a colour-only distinction must fail the not-colour-alone assertion.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice F: The non-visual equivalent, performance, and journeys

**Acceptance criteria**: every canvas has a keyboard-navigable tree/list of the same model — nodes, edges, direction, provenance, confidence — with screen-reader labels and its own deep link; the toggle between visual and non-visual is a first-class control, not a hidden setting; 10k-node exploration and 1k-node lineage budgets enforced in CI on a fixture, failing the build; the explorer chunk is absent from the initial bundle; Playwright journeys for search → asset → lineage → impact, and for a time-travel investigation; zero axe violations; a structural assertion that no console-only endpoint was introduced.
**RED**: The non-visual equivalence test asserts the alternative view exposes **the same model**, not a summary of it — a degraded list that omits confidence or provenance is a compliance gesture rather than an equivalent. Second RED: the CI budget test must **fail** the build on a fixture exceeding the node budget. Mutator watch: an equivalence check comparing only node counts must fail against a fixture with matching counts and differing edges.
**Done when**: criteria met, mutation report reviewed, commit approved.

## Open questions from the renderer analysis, resolved

The renderer analysis closed with seven open questions. **Five of them dissolve once the licence problem is fixed and the hybrid is dropped** — they were consequences of the proposed architecture, not properties of the problem. Recorded so the same questions are not re-opened later as if they were unanswered.

| # | Question | Verdict |
|---|---|---|
| 1 | Cytoscape under strict CSP | **Resolved.** Canvas and WebGL rendering need no `eval`. The concern was specifically the `expand-collapse` extension, which this epic does not use — it collapses compound parent/child nodes, a different problem from fetching neighbours |
| 2 | `elkjs` web worker + `Blob` URL under CSP | **Dissolved.** d3-dag is a plain library on the main thread with no worker and no `Blob`, so the risk does not exist. A second reason to prefer it, independent of the licence |
| 3 | Sigma.js CSP / shader compilation | **Dissolved.** Sigma is not adopted |
| 4 | Bundle size against the footprint budget | **Open, but measurable.** Now an acceptance criterion above, gzipped, measured rather than estimated |
| 5 | Whether lineage nodes show column lists inside table nodes | **Genuinely open, and it decides Slice C.** If yes, React Flow's component-per-node model earns its place; if lineage nodes stay simple, one renderer could serve both shapes and the two-renderer rule would be worth re-examining rather than assumed. Decide this before writing Slice C, not during |
| 6 | Porting the radial BFS layout to graphology | **Dissolved.** Sigma is not adopted, and Cytoscape's `breadthfirst` replaces the hand-written layout outright |
| 7 | Maintaining three rendering paradigms | **Dissolved.** Two, per `00f` consequence 2 |

## Explicitly deferred (with destination)

- **3-D / force-directed exploration in 3-D** → not doing. The reference implementation studied ships a third renderer for this; it is the clearest example of accretion in `00f-ui-architecture.md`'s scale-discipline section.
- **Graph editing on the canvas** → the API and metadata-as-code (Epic 20) are the write paths. A canvas that writes needs undo, conflict resolution, and validation — three products in disguise.
- **Saved / shared canvas layouts** → the URL already restores state, which covers sharing. Persisted layouts need storage, permissions, and a lifecycle.
- **Real-time collaborative cursors** → a catalog is not a whiteboard.
- **Automatic layout tuning per graph** → deterministic layout is a testing requirement (`00f`); adaptive layout trades that away.
- **Analytics overlays beyond degree** → Epic 38 decides PageRank's fate in its own bake-off; nothing here depends on the outcome.

## Pre-PR quality gate

1. **Stryker** — 0 missed on the graph model and expansion logic.
2. Refactoring assessment. 3. `tsc --noEmit` strict; ESLint clean.
4. **Truncation is visible** — asserted on a budget-limited fixture (Slice B).
5. **A retracted entity is visible at a time before its retraction** (Slice D).
6. **Derived ≠ asserted, without colour alone**, using Epic 39's shared components (Slice E).
7. **Non-visual equivalent exposes the same model**, zero axe violations (Slice F).
8. **No console-private endpoint** — asserted against the OpenAPI document.
