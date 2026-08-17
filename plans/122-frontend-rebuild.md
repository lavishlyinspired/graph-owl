# Plan 122 — Frontend rebuild: GraphOWL Console and Reco Now

**Status**: planned, 17 August 2026. Not started.
**Trigger**: two design mockups delivered at
`samples/GraphOWL and Reco Now UI Mockups3/` describing a substantially
richer product surface than either shipped frontend. User asked for both
frontends to be rebuilt at new locations, reusing existing code and APIs
where they exist and implementing what does not.

**Children**: `plans/122a-graphowl-app.md` (console), `plans/122b-reconow-app.md`
(Reco Now). This file is the umbrella: analysis, capability matrix, the
decisions both children inherit, sequencing, and archival.

**Companions — binding, read before implementing any slice**:
`00f-ui-architecture.md` (stack, non-negotiables, CI budgets),
`00h-ui-design-system.md` (tokens, the five patterns, screen inventory),
`00d-api-conventions.md` (URL shape, status codes, error body, pagination),
`00a-product-position.md` (what this competes on), `00i-licensing.md`,
`plans/120-domain-agnostic-console-and-investigation-workspace.md` (the
console/reco split this plan builds directly on top of).

---

## 0. Goal

Replace both frontends with implementations of the delivered mockups:

| | From | To |
|---|---|---|
| GraphOWL Console | `ui/` | `graphowl-app/` |
| Reco Now | `ext-apps/Reco/frontend` | `ext-apps/RecoNow/` |

Both old apps are archived once their replacement is complete and cut over
— **not before**, and not per-slice. The working tree must always have one
console that works.

---

## 1. What the mockups actually are

Two single-file **Claude Design (`dc`) templates** — `GraphOWL Console.dc.html`
(3,093 lines, 267KB) and `Reco Now.dc.html` (2,242 lines, 196KB) — plus a
shared `support.js` runtime and four concept documents under `uploads/`.

They are **a visual and behavioural specification, not code to port**:

- Fixed 1440px width, no responsive behaviour, inline styles throughout.
- Templated with `{{ binding }}`, `<sc-if>`, `<sc-for>` — a bespoke runtime,
  not React. The state is a single literal object at the bottom of each file.
- No routing, no data fetching, no error/loading/empty states, no
  accessibility semantics (div-based controls, no roles, no focus order).

**What to take from them**: the screen inventory, the information
architecture, the copy (which is unusually good and carries the product
argument), the interaction model, and the two design languages. **What not
to take**: the markup, the inline styles, the fixed width, or the
hand-placed graph coordinates.

### 1.1 The four concept documents are the product argument

`uploads/concept.md`, `reconow-ui-concept.md`,
`reconow-with-graphowl-concept.md`, `reconow-ui-agent-concept.md` state the
positioning both mockups encode. The load-bearing lines:

> **Reco owns the business experience. GraphOWL owns graph semantics.**

> **Reco Now is the workflow lens over GraphOWL.** The user sees GST
> concepts and actions; underneath, every important screen is powered by
> GraphOWL's graph, evidence, provenance, reasoning, temporal and
> entity-resolution capabilities.

> LLMs and agents operate **on top of** the reconciliation + GraphOWL
> semantic layer. They are not the source of truth. The deterministic
> engine decides facts; GraphOWL provides relationships, evidence and
> reasoning; the LLM interprets and phrases.

This is consistent with Plan 120's shipped split and with
`00a-product-position.md`. It is not a new direction — it is the same
direction, drawn properly.

### 1.2 The two apps have deliberately different design languages

This is a design decision in the mockups, not an inconsistency, and the
plan preserves it:

| | GraphOWL Console | Reco Now |
|---|---|---|
| Type | IBM Plex Sans + IBM Plex Mono | Public Sans + IBM Plex Mono |
| Theme | Dual (dark default, light toggle) | Light only, warm paper `#f4f2ee` |
| Accent | Cyan/teal `#5cc7d8` dark / `#0f7f92` light | Indigo `#5b6bb5` |
| Register | Instrument panel — dense, monospace-heavy, technical | Document — calm, roomy, financial |

**Consequence for "a shared design system": share the primitives, not the
look.** Radix behaviour, Tailwind config shape, table/drawer/queue
components and the a11y harness are shared; tokens and chrome are per-app.
Do not force one palette across both.

> ⚠ **Conflict to resolve before A0 lands.** Plan 120 Slice F (`b69c6be`)
> shipped the current console's visual refresh as "dark theme kept per
> direct user choice, **new indigo accent**". The GraphOWL mockup specifies
> a **cyan/teal** accent. These are incompatible; the mockup is newer and
> is the thing being asked for, so the plan proceeds on cyan/teal, but this
> is recorded as a deliberate reversal of a user-chosen decision rather
> than an oversight. If indigo was the stronger preference, say so at A0
> and the token file changes — nothing downstream does.

---

## 2. Screen inventory

### 2.1 GraphOWL Console — 24 destinations

Taken verbatim from the mockup's `nav` array plus the two screens reached
contextually (`entity`, `pipeline`).

| Group | Items |
|---|---|
| HOME | Overview |
| UNDERSTAND | Explore · Entity · Knowledge |
| TRACE | Lineage · Paths · History · Evidence |
| GOVERN | Validation · Contradictions · Resolution · Drift · Governance |
| INGEST | Sources · Connectors *(+ Source mapping, reached from Sources)* |
| VOCABULARY | Studio *(7 tabs: Build · Glossary · Business view · Proposals · Graph · Validate · SPARQL · Export)* |
| INSIGHT | Analytics · Agent runs |
| PLATFORM | Workbench · Packs · Agents · MCP · Admin |

23 nav items (`Connectors` appears twice — INGEST and PLATFORM — and is one
destination) plus Source mapping, reached from Sources: **24 destinations
against the CI-enforced budget of 30** (`00f` "Budgets").
It fits, but only because the mockup already applies the five-pattern
discipline — Studio absorbs eight surfaces into one route via tabs, and
TRACE/GOVERN are one pattern in nine configurations. **Do not let any of
those become routes of their own.**

Bespoke screens in the mockup (their own layout): `overview`, `explore`,
`entity`, `agents`, `studio`, `analytics`, `runs`, `pipeline`. Everything
else renders through one generic KPI + viz + table + drawer template —
which is `00h`'s five patterns, arrived at independently. Build the generic
template first; it is 16 of the 24 screens.

### 2.2 Reco Now — 28 destinations

| Group | Items |
|---|---|
| HOME | Dashboard |
| RECONCILE | Upload & map · Periods · Register · Exceptions · Case detail · Cross-period |
| ITC | ITC position · At risk · Eligibility |
| COMPLIANCE | Authority · Obligations |
| SUPPLIERS | Suppliers · Supplier risk · Follow-ups |
| OPERATE | Review queue · IMS · Approvals · Assistants |
| DELIVER | Deliverables · Analytics |
| DATA | Imports · Sources · Mappings |
| SETTINGS | Rules · GSTINs · Users · New session |

Bespoke: `home`, `register`, `case`, `agents`, `pipeline`, `analytics`.
The other 22 use the generic template.

`00f`'s 30-route budget governs *the console*, not Reco Now. Apply the same
discipline anyway — 28 is close enough that a careless slice breaks it.

### 2.3 Global chrome, both apps

Shared shape, different content:

- **Left rail**, collapsible, grouped, per-item badge counts.
- **Top bar**: identity switcher (workspace / client), global search that
  doubles as an ask box (⌘K), theme toggle *(console only)*, an at-risk or
  as-of chip, an **inbox** flag, avatar.
- **"Waiting on you" inbox** — the single approval queue. Both mockups make
  the same promise in copy: *"agents queue here; nothing applies itself"* /
  *"Automatic assistants never appear here — they only write text. Anything
  that leaves Reco Now or changes a number needs this queue."* This is a
  product invariant the implementation must actually honour, not a label.
- **Banner + Undo** on every mutating screen.

---

## 3. What already exists

### 3.1 GraphOWL API — 157 paths

`openapi.json` (regenerated 16 Aug) exposes 157 paths. The overwhelming
majority of the console mockup is already backed. Full mapping in
`122a`; the summary:

| Verdict | Count of mockup surfaces | Examples |
|---|---|---|
| **Backed today** | ~15 of 24 | Explore, Entity, all four TRACE screens, all five GOVERN screens, Sources/Connectors, Workbench, Packs, MCP, Admin |
| **Partially backed** | ~4 | Overview, Vocabulary Studio, Agents, Source mapping |
| **No API at all** | ~4 | Agent runs, Analytics, workspace isolation, aggregated inbox |

### 3.2 GST is data, not endpoints — and must stay that way

**There is not one GST-specific HTTP route in the 157.** No `/periods`, no
`/itc`, no `/gstin`, no `/invoice`. GST lives entirely as pack content —
`packs/gst/ontology.ttl`, `law/`, ten SPARQL queries under `queries/`,
fixtures — consumed through the neutral `/sparql`, `/cypher`, `/findings`,
`/packs/{pack}/reconcile` and `/graph/context` surfaces.

That is the domain-neutrality rule holding (`plans/105-domain-neutrality.md`,
and the standing constraint that GST and hospitality are interchangeable
packs, never the product's identity). **Reco Now's GST screens must be
built on the neutral primitives plus Reco's own backend. No slice in
`122b` may add a GST noun to the Rust API.** If a Reco screen seems to
need one, the answer is a pack query or a Reco-side endpoint.

### 3.3 The existing console (`ui/`) — substantial, and worth harvesting

React 19 · Vite 6 · TS strict · Tailwind 4 · Radix · AntV G6 · React Flow ·
Vitest · Playwright + axe · Stryker. 13 routes, no router (a `?section=`
query param plus `history.replaceState`). 1,997-line hand-written `api.ts`
over a 1,583-line **generated** `generated/api.d.ts` with a round-trip test.

13 feature directories, ~950 tests green. Genuinely reusable, in rough
order of value:

| Asset | Why it is worth moving |
|---|---|
| `generated/api.d.ts` + `roundtrip.test.ts` | OpenAPI-generated types. `00f`: the API layer is "never hand-written". Regenerate, keep the pattern. |
| `features/ontology-builder/` (22 files) | React Flow canvas, layout, flow model, formats — all unit-tested pure functions. |
| `features/packs/` (19), `features/review/` (17) | Pack admin and the review-queue pattern, which is one of `00h`'s five. |
| `features/vocabulary/` (7) | Tree + detail over glossaries — the base for Studio's Build tab. |
| `graph/`, `lib/`, `theme.ts`, `components/ui/` | G6 wiring, shadcn primitives, the token bridge. |
| `routes.ts` + `routes.structural.test.ts` | The route-budget guard. **Port this on day one**, not at the end. |
| `scripts/check-budgets.mjs`, `eslint-rules/`, `stryker.config.mjs` | The CI budget and mutation harness. |

**Do not port** the `?section=` navigation (adopt a real router — 24 routes
with tabs and deep links is past what `replaceState` should carry), the
GST-specific reconciliation surface (Plan 120 Slice E already narrowed it;
its remains belong in Reco Now), or `api.ts`'s hand-written half where the
generated types already cover it.

### 3.4 Reco Now today — the frontend is thin, the backend is the problem

`ext-apps/Reco/frontend`: plain **JavaScript** React 18 (`matcha-frontend`),
5 pages (`Upload`, `Map`, `Reconcile`, `Act`, `Intelligence`), 3 components,
3 dependencies, **no TypeScript, no tests**. Against a 28-screen mockup this
is close to a greenfield build; harvest `format.js` and the reconciliation
table shape, little else.

`ext-apps/Reco/backend`: FastAPI, 18 routes, ~85KB of Python across
`main.py`, `graphowl_client.py`, `reconciliation.py`, `native_findings.py`,
`ai.py`, `exporters.py`. The reconciliation and RDF-projection logic is real
and worth keeping.

**The blocker is persistence.** All state is a module-level
`SESSION: dict = {}` — one session, one client, one period, lost on restart.
The mockup requires the exact opposite: a client switcher whose copy reads
*"Cases, ITC and follow-ups never cross clients"*, a period picker, and
durable IMS decisions, follow-ups, approvals, notes and deliverables. **No
Reco screen above the dashboard can be built honestly until this is
replaced.** It is `122b`'s first slice for that reason.

Its GraphOWL integration is currently three endpoints — `/graph/import/rdf`,
`/findings`, `/packs/{pack}/reconcile`. The mockup's case detail, evidence
chain and cross-period screens need far more: `/graph/paths`,
`/findings/{id}/evidence-graph`, `/graph/context`, `/sparql`, `asof`.

---

## 4. Resolved decisions

Confirmed with the user, 17 August 2026, before planning.

| # | Decision | Consequence |
|---|---|---|
| **D1** | **`graphowl-app/` replaces `ui/` as the embedded console.** | `crates/graph-owl-ui/build.rs` repoints from `ui/dist` to `graphowl-app/dist`. `00f`'s "embedded in the binary" stands: no version drift, one service, OIDC PKCE with in-memory tokens. All CI budgets transfer to the new app. |
| **D2** | **Reco Now keeps a Python FastAPI backend, gains a real database.** | GST workflow state stays out of the neutral engine. Existing reconciliation/RDF code is reused. `ext-apps/RecoNow/` holds frontend + backend. |
| **D3** | **API first, per slice, TDD.** | Any frontend slice needing data that does not exist ships its Rust endpoint **first** — RED → GREEN → mutate → kill → refactor — then the UI. Honours `00f`'s "every capability the console offers must exist in the API first... it never gets an endpoint of its own." Slices where this applies are marked **`.api`** and are separate commits. |
| **D4** | **GraphOWL first, then Reco Now.** | Shared API and design-system work lands once; Reco consumes it. Matches the concept docs' own layering. |

---

## 5. Standing constraints

Every slice in both children inherits these. They are not advice.

1. **TDD, no exceptions.** RED → GREEN → MUTATE → KILL → REFACTOR. No
   production code without a failing test — frontend included. `00f`'s
   testing section and the `testing` / `front-end-testing` / `react-testing`
   skills govern how.
2. **Build-vs-adopt check per slice, not per epic.** `plans/00l-build-vs-adopt.md`
   gets a row each time, whichever way it goes. Licence is a gate: permissive
   only.
3. **The console never gets a privileged endpoint** (`00f` non-negotiable).
4. **Authorization is the server's, always.** The UI hides what the API says
   is invisible; it never decides.
5. **Derived facts are visibly derived**, everywhere, with derivation
   reachable. The mockups already do this well (`INFERRED` chips, confidence
   with a reason, "WHY GRAPHOWL BELIEVES THIS") — keep it.
6. **No GST noun in the Rust API** (§3.2).
7. **Every magic number needs a stated reason in its plan** (`00i` rule 4).
   The mockups are full of numbers — `0.84` confidence, `98.2%` grounding,
   `₹12.4L`, a `$2,000` budget. These are **placeholder fixture data**, not
   specifications. Do not hard-code one into production code, and do not
   copy a threshold out of a mockup without deriving it.
8. **Licensing** (`00i`): the mockups and concept docs are our own artifacts
   and may be read freely. Nothing in this plan permits opening the
   restricted references under `.claude/docs/referenceRepo/`.
9. **Never name the third-party reference systems** in any committed file.

### 5.1 The bundle budget is already breached — this rebuild is the fix

Measured 15 Aug 2026 (`00f`, "Bundle measurement"):

| Budget | Ceiling | Current `ui/` |
|---|---|---|
| Initial JS, gzipped | 350KB | **747.1KB — over** |
| Route chunk, gzipped | 100KB | 83.2KB — ok |
| Runtime dependencies | 40 | 29 — ok |
| Routes | 30 | 13 — ok |
| axe violations | 0 | 0 |

`00f` names the lever and forbids another revision: *"code-splitting the
explorer and workbench routes... not another revision."* A greenfield app
is the cheapest possible moment to do that.

**Therefore: route-level `React.lazy` from `graphowl-app`'s first commit,
with G6, React Flow and the SPARQL/Cypher editor each behind their own
dynamic import.** `npm run check:budgets` is an acceptance criterion on
*every* `graphowl-app` slice, not a final gate — a rebuild that lands at
747KB again has failed regardless of how the screens look.

### 5.2 The build/test loop

`CLAUDE.md`'s rules apply unchanged and are easy to forget on frontend work:

- Frontend test and mutation runs are **not free because they are not Rust**.
  `stryker` spawns 17 runners; do not interleave with a Rust suite.
- `crates/graph-owl-ui/build.rs` watches `dist`, **not** `src` — editing
  TypeScript triggers no Rust rebuild; `npm run build` is the only thing
  that does.
- Batch verification per epic, not per slice.

---

## 6. Sequencing

```
122a  GraphOWL Console  ──────────────────────────────►  cutover, archive ui/
      A0 ─ A1 ─ A2 ─ A3 ─ A4 ─ A5 ─ A6 ─ A7 ─ A8 ─ A9 ─ A10 ─ A11
                    │                        │
                    │  shared primitives     │  agent/analytics APIs
                    ▼                        ▼
122b  Reco Now                        B0 ─ B1 ─ ... ─ B10 ─► cutover, archive
```

Reco Now starts once `122a`'s A0–A3 have landed — that is where the shared
primitives (table, drawer, queue, banner+undo, a11y harness) stabilise.
`122b`'s B0 (`.api`, backend persistence) has no frontend dependency and may
start in parallel at any time.

---

## 7. Archival

**Only after the replacement is cut over and verified live**, per child
plan's final slice:

| Move | When |
|---|---|
| `ui/` → `_archived/ui/` | `122a` A11 |
| `ext-apps/Reco/frontend` → `_archived/reco-frontend/` | `122b` B10 |
| `ext-apps/Reco/backend` → `ext-apps/RecoNow/backend` (moved, not archived — it is being extended) | `122b` B0 |
| `ext-apps/Reco/research`, `SAMPLE`, `graph-owl-reco-now.html` | keep in place; research and fixtures, not code |

`_archived/ui-concept/` and `_archived/samples-gst/` already exist as
precedent for how this repo archives. Follow it: move, do not delete, and
add a line to `_archived/README.md` saying what replaced it.

**`EPIC-STATUS.md` is generated — edit `DEMOS.md` and regenerate** on every
slice shipped, per the standing rule. Push each commit to `origin` as it
lands rather than batching.

---

## 8. Risks, stated plainly

| Risk | Why it is real here | Mitigation |
|---|---|---|
| **The mockups over-promise the backend.** | Four of 24 console surfaces have no API at all, and the two most visually striking (Agent runs, Analytics) are among them. | D3. The `.api` sub-slices are sized honestly in `122a`; if one turns out to be an epic, it gets its own plan rather than swelling a UI slice. |
| **Scope.** 52 screens across two apps. | This is the largest single piece of work in the repo's history. | The generic template covers 38 of the 52 (16 console, 22 Reco). Build it first and prove it on the cheapest group (TRACE) before the bespoke screens. |
| **Two consoles in the binary during migration.** | A0 embeds the new app at `/next/` alongside the old one at `/`. | Temporary and measured against the 50MB budget at A0. A11 removes it. |
| **Reco persistence is a bigger slice than it looks.** | Multi-client × multi-period × durable workflow state, replacing a dict. Migrations, isolation, and a "cases never cross clients" guarantee that must be *tested*, not asserted. | B0 is scoped as its own commit with its own acceptance criteria, ahead of any screen. |
| **Fixture data reads as specification.** | The mockup copy is confident and specific (`98.2%` grounding, `0.94` resolution confidence). It is invented. | Constraint 7. Every number in the new UI comes from an API response or a plan-stated reason. |
| **A "not yet wired" screen ships and stays.** | The failure mode D3 exists to prevent. | No screen merges rendering a panel it cannot populate. If the API is not there, the slice is not done. |

---

## 9. Next step

Read `plans/122a-graphowl-app.md`, start at **A0**. Before writing A0's
first line: resolve the accent-colour conflict in §1.2, and run the
build-vs-adopt check for the router choice (§3.3) and for any charting
library `122a` A9 needs.
