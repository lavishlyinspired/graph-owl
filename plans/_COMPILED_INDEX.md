# Plan Index

> **Generated** by `scripts/compile-plan-index.py`. Do not hand-edit —
> every column is read from the plan's own `**Status**` /
> `**Depends on**` / `**Unblocks**` header, so the index cannot
> disagree with the file it points at. Re-run it after adding or
> restatusing a plan.

Rebuilt 28 July 2026, replacing a hand-written index in which **60 of
the 86 files named did not exist** and the epic numbering was a
different scheme entirely (`08-authorization.md` when Epic 8 is
Search; `16-search.md` when Epic 16 is ingestion). An index that
misroutes is worse than none, because it gets consulted instead of
`ls`.

**79 plan documents.** `DEMOS.md` remains the authority on
what is *built* — this file is the authority on what *exists* and how
the documents relate.

## Standing reference

| File | Role |
|---|---|
| [`00a-product-position.md`](00a-product-position.md) | graph-owl — Product Position |
| [`00b-architecture.md`](00b-architecture.md) | graph-owl — Architecture |
| [`00c-domain-model.md`](00c-domain-model.md) | graph-owl — Domain Model |
| [`00d-api-conventions.md`](00d-api-conventions.md) | graph-owl — API Conventions |
| [`00e-crate-architecture.md`](00e-crate-architecture.md) | Plan: Crate Architecture |
| [`00f-ui-architecture.md`](00f-ui-architecture.md) | Web Console Architecture |
| [`00g-operations.md`](00g-operations.md) | graph-owl — Operations |
| [`00h-ui-design-system.md`](00h-ui-design-system.md) | graph-owl — Console Design System |
| [`00i-licensing.md`](00i-licensing.md) | graph-owl — Licensing & Clean-Room Rules |
| [`00j-language-boundaries.md`](00j-language-boundaries.md) | graph-owl — Language Boundaries |
| [`00k-standards-conformance.md`](00k-standards-conformance.md) | 00k — Standards Conformance |
| [`00l-build-vs-adopt.md`](00l-build-vs-adopt.md) | 00l — Build vs Adopt |
| [`00m-capability-mapping.md`](00m-capability-mapping.md) | Capability-to-Plan Mapping |
| [`00n-large-ontology-reality.md`](00n-large-ontology-reality.md) | 00n — Large Ontologies at Scale: an honest assessment |

## Sequencing

| File | Role |
|---|---|
| [`DEMOS.md`](DEMOS.md) | graph-owl — Demo Tracer |
| [`ROADMAP.md`](ROADMAP.md) | graph-owl — End-to-End Roadmap |

## Epic plans

| File | Epic | Status | Depends on |
|---|---|---|---|
| [`01-api-conventions.md`](01-api-conventions.md) | 1 | **Shipped** — slices A–J. The contract is generated from a single route table, served at `/openapi.json`, committed, drift-guarded and validated as O… | nothing |
| [`02-entity-hierarchy.md`](02-entity-hierarchy.md) | 2 | **Shipped** — service → database → schema → table → column, 34 assets from the connector. Demo 1 | Epic 1 (conventions, relationship taxonomy) |
| [`03-versioning.md`](03-versioning.md) | 3 | **Shipped** — envelope, history, `If-Match`/412, and Slice J's `EventSink` with create / update / soft-delete / restore all announced. Demo 2. `HardD… | Epic 2 (four entity types to apply the envelope to) |
| [`04-engine-triples.md`](04-engine-triples.md) | 4 | Slices A–H complete, with two named carry-overs — cardinality and value type are recorded per predicate but not *enforced* on write (both are constra… | Epic 3 (four entity types with an envelope to project) |
| [`05-engine-constraints.md`](05-engine-constraints.md) | 5 | Not started | Epic 4 (triples to validate) |
| [`06-engine-reasoning.md`](06-engine-reasoning.md) | 6 | **In progress** — Slice A shipped 28 Jul 2026 (eight axioms, fixpoint, dedup). B's fixpoint/dedup landed with it because a symmetric property does no… | Epic 4 (triples), Epic 5 (ontology types) |
| [`07-engine-query.md`](07-engine-query.md) | 7 | **In progress** — slices A–C shipped (SPARQL over flakes, HTTP surface, pattern pushdown). `sparopt` not in the path; triple-term patterns are Epic 9… | Epic 4 (triples), Epic 13 (authorization to compile into queries), **Epic 7a** (property-… |
| [`07a-engine-traversal.md`](07a-engine-traversal.md) | 7a | **In progress** — core shipped (slices A–E): frontier primitive, `neighbours`/`subgraph`, `shortest_path`, `all_paths`, `detect_cycles` | Epic 4 (triples, SPOT/POST/OPST indexes) |
| [`07b-engine-cypher.md`](07b-engine-cypher.md) | 7b | Not started — **scheduled** (was optional; see the status change below) | Epic 7 (SPARQL plan is the lowering target), Epic 7a (traversal), **Epic 7c (LPG projecti… |
| [`07c-engine-lpg.md`](07c-engine-lpg.md) | 7c | Not started | Epic 4 (flakes), Epic 1 (relationship taxonomy) |
| [`07d-engine-bolt.md`](07d-engine-bolt.md) | 7d | Not started | Epic 7b (Cypher), Epic 7c (LPG projection), Epic 12 (auth), Epic 13 (authorization) |
| [`08-engine-search.md`](08-engine-search.md) | 8 | **In progress** — Slice A shipped over Postgres full-text search: weighted `tsvector`, GIN, prefix-matched conjunctive terms, `ts_rank_cd` ordering,… | Epic 3 (change events to subscribe to), Epic 2 (FQNs to rank on), Epic 25 (tags, for face… |
| [`09-engine-rdf-io.md`](09-engine-rdf-io.md) | 9 | Not started | Epic 4 (triples to serialize), Epic 7 (CONSTRUCT produces Turtle) |
| [`09a-lpg-interchange.md`](09a-lpg-interchange.md) | 9a | Not started | Epic 7c (LPG projection), Epic 9 (RDF I/O — shares the streaming-serializer shape) |
| [`10-operability.md`](10-operability.md) | 10 | **In progress** — Slices A, B, C, D and E shipped into Demo 2: typed config, three-valued readiness, graceful drain, structured logging with request-… | Epic 1 (a server, an error model, and a contract to instrument) |
| [`100-profile-detection-and-routing.md`](100-profile-detection-and-routing.md) | 100 | Not started — **prerequisite for Epics 98 and 99, and now load-bearing**. With SNOMED (EL), DBpedia (QL) and FIBO (constructs outside RL) all in scop… | Epic 6 (RL engine), Epic 24 (ontologies as entities) |
| [`101-sparql-federation.md`](101-sparql-federation.md) | 101 | Not started — **scheduled** | Epic 7 (algebra and executor), Epic 13 (authorization) |
| [`102-read-write-partitions.md`](102-read-write-partitions.md) | 102 | Not started — **planned, entry condition is a measurement** | Epic 4 (the flake table), Epic 37a (the measurement) |
| [`103-in-process-traversal.md`](103-in-process-traversal.md) | 103 | Not started — **entry condition is a measurement (Epic 37a)** | Epic 7a (the `TraversalEngine` port), Epic 37a (the trigger) |
| [`104-ontology-alignment.md`](104-ontology-alignment.md) | 104 | Not started — **new, 28 July 2026**. Created because `00n-large-ontology-reality.md` §2.5 found it genuinely uncovered | Epic 33 (ontology packs — supplies the vocabularies), Epic 100 (profile detection), Epic… |
| [`11-people-and-ownership.md`](11-people-and-ownership.md) | 11 | **In progress** — shipped into Demo 2 | Epic 3 (envelope carries `owners`) |
| [`12-13-security.md`](12-13-security.md) | 12–13 | **In progress** — OIDC/PKCE against Auth0 with server-side JWKS RS256 shipped, reviewed and hardened (Slices A–C, and B's key rotation is what JWKS g… | Epic 11 (`Principal` seam), Epic 11 (users and teams to attach roles to) |
| [`14-mcp-activation.md`](14-mcp-activation.md) | 14 | Not started | Epic 7a (subgraph retrieval for agent context), Epic 13 (authorization — **hard gate**),… |
| [`15-connectors.md`](15-connectors.md) | 15 | **In progress** — Postgres connector and deletion detection shipped (Demo 1) | Epic 2 (hierarchy to populate), Epic 3 (versioning to make re-runs observable) |
| [`16-ingestion-apis.md`](16-ingestion-apis.md) | 16 | Not started | Epic 1 (contract), Epic 15 (upsert semantics) |
| [`17-entity-resolution.md`](17-entity-resolution.md) | 17 | Not started | Epic 4 (`sameAs` in the graph), Epic 15 + 16 (two write paths make this necessary) |
| [`18-inbound-events.md`](18-inbound-events.md) | 18 | Not started | Epic 16 (ingestion contract), Epic 17 (resolution, so pushes do not duplicate) |
| [`19-streaming.md`](19-streaming.md) | 19 | Not started | Epic 16 (ingestion contract), Epic 18 (dedup and ordering machinery) |
| [`20-metadata-as-code.md`](20-metadata-as-code.md) | 20 | Not started | Epic 15 (idempotent upsert and reconciliation machinery) |
| [`21-document-ingestion.md`](21-document-ingestion.md) | 21 | Not started | Epic 16 (ingestion), Epic 17 (mention resolution) |
| [`22-custom-properties.md`](22-custom-properties.md) | 22 | Not started | Epic 3 (the envelope's `extension` field) |
| [`23-domains.md`](23-domains.md) | 23 | Not started | Epic 11 (domains and products are owned) |
| [`24-business-semantics.md`](24-business-semantics.md) | 24 | Not started | Epic 2 (FQN derivation and the hierarchy terms attach to), Epic 11 (term reviewers), Epic… |
| [`25-classification.md`](25-classification.md) | 25 | Not started | Epic 3 (envelope carries `tags`), Epic 11 (term reviewers are users) |
| [`26-lifecycle-certification.md`](26-lifecycle-certification.md) | 26 | Not started | Epic 11 (issuers are principals), Epic 24 (metrics are certifiable) |
| [`27-contracts.md`](27-contracts.md) | 27 | Not started | Epic 2 (schemas to guarantee), Epic 3 (version diffs detect breakage) |
| [`28-usage.md`](28-usage.md) | 28 | Not started | Epic 16 (push ingestion), Epic 11 (consumers are principals) |
| [`29-lineage.md`](29-lineage.md) | 29 | Not started | Epic 15 (connectors assert lineage), Epic 2 (columns for column-level lineage), **Epic 7a… |
| [`30-quality-results.md`](30-quality-results.md) | 30 | Not started | Epic 29 (lineage, for propagating trust signals) |
| [`31-memory.md`](31-memory.md) | 31 | Not started | Epic 3 (envelope), Epic 11 (people), Epic 14 (MCP surface to serve it) |
| [`32-agent-capabilities.md`](32-agent-capabilities.md) | 32 | Not started | Epic 14 (read surface, validated by real usage), Epic 31 (memory to write into) |
| [`33-ontology-packs.md`](33-ontology-packs.md) | 33 | Not started | Epic 24 (glossary and taxonomy model), Epic 9 (standards import) |
| [`34-entity-expansion.md`](34-entity-expansion.md) | 34 | Not started | Epic 8 (each new type indexes for free — the property being demonstrated), Epic 3 (envelo… |
| [`35-collaboration.md`](35-collaboration.md) | 35 | Not started | Epic 11 (users), Epic 12 (real identity on posts) |
| [`36-reference-apps.md`](36-reference-apps.md) | 36 | Not started | Epic 14 (MCP), Epic 16 (SDKs), Epic 29 (graph API) |
| [`37a-scale.md`](37a-scale.md) | 37a | Not started | Epic 34 (a realistic entity mix to generate) |
| [`37b-portability.md`](37b-portability.md) | 37b | Not started | Epic 3 (history is part of what must survive) |
| [`37c-embeddable.md`](37c-embeddable.md) | 37c | Not started | Epic 1 (stable contract); benefits from Epic 34 (wide surface to validate against) |
| [`38-graph-analytics.md`](38-graph-analytics.md) | 38 | Not started — **narrow scope, and a deliberate reversal** | Epic 7a (traversal), Epic 4 (flakes), Epic 28 (usage signals, for comparison) |
| [`39-ui-foundation.md`](39-ui-foundation.md) | 39 | **In progress** — shell, search, entity page and time control shipped; Slice E's trust components and the base-direction primitive still open | Epic 1 (API conventions + OpenAPI), Epic 8 (search), Epic 12 (authn), Epic 13 (authz) |
| [`40-ui-graph-explorer.md`](40-ui-graph-explorer.md) | 40 | In progress — **differentiator**. Slices B and D shipped on the SVG canvas (28 Jul 2026); Slice A's model exists but not its `Sid`-derived identity;… | Epic 39 (console shell, trust components), Epic 7a (traversal), Epic 4 (flakes, time trav… |
| [`41-ui-workbench-governance.md`](41-ui-workbench-governance.md) | 41 | Not started | Epic 39 (shell, trust components), Epic 40 (graph model and renderers), Epic 5 (constrain… |
| [`42-ui-semantic-surfaces.md`](42-ui-semantic-surfaces.md) | 42 | Not started | Epic 39 (shell, patterns, trust components), Epic 41 (admin section, schema-driven forms) |
| [`43-framework-integrations.md`](43-framework-integrations.md) | 43 | Not started | Epic 14 (MCP), Epic 13 (authorization), Epic 31 (memory), Epic 16 (Python SDK), Epic 7 (q… |
| [`93-console-overview.md`](93-console-overview.md) | 93 | **In progress** — Overview shipped (Demo 3) | Epic 2 (hierarchy), Epic 3 (envelope), Epic 4 (graph), Epic 13 (authorization) |
| [`94-rdf12-alignment.md`](94-rdf12-alignment.md) | 94 | Not started | Epic 4 (flakes, reified relationships), Epic 9 (serialization) |
| [`95-owl-rl-completion.md`](95-owl-rl-completion.md) | 95 | Not started | Epic 6 (the eight rules and the fixpoint that runs them) |
| [`96-shacl-sparql.md`](96-shacl-sparql.md) | 96 | Not started — **blocked on the specification stabilising** | Epic 5 (SHACL Core), Epic 7 (SPARQL) |
| [`97-incremental-parallel-reasoning.md`](97-incremental-parallel-reasoning.md) | 97 | Not started — **the measurement now demands it (28 Jul 2026)**. A stated requirement of 10⁸–10⁹ triples makes `06`'s wholesale-replacement-per-run ar… | Epic 6 (semi-naive fixpoint), Epic 37a (the measurement) |
| [`98-owl-el-reasoning.md`](98-owl-el-reasoning.md) | 98 | Not started — **scheduled, and the trigger has fired (28 Jul 2026)**. SNOMED CT was named as a required ontology; OWL 2 EL is the profile it was desi… | Epic 6 (overlay, budgets, explainability), Epic 24 (ontologies as entities) |
| [`99-owl-ql-reasoning.md`](99-owl-ql-reasoning.md) | 99 | Not started — **scheduled, with a named consumer (28 Jul 2026)**. DBpedia is the QL shape — a vast ABox against a thin TBox, where materialising infe… | Epic 7 (query algebra to rewrite), Epic 6 (explanation contract) |

## Completed, kept as record

| File | Status |
|---|---|
| [`90-done-table-entity.md`](90-done-table-entity.md) | All slices (A-E) done — ready to close out |
| [`91-done-relationships.md`](91-done-relationships.md) | All slices (A-C) done — ready to close out |

## Plans with no `**Status**` header

None — every epic plan states its status.
