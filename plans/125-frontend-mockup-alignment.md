# Plan 125: Frontend Mockup Alignment

## Problem

The GraphOWL React console (`graphowl-app/`) has significant visual gaps compared to the
HTML mockup (`samples/GraphOWL and Reco Now UI Mockups3/GraphOWL Console.dc.html`). Users
see empty panels, missing sections, and simplified layouts that don't match the designed
UI. This plan fixes the visual gaps **without touching any core logic, API layer, or
Reco Now code**.

## Scope

Only `graphowl-app/src/routes/` and `graphowl-app/src/lib/strings.ts`. No Rust code,
no `ext-apps/`, no `graphowl-server/` changes.

## Gap Inventory (by priority)

### P0 — Pages that are placeholders or severely incomplete

| Page | Current state | Mockup has |
|---|---|---|
| **Knowledge** | `PlaceholderPage` — title only | Full page with entities, documentation, search |
| **Pipeline** | `PlaceholderPage` — title only | 5-step progress bar, dataset sidebar, mapping table, raw data toggle |

### P1 — Pages with real data but missing major mockup sections

| Page | Current state | Mockup has |
|---|---|---|
| **Overview** | 4 stat tiles + health bars + activity list | 8 KPI cards (2 rows), Graph Health bars with colors, Recent Activity timeline, **Consumers of This Graph** grid (4 cards) |
| **Explore** | Graph canvas + simple detail sidebar | Graph canvas + **rich detail sidebar** with: relationship header with tags, confidence bar with value, reasoning chain (numbered steps), evidence panel (3 items with supports/contradicts), action buttons |
| **Entity** | Facts table (3 cols) + contradiction + sidebar | **Tab navigation** (Overview/Graph/Evidence/Lineage/History/Queries), facts table with **4th column (confidence)**, richer contradiction UI |
| **Runs** | Simple table + detail sidebar | **Split layout** (runs list 396px + run detail), run detail with output card, cited facts, run metadata, tools called chips, job progress bar |

### P2 — Pages with mock data that look correct visually (lower priority)

| Page | State |
|---|---|
| Analytics | All mock — looks good, matches mockup |
| Agents | All mock — looks good, matches mockup |
| Admin | All mock — looks good, matches mockup |
| Tasks, Quality, Privacy, Security, API Keys | All mock — acceptable |
| Studio (Proposals, Validate, Export) | Mock data — acceptable |

## Implementation Slices

### Slice 1: Overview Page Enhancement

**File:** `graphowl-app/src/routes/home.tsx`

Add the missing sections from the mockup:

1. **Expand KPI grid to 8 cards (2 rows of 4):**
   - Row 1: Assets, Relationships, Ontology Classes, Contradictions
   - Row 2: Validation Issues, Drift Alerts, Low Confidence, As Of
   - Each card: mono label, large value, subtitle, optional color (amber for issues)

2. **Graph Health panel** — horizontal bar chart with 5 metrics:
   - Coverage 91%, Validation 97%, Confidence 83%, Freshness 90%, Governance 74%
   - Two callout boxes below: amber "17 contradictions open", gray "139 low-confidence facts"

3. **Recent Activity timeline** — keep existing but enrich with colored dots and richer metadata

4. **Consumers of This Graph** — 4-column grid of consumers:
   - Reco Now (184,220 calls), Agents via MCP (2,828), ITC exposure report (96), Console users (1,204)
   - Each card: name, call count, detail text, color accent

**Data source:** Use `fetchOverview()` response, extend with mock data for sections the API doesn't cover yet (consumers, health bars). Clearly mark mock sections.

**Strings to add:** `overviewKpiRelationships`, `overviewKpiOntologyClasses`, `overviewKpiContradictions`, `overviewKpiValidationIssues`, `overviewKpiDriftAlerts`, `overviewKpiLowConfidence`, `overviewKpiAsOf`, `overviewHealthTitle`, `overviewHealthCoverage`, `overviewHealthValidation`, `overviewHealthConfidence`, `overviewHealthFreshness`, `overviewHealthGovernance`, `overviewConsumersTitle`, `overviewConsumerRecoNow`, `overviewConsumerAgents`, `overviewConsumerReport`, `overviewConsumerUsers`, etc.

### Slice 2: Explore Detail Sidebar Enhancement

**File:** `graphowl-app/src/routes/explore.tsx`

Enhance the right sidebar (352px) to match the mockup's rich detail panel:

1. **Relationship header** — show relationship type as a tag (teal), with INFERRED/ASSERTED badge, FQN below

2. **Tabs** — Detail (active), Evidence, Provenance, Time (can be simple tab buttons, content in Detail tab for now)

3. **Confidence section** — horizontal bar with percentage value and explanatory note

4. **Reasoning chain** — numbered steps explaining why GraphOWL believes this relationship:
   - Each step: number, description, source reference
   - Data comes from `fetchAssetGraph()` edge metadata

5. **Evidence panel** — list of evidence items with supports/contradicts color coding

6. **Action buttons** — "Trace path to..." (primary), "Lineage", "History" (secondary)

**Data source:** Extend from existing `fetchAssetGraph()` response. Use edge metadata for reasoning steps. Mock evidence items for now.

### Slice 3: Entity Page Tabs + Confidence Column

**File:** `graphowl-app/src/routes/entity.tsx`

1. **Add tab navigation** below the header:
   - Tabs: Overview (active), Graph, Evidence, Lineage, History, Queries
   - Tab content switching (Overview = current content, others = coming soon placeholders)
   - Tabs use the same accent-underline style as Studio

2. **Add confidence column to facts table:**
   - Change from 3-column (predicate, value, state) to 4-column (predicate, value, state, confidence)
   - Confidence values come from the asset's fact metadata
   - Color-code: green for >= 0.90, amber for 0.60-0.89, red for < 0.60

3. **Enrich contradiction box:**
   - Show source names (not just memory content)
   - Show confidence scores per source
   - Add "Accept A" / "Accept B" / "Keep unresolved" buttons (matching mockup)

### Slice 4: Knowledge Page

**File:** `graphowl-app/src/routes/knowledge.tsx`

Replace the placeholder with a real page matching the mockup's generic template:

1. **Header** — title "Knowledge", description, search input
2. **KPI row** — 4 tiles: Total Entities, Documented, Orphaned, Last Indexed
3. **Entity table** — columns: Name, Type, FQN, Documentation Status, Last Updated
4. **Detail sidebar** — entity preview with definition, links, usage count

**Data source:** Use `fetchAssets()` if available, otherwise mock data with clear markers.

### Slice 5: Pipeline (Source Mapping) Page

**File:** `graphowl-app/src/routes/pipeline.tsx`

Replace the placeholder with the full source mapping UI:

1. **5-step progress bar** — Connect ✓, Map (current), Infer, Verify, Publish
2. **Dataset sidebar** (236px) — list of datasets with status (confirmed/pending)
3. **Mapping table** — columns: Ontology Property, Source Column, Value, Confidence
4. **Raw data toggle** — show/hide sample rows from the source
5. **Unmapped columns panel** — tagged chips for unmapped columns
6. **"What mapping decides"** explanatory panel

**Data source:** Mock data for now, structured to match the mockup's `V` object.

### Slice 6: Runs Page Enhancement

**File:** `graphowl-app/src/routes/runs.tsx`

Restructure from simple table to split layout matching the mockup:

1. **Split layout** — runs list (396px left) + run detail (flex:1 right)
2. **Run list cards** — each card shows: ID, kind badge, status badge, agent name, input preview, tokens, latency
3. **Run detail panel** — sections:
   - Output card with full text + "landed in" link
   - Cited facts list with checkmarks
   - Run metadata (trigger, input, tokens, latency)
   - Tools called as tag chips
4. **Filter chips** — All, Retriever, Reasoner, Explainer, Actioner, etc.
5. **Job progress bar** (optional) — for batch runs

**Data source:** Extend from existing `fetchProposals()` response. Add mock run data for the detailed sections.

## Execution Order

1. Slice 1 (Overview) — highest visual impact, most users see this first
2. Slice 2 (Explore sidebar) — core interaction surface
3. Slice 3 (Entity tabs) — core interaction surface
4. Slice 6 (Runs) — significant layout change
5. Slice 4 (Knowledge) — placeholder replacement
6. Slice 5 (Pipeline) — placeholder replacement

## Constraints

- **No Rust changes** — all work is in `graphowl-app/`
- **No API changes** — use existing endpoints, mock what's missing
- **No Reco Now touchpoints** — `ext-apps/RecoNow/` is out of scope
- **No new dependencies** — use existing React, Tailwind, and components
- **Route budget** — stays at 29 routes (no new routes added)
- **Strings** — all user-visible text goes through `lib/strings.ts`
- **Mock data** — clearly marked with `// MOCK` comments, easy to replace when backend catches up

## Verification

After each slice:
1. `npx tsc --noEmit` — must pass
2. Visual check at `http://localhost:5180/` — page matches mockup layout
3. Existing pages still work — no regressions
4. `npm run build` — production build succeeds
