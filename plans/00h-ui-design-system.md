# graph-owl — Console Design System

**Crate scope**: frontend sources in `ui/`; served by `graph-owl-ui`
**Companion to** `00f-ui-architecture.md`, which owns the stack, the budgets, and the non-negotiables. This document owns **what it looks like, what the pieces are called, and which screen exists for which epic.**

## The reference, measured

A mature console in this category was profiled directly rather than described from memory. The numbers matter because they are what the design system exists to avoid:

| Measured | Value | What it means |
|---|---|---|
| Page components | **109** | Five separate "customize" pages; four separate pages for one entity family (detail, schema, version, schema-version) |
| Component groups | **77** | |
| Runtime dependencies | **112** | Plus 87 dev — the 199 figure in `00f` is the sum |
| Colour variables | **374** | `@grey-1` … `@grey-30`, `@green-1` … `@green-600`, with no semantic meaning attached to any of them |
| Graph libraries | **3** | A DAG renderer, a general graph renderer, and a 3-D force renderer |
| Rich-text editors | **3** | Three separate editor stacks in one application |
| Type families | **3** | One UI sans, two monospace |

**None of this was a bad decision.** Each was a reasonable local choice; the aggregate is what accretion looks like after a decade. The design system's job is to make the reasonable local choice also the globally consistent one, so the same decade produces a different aggregate.

The single most instructive number is **374 colour variables with no semantics**. When a token is called `@grey-17`, nobody can know whether it is a border, a disabled label, or a table stripe — so the next person adds `@grey-18`. Semantic naming is not tidiness; it is the mechanism that stops the count growing.

## The theme: familiar, then deliberately not

The visual family is **kept close on purpose**. An evaluator comparing this against the incumbent should feel at home in the first thirty seconds: Inter, a blue-family accent, light neutral surfaces, subtle elevation, an 8-point grid, a left rail with a top bar. Familiarity is a feature — a graph engine already asks a buyer to learn a new data model, and making them learn a new visual language at the same time is a cost with no return.

The twist is that **three things this product knows and the incumbent does not become part of the visual language rather than badges bolted onto it.**

| Twist | Why a knowledge graph needs it |
|---|---|
| **Epistemic status is a visual primitive** | Every fact is asserted, inferred, or uncertain. A catalog only stores what someone typed; this engine *derives* facts (Epic 6) and *scores* them (`00c`). If inference renders identically to assertion, the product's most valuable output is also its most dangerous |
| **Time is part of the chrome, not a control on one screen** | `op = false` is a retraction, not a delete (`00b`). The console can render any past state — so "which moment am I looking at" is a persistent property of the session, like which account you are signed into |
| **The graph is the substrate, not a tab** | A catalog is a document collection with a lineage feature. This is a graph with document-shaped views over it. Every entity surface offers a one-key jump into the graph at that node, from anywhere |

Everything else — spacing, type scale, form controls, table density, empty states — stays conventional. **A twist that touches everything is a rewrite, not a twist.**

**Revision, 14 Aug 2026 (see `00f-ui-architecture.md`'s own dated entry for the full reasoning): the component layer moves from Ant Design to shadcn/Tailwind v4.** The two-tier semantic token system below is unaffected in *shape* — it was never Ant-Design-specific, only expressed through its theme API — but every component in this document's inventory is being rebuilt on Radix UI primitives and Tailwind utility classes, so the concrete implementation of "how a token reaches a component" changes from an antd `ConfigProvider` theme object to Tailwind's `@theme` CSS variables. The **twist table** above, the **five patterns** below, and the **screen inventory** are product/UX decisions independent of which library renders them and are not reopened by this revision — only their implementation is.

## Tokens

Two tiers, and only the second is ever used in a component.

**Tier 1 — primitives.** A ramp per hue, 11 steps, machine-generated. Never referenced directly. A component using `--blue-600` instead of a semantic token is a review-blocking finding, for the same reason `@grey-17` became `@grey-18`.

**Tier 2 — semantic tokens.** ~48 names. This is the whole vocabulary.

```
surface           page background
surface-raised    cards, panels
surface-sunken    table stripes, wells, code blocks
surface-overlay   modals, popovers, drawers

text              body
text-muted        secondary, metadata
text-subtle       placeholder, disabled
text-inverse      on accent

border            default
border-strong     inputs, focused containers
border-subtle     table row dividers

accent            primary action, links, selection      indigo
accent-hover / accent-active / accent-subtle

success / warning / danger / info                       + -subtle for each

--- the twist, and the reason this list is not just a generic system ---

derived           inferred fact, inference edge         violet
derived-subtle    derived-fact background wash

asserted          stated by a human or a source         inherits text/border — the DEFAULT
                                                        (asserted must never need a badge; the
                                                         unusual thing is what gets marked)

confidence-high   ≥ 0.8   assert band                   inherits success family
confidence-mid    0.5–0.8 surface band                  inherits warning family
confidence-low    < 0.5   ignore band                   inherits text-subtle

temporal          historical view chrome                teal
temporal-subtle   as-of banner background

graph-node / graph-node-selected / graph-edge / graph-edge-derived / graph-canvas
```

**Three token decisions worth stating:**

1. **`asserted` inherits the default treatment and has no marker.** Marking both states doubles the visual noise and halves the signal. The default is "a human or a system of record said this"; deviation from that is what earns ink.
2. **Confidence maps onto the semantic families rather than a fourth palette.** A user already reads green/amber/grey as good/caution/ignore; teaching a separate confidence palette spends learning budget on something already learned.
3. **`temporal` is the one token allowed to tint global chrome.** Nothing else may. A tinted whole-app chrome is the strongest signal the system has, and it is spent on exactly one thing: *you are not looking at now.*

**Dark mode is a first-class theme, not an inversion**, and the **explorer canvas is dark in both themes**. A dense node-link diagram on white is glare; every graph tool converges on a dark canvas for a physical reason, not a fashionable one. The canvas is a distinct surface, like a video player in a light page.

### Type and space

| | |
|---|---|
| UI | **Inter** — variable, subset to Latin + the glyphs actually used |
| Mono | **JetBrains Mono** — one monospace, for FQNs, IRIs, SPARQL, Cypher, JSON. Not two, not three |
| Scale | 12 / 14 / 16 / 20 / 24 / 32, 1.5 body line height. Six sizes |
| Space | 4-point base, 8-point rhythm: 4 8 12 16 24 32 48 64 |
| Radius | 6px controls, 10px cards, 9999 pills. Three values |
| Elevation | Four: flat, raised, overlay, popover. **Not** the reference's eleven shadow variables |
| Motion | 120ms controls, 200ms panels, 0ms under `prefers-reduced-motion` |

**FQNs are always monospace.** A fully-qualified name is a structured identifier where character-level differences carry meaning, and proportional type hides exactly those differences.

## Chrome

```
┌────────────────────────────────────────────────────────────────┐
│ ◈ graph-owl   [ search…                    ⌘K ]   ◷ now   ◑ ⚙ ● │  top bar
├──────┬─────────────────────────────────────────────────────────┤
│      │  Sales.Orders                            ⌘G → graph     │
│ ⌕    │  warehouse.public.sales.orders           Table          │
│ ⬡    │  ┌───────────────────────────────────────────────────┐  │
│ ⇄    │  │ ✓ Certified  ·  ◐ 0.82  ·  Finance  ·  @data-plat │  │  trust bar
│ ⌸    │  └───────────────────────────────────────────────────┘  │
│ ⚑    │  Overview │ Lineage │ Relations │ Knowledge │ Quality …  │
│      │                                                          │
│ ⚙    │  ← detail                              ← context rail →  │
└──────┴─────────────────────────────────────────────────────────┘
```

- **`◷ now`** is the time control, in the top bar on **every** screen. Click it, pick a moment, and the entire console re-renders as of that time with teal chrome and a persistent "viewing 12 March 2026" banner that cannot be dismissed without returning to now. This is the differentiator, so it lives where the differentiator can be seen rather than behind a tab on one screen.
- **`⌘G`** jumps to the graph explorer centred on whatever is on screen, from anywhere. The graph is one keystroke from every surface.
- **Six rail items, not twenty**: Discover, Explore, Lineage, Workbench, Governance, Settings. The reference's rail is user-customizable; ours is not — a customizable rail means no two users can help each other, and it is one of the five "customize" pages that got it to 109.
- The **context rail** is the twist on the standard right-hand panel: it shows the current entity's *graph neighbourhood* — nearest nodes, relationship types, one-hop counts — not a property dump. On a graph product, "what is next to this" is the ambient question.

## The five patterns

Roughly twenty epics have a user-facing surface. They resolve into **five reusable patterns**, and that ratio is the design system's actual output. A new capability is expected to arrive as a configuration of one of these, not as a new screen.

| Pattern | Shape | Serves |
|---|---|---|
| **Composable entity page** | Envelope header + trust bar + tab slots | Every entity type (Epics 2, 22, 26, 27, 28, 30, 34) |
| **Graph surface** | Canvas or DAG + inspector + non-visual equivalent | Epics 4, 6, 7a, 7c, 29, 38 |
| **Hierarchical vocabulary browser** | Tree + detail + relations panel | Glossary (24), classifications (25), domains (23), ontology packs (33) |
| **Review queue** | Proposal + evidence + accept/reject/defer + audit | Resolution (17), extraction (21), violations (5), proposals (35) |
| **Schema-driven form** | JSON Schema → form → validate → submit | Connectors (15), custom properties (22), policies (13), contracts (27) |

**The vocabulary browser is the clearest saving.** The reference ships separate Glossary, Classification, and Domain applications; they are one tree-plus-detail pattern over three vocabularies that differ in their relation types and nothing else. Same for the review queue: adjudicating a duplicate-entity merge, an extracted triple, and a proposed description are the same interaction — *a machine or a colleague proposes, a human decides, the decision is recorded with a reason.*

## Screen inventory — every epic, accounted for

The completeness requirement: **every epic either has a named surface or an explicit "no UI" with a reason.** An epic whose UI nobody assigned is an epic that ships without one.

| Epic | Surface | Pattern | Where |
|---|---|---|---|
| 1 API conventions | — | — | Contract only; the generated client is its consumer |
| 2 Hierarchy & columns | Entity page, column table | Entity | 39 |
| 3 Envelope & versioning | History tab, **version diff viewer** | Entity | 39 |
| 4 Triple storage & time travel | **Time control in the chrome**, as-of banner | Chrome | 39 shell, 40 canvas |
| 5 Constraint validation | Violations queue; Quality tab | Review | 41 |
| 6 Reasoning overlay | Derived treatment everywhere; **explanation panel** | Tokens + Entity | 39 E, 40 E, 41 |
| 7 SPARQL | Workbench | — | 41 |
| 7a Traversal | Explorer expansion, path finding | Graph | 40 |
| 7b Cypher | Workbench, second language | — | 41 |
| 7c LPG | **Triple ⇄ property-graph view toggle** on the Knowledge tab | Entity | **42** |
| 7d Bolt | Admin: endpoint status, active sessions | Form | **42** |
| 8 Search | Discovery, facets, `⌘K` | — | 39 |
| 9 RDF I/O | Export dialog (format, scope, preview) | Form | **42** |
| 9a LPG interchange | Same dialog; projection-target admin | Form | **42** |
| 10 Operability | Admin: health, budgets, degraded states | — | 41 |
| 11 People & ownership | Admin principals; ownership-gap report | Form | 41 |
| 12 Authentication | Login, session, token expiry | — | 39 |
| 13 Authorization | Policy editor **with dry-run preview** | Form | 41 |
| 14 MCP + events | **Agent activity: sessions, reads, writes, webhooks** | — | **42** |
| 15 Connectors | Admin: config, test, run history | Form | 41 |
| 16 Ingestion APIs | Admin: tokens, batch jobs | Form | 41 |
| 17 Entity resolution | **Merge adjudication queue** | Review | **42** |
| 18 Inbound events | Admin: webhook registry, deliveries | Form | 41 |
| 19 Streaming | Admin: consumer lag, throughput | — | 41 |
| 20 Metadata-as-code | **Drift view** — declared vs actual | Review | **42** |
| 21 Document ingestion | **Extraction review queue** | Review | **42** |
| 22 Custom properties | Definition admin + entity-page rendering | Form + Entity | 39, 41 |
| 23 Domains & products | Vocabulary browser | Vocabulary | **42** |
| 24 Business semantics | **Glossary browser**, SKOS relations, metrics | Vocabulary | **42** |
| 25 Classification | Tag browser, mutual exclusivity | Vocabulary | **42** |
| 26 Lifecycle & certification | Trust bar; transition control | Entity | 39, 41 |
| 27 Data contracts | Contract tab, compatibility status | Entity | 39 |
| 28 Usage & popularity | Usage tab, trend | Entity | 39 |
| 29 Lineage | Lineage + column lineage + impact | Graph | 40 |
| 30 Quality & incidents | Quality tab, health rollup | Entity | 39, 41 |
| 31 Memory | Memory panel + admin | Entity | 41 |
| 32 Agent capabilities | Agent activity; write-back audit | — | **42** |
| 33 Ontology packs | Vocabulary browser; pack install | Vocabulary + Form | **42** |
| 34 Entity expansion | **Nothing** — the composable page absorbs new types by design | Entity | 39 D |
| 35 Collaboration | Threads, proposals | Review | **42** |
| 36 Reference apps | — | — | External |
| 37 Scale / portability | Admin: export, restore, budgets | Form | 41 |
| 38 Analytics | Governance reports: orphans, silos, blast radius | — | 41 |
| 37a Scale validation | Admin: budget headroom against measured limits | Form | 41 |
| 37b Backup & portability | Admin: export, restore, verify | Form | 41 |
| 37c Embeddable library ★ | **Nothing** — the embedding host supplies its own UI; a library that shipped a console would be the opposite of embeddable | — | — |
| 39 Console foundation | *is* the shell | All | 39 |
| 40 Graph explorer ★ | *is* the graph surface | Graph | 40 |
| 41 Workbench & governance | *is* the workbench | Review + Form | 41 |
| 42 Semantic surfaces | *is* the vocabulary + review set | Vocabulary + Review | 42 |
| 43 Framework integrations | — | — | External, like 36 — the consumer is someone else's agent framework |
| 93 Console overview | *is* the landing surface | — | 93 |
| 94 RDF 1.2 alignment | Export dialog gains `rdf:reifies`; **base-direction rendering is a foundation primitive, not a screen** — see below | Form + Tokens | **42** export, **39** direction |
| 95 OWL RL completion | Extends the explanation panel — more axioms, same surface | Entity + Review | 41 |
| 96 SHACL-SPARQL | Violations queue unchanged; the **constraint editor must accept SPARQL**, which makes it a second editor surface in the workbench | Review + — | 41 |
| 97 Incremental & parallel reasoning | Admin: reasoning job state, **staleness of the overlay** — a derived fact whose age is invisible is a derived fact nobody can weigh | Form | 41 |
| 98 OWL EL reasoning | **Nothing of its own** — a profile is a routing decision, surfaced by Epic 100 | — | via 100 |
| 99 OWL QL reasoning | **Nothing of its own** — as above | — | via 100 |
| 100 Profile detection & routing | **Ontology profile badge + which reasoner will run and why.** New surface: "what can this ontology support" is a question users ask before they trust an answer | Entity + Form | 41 |
| 101 SPARQL federation | Workbench `SERVICE` support, and **remote results attributed in the result grid** — an unattributable remote row is the epic's own named danger | — + Tokens | 41 |
| 102 Read/write partitions | Admin: partition health, replication lag | Form | 41 |
| 103 In-process traversal | **Nothing** — a performance path with no user-visible behaviour change | — | — |

**Epic 34 contributing no UI at all is the design working.** Entity expansion adds five entity families; if that required UI work, the composable entity page would have failed its purpose.

### The 39–103 rows were missing, and two of them were real gaps

The table above stopped at Epic 38 — every epic from the standards-depth and full-semantics phases was unassigned, which is the exact condition the completeness requirement exists to catch. Most resolve to "extends an existing surface", which is the five-pattern design working as intended. **Two do not, and would have shipped without a console:**

- **Base direction (Epic 94 Slice C).** `rdf:dirLangString` carries a base direction, and a store that knows a label is right-to-left while the console renders it left-to-right has made the screen *less* correct than the database. This is a text-rendering primitive — every component that renders a user-supplied label must honour `dir`, which makes it Epic 39's concern and a token-level rule, not a screen. It is also the one place in this design system where a correctness bug is invisible to a reviewer who reads only English.
- **Ontology profile and reasoner routing (Epic 100).** "Which profile is this ontology in, and therefore which reasoner ran and what could it not conclude" is a question asked *before* trusting an answer, not after. Epics 98 and 99 add reasoners with different completeness guarantees; without a surface naming which one ran, the console shows conclusions whose strength the user cannot assess — which is `00f` non-negotiable 4's problem in a new place.

**No new epic.** Three surfaces do not justify one; Epic 42 exists because *fifteen* had no home. These attach to 39, 41 and 42 as slices, and the route budget below is unaffected because none of them is a new route.

**Fifteen surfaces had no home** across Epics 39–41 — the vocabulary browsers, the review queues, agent activity, interchange, and the property-graph view. They are now **Epic 42**, not quietly appended to 41, because an epic that absorbs everything unassigned stops being estimable.

## Route budget

| | Reference | graph-owl |
|---|---|---|
| Page components | 109 | **≤ 30 routes**, CI-asserted |
| Runtime dependencies | 112 | **≤ 40** (`00f`) |
| Graph libraries | 3 | **2**, one per graph shape |
| Rich-text editors | 3 | **1** — Markdown with preview |
| Type families | 3 | **2** — one sans, one mono |
| Colour tokens in components | 374 primitives | **~48 semantic**; primitives unreachable from components |

The route budget is the one that governs the others. Thirty routes across twenty-five entity types and forty-one epics is only reachable through the five patterns — which is the point of having them, and the check that they are being used.

## Accessibility, restated as design constraints

`00f` makes accessibility a gate. Three consequences bind this document specifically:

1. **No status is conveyed by colour alone.** Derived facts take a hairline left border and a label; confidence takes a band label, not only a hue; the temporal state takes a text banner, not only teal chrome. Every one of these must survive a greyscale screenshot pasted into a ticket.
2. **The graph canvas has a non-visual equivalent** exposing the same model — nodes, edges, direction, provenance, confidence — keyboard-navigable and separately deep-linkable (`40` Slice F).
3. **Focus is always visible and never only a colour change.** A 2px offset ring on `accent`, on every interactive element, including nodes on the canvas.

## Explicitly not in the design system

| Not doing | Why |
|---|---|
| A user-customizable navigation rail | Five "customize" pages is a meaningful share of how 109 was reached, and a rail nobody shares is a rail nobody can support |
| Per-tenant white-labelling | Single-tenant deployment (`ROADMAP.md`). A theming API is a product commitment |
| A component library published for external use | The console is the consumer. Publishing multiplies the API-stability burden |
| Dashboard/widget composition | `00f` — the least defensible surface in the reference |
| Icon set of our own | One licensed set, subset to what is used |
| Illustration system | Empty states use type and one icon. Illustrations are an ongoing commission |
