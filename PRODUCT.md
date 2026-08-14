# Product

## Register

product

## Users

Two groups, same console: an **evaluator** comparing graph-owl to an incumbent enterprise data catalog (Alation/Collibra/Atlan-shaped competitors), deciding whether to adopt it in the first fifteen minutes of a demo — and a **daily user** (data engineer, architect, compliance/CA analyst) working the graph explorer, reconciliation workspace, and ontology builder as part of their actual job. Both are technical, both are comparing this against tools that already look finished.

## Product Purpose

graph-owl is a knowledge graph engine (triples, time-travel, reasoning, constraint validation) for enterprise metadata. The console exists because those differentiators — lineage, inference, drift, confidence — are visual by nature and nearly incommunicable as raw JSON. Per the project's own architecture doc: "a graph product without a graph view loses evaluations to products with worse graphs and better pictures." Success is the console reading as a credible, finished data-catalog product on first look, not a working prototype wrapped around a database admin panel.

## Brand Personality

Precise, calm, technical-confident. Reads as familiar to anyone who has used a mainstream enterprise catalog — same information density, same conventions for tables/forms/panels — while its graph, lineage, and reasoning surfaces are distinctly better-looking than a generic CRUD admin screen. Voice: matter-of-fact, never cute; copy explains what a screen is for in one sentence, not marketing language.

## Anti-references

- Generic "AI SaaS" look: cream/sand body backgrounds, gradient-text headings, eyebrow labels on every section, oversized rounded cards, glassmorphism.
- A bare developer/admin tool: unstyled default form controls, inconsistent spacing, dense but disorganized panels, "it works but nobody designed it."
- Anything that looks like a rebuilt clone of a specific named competitor's visual identity (per this project's licensing rules, no palette or component styling may be copied from any reference product).

## Design Principles

1. **Density with hierarchy** — enterprise catalog users expect dense tables and many labels on screen; the discipline is grouping and spacing that density legibly, not removing it.
2. **One visual vocabulary everywhere** — the same button, same card, same table, same spacing scale on every one of the console's ~12 sections. A control that looks different in two places reads as a bug, not a feature.
3. **The graph is the hero, not the chrome around it** — canvases (Ontology Builder, Explorer, lineage) get maximum space; controls overlay the canvas rather than competing with it for a separate row.
4. **State-rich, restrained color** — the brand teal/navy pair is reserved for actions, selection, and status; everything else is neutral gray-scale tuned for WCAG AA contrast (already a documented constraint in this codebase — several fixes are on record for exactly this).
5. **Earned familiarity over novelty** — standard product-UI affordances (top bar + side nav, breadcrumbs, tabs, standard modals) done precisely, not reinvented for flavor.

## Accessibility & Inclusion

WCAG 2.1 AA is an existing, enforced constraint in this codebase (axe checks in CI, documented contrast fixes in `ui/src/theme.ts`). Any new color or component must be checked against the same bar: body text ≥4.5:1, large text ≥3:1, no color-only state signaling.
