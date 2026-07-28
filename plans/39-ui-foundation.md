# Plan: Console Foundation, Discovery & Entity Pages (Epic 39)

**Branch**: feat/ui-foundation
**Status**: **In progress** — shell, search, entity page and time control shipped; Slice E's trust components and the base-direction primitive still open
**Depends on**: Epic 1 (API conventions + OpenAPI), Epic 8 (search), Epic 12 (authn), Epic 13 (authz)
**Unblocks**: Epic 40 (graph explorer), Epic 41 (workbench & governance)
**Crates**: **`graph-owl-ui`** (new — embeds and serves the built SPA) · `graph-owl-server` (mounts it) · frontend sources in **`ui/`**, outside `crates/`

**Read `00f-ui-architecture.md` first.** It carries the stack, the budgets, the non-negotiables, and the positioning reversal that justifies a console existing at all. This plan implements them; it does not re-argue them.

## Goal

The shell everything else hangs off, plus the two screens people use every day: **find a thing, read the thing.**

Nothing in this epic is a differentiator. That is deliberate — the differentiators land in Epic 40 and 41, and they need a shell, an authenticated session, a generated API client, a design language for confidence and derivation, and a page to hang an asset on. Building those badly is how the later epics get expensive.

## Resolved decisions

1. **Embedded in the binary, feature-gated off.** `graph-owl-ui` embeds the Vite build output via `rust-embed` and exposes a router `graph-owl-server` mounts. `--no-default-features` compiles the assets out entirely, for headless API/MCP deployments. This follows `00a-product-position.md`'s operational-simplicity budget: no second service, no CDN, no reverse proxy, and no possibility of console/API version drift.
2. **The API client is generated from the OpenAPI document, never hand-written.** A hand-written client drifts from the contract in silence, and every API change becomes a manual audit nobody performs. Generation makes drift a **build failure**.
3. **A search-first shell, not a dashboard.** The landing surface is a search box and recent activity. A configurable dashboard is the single largest surface by page count in comparable products and the least defensible — `00f-ui-architecture.md` lists it as explicitly not doing.
4. **One composable entity page for every entity type.** The page is driven by the Epic 3 envelope, which every entity shares; a type contributes an Overview renderer and nothing else. Twenty-five entity types × a bespoke page each is how a page count reaches 109. It also means a **new entity type is viewable without a UI release**, which Slice D asserts.
5. **Tokens live in memory only.** OIDC public client with PKCE against Epic 12's provider config. Never `localStorage`, never `sessionStorage`, never a non-`HttpOnly` cookie. An XSS in a catalog console holding a persisted token is a credential-theft path into everything the catalog can read.
6. **Empty, partial, and denied states are designed in this epic, not retrofitted.** A fresh deployment is empty; a restricted principal sees a partial graph; a truncated result is partial by design. All three are the *normal* first experience, and a component that renders them as a spinner or a blank box is unfinished.
7. **Strings are externalized from the first commit.** Retrofitting i18n means touching every component; enforcing it from the start is a lint rule. No user-visible literal in a component.

## Implementation reference

```rust
// graph-owl-ui — embeds the built SPA; no business logic, no API surface of its own
#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../../ui/dist"]
struct Assets;

pub fn router() -> axum::Router;   // static assets + SPA fallback
```

**The SPA fallback rule, stated precisely** because getting it wrong is subtle and expensive:

| Request | Response |
|---|---|
| `/assets/index-abc123.js` | The asset, immutable cache headers (content-hashed filename) |
| `/tables/xyz` (no matching asset) | `index.html`, `200`, `no-cache` |
| `/api/v1/nonexistent` | **The API's own `404`** — never `index.html` |

A fallback that swallows API 404s makes every mistyped endpoint look like a working page, turns integration bugs into blank screens, and breaks generated-client error handling. The mount order asserts it.

### The composable entity page

```
EntityPage
├── EnvelopeHeader      name · FQN · type · owner · domain · tags     (Epic 3 envelope)
├── TrustBar            certification · confidence · deprecation      (shared, Slice E)
└── Tabs
    ├── Overview        ← the ONLY type-specific renderer
    ├── Lineage         (Epic 40 fills this; a stub with a link here)
    ├── Relationships   generic — every entity has them
    ├── Knowledge       triples, derived facts, memories (Epic 41)
    ├── Quality         constraint violations (Epic 41)
    └── History         version timeline; time-travel entry point (Epic 40)
```

Every tab except Overview is driven by generic API responses and needs no per-type code. **A type contributes an Overview renderer and nothing else.** A type with no registered renderer falls back to a schema-driven property table — which is the whole point of decision 4.

## Acceptance criteria

- [ ] `graph-owl-server` serves the console at `/` with no additional process or configuration.
- [ ] `--no-default-features` produces a binary with **zero** embedded assets, asserted by binary inspection.
- [ ] The API client is generated from the OpenAPI document; a schema change that breaks the console **fails the build**.
- [ ] The SPA fallback never intercepts a request under the API prefix.
- [ ] OIDC/PKCE login works against Epic 12's provider; tokens are never written to any persistent browser storage.
- [ ] Search returns results with facets, keyboard-navigable, cursor-paginated per `00d-api-conventions.md`.
- [ ] Every entity type renders on the composable page, including one with **no registered Overview renderer**.
- [ ] Confidence, derivation, and certification use **one** shared component set, asserted structurally.
- [ ] Empty, partial, denied, and error states are designed and tested for every surface in this epic.
- [ ] Zero axe violations; every journey completable by keyboard alone.
- [ ] All `00f-ui-architecture.md` budgets enforced in CI as **failures**, not warnings.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first. Frontend mutation testing uses Stryker; Rust uses `cargo mutants`.

### Slice A: The binary serves a page

**Acceptance criteria**: `graph-owl-ui` embeds a built asset directory; `graph-owl-server` mounts it at `/`; a hashed asset returns with immutable cache headers; an unknown non-API path returns `index.html` with `200`; **an unknown path under the API prefix returns the API's `404` with the `00d` error body**; `--no-default-features` compiles the assets out; a missing `ui/dist` at build time produces a clear error, not a silently empty binary.
**RED**: The API-404 test. A fallback registered ahead of the API router makes every 404 a `200 text/html`, the generated client parses HTML as JSON, and the user sees a blank page instead of an error. Also RED the feature-off test by asserting a known asset string is **absent** from the compiled binary — a feature flag that gates the route but still embeds the bytes fails the binary budget while appearing to work.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice B: Generated client and authenticated session

**Acceptance criteria**: the OpenAPI document generates a typed client into `ui/src/api/`; generation runs in CI and **fails on drift**; OIDC/PKCE login, silent refresh, and logout; a `401` triggers re-authentication once and does not loop; a `403` renders the denied state, not the login screen; the principal's identity and permissions are fetched once and cached; the token is held in memory only.
**RED**: The token-storage assertion — after a full login, inspect `localStorage`, `sessionStorage`, and `document.cookie` and assert the token appears in none of them. This is decision 5, and it is the kind of requirement that survives review and dies in an implementation detail three months later. Second RED: the `401`-loop test, because a refresh that itself `401`s is the classic infinite redirect.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice C: Search and discovery

**Acceptance criteria**: a search box returning Epic 8 results with type, owner, domain, tag, and certification facets; result rows show enough to choose without navigating; cursor pagination per `00d-api-conventions.md`, **not** offset; the query and every facet live in the URL and restore on reload; keyboard-only operation from box to result to entity; an empty index renders the designed first-run state with a next action, not "0 results"; a search the principal is not permitted to see returns the filtered set with a **consistent count**.
**RED**: The count-consistency test. If the total is computed before authorization filtering, a user sees "47 results" above 12 rows — which leaks the existence of 35 assets they cannot see. That is an authorization bug wearing a pagination costume, and `00d-api-conventions.md` requires the filtered count. Second RED: the URL-restore test on a facet combination, because non-deep-linkable search is non-negotiable 2 broken on the first screen.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice D: The composable entity page

**Acceptance criteria**: one route renders any entity from its Epic 3 envelope; the header shows name, FQN, type, owner, domain, tags; tabs render from generic responses; the Overview slot resolves a per-type renderer from a registry; **an entity of a type with no registered renderer renders a schema-driven property table rather than an error**; a partial response (some fields the principal cannot read) renders what is permitted and marks the rest as restricted, not missing; a deleted-then-restored entity renders from its current version; the URL is stable and shareable.
**RED**: The unknown-type test, using a type deliberately absent from the registry. This is decision 4's whole justification: if a new entity type requires a UI release to be viewable, the console becomes the bottleneck on every backend epic that adds one. Second RED: restricted-vs-missing — rendering a denied field as empty tells the user the data does not exist, which is a different and wrong statement.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice E: Trust, confidence, and derived facts

**Value**: This slice creates the visual language Epics 40 and 41 depend on. Building it later means building it three times, inconsistently.
**Acceptance criteria**: one shared component set renders confidence bands (`00c-domain-model.md`: ≥0.8 assert, 0.5–0.8 surface, <0.5 ignore), derivation (asserted vs inferred, with the derivation reachable), certification and deprecation, and provenance (source, `t`, ingested-by); derived facts are visually distinct **everywhere they appear**; the distinction is **not colour alone**; a structural test asserts no route renders confidence or derivation with anything other than the shared components.
**RED**: The structural single-source test — grep-equivalent over the route tree asserting the shared components are the only renderers of these facts. Without it, Epic 40's canvas grows its own confidence styling and the two drift, which is exactly how a user learns to distrust the indicator. Second RED: the not-colour-alone assertion, since a colour-only derived/asserted distinction is invisible to a colour-blind user and to a printed screenshot in a review.
**Done when**: criteria met, mutation report reviewed, commit approved.

**Base direction belongs to this slice's component set** (added 28 July 2026, from Epic 94 Slice C). `rdf:dirLangString` carries a base direction, so every component that renders a **user-supplied** label — name, description, tag, graph-node caption — must set `dir` from the data, defaulting to `auto` and never hard-coding `ltr`. It sits here rather than in a screen because it applies wherever a label is drawn, which is the same reason confidence and derivation sit here.

The reason it is a correctness rule and not a nicety: once the store knows a label is right-to-left, a console that renders it left-to-right is **less correct than the database**, and it became so by learning more. **Additional RED**: real Arabic or Hebrew text renders right-to-left in the entity header, in search results, and on a graph node — the same assertion Epic 94 makes server-side, made again at the DOM. Mutator watch: dropping `dir` must fail; hard-coding `ltr` must fail. This is the one rule in the design system whose violation is **invisible to a reviewer who reads only English**, which is why it is pinned to a test rather than to a convention.

### Slice F: States, budgets, and journeys

**Acceptance criteria**: designed and tested empty, loading, partial, denied, and error states for every surface in this epic; a Playwright journey covering **first run on an empty database** through connecting a source to viewing the first ingested asset; all `00f-ui-architecture.md` budgets enforced in CI as build failures; zero axe violations; a full keyboard-only journey; strings externalized with a lint rule failing on a literal.
**RED**: The empty-database journey. It is the first thing every evaluator sees and the last thing anyone tests, and a console that opens on a wall of empty panels loses the evaluation in the first thirty seconds. Second RED: assert the budget check **fails** the build on a deliberately oversized bundle — a budget that warns is a budget that is exceeded by the third month.
**Done when**: criteria met, mutation report reviewed, commit approved.

## Explicitly deferred (with destination)

- **Graph canvas, lineage, time-travel** → Epic 40. The History tab links to it; it does not render it.
- **Query workbench, violations, memory authoring, admin** → Epic 41.
- **Bulk editing, saved searches, notifications, personal home** → after the differentiators. Each is a real feature and none is why anyone evaluates this product.
- **Ontology/shape authoring GUI** → not doing (`00f-ui-architecture.md`). Metadata-as-code (Epic 20) is the path.
- **Server-side rendering** → an authenticated internal console gets nothing from SSR and pays a deployment model for it.
- **Theming beyond light/dark** → a white-label system is a product decision, not a UI one.

## Pre-PR quality gate

1. **Stryker** on the frontend, `cargo mutants` on `graph-owl-ui` — 0 missed.
2. Refactoring assessment.
3. `cargo test/clippy/fmt`; `tsc --noEmit` strict; ESLint clean.
4. **API-404 not swallowed** by the SPA fallback (Slice A).
5. **No token in persistent browser storage**, asserted by inspection (Slice B).
6. **Generated client matches the live OpenAPI document** — build fails on drift (Slice B).
7. **Zero axe violations and a passing keyboard-only journey** (Slice F).
