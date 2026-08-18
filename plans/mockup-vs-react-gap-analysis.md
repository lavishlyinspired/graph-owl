# Mockup vs React App — Gap Analysis

**Mockup:** `samples/GraphOWL and Reco Now UI Mockups3/GraphOWL Console.dc.html`
**React:** `graphowl-app/src/routes/`

---

## Top-Level Navigation

| Mockup Nav Group | Mockup Items | React Route | Gap |
|---|---|---|---|
| — (Overview) | Knowledge graph overview | `home.tsx` | ✅ KPI grid, graph health, recent activity. Mockup has "Consumers of this graph" section; React has placeholder cards. **Gap:** consumer cards not wired to real data. |
| Build | Explore, Entity, Lineage, Paths, History, Evidence, Validation, Contradictions, Resolution, Drift | `explore.tsx`, `entity.tsx`, `lineage-view.tsx`, `paths.tsx`, `history.tsx`, `evidence.tsx`, `validation.tsx`, `contradictions.tsx`, `resolution.tsx`, `drift-view.tsx` | ✅ All routes exist. Most are functional with live data. |
| Analyze | Analytics, Workbench | `analytics.tsx`, `workbench.tsx` | ⚠️ `analytics.tsx` is a **PlaceholderPage** (slice A9). `workbench.tsx` exists but check if it's also placeholder. |
| Studio | Build, Glossary, Business, Proposals, Graph, Validate, SPARQL, Export | `studio.tsx` (sub-tabs in `studio/` dir) | ✅ All 8 sub-tab routes exist: `BuildTab`, `GlossaryTab`, `BusinessTab`, `ProposalsTab` (via `NotYetBuilt`?), `GraphTab`, `ValidateTab` (via `NotYetBuilt`?), `SparqlTab`, `ExportTab` (via `NotYetBuilt`?). **Need to verify which are stubs.** |
| Admin | Agents, Tasks, Knowledge, Lineage, Quality, Sources, Privacy, Governance, Packs, Connectors, API Keys, MCP, Security, Budget | `admin.tsx` (5 tabs), plus standalone `agents.tsx`, `knowledge.tsx`, `governance.tsx`, `sources.tsx`, `connectors.tsx`, `mcp-tools.tsx`, `packs.tsx` | 🔴 **Major structural gap.** Mockup has 14 admin sub-tabs. React admin has 5 (teams, users, webhooks, health, budgets). Many admin areas live outside `admin.tsx` as standalone routes. |

---

## Page-by-Page Comparison

### 1. Overview / Home (`home.tsx` vs mockup lines 136–215)

| Element | Mockup | React | Status |
|---|---|---|---|
| KPI grid (4 columns, 2 rows = 8 stats) | 8 KPIs: entities, facts, sources, conflicts, pending, certified, deprecated, confidence | `KpiGrid` with 8 items from API | ✅ Match |
| Graph Health bar chart | 5 bars (ownership, lineage, confidence, freshness, certification) with percentages | `KpiGrid` + `BarChart` from `@/components/BarChart` | ✅ Present |
| Recent Activity feed | 6 items with colored dots, text, metadata | Rendered from `activity` data | ✅ Present |
| "Consumers of this graph" card grid | 4 consumer cards with call counts | Exists but data may be placeholder | ⚠️ Minor gap — consumer data likely mock |
| Contradictions / low-confidence callouts | 2 info cards at bottom of health panel | Present in panel | ✅ Present |

### 2. Explore (`explore.tsx` vs mockup lines 217–345)

| Element | Mockup | React | Status |
|---|---|---|---|
| Breadcrumb bar with filters and "Save investigation" | Top bar with filter chips + save button | `Toolbar` with mode toggles | ⚠️ Different UX — React has mode toggles (Nodes/Paths/Compare), mockup has filter chips |
| G6/ReactFlow canvas with SVG edges | Full canvas with nodes, edges, edge labels, zoom controls | `G6Graph` component with G6 v5 | ✅ Match |
| Edge labels on canvas | Positioned `div` labels on edges | G6 edge labels via config | ✅ Present |
| Legend bar (bottom-left) | 5 legend items (inferred, asserted, contradicted, etc.) | Present at bottom | ✅ Present |
| Zoom controls (right edge) | +/- and fit buttons | G6 built-in controls | ✅ Present |
| Investigation panel (top-left) | Investigation card with pinned entities | Present in canvas | ✅ Present |
| **Right sidebar** (352px) | Relationship detail panel with tabs, confidence bar, reasoning chain, evidence list, "Trace path" button, Lineage/History buttons | `InspectorSidebar` component | ✅ Match — sidebar has reasoning, evidence, actions |

### 3. Entity Detail (`entity.tsx` vs mockup lines 347–434)

| Element | Mockup | React | Status |
|---|---|---|---|
| Entity header with status badges (CERTIFIED, 1 CONTRADICTION) | Header with name, badges, FQN, action buttons | `EntityHeader` with `EntityBadges` | ✅ Present |
| "Open in Explorer" / "Pin to investigation" buttons | Two action buttons | Buttons in header | ✅ Present |
| Tab bar (Facts, Versions, Lineage, Evidence, etc.) | 6 tabs under header | `entityTabs` array with tab switching | ✅ Present |
| Facts grid (4 columns: predicate, value, state, confidence) | Grid with 6+ rows | `FactsTab` | ✅ Present |
| Contradiction card (Source A ≠ Source B with Accept/Reject) | Inline contradiction display with source comparison | `ContradictionsTab` or inline in facts | ⚠️ Mockup shows inline contradiction card; React may show it in a separate tab |
| History sidebar (timeline with date, text, delta) | Right column with 4 history entries | `HistoryTab` or separate panel | ⚠️ Check if history is in sidebar or tab |
| "Impact if changed" sidebar | 5-row impact table | `ImpactTab` or inline | ⚠️ May be missing or in different location |

### 4. Agents (`agents.tsx` vs mockup lines 436–594)

| Element | Mockup | React | Status |
|---|---|---|---|
| 5-KPI stat row (total runs, tokens, cost, grounding, latency) | `llmStats` grid | `KpiGrid` with agent stats | ✅ Present |
| Pipeline visualization (6-stage horizontal flow) | "ONE PIPELINE, NOT FIVE SEPARATE BOTS" | Not visible in `agents.tsx` — may be elsewhere | 🔴 **Missing** — pipeline viz not in React |
| Agents table (name, trigger, runs, grounding, toggle, "Run now") | 5-agent roster with toggle switches and run buttons | `AgentGrants` + `AgentActivity` components | ⚠️ Partial — React has grants/activity, mockup has richer roster with toggles |
| Run trace detail (tool calls, args, tokens, latency) | "RUN TRACE · GRAPH INVESTIGATOR" with 6 rows | `TraceDetail.tsx` exists | ✅ Trace detail component exists |
| Answer citation box | "ANSWER · 6 CITATIONS" with grounded answer text | In `TraceDetail` or `AgentActivity` | ⚠️ Verify citation box is in trace view |
| Tokens by model (bar chart + spend) | 3-model bar chart with monthly spend | Mockup-only at this stage | 🔴 **Missing** — model usage breakdown not in React |
| Guardrails list | 4 guardrail items with check/cross marks | Not in `agents.tsx` | 🔴 **Missing** |
| MCP tools exposed (tag cloud) | 9 tool tags | Not in `agents.tsx` | 🔴 **Missing** — may be in `mcp-tools.tsx` |

### 5. Studio — Build Tab (`studio/BuildTab.tsx` vs mockup lines 596–800)

| Element | Mockup | React | Status |
|---|---|---|---|
| SKOS tree sidebar (268px, collapsible) | 9-term tree with indentation, dots, labels | `BuildTab` has tree sidebar | ✅ Present |
| Concept detail (labels, docs, semantic relations, mappings) | prefLabel, altLabel, hiddenLabel, definition, scopeNote, broader/narrower/related, external mappings | `BuildTab` renders concept detail | ✅ Present |
| qSKOS checks sidebar | 6 quality checks with codes, labels, fix links | `BuildTab` has validation sidebar | ✅ Present |
| Scheme metadata (Dublin Core) | 8-field metadata table | In sidebar | ✅ Present |
| Export formats + standards | 7 export format tags + 6 standard tags | In sidebar | ✅ Present |
| "Propose a term" button (bottom of tree) | Action button | Present | ✅ Present |

### 6. Studio — Glossary Tab (`studio/GlossaryTab.tsx` vs mockup lines 802–837)

| Element | Mockup | React | Status |
|---|---|---|---|
| Candidate terms grid (6 columns) | 7-row grid with term, source, count, placement, match, action buttons | `GlossaryTab` renders candidate table | ✅ Present |
| "Promote selected" button with parent selector | Top-right actions | Present | ✅ Present |
| Info box about promote behavior | Bottom explanation text | Present | ✅ Present |

### 7. Studio — Business Tab (`studio/BusinessTab.tsx` vs mockup lines 839–870)

| Element | Mockup | React | Status |
|---|---|---|---|
| Business view card grid (2 columns) | 6 term cards with definition, also-called, sits-under, used-in | `BusinessTab` renders term cards | ✅ Present |
| "Share read-only link" button | Top-right action | Present | ✅ Present |

### 8. Studio — Proposals Tab

| Element | Mockup | React | Status |
|---|---|---|---|
| Proposal cards with state badges, rationale, action buttons | 5 proposal cards with Accept as altLabel / Reject / Approve as concept | Check `NotYetBuilt.tsx` or actual component | ⚠️ **Likely stub** — `NotYetBuilt.tsx` is used for unbuilt tabs |

### 9. Studio — Graph Tab (`studio/GraphTab.tsx` vs mockup lines 912–939)

| Element | Mockup | React | Status |
|---|---|---|---|
| Vocabulary graph canvas with bubbles + edges | SVG canvas with positioned bubbles, edges, legend, Re-layout/Fit/Connect buttons | `GraphTab` renders G6 graph | ✅ Present |
| Connect mode (click two bubbles to link) | "Connect" button in toolbar | Present | ✅ Present |

### 10. Studio — Validate Tab

| Element | Mockup | React | Status |
|---|---|---|---|
| qSKOS validation grid (check, meaning, affected, severity, fix) | 6-row validation results grid | Check if `NotYetBuilt.tsx` or real component | ⚠️ **Likely stub** |

### 11. Studio — SPARQL Tab (`studio/SparqlTab.tsx` vs mockup lines 988–1050)

| Element | Mockup | React | Status |
|---|---|---|---|
| Natural language search bar + "Generate query" + "Run" | Search bar with keyboard shortcut hint | `SparqlTab` has query editor + run | ✅ Present |
| Split view: SPARQL editor (left) + results grid (right) | Monaco editor + results table | `SparqlTab` uses `CodeMirror` + results grid | ✅ Present (different editor, same UX) |
| Query history sidebar | Not in mockup visible area | `SparqlTab` has history | ✅ Present |

### 12. Admin — Sources (`sources.tsx` vs mockup knowledge/lineage source sections)

| Element | Mockup | React | Status |
|---|---|---|---|
| Source cards with status dots, progress bars, sync times | Mockup shows in "Knowledge" and "Lineage" tabs | `sources.tsx` has data grid + detail panel | ✅ Functional, different layout |
| KPI row (source count, objects, stale, runs) | `kpis` array | `KpiGrid` with 4 KPIs | ✅ Present |
| Data grid with columns | `grid` with columns + rows | Table with 5 columns | ✅ Present |
| Detail slide-out panel with run history | Right panel (420px) | `selected` state drives slide-out | ✅ Present |

### 13. Admin — Governance (`governance.tsx` vs mockup governance section)

| Element | Mockup | React | Status |
|---|---|---|---|
| KPI row (certified, unowned, deprecated, retired) | 4 KPIs | `KpiGrid` with 4 KPIs from API | ✅ Match |
| Policy list with delete | Policy rows with name, rule count, roles | Policy list with delete button | ✅ Present |
| Policy composer (name, roles, rules with operations) | Inline form with rule builder | Full composer with dry-run + save | ✅ Present (richer than mockup) |
| Dry-run outcome display | Not in mockup visible section | Admitted/denied/total counts | ✅ Present (React has more) |

### 14. Admin — Connectors (`connectors.tsx` vs mockup connectors section)

| Element | Mockup | React | Status |
|---|---|---|---|
| Connector cards with status badges | Cards with connection status | `connectors.tsx` has Postgres form only | ⚠️ Mockup shows multi-connector cards; React has single Postgres form |
| Connection test + sync run | Test button + run button | Test + run present | ✅ Present |

### 15. Admin — Packs (`packs.tsx` vs mockup packs section)

| Element | Mockup | React | Status |
|---|---|---|---|
| Available packs list with install | Pack cards with install button | Available + installed lists | ✅ Present |
| Installed packs with inspect | Installed list with detail panel | Detail slide-out with terms, overrides, upgrade | ✅ Present |
| Override management (hide/relabel/reparent) | Override form | Override CRUD in detail panel | ✅ Present |
| Upgrade with dry-run | Upgrade form | Upgrade form + dry-run | ✅ Present |

### 16. Admin — MCP (`mcp-tools.tsx` vs mockup MCP section)

| Element | Mockup | React | Status |
|---|---|---|---|
| Enabled MCP servers table | Server list with status | `mcp-tools.tsx` — check content | ⚠️ Verify if it matches mockup's server table |
| Capability exposure toggles | Toggle switches per capability | Check for toggles | ⚠️ Likely simpler than mockup |
| Session log | Recent MCP sessions list | Check for session log | ⚠️ May be missing |

---

## Pages Present in Mockup but Missing/Stubbed in React

| Mockup Section | Mockup Location | React Status | Priority |
|---|---|---|---|
| **Analytics** (A9) | Top-level nav | `PlaceholderPage` | 🔴 High — completely empty |
| **Tasks** (admin sub-tab) | Admin nav | No route exists | 🔴 High — no `tasks.tsx` |
| **Quality** (admin sub-tab) | Admin nav | No route exists | 🔴 High — no `quality.tsx` |
| **Privacy** (admin sub-tab) | Admin nav | No route exists | 🔴 High — 4 sub-tabs (Overview, Metadata, Movement, Controls) |
| **API Keys** (admin sub-tab) | Admin nav | No route exists | 🟡 Medium |
| **Security** (admin sub-tab) | Admin nav | No route exists | 🟡 Medium — sessions, SSO, MFA, IP whitelist |
| **Budget** (admin sub-tab) | Admin nav | Admin has "budgets" tab but mockup shows richer cost waterfall + model pricing | ⚠️ Check depth |
| **Studio Proposals** | Studio tab | Likely `NotYetBuilt.tsx` | 🟡 Medium |
| **Studio Validate** | Studio tab | Likely `NotYetBuilt.tsx` | 🟡 Medium |
| **Studio Export** | Studio tab | Likely `NotYetBuilt.tsx` | 🟡 Medium |
| **Bottom Inspector Panel** | Below canvas on all pages | Not present | 🟡 Medium — Comment/Discussion panel |
| **Inbox dropdown** | Top nav, fixed position overlay | Not present | 🟡 Medium — approval queue for agent actions |
| **Workspace switcher** | Top nav, dropdown | Not present | 🟢 Low — multi-workspace support |

---

## Summary Counts

| Category | Count |
|---|---|
| Pages fully matching mockup | ~12 |
| Pages partially matching | ~6 |
| Pages stubbed/placeholder | ~3 (analytics, proposals, validate) |
| Pages missing entirely from React | ~6 (tasks, quality, privacy, api-keys, security, bottom inspector) |
| Mockup features missing in existing React pages | ~8 (pipeline viz, model usage, guardrails, MCP tool cloud, consumer cards, contradiction inline card, impact panel, inbox) |

---

## Recommended Implementation Priority

### Phase 1 — Fill stubs (low effort, high visibility)
1. `analytics.tsx` — wire to real data or at minimum show KPIs + charts
2. `NotYetBuilt` tabs in Studio (Proposals, Validate, Export) — at minimum show grid/card layouts matching mockup
3. Add bottom inspector panel (Comment/Discussion) to entity and explore views

### Phase 2 — Missing admin sub-tabs (medium effort)
4. `tasks.tsx` — agent task queue with approve/reject
5. `quality.tsx` — compliance dashboard with sub-tabs (assignments, components, reports, config)
6. `privacy.tsx` — 4 sub-tabs: overview KPIs, classification matrix, data movement chart, access control table

### Phase 3 — Richer existing pages (high effort)
7. Agents page: pipeline visualization, model usage/cost breakdown, guardrails list, MCP tool cloud
8. Entity detail: inline contradiction card, impact-if-changed panel, richer history sidebar
9. Admin budget: cost waterfall chart, model pricing table
10. Admin security: session table, SSO card, MFA stats, IP whitelist
11. Admin API keys: key management table
12. MCP tools: server table with capability toggles, session log

### Phase 4 — Cross-cutting UX (high effort)
13. Inbox dropdown (approval queue)
14. Workspace switcher
15. Search bar with `⌘K` shortcut (command palette)
