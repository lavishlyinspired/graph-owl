# Plan 112 — Graph storage reachable from the console ★

**Status**: Slices A–C shipped; D scoped and deferred with a stated reason.
**Branch**: main. **Trigger**: the capability assessment's "graph storage
under-utilised in the UI" row, taken one capability at a time.

## What the assessment got wrong, measured against the tree

Three of its graph/UI rows are already closed and must not be re-scheduled:

| Doc's claim | Verified today | Verdict |
|---|---|---|
| Path finding UI 🔴 | `PathFinder` under Explore (Plan 111 Slice A): depth, direction, shortest/all, `asOf` | Closed |
| Traversal UI 🔴 | `GraphExplorer` expand-on-click, `graph/diff.ts`, `TimeControl`, Cytoscape canvas | Closed |
| Time travel UI 🔴 | `TimeControl`, `?asOf=`, `?compareTo=`, workbench SPARQL+Cypher as-of | Closed |

## What is genuinely open, and the evidence it is real

Each checked by grep rather than accepted:

| # | Gap | Evidence |
|---|---|---|
| 1 | **The explorer cannot filter by relationship type.** | `SubgraphQuery` carries `hops`, `direction`, `max_nodes`, `as_of` and nothing else; `asset_subgraph` passes `relationship_types: None` even though `EdgeFilter.relationship_types: Option<Vec<String>>` exists |
| 2 | **`PathFinder` never sends the filter its own API accepts.** | `api.findPaths` and `POST /graph/paths` both take `relationshipTypes`; the screen sends `from`/`to`/`direction`/`hops`/`maxPaths`/`asOf` |
| 3 | **Analytics has no console caller.** | `GET /assets/{id}/analytics` and `api.assetAnalytics` exist; `grep assetAnalytics ui/src` matches only the definition |
| 4 | **No fact-level provenance surface.** | `node_sources` is reached only from the evidence-graph handler; nothing answers "who asserted this" as a screen |

## Three corrections to the draft, and why they matter

**1. "An unknown relationship type is `400`" is wrong, and it contradicts this
plan's own neutrality rule.** There is no single relationship vocabulary to
validate against: catalog edges use `RelationshipType`'s seven values, but a
pack's edges carry whatever string its `relType` flakes hold. Validating
against a compiled list is exactly the hardcoding rule 2 forbids; validating
against observed data means walking the graph in order to reject the request.
**An unknown type simply matches nothing** — and the *other* acceptance
criterion already covers that case honestly ("nothing matches these filters",
never "nothing is connected"). Dropped.

**2. "Filter options derived from the returned edges" is circular.** Once a
filter is applied the response contains only the selected types, so the option
list collapses to the current selection and the user can never widen it again.
Options must come from the **unfiltered** walk and be held separately from the
filtered result.

**3. Expansion must carry the filter.** `GraphExplorer` grows the picture by
re-fetching one hop around a clicked node. A filter applied to the initial walk
and dropped on expansion produces a graph that is half filtered and half not —
silently inconsistent, and the reader has no way to tell which half they are
looking at.

## Three additions

**4. A pack with no edges must not get an enabled control with nothing in it.**
The filter's own empty state is part of the slice.

**5. Deployment is part of "done".** `ui/dist` is gitignored and `rust-embed`
reads it at macro-expansion time, so a console change is not live until
`npm run build` → `cargo build --release -p graph-owl-server` → restart. Plan
111 shipped three slices that were correct in the tree and invisible in the
browser for exactly this reason. Browser verification without this sequence
verifies the previous build.

**6. Slice D is a disclosure surface, not just a panel.** `node_sources` takes a
bare `Sid` and applies **no authorization at all** — today that is contained
because the only caller reaches it through a finding the principal could
already see. "Click any node → its source documents" would let a caller
enumerate which import documents assert facts about any subject, and pack
subjects have no per-subject authorization anywhere in this system (Plan 111
Slice A's recorded limitation). So Slice D is scoped to nodes **already
returned by an authorized walk** — never an arbitrary-subject lookup — or it
waits for the policy-model change.

## The test every slice must pass

1. Works if the only installed pack were hospitality.
2. The pack declares it, or the console does not assume it — relationship-type
   values come from the caller, never a list compiled into Rust.
3. An empty pack sees an honest empty state, not a broken screen.

`scripts/check-namespace-neutrality.py` gates (1) as a build failure.

## Slices

### Slice A — Relationship filter in the explorer ✅

- **Server**: `SubgraphQuery.relationship_types: Option<Vec<String>>`, threaded
  into `asset_subgraph` instead of `None`.
- **UI**: a multi-select on `GraphExplorer`, options from the *unfiltered* walk,
  carried into expansion.
- **RED**: filtering to one type changes the answer; a filtered-empty graph
  reads "nothing matches these filters", never "nothing is connected"; the
  option list does not collapse after filtering; expansion keeps the filter.
- **Mutants**: the filter swallowed (`None` regardless of input); intersection
  becoming union; the empty-vs-unfiltered distinction (`Some(vec![])` must mean
  "match nothing", not "match everything" — the dangerous direction).

### Slice B — `PathFinder` exposes the filter it already supports ✅

UI-only. Same option-source posture as Slice A.

- **RED**: a filtered request returns only routes whose edges match; a
  relationship present unfiltered is absent when filtered out.
- **Mutant**: the filter dropping to `undefined` — a silent fallback to
  "follow everything", which reads as a working filter that does nothing.

### Slice C — Asset analytics reaches the console ✅

The missing `api.assetAnalytics` caller: degree in/out, orphans, edge types,
truncation.

- **RED**: a truncated walk renders as truncated; orphans and edge types come
  from the payload, never a pack-specific list.
- **Mutant**: dropping `truncated`; hardcoding an edge-type list.

### Slice D — Provenance explorer (scoped, deferred)

Deferred with the reason in correction 6 above, not because it is
unimportant. The honest version is "provenance for a node this walk already
returned", which is a small extension of the evidence panel rather than a new
explorer — and the general form needs a policy that can name a namespace or a
class. Recorded so it is not rediscovered as missing.

## Out of scope, with reasons

- **Source/named-graph filtering inside the walk** — needs an `EdgeFilter`
  extension in the engine; an engine epic, not a console fix.
- **Whole-graph analytics** — Epic 38's purity boundary forbids unbounded
  computation on a synchronous request.
- **Per-edge "why"** — `ReasoningPanel` already explains derivations.
- **Graph diff as its own screen** — `?compareTo=` already is that mechanism.

## What the browser found that nothing else could

Two defects shipped past `tsc`, ESLint, 775 unit tests and a 100% mutation
score, and both were reader-facing:

**1. A shared filtered URL had no filter control and blamed the wrong thing.**
`?edges=parentDatabase` loads with the filter already set, so no unfiltered
walk ever happens in that session and an accumulator seeded from walk responses
is empty: the control disappeared and the empty picture read *"Nothing is
connected to this node at this depth"* — the exact claim this slice exists to
avoid making. **Correction 2 in this plan was right that the options cannot
come from the filtered response, and still had the wrong source.** They now
come from the analytics payload, which walks unfiltered by construction, so the
answer holds no matter what the current walk was narrowed to. Found on the very
link the slice made shareable.

**2. Namespace noise and raw UUIDs.** The summary read "connected by
`1:parentSchema`" and every row of the table was a hex string, while the canvas
directly above it had the names. Both now resolve — prefer a known name, fall
back to the identifier, never invent one.

Neither is exotic. Both are the kind of thing only a person looking at the
screen notices, which is the argument for addition 5 being a gate item rather
than a nicety.

## Pre-PR gate

1. Stryker 0-missed on the new filter/panel logic.
2. `tsc --noEmit`, ESLint, `fmt`, `clippy`, touched-crate tests green.
3. Truncation visible (C). 4. Filtered-empty ≠ not-connected (A).
5. Route budget ≤ 30, `routes.structural.test.ts` green.
6. Namespace-neutrality script green.
7. **Built and restarted before any browser check** (addition 5).
