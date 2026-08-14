# Web Console Architecture

**Crate scope**: `graph-owl-ui` (embeds and serves the build output) · `graph-owl-server` (mounts it) · frontend sources in `ui/`, outside `crates/`

Companion to `00a`–`00e`. Describes the **target** state; sections marked **(built)** already exist. Where the implementation and this document disagree, this document is right and the code has drifted.

## Positioning change

`ROADMAP.md` has said, since it was written: *"Web UI (product) — API and MCP are the product; Epic 36 is a proof."*

That is now a **partial reversal**, and it is a positioning decision rather than a scope decision, so it is recorded rather than absorbed:

- **What stands**: the API and MCP surfaces remain the product. Every capability the console offers must exist in the API first. The console is a *client*, never a privileged one, and it never gets an endpoint of its own.
- **What changed**: a graph engine whose output you cannot see is a very hard thing to evaluate, adopt, or trust. Lineage, inference, constraint violations, and time-travel are all things that are *obvious* in a picture and nearly incommunicable in JSON. The differentiators in `00a-product-position.md` are disproportionately visual, and shipping them API-only means shipping them undemonstrable.

The honest framing: the console exists because **the differentiators need a demo surface**, and because a graph product without a graph view loses evaluations to products with worse graphs and better pictures.

## Scale discipline

A mature catalog console in this category runs to **109 page components, 77 component groups, and 199 npm dependencies (112 runtime + 87 dev)** — measured directly from a production reference implementation, not estimated. It also carries **374 raw colour variables**, three graph libraries, three rich-text editors, and three type families. Those numbers are the warning, not the target.

Three consequences, all binding:

1. **The console covers the differentiators and the daily path; it does not cover the API.** Anything reachable by API but rare in daily use stays API-only. There is no goal of UI parity with the endpoint list, and claiming one is how a 109-page console happens.
2. **One graph renderer per graph *shape*, not per feature.** The same reference implementation ships three graph libraries — a DAG renderer, a general graph library, and a 3-D force renderer. That is what accretion looks like. This project commits to exactly two, for a stated reason (below), and adding a third requires a shape neither handles.
3. **Dependency budget, enforced in CI**, alongside the existing binary and memory budgets in `00a-product-position.md`.

## Stack

| Concern | Choice | Why |
|---|---|---|
| Framework | React + TypeScript strict | Largest hiring pool and component ecosystem for this kind of console |
| Build | Vite | Fast; produces the static bundle `graph-owl-ui` embeds |
| Server state | TanStack Query | Caching, invalidation, and background refresh are most of a catalog console's logic |
| Client state | Zustand | Explorer canvas state (selection, expansion, filters) is genuinely client-side; a small store beats a reducer framework |
| Routing | React Router | Deep-linkable entity URLs are a hard requirement (below) |
| Forms | JSON-Schema-driven | Epic 1 already emits JSON Schema. Generating connector and custom-property forms from it, rather than hand-writing them, is the single biggest UI-code saving available and keeps forms correct as schemas evolve |
| Editor | CodeMirror | SPARQL and Cypher editing with syntax highlighting; lighter than the alternative |
| Component layer | **shadcn/Tailwind v4** (revised — see below) | ~~Ant Design~~ superseded 14 Aug 2026 |
| Lineage / DAG | React Flow (`@xyflow/react`) + **d3-dag** layout | Lineage is a **DAG**. Layered layout is the correct answer and force-directed is actively wrong for it. d3-dag rather than ELK: see the licence note below |
| Ontology authoring / editing | React Flow (`@xyflow/react`) + **d3-hierarchy** (tree/radial) + **d3-force** (force) layouts | Revised 14 Aug 2026 — see below. Same renderer as lineage, different layout modules for the same reason d3-dag was picked over ELK: MIT, small, and this project already trusts the d3 layout ecosystem |
| Graph exploration | **AntV G6** (revised — see below) | ~~Cytoscape.js~~ superseded 14 Aug 2026 |
| Testing | Vitest browser mode + Testing Library, Playwright for journeys | Per the global guidelines |
| i18n | Externalized strings from the first commit | Retrofitting i18n is a rewrite of every component; the cost now is a lint rule |

**Two renderers, deliberately — the rule survives the 14 Aug 2026 revision below unchanged.** Lineage and ontology authoring are both DAG/tree-shaped, modest node count, layout-driven — one renderer (React Flow) covers both, paired with whichever layout module fits the shape (d3-dag for a strict DAG, d3-hierarchy/d3-force for the ontology builder's radial/tree/force modes). Exploration is an arbitrary cyclic graph at large scale, where large-graph rendering and built-in layouts are the point. One library doing both does neither well. This is the boundary; a third renderer needs a third graph shape.

### Revision, 28 Jul 2026: Cytoscape replaces Sigma.js, d3-dag replaces ELK

Both rows above changed. The **rule** did not: still exactly two renderers, still one per shape.

**Exploration: Cytoscape.js, not Sigma.js.** Sigma was chosen because it was the WebGL option and Cytoscape was Canvas-only. That distinction expired — Cytoscape ships a WebGL renderer from v3.31, so the one property that decided the original choice is now common to both. What is *not* common: Cytoscape has deterministic layouts built in (`breadthfirst`), and Sigma has no layouts at all — with Sigma the layout is code this project writes, owns, and tests. Since the testing rule below requires a deterministic layout, choosing Sigma means paying for that requirement in hand-written code in order to use a library whose advantage no longer exists. Both are MIT.

**Lineage layout: d3-dag, not ELK.** `elkjs` is **EPL-2.0**, which is copyleft, and `00i-licensing.md` rejects copyleft dependencies by default. EPL-2.0 is *file-level* weak copyleft, so an unmodified `elkjs` used as a dependency would not force this project's own code open — it is a policy conflict, not a legal blocker, and adopting it would need a deliberate documented exception rather than a silent one. That exception is unnecessary: **d3-dag** is MIT, TypeScript-first, does layered Sugiyama layout, and is a fraction of `elkjs`'s size (`elkjs` is transpiled Java and carries that weight into a bundle this project embeds in its binary).

ELK's genuine advantage over the simpler layered layouts is edge *routing* at fork points. That advantage is mostly unrealised in the standard React Flow integration, which maps ELK's **node positions** back and lets React Flow draw its own bezier edges — consuming ELK's bend points needs explicit port mapping and custom edge components on top. So the routing argument costs extra work before it pays anything, and it is not the reason to take a copyleft dependency into an embedded binary.

**Rejected: a Cytoscape + Sigma hybrid switching at a node-count threshold.** Two libraries for *one* shape is the accretion consequence 2 exists to prevent, and a mid-session renderer swap discards the layout at the exact moment the user most needs it — their mental map of where things are is the main thing keeping a large graph legible. It also fails the "every magic number needs a stated reason" rule (`00i`): the proposed 10,000-node switch point is the *interactivity budget* below, not a measured Cytoscape limit, and a budget silently reused as a capability measurement is how a budget stops meaning anything. If Cytoscape's WebGL renderer misses the 10k budget on a CI fixture, that measurement is the trigger to revisit — not the assumption that it will.

### Revision, 14 Aug 2026: component layer to shadcn/Tailwind, exploration to AntV G6, ontology builder to React Flow

**Recorded per this document's own protocol for reversing a dated decision — name what changed, why it was decided that way originally, and why it is changing now.** Plan `117-console-visual-refresh.md` split a pasted external stack-migration proposal into scored slices specifically so a decision like this would not happen by silent drift; the explicit user decision on 2026-08-14 was to treat it as a deliberate architecture revision rather than filter it down to what already fit. This entry is that revision.

**What is different from every earlier revision in this file: this one is not backed by a fresh measured spike.** The 28 Jul 2026 entry above earned its two swaps (Cytoscape-over-Sigma, d3-dag-over-ELK) with a stated comparison. `00l-build-vs-adopt.md`'s spike discipline — same corpus, same assertions, numbers not vibes — is the project's own standing bar for exactly this kind of call, and Plan 117 sequenced a Cytoscape-vs-Sigma-vs-G6 bake-off (its Slice E) to happen *after* the cheaper styling pass (Slice A) shipped and was judged, specifically to avoid re-litigating a rendering choice by preference. That sequencing was offered to the user explicitly during this session and declined in favour of committing now. The decision below is real and binding, but it is a **product-direction override, not a measured finding** — the honest label for it, not a spike result dressed as one.

**Graph exploration: AntV G6 replaces Cytoscape.js.** MIT. Built-in layouts (including deterministic tree/radial/dagre modes) and built-in WebGL rendering cover in one dependency what Cytoscape plus this project's own layout config covered before — closer to a like-for-like swap than Sigma ever was, which is part of why the 28 Jul 2026 entry preferred Cytoscape over Sigma in the first place (Sigma's missing layouts, not Cytoscape's WebGL, was the deciding property). The testing rule two sections down (assert the model, not the picture) is unchanged and binding on the G6 port — `graph/cytoscape.ts`'s pure functions need G6-shaped equivalents with equivalent coverage carried forward, not dropped in the rewrite.

**Ontology authoring: React Flow replaces the inline Cytoscape instance in the Ontology Builder, and `features/ontology/OntologyEditor.tsx` (the separate text-first editor) merges into it as a Code tab rather than staying a second surface.** This also resolves the open conflict this file has carried since the Ontology Builder shipped: the "Explicitly not in the console" table below said "Ontology/shape authoring GUI... a GUI competes with the CLI and loses to review workflow," while the Ontology Builder existed anyway. That row is retired below, not silently — the GUI is a kept, deliberate exception, and the merge folds the CLI/review-workflow-friendly path (raw Turtle/N-Triples/JSON-LD text, Check/Save against the real `/ontology-editor/*` API) into the same surface as a first-class tab rather than a separate, competing screen.

**Component layer: shadcn/Tailwind v4 replaces Ant Design, console-wide.** This directly reverses the 250KB→350KB budget revision below, which justified Ant Design specifically because "building the same table density, form controls, tree, and panel treatment by hand would cost months and land somewhere worse." shadcn/ui does not resolve that trade the way a second component *library* would — it is a source-copy convention over Radix UI primitives (MIT) plus Tailwind utility classes, so "building it by hand" is closer to true here than it was for the 28 Jul options, just with accessible, unstyled primitives underneath rather than truly from scratch. What that means for the bundle-budget numbers below is **not yet measured** — it depends entirely on which Radix primitives end up used and how much of Tailwind's generated CSS survives its own tree-shaking, and this document does not assert a number it has not checked. The 350KB budget stands until a real build is measured against it; if it moves, that is a further, separately dated revision with the same honesty this one is written with, not a preemptive claim now.

**Not adopted: ELK.js for ontology-specific layout, despite being floated as a later step.** `elkjs` is EPL-2.0, and the 28 Jul 2026 entry above already rejected it on exactly that basis — copyleft, rejected by default per `00i-licensing.md`, needing a deliberate documented exception to use at all. Nothing about this revision changes that policy. d3-hierarchy and d3-force (both ISC, both already the same trusted-ecosystem call the 28 Jul entry made for d3-dag) cover the ontology builder's radial/tree/force layout needs without reopening the licence question. If a genuine ontology-layout requirement later needs something ELK-specific, that is its own licensing exception to argue for on its own merits — not something to fold into this revision's already-large scope.

## Non-negotiables

1. **No private endpoints.** Every console call uses the public, documented, versioned API. If a screen needs an endpoint, that endpoint is designed per `00d-api-conventions.md` and available to everyone. A UI-only backend is how an API rots.
2. **Deep-linkable everything.** Every entity, query result, explorer view, and point in time has a URL that restores it. A graph view you cannot paste into a ticket is a graph view nobody shares — and sharing is the point of a catalog.
3. **Authorization is the server's, always.** The console hides what the API says the principal cannot see. It never decides. A client-side permission check is a UI convenience over a server decision, never a substitute for one.
4. **Derived facts are visibly derived.** Anything from the Epic 6 reasoning overlay is visually distinct from an asserted fact, everywhere it appears, with its derivation available. `00b-architecture.md` says the overlay is never persisted; the console must never let a user mistake inference for assertion.
5. **Confidence is always shown where it exists.** `00c-domain-model.md`'s bands (≥0.8 assert, 0.5–0.8 surface, <0.5 ignore) map to consistent visual treatment. An 0.55-confidence lineage edge drawn identically to a 1.0 one is a lie by omission.
6. **Empty and partial states are designed, not defaults.** A new deployment is empty; a restricted principal sees a partial graph; a truncated traversal returns partial results. All three are the *normal* first experience and each needs a real design.
7. **Accessibility is a gate, not a pass.** Keyboard navigation and screen-reader labels on every interactive surface. The graph canvas gets a **non-visual equivalent** — a navigable list/tree of the same data — because a WebGL canvas is otherwise entirely unusable, and "it's a graph" is not an exemption.

## Delivery: embedded in the binary

The built SPA is embedded into `graph-owl-ui` and served by `graph-owl-server`. One binary, one process, no separate web server, no CDN, no reverse-proxy configuration.

This follows directly from `00a-product-position.md`'s operational-simplicity budget — Postgres as the only required service. It also means the console version can never drift from the API version, which removes a whole class of support problem.

Two constraints it creates, both accepted:

- **Binary size.** The console must not blow the 50MB budget. Compressed assets, code-split routes, and the explorer renderer loaded lazily. The budget is CI-enforced.
- **Feature-gated.** `--no-default-features` produces a headless server with no assets compiled in, for deployments that only want the API and MCP surfaces.

## Auth

The console is an OIDC public client (PKCE), using Epic 12's provider configuration. Tokens are held in memory, refreshed silently, and never written to `localStorage` — an XSS in a catalog console with a persisted token is a credential-theft path into everything the catalog can read.

## Structure

```
ui/
  src/
    api/          generated from the OpenAPI document — never hand-written
    routes/       one directory per route, colocated with its tests
    features/     discovery, explorer, lineage, workbench, governance, memory
    components/   shared primitives only; a component used once lives with its route
    graph/        the two renderers, behind one internal interface
    auth/         OIDC/PKCE
    i18n/
  e2e/            Playwright journeys
```

**The API client is generated from the OpenAPI document** (Epic 1), not written by hand. A hand-written client drifts from the contract silently and turns every API change into a manual audit.

## Testing

Per the global guidelines: behaviour, not implementation; tests first.

| Layer | Tool | Covers |
|---|---|---|
| Component behaviour | Vitest browser mode + Testing Library | What a user can do on a surface |
| Graph rendering | Deterministic layout + assertions on the **model**, not pixels | Layout correctness without screenshot brittleness |
| Journeys | Playwright | Search → asset → lineage → impact; write a memory; run a query |
| Accessibility | axe in CI, plus keyboard-only journeys | Non-negotiable 7 |
| Contract | Generated client compiled against the live OpenAPI document | Catches drift at build time |

**Graph tests assert the model, not the picture.** With a seeded deterministic layout, assert that node X is upstream of node Y, that a derived edge carries the derived treatment, and that a low-confidence edge is marked — never that a pixel is at a coordinate. Screenshot tests on a graph canvas fail on every renderer upgrade and catch almost nothing.

## Budgets (CI-enforced, alongside `00a`)

| Budget | Limit |
|---|---|
| Initial JS bundle (gzipped) | **350KB** (revised — see below) |
| Route chunk (gzipped) | 100KB |
| Total embedded assets | 8MB |
| Routes (page components) | 30 |
| Runtime dependencies | 40 |
| Time to interactive, mid-tier laptop | 2s |
| Explorer interactive at | 10,000 nodes |
| Lineage interactive at | 1,000 nodes |
| axe violations | 0 |

The dependency budget is the one that matters most. 199 is where you land without a number; 40 is a number.

### Budget revision: initial JS 250KB → 350KB

**Recorded rather than silently raised**, per `00a-product-position.md`'s rule that a budget is revised deliberately with the reason.

The console adopts **Ant Design** for its component layer. That decision came from a product requirement — the console should read as familiar to anyone evaluating it against the incumbent in this category, and that category's look *is* substantially Ant Design's look. Building the same table density, form controls, tree, and panel treatment by hand would cost months and land somewhere worse.

The measured cost is **~330KB gzipped** for the component set this console needs (Layout, Table, Tree, Form, Card, Descriptions, Tag, Breadcrumb, Statistic). Deep-importing icons was tried and moved nothing — the weight is antd's core, not its icon set.

So the trade is stated plainly:

| | |
|---|---|
| **Bought** | The category's visual conventions, for free, under MIT. Accessibility, keyboard handling, and i18n already solved in every control |
| **Paid** | 80KB gzipped over the original budget, and a large dependency whose upgrades we now track |
| **Rejected alternative** | Hand-built components inside 250KB. Cheaper to ship, far more expensive to finish, and it would not look like what the buyer expects |

**What does not move**: the route budget (30), the dependency budget (40), the route-chunk budget (100KB), and zero axe violations. If the initial bundle needs to move again, the honest answer is code-splitting the explorer and workbench routes — Epic 40's renderer is already specified as lazily loaded — not another revision.

**Licensing note**: Ant Design is MIT. The console uses it directly; it does not transcribe any other product's palette, LESS, or component implementations. See `plans/00i-licensing.md`.

**Superseded 14 Aug 2026** by the shadcn/Tailwind revision above. This table's own 330KB Ant Design measurement and the "bought/paid/rejected" framing stay here as the historical record of why Ant Design was chosen in the first place — useful context for judging whether the new component layer is actually cheaper, once it is measured rather than assumed. The 350KB ceiling itself is unchanged and still binding; re-measure against it once the shadcn/Tailwind migration lands, and record a further dated revision here if it moves, the same way this one was.

### Bundle measurement, 15 Aug 2026: 747.1KB gzipped against the 350KB ceiling

**The migration above compiles, tests green (950/950), lints clean, and now has a real number against the ceiling the 14 Aug entry deliberately left unmeasured.** `npm run check:budgets` on the completed shadcn/Tailwind + AntV G6 + React Flow console:

| Metric | Measured | Budget |
|---|---|---|
| Initial JS bundle (gzipped) | **747.1KB** | 350KB — **2.1x over** |
| Route chunk (`index.esm-*.js`, gzipped) | 83.2KB | 100KB — within budget |
| Runtime dependencies | 29 | 40 — within budget |

**This is a real finding, not a preemptive claim** — the 14 Aug entry's own condition for writing this section. `antd` itself was confirmed unused (zero imports anywhere in `src/`, verified by grep) and removed from `package.json` during this measurement; the bundle number did not move, confirming it was already dead weight rather than the cause. The actual weight is the two large runtime libraries the 14 Aug revision adopted for their own stated reasons — AntV G6 (WebGL graph exploration) and React Flow plus its layout modules (lineage/ontology authoring) — both genuinely needed for the capabilities they cover, loaded eagerly in one initial chunk rather than split by route.

**Not fixed here, per this document's own rule** (`00i-licensing.md` rule 4 and this file's revision protocol): a budget miss is a trigger to record and hand off, not to patch inside the same session that found it. The route-budget section above already names the correct lever — "code-splitting the explorer and workbench routes... not another revision" — and Epic 40's renderer was already specified as lazily loaded before this measurement existed. That work (route-level `React.lazy` for the graph explorer, lineage, and ontology builder surfaces, so G6 and React Flow load only when their route is visited) is unscoped and belongs in its own plan slice, not folded into finishing this migration's compile/test/lint pass.

## Epics

| Epic | Scope | Plan |
|---|---|---|
| 39 | Shell, auth, search & discovery, entity pages | `39-ui-foundation.md` |
| 40 ★ | Graph explorer, lineage, time-travel, impact analysis | `40-ui-graph-explorer.md` |
| 41 | Query workbench, validation, governance, memory, admin | `41-ui-workbench-governance.md` |
| 42 | Vocabulary browsers, review queues, agent activity, interchange | `42-ui-semantic-surfaces.md` |

**`00h-ui-design-system.md` is this document's companion**: tokens, chrome, the five reusable patterns, and a screen inventory mapping **every** engine epic to a surface or to an explicit "no UI". That inventory is what produced Epic 42 — fifteen surfaces Epics 39–41 did not cover.

## Explicitly not in the console

| Not doing | Why |
|---|---|
| Dashboard builder / customizable home | The reference implementation's largest surface by page count and the least defensible |
| In-console ETL or pipeline authoring | Crosses into the data plane (`00a-product-position.md`) |
| Notebook / BI features | Bolt (Epic 7d) makes the real tools work; reimplementing them is a losing trade |
| A second admin console | Admin is a section, not an application |
| Mobile-native apps | Responsive web; a catalog is a desk activity |
| Rich WYSIWYG editing | Markdown with preview. A WYSIWYG editor is a multi-year commitment in disguise |

**Resolved exception, 14 Aug 2026 (Plan 117 Slice G): "Ontology/shape authoring GUI" is retired from this table, not silently — it is a kept, deliberate exception, not an oversight.** `ui/src/features/ontology-builder/` (a visual, canvas-based ontology editor) was built and actively extended before this row was ever checked against it, which is the drift Plan 117 flagged rather than let compound. The original reasoning — "metadata-as-code (Epic 20) is the intended path; a GUI competes with the CLI and loses to review workflow" — is answered by the same-day merge in the revision above: the CLI-friendly text/review path (`features/ontology/OntologyEditor.tsx`, Check/Save against the real `/ontology-editor/*` API) is now a tab *inside* the Ontology Builder rather than a competing screen, so the GUI and the metadata-as-code path are one surface instead of two that could drift from each other.