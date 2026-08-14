# Plan 117 — Console visual refresh: story split

**Status**: split only, nothing scoped for implementation yet. **Branch**: main.

**Trigger**: a pasted external stack-migration proposal ("rebuild GraphOWL's
frontend around Tailwind v4, shadcn/ui, Base UI, Sigma.js/AntV G6 replacing
Cytoscape, Monaco, a redesigned app shell, an AI command surface...") arrived
in the same session as unrelated Ontology Builder fixes. Split out here
because it is epic-scale and solution-shaped, not because the underlying
complaint — "the console feels like a developer/admin tool, not a modern
semantic workspace" — is wrong. It is real and `00f-ui-architecture.md`
already says so in its own words: *"a graph engine whose output you cannot
see is a very hard thing to evaluate, adopt, or trust... the console exists
because the differentiators need a demo surface."*

**Explicit user decision (2026-08-14): treat this as a deliberate
architecture revision, not filter it down to what already fits.** That
changes what this document has to do versus a normal split — every child
story that reverses a recorded `00f`/`00h` decision names the decision, the
reason it was made, and what a revision entry would need to say. Nothing
below is a silent drift; `00f`'s own "Revision, 28 Jul 2026" section is the
precedent for how a reversal gets recorded here.

## What's already true — checked live before splitting, not assumed

`00f-ui-architecture.md` and `00h-ui-design-system.md` are not blank slate.
Several things the pasted proposal recommends as new are already decided,
some the same way, some the opposite way, for stated reasons:

| Proposal says | `00f` already says | Reason on record |
|---|---|---|
| Tailwind v4 + shadcn/ui + Base UI | **Ant Design**, deliberately | MIT; "the console should read as familiar to anyone evaluating it against the incumbent... that category's look *is* substantially Ant Design's look"; bundle budget raised 250KB→350KB specifically to afford it |
| Sigma.js or AntV G6 for the graph | **Cytoscape.js**, exactly one renderer per shape | Sigma was evaluated and rejected 28 Jul 2026 — "the one property that decided the original choice [WebGL] is now common to both... Cytoscape has deterministic layouts built in... Sigma has no layouts at all." AntV G6 was never evaluated but would be a **second renderer for the same shape** Cytoscape already covers, which consequence 2 of the scale-discipline section forbids without "a shape neither handles" |
| React Flow for node editors | **Already adopted**, for lineage (a DAG) | Confirms the proposal's instinct, not a gap |
| Zustand for client state | **Already adopted** | Confirms the proposal's instinct, not a gap |
| Monaco for code editing | **CodeMirror**, deliberately | "lighter than the alternative" — stated reason, not an oversight |
| — | **40 runtime deps, 350KB gzipped initial bundle, CI-enforced** | "The dependency budget is the one that matters most. 199 is where you land without a number; 40 is a number." |

**A live finding from today's own work, not theory**: the Ontology Builder's
node canvas got a full "light colour + category icon" visual pass this
session (Plan-tracked separately) — pastel fills, coloured ring borders, a
distinct glyph per entity category — using **zero new dependencies**, on top
of the existing Cytoscape + Ant Design stack. That is direct evidence the
"feels dated" complaint and "swap the whole stack" solution are not the same
size problem. Some slices below exploit that; others are the parts that
genuinely cannot be reached without a stack change.

## Open question this plan does not resolve unilaterally

**`00f`'s "Explicitly not in the console" table lists "Ontology/shape
authoring GUI"** — reason given: "Metadata-as-code (Epic 20) is the intended
path; a GUI competes with the CLI and loses to review workflow." **Yet
`ui/src/features/ontology-builder/` (a visual, Cytoscape-based ontology
builder) already exists and was actively extended this same session.**

Either the doc has drifted and needs updating to record the ontology builder
as a deliberate exception (with a reason, the same way every other reversal
in this file is required to have one), or the feature itself is out of scope
and continuing to invest in it compounds a documentation debt. **This is a
product decision, not a splitting decision** — flagged in the Parking Lot,
not resolved here.

## Parent

A person evaluating or using graph-owl's console experiences it as a modern,
trustworthy semantic workspace — distinctive enough to not lose evaluations
"to products with worse graphs and better pictures" (`00f`'s own framing) —
without breaking the console's CI-enforced budgets or silently reversing a
dated decision.

- **Actor**: an evaluator comparing graph-owl to an incumbent catalog product,
  and a daily user (CA/data engineer/architect) working the graph explorer,
  ontology builder, and workbench.
- **Need**: visual and interaction quality that reads as considered rather
  than default-admin-panel, on the surfaces that carry the product's actual
  differentiators (graph, lineage, reasoning, confidence).
- **Outcome**: stronger first impression in evaluation, less friction in
  daily use, no regression to the budgets or decisions `00f`/`00h` record.
- **Current constraint**: the proposal as pasted is a library shopping list,
  not a capability list — several items reverse specific, dated, reasoned
  decisions with no new evidence offered for the reversal, and several others
  are already done.

## Recommended first slice

**A user browsing any graph surface (Ontology Builder or the Explorer) sees
entity nodes as light-tinted, colour-and-icon-coded circles with legible
labels — not flat single-colour dots — using the existing stack.**

Why this first: it is the single highest-value, lowest-risk piece of the
whole proposal. It directly answers "the UI feels simple/outdated," it is
**already proven working today** in the Ontology Builder (this session,
zero new dependencies), it needs no `00f` revision, and it de-risks nothing
downstream by being first — but its existence is itself evidence that
changes how expensive the later slices need to be judged.

## Split candidates

| Slice | Value | Includes | Defers | Acceptance examples | Release constraint | `00f`/`00h` revision needed? |
|---|---|---|---|---|---|---|
| **A — Node/edge visual language on the existing stack** | Immediate "looks modern" improvement everywhere Cytoscape or React Flow render a node, at zero dependency cost | Light-tint fills, coloured ring borders, category-derived icon glyphs (the pattern already shipped in Ontology Builder this session), consistent typography/spacing tokens applied to node labels and edge labels across Explorer + Lineage + Ontology Builder | A full design-token system; new component primitives; anything touching Ant Design's own component set | A user viewing the graph explorer sees the same visual language as the ontology builder; before/after screenshot shows tinted nodes with icons instead of flat circles; axe violations stay at 0 | Shippable immediately, additive only | **No.** Same stack, same budgets. |
| **B — Motion/polish pass** | Panel open/close, selection, and loading states feel "alive" instead of instant/jarring | Adopt **Motion** (MIT) narrowly: inspector panel slide-in, node-selection highlight transition, a loading/investigation state sequence for one surface (pick the pack picker or the reconciliation queue as the pilot) | App-wide animation audit; replacing Ant Design's own built-in transitions | A user opening the entity inspector sees it slide in rather than snap; reduced-motion media query is respected; bundle delta is measured and stays inside budget | Shippable behind normal review, one surface at a time | **Yes, additive-only entry.** One new runtime dependency (Motion) — record it in `00f`'s dependency table with the same "bought/paid/rejected alternative" framing the Ant Design budget revision uses. Does not touch the renderer or component-library rules. |
| **C — Data-grid capability for GST domain surfaces** | Sortable, filterable, dense tables for Invoices/Suppliers/Reconciliation rows — the "daily path" `00f` explicitly prioritises over API-parity surfaces | Adopt **TanStack Table** (MIT) for one real surface first (Reconciliation queue is the highest-traffic candidate), headless — styled with existing Ant Design tokens, not a new table look | Replacing every `Table`/`List` usage in the console at once | A user sorts/filters the reconciliation queue by column without a page reload; keyboard navigation works; existing antd `Table` usages elsewhere are untouched by this slice | Shippable independently of every other slice here | **Yes, additive-only entry.** One new dependency; no rule reversal — `00f` does not currently name a table library, so this fills a gap rather than reversing a choice. |
| **D — Command palette for cross-console search and navigation** | `⌘K` search-and-jump across assets, queries, and admin actions — a genuine capability gap today, not a re-skin | Adopt **cmdk** (MIT) for a single palette: entity search (reuses the existing search endpoint), route navigation, and a small fixed action list | Any "AI investigation" framing — `ROADMAP.md`'s "Not on this roadmap" table refuses a hosted agent runtime and agent orchestration; a command palette that *suggests natural-language investigation* crosses into that refusal and needs its own product decision, not this slice | `⌘K` opens the palette from anywhere in the console; typing an entity name and pressing Enter navigates to it; Escape closes without side effects | Shippable independently | **Yes, additive-only entry**, same as B/C — plus a scope note that the palette's action list must not imply an agent runtime the roadmap already refused. |
| **E — Exploration-renderer bake-off (spike, not a commitment)** | An honest, measured answer to "should Cytoscape be replaced," instead of a re-litigation by preference | Per `00l-build-vs-adopt.md`'s existing spike discipline: same corpus (a real graph-owl export, not a toy), same assertions, run against Cytoscape (current), Sigma.js, and AntV G6 — render time at 1k/10k nodes, bundle cost, WebGL support, deterministic-layout support (the exact property that decided the 28 Jul 2026 Cytoscape-over-Sigma call) | Any actual renderer swap — this slice produces a decision, not a migration | A written comparison table with numbers, checked into `spikes/`, answering the same four questions the 28 Jul 2026 revision answered for Sigma; a recommendation with a stated reason, whichever way it goes | Not releasable — spike artifact only, excluded from the workspace/build the same way `spikes/` already is for the Rust side | **Conditional.** If the spike reaffirms Cytoscape, the finding gets **added** to the existing 28 Jul 2026 revision section as confirmation. If it recommends a change, that is a **new dated revision entry**, written the same way, before any implementation slice is scoped. |
| **F — Component-library foundation swap (Ant Design → Tailwind v4 + shadcn/ui + Base UI)** | Full visual-language independence from Ant Design's own look, if A/B/C/D turn out not to be enough | Nothing scoped yet — this is the biggest, riskiest item in the whole proposal and touches every existing component in `ui/src/` | Everything, until a decision is made on the trigger below | N/A — this is a decision slice, not a build slice | Not releasable | **Yes, full revision required**, and it should not be scoped until **A–D ship and are judged against the original complaint.** The bundle-budget section's own logic — Ant Design was paid for *specifically* because hand-building the same density/accessibility/i18n "by hand would cost months and land somewhere worse" — applies unchanged to a shadcn/Base UI rebuild; the trigger for revisiting it is evidence that A–D genuinely cannot reach the desired look, not that a newer library exists. |
| **G — Ontology-authoring GUI legitimacy** | Resolves the standing conflict between `00f`'s refusal and the shipped `ontology-builder` feature before more is invested in it | A decision, recorded in `00f`, either way: (1) the console keeps the feature and `00f`'s "not in the console" table gets an exception entry with a reason, or (2) the feature is descoped back toward the metadata-as-code/CLI path `00f` already commits to | Any new ontology-builder capability until the decision is recorded | `00f`'s "Explicitly not in the console" table either drops the ontology-GUI row with a dated reason, or the row gets an explicit carve-out naming `ontology-builder` | N/A — documentation decision | **Yes — this slice's entire output is a `00f` edit.** |
| **H — Explorer workspace features (focus mode, minimap, multi-perspective views)** | The proposal's "focus mode," "minimap," and "same graph, different perspective" ideas are real UX improvements | Scoped as **extensions of Epic 40** (`40-ui-graph-explorer.md`, graph explorer/lineage/time-travel — already the epic that owns this canvas), not a new epic | Any renderer change — this slice is renderer-agnostic and should be written against whatever E concludes | A user selecting a node dims unrelated nodes and highlights its neighbourhood to N hops; a minimap shows current viewport within the full graph; both pass the existing "assert the model, not the picture" graph-testing rule | Shippable per normal Epic 40 slice cadence | **No new revision** — fits inside Epic 40's existing scope and non-negotiables (accessibility, deep-linking) unchanged. |

## Sequencing

1. **A** first — proven, free, ships this week's worth of visible improvement.
2. **B, C, D** in parallel or any order — each is one bounded dependency
   addition with its own additive `00f` entry; none blocks another.
3. **G** should happen early too — it is cheap (a documentation decision) and
   everything built on `ontology-builder` afterward inherits its answer.
4. **E** (the bake-off) only after A ships and is judged — the visual pass in
   A may itself answer part of "does Cytoscape look dated," separating the
   *rendering engine* question from the *node styling* question that A/B/C/D
   were conflating in the original proposal.
5. **F** is gated on E and on A–D's outcome, not scheduled. It is the most
   expensive item here and the proposal's own stated reason for it — "the
   current UI looks outdated" — is the same reason A already addresses at a
   fraction of the cost.
6. **H** follows E, since its implementation shape depends on which renderer
   wins the bake-off.

## Parking lot

- The proposal's "AI-native command surface" (§7 of the original pasted
  text — reasoning steps, evidence, tool calls, findings as a chat-like
  interaction) is materially different from **D**'s plain command palette
  and reads close to the hosted-agent-runtime refusal in `ROADMAP.md`. Needs
  its own `grill-me` session against that refusal before any slice is
  written, not bundled into this split.
- "Workspaces" (multiple saved panel arrangements, per the original
  proposal's §18) is a persistence + multi-user feature, not a visual one —
  out of this plan's scope entirely; would need its own story split against
  `35-collaboration.md`.
- Dark/light theme: `00h-ui-design-system.md` should be checked for whether
  this is already covered before scoping it as new work here.
- Icon library: **A** above uses hand-authored inline SVG glyphs (as shipped
  in Ontology Builder this session) rather than adding Lucide as a
  dependency. Worth a deliberate decision — hand-authored avoids a
  dependency for a small, fixed glyph set; Lucide (ISC, permissive) buys a
  much larger vocabulary if the console ends up needing more than ~10
  categories. Not urgent; revisit if **A**'s glyph set outgrows hand
  authoring.

## Warnings

- **F is the component-split anti-pattern if scoped before it earns its
  keep.** "Swap the component library" delivers no independently observable
  user value until paired with an actual visual outcome — exactly the
  "build the backend, build the frontend" trap this skill's own guidance
  warns against. Do not let F get scheduled as "obviously next" just because
  it is the proposal's headline ask.
- **E must reuse the existing spike discipline (`00l-build-vs-adopt.md`,
  `spikes/`, excluded from the workspace) rather than a fresh ad hoc
  comparison** — the project has already been burned once by a library that
  looked ideal on reputation and failed on a real corpus (`cypher-parser`,
  recorded in `CLAUDE.md`). The same discipline applies to a graph-rendering
  library chosen from a consultant report rather than a measured spike.
- **Every new dependency introduced by B/C/D must pass the licence check in
  `00i-licensing.md` before adoption** — permissive only, checked before
  reading the library, and the UI-side `deny.toml`-equivalent CI check
  `00i` already requires for `ui/package-lock.json`.

## Next step

Load `planning` for **Slice A** specifically (it is ready — proven, scoped,
no open decisions) to turn it into PR-sized implementation slices with full
TDD execution. **G** is a documentation-only decision and can be resolved by
conversation before A is even implemented. **E** should be scheduled once A
ships, as its own plan file (a spike plan, not an implementation plan).
