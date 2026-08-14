# Plan 114 — Graph canvas legibility and interaction: labels, colour, inspector, temporary hide

**Status**: Slices A, B, D, E shipped. Slice C (collapse — undo one expansion)
planned, not started. **Branch**: main.
**Trigger**: user feedback that "the current cytoscape is not looking good",
plus explicit asks for legible node/edge labels, expand/contract, temporarily
removing nodes, colourful nodes with a legend, draggable nodes, a full-screen
canvas, and a Neo4j-style per-node detail panel. Investigated rather than
assumed: reproduced the actual defect live (a real 2-node evidence graph
rendering as two screen-filling circles with no edge label and no arrowhead),
then read `plans/00f-ui-architecture.md`'s dated 28 Jul 2026 decision, which
commits to exactly two graph renderers project-wide and explicitly rejects a
third ("adding a third requires a shape neither handles"). Presented that
conflict back before writing any code; the user chose to stay inside Cytoscape
rather than adopt AntV G6. This plan is that choice, scoped into slices.

## What's actually wrong, verified rather than assumed

Read `ui/src/graph/{model,cytoscape,GraphCanvas}.tsx` and reproduced a real
evidence graph in the browser (Review → Findings → a GST finding with an
`issuedBy` edge). Three concrete defects, all inside `GraphCanvas.tsx`'s
inline Cytoscape config — `cytoscape.ts` and `model.ts` (116 mutation-tested
assertions between them) are unaffected by any of this and stay untouched:

1. **Edge labels are computed but never drawn.** `toElements()` already
   attaches `data.label = edge.relationship` to every edge (`cytoscape.ts`),
   but the `edge` style selector in `GraphCanvas.tsx` never references
   `data(label)` — only `width`, `line-color`, `curve-style`. The relationship
   name a reader most needs ("issuedBy") is present in the data and invisible
   on screen.
2. **No arrowhead.** `source`/`target` are a directed fact, but no style rule
   sets `target-arrow-shape`, so direction is not visually distinguishable
   from an undirected line.
3. **No fit-to-content.** The canvas is laid out via `layoutOptions()` and
   never `.fit()`. On a small neighbourhood this produces exactly the
   screenshot that prompted the complaint: two nodes at whatever zoom
   Cytoscape defaulted to, filling the visible area edge to edge, illegible.

None of these are model bugs. `cytoscape.test.ts`'s assertions on
`toElements`/`nodeClasses`/`edgeClasses`/`layoutOptions` stay green and
unchanged throughout. They live entirely in the inline `style: [...]` array
and the missing `.fit()` call — exactly the part `00f` calls "a drawing
decision" and does not require picture-level testing for. But "not tested"
should not mean "not extractable and not verifiable at all": Slice A moves the
style array into `cytoscape.ts` as a pure, tested function, so a missing label
or arrowhead is a RED test next time, not a screenshot someone has to notice
by hand.

## Slices

### Slice A — Legibility: edge labels, direction, fit-to-content ✅ shipped

- Extract the inline Cytoscape `style: [...]` array out of `GraphCanvas.tsx`
  into a pure `graphStyle(colors)` function in `cytoscape.ts`, structurally
  typed the same way `Element` already is — no `cytoscape` import needed to
  test it.
- Add the missing edge `label: "data(label)"`, with `text-background-color`/
  `text-background-opacity` so it stays legible over a crossing line, and
  `text-rotation: "autorotate"` so it follows its edge rather than always
  reading horizontally.
- Add `target-arrow-shape: "triangle"` with `target-arrow-color` matching each
  edge class's own line color — a derived edge keeps its cyan arrow, not a
  mismatched default.
- `GraphCanvas.tsx` calls `.fit()` (with padding) after both the initial
  layout and every re-layout on expand — the one imperative addition that has
  to live in the shell itself, undocumented by a unit test on purpose (`00f`:
  graph tests assert the model, not the picture) but stated here as the
  reason.
- **RED**: `graphStyle(colors)` includes an `edge` rule whose `label` reads
  `"data(label)"`; includes `target-arrow-shape`; the `derived` edge rule's
  arrow color matches its own line color rather than the default edge color.
  All fail against today's inline array before it moves.

**Retrospective.** 100% mutation score on `cytoscape.ts` (105/105), `model.ts`
untouched. Two more real defects surfaced only by looking at a live render
after the first fix, not by reading the code:

- **`.fit()` with no zoom cap.** A 2-node evidence graph — the common case,
  a finding's own subject and one neighbour — fit-to-content computed a zoom
  high enough to render an 18px node as a ~100px circle filling the frame.
  Added `MAX_ZOOM = 2` (`cytoscape.ts`), passed as Cytoscape's own `maxZoom`
  option. Documented as a stated, reasoned constant per `00i` rule 4, not a
  guess: generous enough that `.fit()` still does real work on a spread-out
  neighbourhood, low enough that a sparse graph stays legible.
- **`autoungrabify: true` disabled manual dragging, not just automatic
  movement.** The original comment ("nothing moves on its own") was about
  layout determinism (`animate: false`), but the flag it sat next to also
  blocks a reader from dragging one node to see behind another — normal
  graph-reading behaviour Plan 114 explicitly asks to restore. Removed.
- **`text-rotation: "autorotate"` was actively wrong for the same 2-node
  case.** A near-vertical edge rotated its label to read sideways, one letter
  per line, and collided with the source node's own label below it. Switched
  to the (unset) default horizontal — legible at any edge angle, matching how
  Neo4j Browser draws relationship-type labels.

All three were verified by rendering the real evidence graph via `vite dev`
proxied to the already-running `:8080` server (never restarted it, per
standing guidance), not by reasoning about the style array in the abstract.

### Slice B — Temporarily hide a node, without touching the model ✅ shipped

- New, purely client-side visibility state — a `Set<string>` of hidden node
  ids — owned beside whatever already holds `Picture` state (the evidence
  graph's caller, the asset explorer's), **not** inside `GraphModel`. `00f`'s
  own state-ownership table already draws this line: "Explorer canvas state
  (selection, expansion, filters) is genuinely client-side." Hiding is a
  filter, the same shape as the relationship-type filter Plan 112/113 already
  added elsewhere, and must not be confused with the `removed` diff class —
  that means a real transaction retracted the fact; this means a reader does
  not want to look at it right now.
- `visiblePicture(picture, hidden)` — pure function in `cytoscape.ts` — drops
  hidden nodes and any edge touching one, reusing `toElements`'s own existing
  "drop an edge whose endpoint is not present" rule rather than inventing a
  second one.
- A per-node "Hide" action, plus a "`N` hidden — show all" affordance whenever
  the set is non-empty, so a reader can never lose track of the fact the
  picture is incomplete by their own choice — distinct from `truncated`, which
  is the budget's doing, not theirs.
- **RED**: hiding a node removes it and its incident edges from what reaches
  `toElements`; hiding a node with no edges leaves the rest of the picture
  unchanged; showing all restores exactly the picture that existed before any
  hide, node-for-node.

**Retrospective.** One equivalent mutant found and removed rather than tested
around: an `if (hidden.size === 0) return picture;` fast path had no
observable behavioural difference from always filtering (an empty-set filter
returns a new array with the same contents) — `toEqual` cannot and should not
distinguish the two, so the branch was deleted rather than given a test that
would only assert its own existence. 100% mutation score after.

Wired into `GraphCanvas.tsx` as a right-click (`cxttap`) per node, plus the
inspector's own "Hide this node" button (Slice E). Verified live: right-
clicking a column node on a real cataloged table removed it and produced
"1 hidden — show all"; clicking that button restored it exactly.

### Slice D — Colour by kind, with a legend ✅ shipped

Not in the original slice list — added when the user asked directly for
"colorful nodes and a legend" mid-session, and folded into the same TDD cycle
rather than deferred, since it touches the same seam (`toElements`,
`graphStyle`) Slice A had just opened up.

- `kindColor(kind, mode, colors)` and `legendEntries(mode, colors)` — pure,
  `cytoscape.ts` — map each of the five `AssetKind`s to a hex, per light/dark
  mode. **Not picked by eye**: the `dataviz` skill's validated categorical
  palette, slots 1–5 in fixed order (blue/orange/green/amber/magenta) per its
  own rule "assign categorical hues in fixed order, never cycled" — assigned
  to `service/database/schema/table/column` in that order, not for a thematic
  reading. Both rows ran through the skill's own `validate_palette.js`
  against this project's real surfaces (light `#FFFFFF`, dark `#152A45`):
  lightness band, chroma floor, CVD separation (worst adjacent ΔE 8.4–9.1),
  and normal-vision floor all passed; light mode WARNs on contrast-vs-surface,
  which is why the legend and node labels stay in text-ink colours, never
  colour-on-colour (the dataviz skill's own rule: "text wears text tokens,
  never the series colour").
- `toElements` gains an optional `mode` parameter and sets `data.color` per
  node (omitted, not defaulted, for a `null` kind — `node.hidden-kind`'s
  existing style rule is left as the one place that decision is made).
  `graphStyle`'s base `node` rule reads it back via `"background-color":
  "data(color)"`.
- Legend rendered as a small colour dot + plain-ink label per kind, not a
  filled `Tag` with white text — a filled tag was the first attempt and was
  reverted before shipping: several of the five hexes do not clear
  white-text contrast, which the dataviz skill's own text-token rule exists
  to prevent.
- **RED → GREEN → MUTATE**: every kind resolves to a distinct, exact,
  mode-specific hex (`toBe`, not just "5 distinct values" — a first pass with
  only a distinctness check left 10 hex-literal mutants alive); a `null` kind
  never reaches the colour table at all (`"color" in data`, not
  `.toBeUndefined()` — the two are indistinguishable on a key present with
  value `undefined`, which is exactly what the surviving mutant produced).
  100% mutation score, 146/146 mutants killed.

Verified live against a real cataloged table (`hdfc-core.postgres.
core_banking.accounts`): the table node renders amber, its columns pink, the
containing schema green, the database orange — matching the legend exactly.

### Slice E — Per-node inspector, dragging, full screen ✅ shipped

Also added mid-session, also not in the original list — the user asked
specifically for what Neo4j Browser does: a right-hand panel of the selected
node's own properties, node circles a reader can reposition, and a full-
screen canvas.

- `GraphCanvas.tsx` now taps `"node"` generally (not only `.expandable`) to
  select a node and render a `Card` beside the canvas — kind, id (copyable),
  fully qualified name if present, and a "Hide this node" action wired to
  Slice B's `visiblePicture`. Untested directly, matching this file's
  standing convention (`00f`: assert the model, not the picture) — the data
  it renders (`GraphNode | DiffNode`) is already typed and exercised
  elsewhere.
- **A real layout bug, found live, not in review.** The canvas host used
  `flex: 1` with no `minWidth: 0`; a flex item's default min-width is its
  content size, which floored the host at its pre-inspector width and pushed
  the 240px inspector card off the right edge of the viewport instead of
  sharing the row. Fixed by adding `minWidth: 0` to the host and
  `flexShrink: 0` to the card — verified by re-rendering the same real table
  and confirming the panel now renders inside the viewport with the canvas
  correctly narrowed beside it.
- Opening or closing the inspector, and toggling full screen, both change how
  much width Cytoscape's own canvas actually has — both now call
  `instance.resize()` (full screen also re-`.fit()`s), or the canvas keeps
  drawing at its stale size until an unrelated re-layout happens to fix it by
  accident.
- `autoungrabify: true` removed (see Slice A's retrospective) — dragging
  verified live by moving the seed node on a real table's neighbourhood and
  confirming every edge followed it.
- Full screen via the native Fullscreen API on the canvas's own wrapper
  element, not the whole page — verified live: entering full screen expanded
  the canvas to the viewport with the legend and exit control still visible;
  exiting restored the original layout.

All copy externalized to a `COPY` object per this repo's
`local/no-raw-jsx-text` lint rule, matching `AgentChat.tsx`'s own convention.

### Slice C — Collapse: undo one expansion (planned, not started)

- **The hard part, stated rather than hand-waved.** `GraphModel.expand()`
  currently merges an expansion's nodes/edges into one flat set with no record
  of *which* expansion contributed *which* node — `mergeNodes`/`mergeEdges`
  dedupe by id and silently drop that attribution. Collapsing a node naively
  ("remove everything it added") is wrong the moment two expanded nodes share
  a neighbour: collapsing the first must not remove a node the second
  expansion still needs.
- Needs `GraphModel` to keep, per expanded node, the id set it actually
  contributed (e.g. `Record<string, {nodes: string[]; edges: string[]}>`
  alongside `expanded`), so `collapse(model, nodeId)` can remove only what
  becomes unreachable from the seed once that expansion's contribution is
  subtracted — a real reachability recompute, not a filter.
- Must interact correctly with `replay()` (time-travel/diff): a collapsed node
  has to re-expand identically if the reader expands it again, and collapsing
  must never be confused with the diff `removed` state Slice B already had to
  keep distinct from a hide.
- Not scoped further here; the next planning pass should run this as its own
  RED-GREEN cycle against `model.test.ts`'s existing suite, not bolted onto
  Slice B's simpler filter.

## What Neo4j Browser's pattern actually informed, and what it didn't

The user asked specifically what Neo4j does well: in-circle labels, crisp
edges, a right-hand properties panel. Three of those were applied directly —
the inspector (Slice E) is that panel; the horizontal edge label (Slice A's
retrospective) is the same legibility call Neo4j makes for the same reason,
found independently by looking at a real render before this was raised. **In-
circle labels were not attempted.** Cytoscape draws a label as a separate
canvas `fillText` call outside the node shape by default; making it read
*inside* an 18px circle means either a much larger node (a real layout
change, not a style tweak) or an icon-only glyph with the name moved
elsewhere — a genuine redesign, not a slice, and not attempted here under
this session's time budget. yFiles and NetworkX were not separately
researched: yFiles is a commercial product this project has no licence
relationship with, and NetworkX is a graph-algorithms library with no
rendering opinion of its own to borrow from.

## Explicitly not done

- **No new graph rendering library.** G6 was considered and rejected against
  `00f`'s existing "exactly two renderers" decision — recorded here rather
  than silently dropped, per this project's own rule that a rejection nobody
  writes down gets re-proposed every few months.
- **In-circle node labels** (Neo4j's own visual pattern) — a real node-shape
  redesign, not attempted; see above.
- **Ontology-driven presentation metadata** (pack-supplied icon/visualGroup
  per semantic type) is a real, separate idea from the session that produced
  this plan, but a much larger one — it touches the pack loader and ontology
  storage on the Rust side, not just the console. Not scoped here.
- **Bundle budget.** The initial bundle was already over its 350KB gzipped
  budget before this plan (694.6KB, per the prior session that shipped the
  invoice-count modal) and stayed over it after (703.4KB) — Slices D/E's antd
  `Card`/icon usage added roughly 9KB to an existing, already-accepted
  violation, not a new one. Not addressed here; a real fix is code-splitting
  the console, which is its own piece of work.
