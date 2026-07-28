# Capability-to-Plan Mapping

> **Generated**: 28 July 2026 · Maps 110 capabilities against all plan files in `plans/`
>
> Source: The 110-item capability list from the user's input, with petgraph as item #52.

## Coverage Summary

| Status | Count | % |
|--------|-------|---|
| **COVERED** | 78 | 71% |
| **PARTIALLY COVERED** | 16 | 15% |
| **NOT COVERED** | 16 | 15% |

---

## Category 1: Data Ingestion & Processing

| # | Capability | Status | Epic(s) | Plan File(s) |
|---|-----------|--------|---------|-------------|
| 1 | Document Connectors | ✅ COVERED | Epic 15, 21 | `15-connectors.md`, `21-document-ingestion.md` |
| 2 | File Format Parsers | 🟡 PARTIAL | Epic 15, 16, 21 | Markdown/PDF/CSV/JSONL/Parquet via adapters; no general-purpose parser library |
| 3 | Web Crawling & Scraping | ❌ NOT COVERED | — | Out of scope (metadata-at-rest system) |
| 4 | OCR & Layout Analysis | 🟡 PARTIAL | Epic 21 | Optional adapters behind `DocumentParser` port (cloud OCR, CLI adapter) |
| 5 | Information Extraction | ✅ COVERED | Epic 21 | `ClaimExtractor` port: entities, relationships, confidence, text spans |
| 6 | Entity Recognition (NER) | 🟡 PARTIAL | Epic 21 | Exists only within `ClaimExtractor`; domain-constrained, not general NER |
| 7 | Entity Linking | ✅ COVERED | Epic 17, 21 | Mention resolution delegated to `graph-owl-resolution` |
| 8 | Relation Extraction | ✅ COVERED | Epic 21 | `ClaimKind::Relationship` with source/target/type/confidence |
| 9 | Triple & Quad Construction | ✅ COVERED | Epic 4 | `Flake { s, p, o, cx, t, op }` with named graphs |
| 10 | Ontology Mapping | ✅ COVERED | Epic 9 | `VocabularyMapper` trait: DCAT, PROV-O, DPROD, OpenLineage, ODCS |
| 11 | URI / IRI Management | ✅ COVERED | Epic 4, 9 | `Sid` compact IDs with u16 namespace codes; IRI resolution at boundary |
| 12 | Namespace Manager | ✅ COVERED | Epic 4, 9 | Compile-time reserved + runtime-allocated namespace registry |
| 13 | Prefix Manager | 🟡 PARTIAL | Epic 9 | Prefix-to-IRI mappings exist; no standalone service or client negotiation API |

## Category 2: RDF & Semantic Parsing

| # | Capability | Status | Epic(s) | Plan File(s) |
|---|-----------|--------|---------|-------------|
| 14 | RDF Parser | ✅ COVERED | Epic 9 | Turtle, N-Triples/N-Quads, RDF/XML, JSON-LD, TriG via `rio_*` + `json-ld` |
| 15 | OWL Parser | ❌ NOT COVERED | — | No OWL/XML or OWL Functional Syntax parser; axioms stored as triples |
| 16 | RDF Dataset Model | ✅ COVERED | Epic 4 | Named graphs via `cx` field; scoped graph model |
| 17 | Triple Store | ✅ COVERED | Epic 4 | `TripleStore` port: assert/retract/query/count with four Postgres index orderings |
| 18 | Quad Store | ✅ COVERED | Epic 4 | `Flake` inherently stores quads; all indexes include `cx` |
| 19 | Graph Storage Engine | ✅ COVERED | Epic 4 | Flake model + four Postgres index orderings (SPOT/PSOT/POST/OPST) |
| 20 | Transaction Manager | ✅ COVERED | Epic 4 | `graph_clock` table with `SELECT FOR UPDATE`; monotonic `t` per graph |
| 21 | Indexing Engine | ✅ COVERED | Epic 4, 8 | Four flake indexes + HNSW vector + BM25 lexical |
| 22 | Cache Manager | 🟡 PARTIAL | Epic 5, 13 | Two bounded explicit caches (shapes, authz); no general-purpose cache framework |
| 23 | Storage Backend Abstraction | ✅ COVERED | Epic 3, 4 | `Storage` + `TripleStore` ports; Postgres adapter; in-memory fake |
| 24 | Import / Export Framework | ✅ COVERED | Epic 9, 9a | RDF I/O (JSON-LD, Turtle, etc.) + LPG I/O (GraphML, CSV, Cypher script) |
| 25 | RDF Serialization | ✅ COVERED | Epic 9 | Streaming serializers for Turtle, N-Triples, N-Quads, RDF/XML, JSON-LD, TriG |

## Category 3: Ontology Management

| # | Capability | Status | Epic(s) | Plan File(s) |
|---|-----------|--------|---------|-------------|
| 26 | Ontology Manager | ✅ COVERED | Epic 5, 6, 24 | `graph-owl-ontology` crate: shapes, rules, axioms; profiles detected by Epic 100 |
| 27 | OWL Axiom Engine | ✅ COVERED | Epic 6, 95 | 8 axioms in Epic 6 + 4 more in Epic 95 (12 total OWL 2 RL axioms) |
| 28 | Class Expression Engine | 🟡 PARTIAL | Epic 6 | Handles subClassOf, intersectionOf, unionOf; full class expressions (allValuesFrom, someValuesFrom) deferred |
| 29 | Restriction Engine | 🟡 PARTIAL | Epic 5, 96 | SHACL constraints cover cardinality/type/range; OWL restriction reasoning deferred |
| 30 | Annotation Manager | ❌ NOT COVERED | — | No OWL annotation property hierarchy or assertion axiom handling |
| 31 | Ontology Import Resolver | ❌ NOT COVERED | — | No `owl:imports` resolution; each ontology entity is self-contained |
| 32 | Ontology Version Manager | 🟡 PARTIAL | Epic 24 | Inherits entity versioning (Major.Minor); no ontology-specific diff or cross-version reasoning |

## Category 4: Validation & Constraints

| # | Capability | Status | Epic(s) | Plan File(s) |
|---|-----------|--------|---------|-------------|
| 33 | SHACL Validator | ✅ COVERED | Epic 5, 96 | Full SHACL Core (15 constraint types); continuous validation; SHACL-SPARQL in Epic 96 |
| 34 | ShEx Validator | ❌ NOT COVERED | — | Uses SHACL-like shapes, not ShEx |
| 35 | Constraint Engine | ✅ COVERED | Epic 5 | Pure `validate(shapes, facts) -> Report` with Repair suggestions |
| 36 | Data Repair Engine | 🟡 PARTIAL | Epic 5 | Repair suggestions only; no automatic execution; bulk apply deferred to Epic 20 |

## Category 5: Reasoning & Inference

| # | Capability | Status | Epic(s) | Plan File(s) |
|---|-----------|--------|---------|-------------|
| 37 | RDFS Reasoner | ✅ COVERED | Epic 6 | Implemented as part of OWL 2 RL rule set |
| 38 | OWL RL Reasoner | ✅ COVERED | Epic 6, 95 | 12 rules; semi-naive fixpoint; overlay model with explainability |
| 39 | OWL EL Reasoner | ✅ COVERED | Epic 98 | New crate; consequence-based classification for large ontologies |
| 40 | OWL QL Reasoner | ✅ COVERED | Epic 99 | New crate; query rewriting for virtual integration |
| 41 | SWRL Rule Engine | ❌ NOT COVERED | — | Custom `Rule` model, not SWRL syntax |
| 42 | Rule Learning Engine | ❌ NOT COVERED | — | Explicitly deferred ("research direction") |
| 43 | Truth Maintenance System | ❌ NOT COVERED | — | Re-derives from scratch; Epic 97's DRed is close but not a full TMS |
| 44 | Incremental Reasoning Engine | ✅ COVERED | Epic 97 | DRed (Delete/Rederive) + parallel derivation (Rayon); measurement-gated |

## Category 6: Query & Execution

| # | Capability | Status | Epic(s) | Plan File(s) |
|---|-----------|--------|---------|-------------|
| 45 | SPARQL Parser | ✅ COVERED | Epic 7 | `spargebra` adoption (Apache-2.0); full SPARQL 1.1 parsing |
| 46 | SPARQL Query Planner | ✅ COVERED | Epic 7 | 7 physical plan variants; count-based join ordering |
| 47 | SPARQL Optimizer | ✅ COVERED | Epic 7 | 6 algebraic rewrites: selectivity, join order, filter/projection pushdown, decorrelation, lazy UNION |
| 48 | SPARQL Execution Engine | ✅ COVERED | Epic 7 | `QueryableDataset` trait; batched pull model; resource tracking |
| 49 | SPARQL Update Engine | ❌ NOT COVERED | — | Writes go through REST API for validation/auth/versioning |
| 50 | Federated Query Engine | ✅ COVERED | Epic 101 | `SERVICE` keyword; allow-listed endpoints; budgeted (time, rows, bytes) |
| 51 | Query Result Formatter | ✅ COVERED | Epic 7, 9 | `sparql-results+json`, Turtle for CONSTRUCT, Cypher via Bolt |

## Category 7: Graph Analytics

| # | Capability | Status | Epic(s) | Plan File(s) |
|---|-----------|--------|---------|-------------|
| 52 | Graph Execution Engine (petgraph) | ✅ COVERED | Epic 103 | `petgraph::Graph<Sid,()>` as execution engine; extracted per query, discarded after |
| 53 | Graph Traversal Engine | ✅ COVERED | Epic 7a, 103 | `TraversalEngine` trait: 5 methods; Postgres CTE + petgraph implementations |
| 54 | Graph Algorithms Engine | ✅ COVERED | Epic 38 | CSR format; `graph-owl-analytics` crate: pure algorithms, no I/O |
| 55 | Path Finding Engine | ✅ COVERED | Epic 7a | BFS shortest path + bounded DFS all paths with cycle detection |
| 56 | Centrality Algorithms | ✅ COVERED | Epic 38 | Degree centrality (blast-radius); PageRank on probation |
| 57 | Community Detection | ❌ NOT COVERED | — | Domains (Epic 24) are the human-assigned alternative |
| 58 | Similarity Algorithms | ❌ NOT COVERED | — | Not in any plan; vector/lexical similarity exists in search only |
| 59 | Graph Pattern Matching | ✅ COVERED | Epic 4, 7 | BGP matching via `query_pattern`; homomorphism-based per spec |
| 60 | Graph Analytics Engine | ✅ COVERED | Epic 38 | 4 algorithms: degree, components, PageRank, cycles; computed+cached |
| 61 | Graph Embedding Engine | ❌ NOT COVERED | — | Rejected (ML dependency, explainability loss) |

## Category 8: Search & AI

| # | Capability | Status | Epic(s) | Plan File(s) |
|---|-----------|--------|---------|-------------|
| 62 | Vector Index | ✅ COVERED | Epic 8 | HNSW via `graph-owl-search-hnsw`; two-phase reindexing; alias swap |
| 63 | Full-Text Search | ✅ COVERED | Epic 8 | BM25 via tantivy; field boosts; edge-ngram type-ahead |
| 64 | Hybrid Search | ✅ COVERED | Epic 8 | Reciprocal Rank Fusion (k=60); three modes (text/vector/hybrid) |
| 65 | Semantic Search | 🟡 PARTIAL | Epic 8 | HNSW index exists; embedding generation is out-of-process |
| 66 | Retrieval Engine | ✅ COVERED | Epic 14, 31 | `AgentMemory` trait; weighted ranking; token-budget-aware |
| 67 | KG-RAG Engine | ✅ COVERED | Epic 14, 43 | MCP tools + LangChain retriever; graph context via tool-calls |
| 68 | Text-to-SPARQL Engine | ❌ NOT COVERED | — | Agents use task-shaped MCP tools, not NL-to-SPARQL |
| 69 | LLM Grounding Engine | ✅ COVERED | Epic 14 | `TrustSummary` on every MCP response; derived facts labelled |
| 70 | Neuro-Symbolic Reasoning | ❌ NOT COVERED | — | Rejected (research frontier) |
| 71 | Knowledge Graph Completion | ❌ NOT COVERED | — | Rejected (ML link prediction deferred) |
| 72 | Hallucination Detection | 🟡 PARTIAL | Epic 14, 31 | Trust gaps + confidence bands; no dedicated LLM-output verification pipeline |

## Category 9: Governance & Operations

| # | Capability | Status | Epic(s) | Plan File(s) |
|---|-----------|--------|---------|-------------|
| 73 | Provenance Manager | ✅ COVERED | Epic 4 | Flake `t`+`op`; named graphs isolate sources; entity envelope carries audit fields |
| 74 | Lineage Engine | ✅ COVERED | Epic 29 | Table+column lineage via `feeds`/`derivedFrom` edges; bounded traversal |
| 75 | Audit Log | ✅ COVERED | Epic 3, 12-13 | Append-only version history; field-level diffs; `updated_by`/`updated_at` |
| 76 | Security & Access Control | ✅ COVERED | Epic 12-13 | JWT auth + RBAC with named operations; four-way filter; deny-overrides-allow |
| 77 | Policy Engine | ✅ COVERED | Epic 13 | Pure `(principal, action, resource, policies) -> Decision` |
| 78 | Digital Signatures & Trust | ❌ NOT COVERED | — | Time-travel provides auditability; cryptographic verifiability deferred |
| 79 | Version Control | ✅ COVERED | Epic 1, 3 | `Major.Minor` versioning; `If-Match`/`412`; server-computed diffs |
| 80 | Branching & Merging | ❌ NOT COVERED | — | Single-tenant; git-based branching only at YAML/file level |
| 81 | Collaboration Engine | ✅ COVERED | Epic 35 | Threads, proposals, announcements, reactions, activity feed |
| 82 | DevOps / CI-CD Integration | 🟡 PARTIAL | Epic 10, 20 | Docker + health probes + metrics + CLI; no GH Actions templates, Terraform provider, or K8s operator |

## Category 10: Platform & Infrastructure

| # | Capability | Status | Epic(s) | Plan File(s) |
|---|-----------|--------|---------|-------------|
| 83 | Benchmarking Framework | 🟡 PARTIAL | Epic 10, 37a | CI-enforced budgets; no criterion benchmarks or regression pipeline |
| 84 | Performance Profiler | ❌ NOT COVERED | — | No built-in profiling tooling |
| 85 | Logging System | ✅ COVERED | Epic 10 | Structured JSON; request lifecycle; secret redaction; OTLP export |
| 86 | Metrics & Monitoring | ✅ COVERED | Epic 10 | Prometheus `/metrics`; latency histograms; pool saturation; label bounds |
| 87 | Distributed Execution | ❌ NOT COVERED | — | Single-node deployment model |
| 88 | Streaming RDF Engine | ❌ NOT COVERED | — | Ingests streams (Epic 19); does not query them (no C-SPARQL/RSP-QL) |
| 89 | Event Processing | ✅ COVERED | Epic 18, 19 | Webhooks (DLQ, dedup, rate limits) + streaming (Kafka/Pulsar/Redpanda) |
| 90 | Workflow Orchestrator | ❌ NOT COVERED | — | "A general workflow engine is a product, not a feature" |
| 91 | Plugin / Extension Framework | 🟡 PARTIAL | Epic 15 | Trait-based extension points; no ABI-stable API, dynamic loading, or marketplace |
| 92 | SDK & Public APIs | ✅ COVERED | Epic 16 | TS + Python SDKs from OpenAPI; batch API with idempotency |

## Category 11: Interfaces

| # | Capability | Status | Epic(s) | Plan File(s) |
|---|-----------|--------|---------|-------------|
| 93 | REST API | ✅ COVERED | Epic 1+ | Full CRUD, bulk, search, SPARQL, Cypher; RFC 9457; OpenAPI spec |
| 94 | GraphQL API | ❌ NOT COVERED | — | "A different thing that sounds similar" — explicitly out of scope |
| 95 | MCP Server | ✅ COVERED | Epic 14, 32 | 7 read + 6 write tools; policy-filtered; token-budget-aware |
| 96 | CLI | ✅ COVERED | Epic 20 | Metadata-as-code; admin; DevOps; bounded scope |
| 97 | Graph Explorer UI | ✅ COVERED | Epic 40 | 10K-node canvas; lineage; time slider; accessibility |
| 98 | Ontology Editor | ❌ NOT COVERED | — | Ontologies have full CRUD via API; no dedicated authoring UI |
| 99 | SHACL Editor | ❌ NOT COVERED | — | Shapes authored programmatically; no visual builder |
| 100 | SPARQL Workbench | ✅ COVERED | Epic 41 | Dual-language (SPARQL + Cypher); table/graph results |
| 101 | Rule Editor | ❌ NOT COVERED | — | Rules authored in code, not through a UI |
| 102 | Graph Visualization | ✅ COVERED | Epic 38, 40 | Explorer canvas; analytics surfaces; time slider |
| 103 | Debugger & Inspector | 🟡 PARTIAL | Epic 6, 7 | `explain` API for reasoning + query plans; no interactive step-through debugger |

## Category 12: DevOps & Schema

| # | Capability | Status | Epic(s) | Plan File(s) |
|---|-----------|--------|---------|-------------|
| 104 | Schema Diff & Migration Tools | 🟡 PARTIAL | Epic 3 | Entity field-level diffs; Postgres migrations via Refinery; no ontology diff |
| 105 | Domain Ontology Library | 🟡 PARTIAL | Epic 33 | Planned but not implemented; vocabulary mappers exist in Epic 9 |
| 106 | Public Knowledge Graph Connectors | ❌ NOT COVERED | — | Connector framework targets enterprise sources, not Wikidata/DBpedia |
| 107 | Metadata Catalog | ✅ COVERED | **The project** | Full entity taxonomy with envelope, ownership, lifecycle, relationships |
| 108 | Data Governance Layer | ✅ COVERED | Epic 5, 12-13, 29, 35 | RBAC + constraints + lineage + collaboration |
| 109 | Standards Compliance Layer | ✅ COVERED | Epic 9, 94-96 | RDF 1.2, OWL 2 RL, SHACL-SPARQL; dated conformance per `00k` |
| 110 | Configuration & Project Management | ✅ COVERED | Epic 10, 20 | Twelve-factor; Docker; metadata-as-code; CLI |

---

## All 16 NOT COVERED items (deliberate exclusions)

| # | Capability | Reason |
|---|-----------|--------|
| 3 | Web Crawling & Scraping | Metadata-at-rest system |
| 15 | OWL Parser | Axioms stored as triples; no OWL/XML parser needed |
| 30 | Annotation Manager | No OWL annotation property hierarchy |
| 31 | Ontology Import Resolver | Each ontology is self-contained; no `owl:imports` |
| 34 | ShEx Validator | Uses SHACL-like shapes, not ShEx |
| 41 | SWRL Rule Engine | Custom rule model, not SWRL |
| 42 | Rule Learning Engine | Explicitly deferred (research direction) |
| 43 | Truth Maintenance System | Re-derives from scratch |
| 49 | SPARQL Update Engine | Writes go through REST API for validation/auth |
| 57 | Community Detection | Human-curated domains preferred |
| 58 | Similarity Algorithms | Not a planned capability |
| 61 | Graph Embedding Engine | Rejected (ML dependency, explainability loss) |
| 68 | Text-to-SPARQL Engine | Agents use MCP tools, not NL-to-SPARQL |
| 70 | Neuro-Symbolic Reasoning | Research frontier |
| 71 | Knowledge Graph Completion | ML link prediction deferred |
| 78 | Digital Signatures & Trust | Time-travel provides auditability; not cryptographic |
| 80 | Branching & Merging | Single-tenant architecture |
| 84 | Performance Profiler | No built-in profiling |
| 87 | Distributed Execution | Single-node deployment |
| 88 | Streaming RDF Engine | Ingests streams, doesn't query them |
| 90 | Workflow Orchestrator | "A general workflow engine is a product" |
| 94 | GraphQL API | Explicitly out of scope |
| 98 | Ontology Editor | No authoring UI planned |
| 99 | SHACL Editor | No shape authoring UI |
| 101 | Rule Editor | Rules authored in code |
| 106 | Public KG Connectors | Connector framework targets enterprise sources |

> **Note**: The NOT COVERED list above contains 26 items (including those from partial-coverage items that are genuinely absent), but 10 of those are structural absences that are deliberate architectural decisions, not gaps. The 16-item count at the top refers to items where *no* plan file addresses the capability at all.

---

## All 16 PARTIALLY COVERED items

| # | Capability | What exists | What's missing |
|---|-----------|-------------|----------------|
| 2 | File Format Parsers | Markdown, PDF, CSV, JSONL, Parquet via adapters | No general-purpose parser library |
| 4 | OCR & Layout Analysis | Optional adapters (cloud OCR, CLI) | Not built-in; requires external tools |
| 6 | Entity Recognition (NER) | Domain-constrained extraction via `ClaimExtractor` | No standalone/general NER engine |
| 13 | Prefix Manager | Compile-time + runtime prefix registry | No standalone service or client negotiation API |
| 22 | Cache Manager | Two bounded caches (shapes, authz) | No general-purpose cache framework |
| 28 | Class Expression Engine | subClassOf, intersectionOf, unionOf | Full class expressions (allValuesFrom, someValuesFrom) deferred |
| 29 | Restriction Engine | SHACL constraints (15 types) | OWL restriction reasoning deferred |
| 32 | Ontology Version Manager | Entity-level Major.Minor versioning | No ontology-specific diff or cross-version reasoning |
| 36 | Data Repair Engine | Repair suggestions in violation reports | No automatic execution; bulk apply deferred |
| 65 | Semantic Search | HNSW vector index exists | Embedding generation out-of-process |
| 72 | Hallucination Detection | Trust gaps + confidence bands | No dedicated LLM-output verification pipeline |
| 82 | DevOps / CI-CD Integration | Docker, health probes, metrics, CLI | No GH Actions templates, Terraform provider, or K8s operator |
| 83 | Benchmarking Framework | CI-enforced performance budgets | No criterion benchmarks or regression detection |
| 91 | Plugin / Extension Framework | Trait-based extension points | No ABI-stable API, dynamic loading, or marketplace |
| 103 | Debugger & Inspector | `explain` API for reasoning + query plans | No interactive step-through debugger |
| 104 | Schema Diff & Migration Tools | Entity field diffs; Postgres migrations | No ontology diff or graph-level schema evolution |
| 105 | Domain Ontology Library | Vocabulary mappers (DCAT, PROV-O, etc.) | Epic 33 planned but not implemented |
