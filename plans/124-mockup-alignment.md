# Plan: Align React App to Mockup (Revised)

**Status**: Active
**Goal**: Close all key gaps between the HTML mockup and the React app.

## Key Findings

1. **Admin** is a single audit screen in the mockup (not 14 tabs). React has 5 tabs — needs restructuring.
2. **Analytics** is a placeholder — needs KPIs + charts.
3. **Agents** is missing pipeline viz, model usage, guardrails.
4. **Studio tabs** (Proposals, Validate, Export) are stubs.
5. **Bottom inspector panel** is missing.
6. **Several standalone pages** exist in mockup but not in React (Tasks, Quality, Privacy, Security, API Keys).

## Slices

### Slice 1: Admin → Single Audit Screen

**Value**: Admin matches mockup's single audit view
**Path**: `admin.tsx` → replace 5-tab layout with audit grid + KPIs + time-series chart
**Acceptance criteria**: Admin shows 4 KPI cards (Users, API Keys, SSO, Audit Events 24H), audit event grid (5 columns: Actor, Action, Object, Time, Result), time-series chart sidebar, callout note, related links
**RED**: Write test for admin audit content
**GREEN**: Implement admin as single audit screen with mockup data
**MUTATE**: Run mutation testing
**KILL MUTANTS**: Address survivors
**REFACTOR**: Assess improvements
**Done when**: Admin matches mockup's audit screen

### Slice 2: Analytics Page

**Value**: Analytics page shows KPIs and charts
**Path**: `analytics.tsx` → replace PlaceholderPage with mockup content
**Acceptance criteria**: Analytics shows entity processing stats, accuracy metrics, latency, cost, model usage chart, pipeline performance
**RED**: Write test for analytics content
**GREEN**: Implement analytics with mockup-specified KPIs and charts
**MUTATE**: Run mutation testing
**KILL MUTANTS**: Address survivors
**REFACTOR**: Assess improvements
**Done when**: Analytics page matches mockup

### Slice 3: Agents Page Enhancement

**Value**: Agents page shows pipeline visualization, model usage, guardrails
**Path**: `agents.tsx` → add pipeline viz, model usage breakdown, guardrails list
**Acceptance criteria**: Agents shows 6-stage pipeline viz, model usage bar chart with costs, guardrails checklist, MCP tool cloud
**RED**: Write test for agents content
**GREEN**: Add pipeline viz, model usage, guardrails to agents page
**MUTATE**: Run mutation testing
**KILL MUTANTS**: Address survivors
**REFACTOR**: Assess improvements
**Done when**: Agents page matches mockup

### Slice 4: Studio Tabs Completion

**Value**: Studio Proposals, Validate, Export tabs show content
**Path**: Replace NotYetBuilt stubs with real content
**Acceptance criteria**: Proposals tab shows proposal cards with state badges. Validate tab shows qSKOS validation grid. Export tab shows export format options.
**RED**: Write tests for each tab
**GREEN**: Implement each tab with mockup-specified content
**MUTATE**: Run mutation testing
**KILL MUTANTS**: Address survivors
**REFACTOR**: Assess improvements
**Done when**: All Studio tabs show content

### Slice 5: Bottom Inspector Panel

**Value**: Bottom inspector panel exists on entity and explore views
**Path**: Add `BottomInspector` component to entity and explore routes
**Acceptance criteria**: Entity and explore views show bottom inspector panel with tabs (Runs, Documentation, Query, Validation, Dedup, Counts)
**RED**: Write test for bottom inspector presence
**GREEN**: Implement bottom inspector panel component
**MUTATE**: Run mutation testing
**KILL MUTANTS**: Address survivors
**REFACTOR**: Assess improvements
**Done when**: Bottom inspector panel present on relevant views

### Slice 6: Missing Standalone Pages

**Value**: Tasks, Quality, Privacy, Security, API Keys pages exist
**Path**: Create missing route files
**Acceptance criteria**: Each page shows mockup-specified content with KPIs, grids, and charts
**RED**: Write tests for each missing page
**GREEN**: Implement each missing page with mockup-specified content
**MUTATE**: Run mutation testing
**KILL MUTANTS**: Address survivors
**REFACTOR**: Assess improvements
**Done when**: All missing pages exist with correct content

## Pre-PR Quality Gate

Before each PR:
1. Mutation testing
2. Refactoring assessment
3. Typecheck and lint pass

---
*Delete this file when the plan is complete.*
