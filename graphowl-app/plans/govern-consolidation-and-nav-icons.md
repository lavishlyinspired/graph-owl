# Plan: Consolidate GOVERN into tabs, reorder nav groups, add sidebar icons

**Status**: Complete

## Goal

Three sidebar changes, requested together: fold the five GOVERN pages into one tabbed "Govern" screen (matching Vocabulary Studio's own pattern) under a renamed GOVERNANCE group; move INSIGHT above INGEST and fold PLATFORM's Agents item into it; add an icon to every sidebar nav item.

## What changed

- **New `src/routes/govern.tsx`**: one page, five tabs (Validation | Contradictions | Resolution | Drift | Governance), each tab rendering the existing route component (`ValidationRoute`, `ContradictionsRoute`, `ResolutionRoute`, `DriftRoute`, `GovernanceRoute`) unchanged — same "absorb many features behind one tab bar" pattern `studio.tsx` already uses for its nine tabs. None of the five components' own internals moved; only their standalone routes are gone.
- **`lib/routes.ts`**: `validation-view`, `contradictions-view`, `resolution-view`, `drift-view`, `governance` removed from `ROUTES`; `govern` added.
- **`lib/nav.ts`**: GOVERN group replaced with a `GOVERNANCE` group containing one item, "Govern" (→ `/govern`). `INSIGHT` moved to sit directly above `INGEST` (previously below it). PLATFORM's `{ label: "Agents", route: "agents" }` moved into INSIGHT, after "Agent runs". Final group order: HOME, UNDERSTAND, VOCABULARY, GOVERNANCE, INSIGHT, INGEST, PLATFORM.
- **Icons**: `lucide-react` added (MIT, tree-shakeable — only the ~18 icons actually imported end up in the bundle). `NavItem` gained an `icon: ComponentType<{ className?: string }>` field; every one of the 24 nav items across all 7 groups has a distinct icon (`Home`, `Compass`, `BookOpen`, `ShieldCheck`, `BarChart3`, `History`, `Bot`, `Database`, `Plug`, `Package`, `Boxes`, `Server`, `Settings`, `ListChecks`, `CheckCircle2`, `Lock`, `ShieldAlert`, `KeyRound`). `chrome/Rail.tsx` renders the icon before the label when expanded, and icon-only (centered, `title` attribute for a tooltip) when collapsed — replacing the previous collapsed-state behavior of showing just the label's first letter.
- **Route budget**: net 4 routes fewer (5 removed, 1 added) — well under the 30-route ceiling.

## Verified

- 397/397 unit tests pass (`nav.test.ts`'s stale `/validation-view` assertion repointed to `/govern`; `tests/first-run.spec.ts`'s same stale route also repointed), `tsc`/`eslint` clean on every touched file.
- Live: Govern's five tabs all render their real content (screenshotted Validation and Drift, both showing real data/UI, tab switching works, no layout breakage). Sidebar icons render correctly both expanded (icon + label) and collapsed (icon only, centered) — screenshotted both states.

## What did not move

`entity`/`explore`/`pipeline` (contextually-reached, not nav items) are unaffected. The five folded-in components' own files, exports, and internal logic are byte-for-byte unchanged — this was a pure routing/nav consolidation, not a rewrite.

---
*Delete this file when the plan is complete.*
