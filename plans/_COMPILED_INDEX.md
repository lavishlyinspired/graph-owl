# Plan Index: Status, Dependencies & Relationships

> Auto-compiled from all `plans/*.md`. Generated 28 Jul 2026.

---

## Standing Reference Documents

| File | Status | Binds | Key Role |
|------|--------|-------|----------|
| `00a-product-position.md` | Standing reference | Everything | Differentiators, budgets, what this competes on |
| `00b-architecture.md` | Standing reference | Everything | Layering, flake model, crate map, error model, decision log |
| `00c-domain-model.md` | Standing reference | Everything | Entities, envelope, FQN, triple projection |
| `00d-api-conventions.md` | Standing reference | API-surface epics | URL shape, status codes, pagination, concurrency |
| `00e-crate-architecture.md` | Standing reference | Crate decisions | Which crates exist/rejected, growth trigger |
| `00f-ui-architecture.md` | Standing reference | Console epics | Stack, budgets, two-renderer rule, non-negotiables |
| `00g-operations.md` | Standing reference | Deploy, data lifecycle | Migration/rollback, DR, retention, runbooks |
| `00h-ui-design-system.md` | Standing reference | UI epics | Design tokens, chrome, five patterns, screen inventory |
| `00i-licensing.md` | Binding | Every session | Clean-room rules, reference-implementation discipline |
| `00j-language-boundaries.md` | Standing decision | Multi-language | Rust in-binary, Python out-of-process, consumers vs components |
| `00k-standards-conformance.md` | Standing reference | Epics 5, 6, 7, 9 | W3C subset conformance registry |
| `00l-build-vs-adopt.md` | Standing reference | Engine epics | Permissive-lib adoption decisions (spargebra, spareval, etc.) |
| `00m-capability-mapping.md` | Generated reference | All | 110-capability coverage map (78 covered, 16 partial, 16 missing) |
| `00n-large-ontology-reality.md` | Standing reference | Scale epics | Fork analysis: catalog vs knowledge-graph product; epic 98 triggered |
| `ROADMAP.md` | Active roadmap | Sequencing | 43 epics, 9 phases, ★ differentiators |
| `DEMOS.md` | Demo tracer | All | 12 demos, cumulative, Indian banking domain |

---

## Phase 1: Walking Skeleton (Epics 1–3) — Shipped

| Plan | Status | Depends On | Deferrals | Slices |
|------|--------|------------|-----------|--------|
| `01-catalog-core.md` | **Shipped** | None | MongoDB storage (Epic 34) | A–J |
| `02-relationships.md` | **Shipped** | 01 | — | A–C |
| `03-types.md` | **Shipped** | 02 | Sealed type system (Epic 99) | A–C |

## Phase 2: Engine (Epics 4–9a) — In Progress

| Plan | Status | Depends On | Deferrals | Slices |
|------|--------|------------|-----------|--------|
| `04-graph-engine.md` | **Building** | 03 | — | A–I (A–H shipped, I deferred) |
| `04a-engine-exhaustion.md` | Planned | 04 | — | Companion |
| `05-constraint-validation.md` | Planned | 04 | — | A–D |
| `06-engine-reasoning.md` | Planned | 04 | OWL 2 EL → Epic 98; RL-only for now | — |
| `07-sparql-query.md` | **Building** | 04, 09 | — | A–D (A–C shipped) |
| `07a-graph-traversal.md` | Planned | 04 | — | A–C (core shipped) |
| `07b-open-cypher.md` | Planned | 09a, 07 | — | — |
| `07c-lpg-sql-access.md` | Planned | 07b | — | — |
| `07d-gql-query.md` | Planned | 07b | — | — |
| `08-authorization.md` | Planned | 03 | — | A–D |
| `09-rdf-io.md` | Planned | 04 | — | — |
| `09a-lpg-interchange.md` | Planned | 04 | — | — |

## Phase 3: Operations & Security (Epics 10–13) — Shipped

| Plan | Status | Depends On | Deferrals | Slices |
|------|--------|------------|-----------|--------|
| `10-observability.md` | **Shipped** | None | Cross-port spans deferred to v2 | — |
| `11-soft-delete.md` | **Shipped** | 02 | Recovery → Epic 23 | — |
| `12-authentication.md` | **Planned** | 03 | JWKS rotation, OIDC/PKCE + console login | — |
| `13-abac.md` | **Planned** | 03, 08 | Policy cache deferred | — |

## Phase 4: Agent Surface (Epics 14, 31–33) — Planned

| Plan | Status | Depends On | Notes |
|------|--------|------------|-------|
| `14-mcp.md` | Planned | 04, 07, 13 | MCP server, Rust SDK (`rmcp`) |
| `31-agent-memory.md` | Planned | 14 | — |
| `32-agent-writeback.md` | Planned | — | — |
| `33-agent-eval.md` | Planned | — | ★ Differentiator |

## Phase 5: Ingestion (Epics 15–21) — Planned

| Plan | Status | Depends On | Notes |
|------|--------|------------|-------|
| `15-connectors.md` | **Building** | 02 | Scheduled runs deferred (Demo 1 gap) |
| `16-search.md` | Planned | 04 | — |
| `17-resolution.md` | Planned | 16, 15 | Entity linking |
| `18-column-propagation.md` | Planned | 03 | — |
| `19-data-quality.md` | Planned | 06 | — |
| `20-classification.md` | Planned | 06, 03 | — |
| `21-document-ingestion.md` | Planned | 15, 16 | OCR deferred; NER behind port per `00j` |

## Phase 6: Business Meaning (Epics 22–30) — Planned

| Plan | Status | Depends On | Notes |
|------|--------|------------|-------|
| `22-business-terms.md` | Planned | 06 | — |
| `23-certifications.md` | Planned | 11, 06 | — |
| `24-lifecycle.md` | Planned | 06 | — |
| `25-masking.md` | Planned | 13 | Column-level masking deferred |
| `26-sla.md` | Planned | 06 | — |
| `27-cost.md` | Planned | 15 | — |
| `28-external-refs.md` | Planned | 17, 09 | — |
| `29-lineage.md` | Planned | 04 | Blocks console lineage DAG (Demo 3 gap) |
| `30-export.md` | Planned | 09 | — |

## Phase 7: Property Graph (Epics 7b–d, 9a) — Planned

| Plan | Status | Depends On | Notes |
|------|--------|------------|-------|
| `07b-open-cypher.md` | Planned | 09a, 07 | — |
| `07c-lpg-sql-access.md` | Planned | 07b | — |
| `07d-gql-query.md` | Planned | 07b | — |
| `09a-lpg-interchange.md` | Planned | 04 | Lossy projection, not storage |

## Phase 8: Scale (Epics 34–38) — Planned

| Plan | Status | Depends On | Notes |
|------|--------|------------|-------|
| `34-mongodb-storage.md` | Planned | 03 | Deferred from Epic 1 |
| `35-plugins.md` | Planned | 14 | — |
| `36-reference-agent.md` | Planned | 14, 31, 32 | Python per `00j` |
| `37a-scale.md` | Planned | 04 | Partitioning, bulk load, performance budgets |
| `37b-scale-validation.md` | Planned | 37a, 05 | — |
| `37c-scale-authorization.md` | Planned | 37a, 13 | — |
| `38-analytics.md` | Planned | 37a | Not traversal; whole-graph structural significance |

## Phase 9: Console UI (Epics 39–42) — Planned

| Plan | Status | Depends On | Notes |
|------|--------|------------|-------|
| `39-console-auth.md` | Planned | 12, 02 | Paired with OIDC/PKCE |
| `40-console-explorer.md` | Planned | 04 | WebGL deferred to Demo 3; SVG gap; lineage DAG blocked on Epic 29 |
| `41-console-governance.md` | Planned | 05, 06 | — |
| `42-console-playbooks.md` | Planned | 03, 08 | — |

## Phase 10: Standards Depth (Epics 94–97) — Not Started

| Plan | Status | Depends On | Notes |
|------|--------|------------|-------|
| `94-rdf-reification.md` | Planned | 04 | Vocabulary change; reified edges already match RDF 1.2 shape |
| `95-shacl-shape-expressions.md` | Planned | 05 | — |
| `96-owl-axiom-support.md` | Planned | 06 | — |
| `97-dcat-provo-odcs.md` | Planned | 09 | — |

## Phase 11: Full Semantics (Epics 98–103) — Not Started

| Plan | Status | Depends On | Notes |
|------|--------|------------|-------|
| `98-owl-el-reasoning.md` | **Triggered** | 06 | Triggered by medical-ontology requirement in `00n` |
| `99-sealed-type-system.md` | Planned | 03, 98 | — |
| `100-profile-detection.md` | Planned | 98, 06 | Required once multiple OWL profiles exist |
| `101-federation.md` | Planned | 07 | — |
| `102-storage-split.md` | Planned | 04 | — |
| `103-tautology-detection.md` | Planned | 06 | — |

## Phase 12: Large Ontologies (Epic 104) — Not Started

| Plan | Status | Depends On | Notes |
|------|--------|------------|-------|
| `104-billion-triple-scale.md` | Planned | 37a, 98, 102 | Fork path per `00n` |

## Auxiliary Plans

| Plan | Status | Purpose |
|------|--------|---------|
| `43-console-discovery.md` | Planned | Epics 43 companion |
| `90-done-table-entity.md` | Archived | Walking skeleton Epic 1, kept as record |
| `91-done-relationships.md` | Archived | Relationship edge Epic 2, kept as record |
| `92-done-types.md` | Planned? | Types epic 3 record? |
| `93-graph-explorer-poc.md` | Planned | Overview POC for visualizer |

---

## Dependency Graph (Simplified)

```
01 ──► 02 ──► 03 ──► 04 ──┬─► 05 ──► 06 ──┬─► 07 ──┬─► 07b ──► 07c ──► 07d
                           │                │         └─► 09a
                           │                ├─► 08 ──► 13
                           │                ├─► 09
                           │                ├─► 10
                           │                └─► 11
                           │
                           ├─► 12 ──► 39
                           ├─► 14 ──┬─► 31
                           │        ├─► 32
                           │        ├─► 33 ★
                           │        └─► 35 ──► 36
                           ├─► 15 ──┬─► 16 ──► 17
                           │        ├─► 18
                           │        ├─► 21
                           │        └─► 27
                           ├─► 19 ──► 20 ──► 22 ──► 23 ──► 24 ──► 25 ──► 26
                           │                                  └─► 28
                           ├─► 29 ──► 30
                           ├─► 34 (deferred)
                           ├─► 37a ──┬─► 37b
                           │         ├─► 37c
                           │         └─► 38
                           ├─► 40
                           ├─► 41
                           ├─► 42
                           ├─► 94─► 95─► 96─► 97
                           ├─► 98 ──┬─► 99
                           │        ├─► 100
                           │        └─► 104
                           ├─► 101
                           └─► 102 ──► 103
```

---

## Deferrals & Exclusions

| Deferred Item | Documented In | Reason |
|--------------|---------------|--------|
| MongoDB storage backend | `01-catalog-core.md`, `34-mongodb-storage.md` | Not needed for Postgres-only launch |
| Sealed type system | `03-types.md`, `99-sealed-type-system.md` | Post-launch; adds immutability guarantees |
| OWL 2 EL reasoning | `06-engine-reasoning.md`, `98-owl-el-reasoning.md` | **Now triggered** by medical-ontology requirement |
| Web crawling/scraping | `00m-capability-mapping.md` | Out of scope (metadata-at-rest system) |
| General-purpose NER | `21-document-ingestion.md`, `00j` | Behind `DocumentParser` port; domain-constrained |
| Cross-port spans (observability) | `10-observability.md` | Deferred to v2 |
| Column-level masking | `25-masking.md` | Deferred |
| Teams & ownership inheritance | `DEMOS.md` | Demo 7 |
| Vector embeddings | `DEMOS.md`, `00j` | Out of process |
| Property graph as storage | `09a-lpg-interchange.md` | Lossy projection target, not storage location |
| Analytics (vs traversal) | `38-analytics.md` | Whole-graph, not bounded walk |
| MongoDB storage backend | `34-mongodb-storage.md` | Post-launch |

---

## Demo Status (from DEMOS.md)

| Demo | Theme | State |
|------|-------|-------|
| 1 | Source → browsable catalog (Epics 1, 2, 15, 39) | **Shipped** — gaps in OpenAPI gen, scheduled runs |
| 2 | Governed catalog (Epics 3, 8, 10, 11, 12, 13) | **Shipped** — gaps in JWKS, OIDC, console sign-in, policy cache |
| 3 ★ | Graph engine (Epics 4, 7, 7a, 40, 93) | **Mostly shipped** — SVG explorer gap, lineage DAG blocked |
| 4 | Reason & validate (Epics 5, 6, 41) | Not started |
| 5 ★ | Agent surface (Epics 14, 31, 32, 43) | Not started |
| 6 | Self-filling (Epics 16–21) | Not started |
| 7 | Business meaning (Epics 22–30, 42) | Not started |
| 8 | Property graph & interop (Epics 7b, 7c, 7d, 9, 9a) | Not started |
| 9 | Scale (Epics 33–38, 36, 37a–c) | Not started |
| 10 | Standards depth (Epics 94–97) | Not started |
| 11 | Full semantics (Epics 98–103) | Not started |
| 12 | Large ontologies (Epic 104 + recalibration) | Not started; fork decision taken |

**Key gaps in shipped demos** (from DEMOS.md):
- Demo 1: OpenAPI generated from code; scheduled run persistence; `source_hash` fingerprinting
- Demo 2: JWKS & key rotation; OIDC/PKCE + login; no decision cache (AuthZ); memory budget/admission control
- Demo 3: WebGL (SVG won't survive 10k nodes); React Flow + d3-dag lineage DAG (blocked on Epic 29)

---

## Coverage (from `00m-capability-mapping.md`)

| Status | Count | % |
|--------|-------|---|
| COVERED | 78 | 71% |
| PARTIALLY COVERED | 16 | 15% |
| NOT COVERED | 16 | 15% |

Top-level categories missing: web crawling/scraping, general NER, SLA alerting, cost forecasting, cross-port traces, custom plugins, property-graph export.

---

## ★ Differentiators (from ROADMAP.md)

| Epic | Description |
|------|-------------|
| 3 | Types |
| 4 | Graph engine |
| 7a | Graph traversal |
| 8 | Authorization |
| 33 | Agent evaluation |
| 39 | Console |
| 40 | Console explorer |

These are marked ★ and described as "not optional polish" — cutting one is a positioning decision, not scope trimming.

---

## Explicitly Not Implemented (from various plans)

- Web crawling/scraping
- Full-text search (→ Epic 16 search index)
- General NER (domain-constrained only)
- Property graph as primary storage
- MongoDB storage (Epic 34, deferred)
- Full OWL 2 DL (RL only; EL triggered per `00n`)
- Analytics (Epic 38, separate from traversal)
- Plugins (Epic 35, future)
