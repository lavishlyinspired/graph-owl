# Epic status

> **Generated** by `scripts/epic-status.py`. Do not hand-edit.
>
> Slice marks come from `DEMOS.md`, which is the authority on what is
> built (its rule 0). Dependencies come from each plan's own
> `**Depends on**` header. Nothing here is restated by hand, so this
> file cannot disagree with either source.

Replaces `_COMPILED_INDEX.md`, deleted 29 July 2026: it named 86 plan
files of which **60 did not exist**, and used a different epic
numbering (`08-authorization.md` when Epic 8 is Search).

`—` in **Slices** means the plan exists and `DEMOS.md` tracks no slice
marks for it yet, which is a different thing from zero of them done.

| Epic | Plan | State | Slices | Depends on |
|---|---|---|---|---|
| **1** | [`01-api-conventions.md`](01-api-conventions.md) | **Shipped** | 12/12 | nothing |
| **2** | [`02-entity-hierarchy.md`](02-entity-hierarchy.md) | In progress | 6/7 | Epic 1 (conventions, relationship taxonomy) |
| **3** | [`03-versioning.md`](03-versioning.md) | In progress | 11/13 (+1 partial) | Epic 2 (four entity types to apply the envelope to) |
| **4** | [`04-engine-triples.md`](04-engine-triples.md) | In progress | 8/11 (+1 partial) | Epic 3 (four entity types with an envelope to project) |
| **5** | [`05-engine-constraints.md`](05-engine-constraints.md) | In progress | 8/9 (+1 partial) | Epic 4 (triples to validate) |
| **6** | [`06-engine-reasoning.md`](06-engine-reasoning.md) | In progress | 7/8 (+1 partial) | Epic 4 (triples), Epic 5 (ontology types) |
| **7** | [`07-engine-query.md`](07-engine-query.md) | In progress | 8/9 | Epic 4 (triples), Epic 13 (authorization to compile into queries), **Epic 7a**… |
| **7a** | [`07a-engine-traversal.md`](07a-engine-traversal.md) | **Shipped** | 5/5 | Epic 4 (triples, SPOT/POST/OPST indexes) |
| **7b** | [`07b-engine-cypher.md`](07b-engine-cypher.md) | Not started | — | Epic 7 (SPARQL plan is the lowering target), Epic 7a (traversal), **Epic 7c (LP… |
| **7c** | [`07c-engine-lpg.md`](07c-engine-lpg.md) | Not started | — | Epic 4 (flakes), Epic 1 (relationship taxonomy) |
| **7d** | [`07d-engine-bolt.md`](07d-engine-bolt.md) | Not started | — | Epic 7b (Cypher), Epic 7c (LPG projection), Epic 12 (auth), Epic 13 (authorizat… |
| **8** | [`08-engine-search.md`](08-engine-search.md) | In progress | 6/8 | Epic 3 (change events to subscribe to), Epic 2 (FQNs to rank on), Epic 25 (tags… |
| **9** | [`09-engine-rdf-io.md`](09-engine-rdf-io.md) | Not started | — | Epic 4 (triples to serialize), Epic 7 (CONSTRUCT produces Turtle) |
| **9a** | [`09a-lpg-interchange.md`](09a-lpg-interchange.md) | Not started | — | Epic 7c (LPG projection), Epic 9 (RDF I/O — shares the streaming-serializer sha… |
| **10** | [`10-operability.md`](10-operability.md) | **Shipped** | 17/17 | Epic 1 (a server, an error model, and a contract to instrument) |
| **11** | [`11-people-and-ownership.md`](11-people-and-ownership.md) | **Shipped** | 2/2 | Epic 3 (envelope carries `owners`) |
| **12** | [`12-13-security.md`](12-13-security.md) | In progress | 13/15 | Epic 11 (`Principal` seam), Epic 11 (users and teams to attach roles to) |
| **13** | [`12-13-security.md`](12-13-security.md) | In progress | 9/10 | Epic 11 (`Principal` seam), Epic 11 (users and teams to attach roles to) |
| **14** | [`14-mcp-activation.md`](14-mcp-activation.md) | In progress | 2/7 (+1 partial) | Epic 7a (subgraph retrieval for agent context), Epic 13 (authorization — **hard… |
| **15** | [`15-connectors.md`](15-connectors.md) | In progress | 13/17 | Epic 2 (hierarchy to populate), Epic 3 (versioning to make re-runs observable) |
| **16** | [`16-ingestion-apis.md`](16-ingestion-apis.md) | Not started | 0/3 | Epic 1 (contract), Epic 15 (upsert semantics) |
| **17** | [`17-entity-resolution.md`](17-entity-resolution.md) | Not started | 0/3 | Epic 4 (`sameAs` in the graph), Epic 15 + 16 (two write paths make this necessa… |
| **18** | [`18-inbound-events.md`](18-inbound-events.md) | Not started | 0/2 | Epic 16 (ingestion contract), Epic 17 (resolution, so pushes do not duplicate) |
| **19** | [`19-streaming.md`](19-streaming.md) | Not started | 0/2 | Epic 16 (ingestion contract), Epic 18 (dedup and ordering machinery) |
| **20** | [`20-metadata-as-code.md`](20-metadata-as-code.md) | Not started | 0/2 | Epic 15 (idempotent upsert and reconciliation machinery) |
| **21** | [`21-document-ingestion.md`](21-document-ingestion.md) | Not started | 0/2 | Epic 16 (ingestion), Epic 17 (mention resolution) |
| **22** | [`22-custom-properties.md`](22-custom-properties.md) | Not started | 0/9 | Epic 3 (the envelope's `extension` field) |
| **23** | [`23-domains.md`](23-domains.md) | Not started | — | Epic 11 (domains and products are owned) |
| **24** | [`24-business-semantics.md`](24-business-semantics.md) | Not started | — | Epic 2 (FQN derivation and the hierarchy terms attach to), Epic 11 (term review… |
| **25** | [`25-classification.md`](25-classification.md) | Not started | — | Epic 3 (envelope carries `tags`), Epic 11 (term reviewers are users) |
| **26** | [`26-lifecycle-certification.md`](26-lifecycle-certification.md) | Not started | — | Epic 11 (issuers are principals), Epic 24 (metrics are certifiable) |
| **27** | [`27-contracts.md`](27-contracts.md) | Not started | — | Epic 2 (schemas to guarantee), Epic 3 (version diffs detect breakage) |
| **28** | [`28-usage.md`](28-usage.md) | Not started | — | Epic 16 (push ingestion), Epic 11 (consumers are principals) |
| **29** | [`29-lineage.md`](29-lineage.md) | In progress | 5/8 | Epic 15 (connectors assert lineage), Epic 2 (columns for column-level lineage),… |
| **30** | [`30-quality-results.md`](30-quality-results.md) | Not started | — | Epic 29 (lineage, for propagating trust signals) |
| **31** | [`31-memory.md`](31-memory.md) | Not started | 0/3 | Epic 3 (envelope), Epic 11 (people), Epic 14 (MCP surface to serve it) |
| **32** | [`32-agent-capabilities.md`](32-agent-capabilities.md) | Not started | 0/2 | Epic 14 (read surface, validated by real usage), Epic 31 (memory to write into) |
| **33** | [`33-ontology-packs.md`](33-ontology-packs.md) | Not started | 0/8 | Epic 24 (glossary and taxonomy model), Epic 9 (standards import) |
| **34** | [`34-entity-expansion.md`](34-entity-expansion.md) | Not started | — | Epic 8 (each new type indexes for free — the property being demonstrated), Epic… |
| **35** | [`35-collaboration.md`](35-collaboration.md) | Not started | — | Epic 11 (users), Epic 12 (real identity on posts) |
| **36** | [`36-reference-apps.md`](36-reference-apps.md) | Not started | — | Epic 14 (MCP), Epic 16 (SDKs), Epic 29 (graph API) |
| **37a** | [`37a-scale.md`](37a-scale.md) | Not started | — | Epic 34 (a realistic entity mix to generate) |
| **37b** | [`37b-portability.md`](37b-portability.md) | Not started | — | Epic 3 (history is part of what must survive) |
| **37c** | [`37c-embeddable.md`](37c-embeddable.md) | Not started | — | Epic 1 (stable contract); benefits from Epic 34 (wide surface to validate again… |
| **38** | [`38-graph-analytics.md`](38-graph-analytics.md) | Not started | — | Epic 7a (traversal), Epic 4 (flakes), Epic 28 (usage signals, for comparison) |
| **39** | [`39-ui-foundation.md`](39-ui-foundation.md) | In progress | 21/23 | Epic 1 (API conventions + OpenAPI), Epic 8 (search), Epic 12 (authn), Epic 13 (… |
| **40** | [`40-ui-graph-explorer.md`](40-ui-graph-explorer.md) | In progress | 10/11 | Epic 39 (console shell, trust components), Epic 7a (traversal), Epic 4 (flakes,… |
| **41** | [`41-ui-workbench-governance.md`](41-ui-workbench-governance.md) | In progress | 3/7 | Epic 39 (shell, trust components), Epic 40 (graph model and renderers), Epic 5… |
| **42** | [`42-ui-semantic-surfaces.md`](42-ui-semantic-surfaces.md) | Not started | 0/3 | Epic 39 (shell, patterns, trust components), Epic 41 (admin section, schema-dri… |
| **43** | [`43-framework-integrations.md`](43-framework-integrations.md) | Not started | 0/4 | Epic 14 (MCP), Epic 13 (authorization), Epic 31 (memory), Epic 16 (Python SDK),… |
| **93** | [`93-console-overview.md`](93-console-overview.md) | Not started | — | Epic 2 (hierarchy), Epic 3 (envelope), Epic 4 (graph), Epic 13 (authorization) |
| **94** | [`94-rdf12-alignment.md`](94-rdf12-alignment.md) | Not started | 0/7 | Epic 4 (flakes, reified relationships), Epic 9 (serialization) |
| **95** | [`95-owl-rl-completion.md`](95-owl-rl-completion.md) | Not started | 0/2 | Epic 6 (the eight rules and the fixpoint that runs them) |
| **96** | [`96-shacl-sparql.md`](96-shacl-sparql.md) | Not started | 0/2 | Epic 5 (SHACL Core), Epic 7 (SPARQL) |
| **97** | [`97-incremental-parallel-reasoning.md`](97-incremental-parallel-reasoning.md) | Not started | 0/2 | Epic 6 (semi-naive fixpoint), Epic 37a (the measurement) |
| **98** | [`98-owl-el-reasoning.md`](98-owl-el-reasoning.md) | Not started | 0/2 | Epic 6 (overlay, budgets, explainability), Epic 24 (ontologies as entities) |
| **99** | [`99-owl-ql-reasoning.md`](99-owl-ql-reasoning.md) | Not started | 0/2 | Epic 7 (query algebra to rewrite), Epic 6 (explanation contract) |
| **100** | [`100-profile-detection-and-routing.md`](100-profile-detection-and-routing.md) | Not started | 0/4 | Epic 6 (RL engine), Epic 24 (ontologies as entities) |
| **101** | [`101-sparql-federation.md`](101-sparql-federation.md) | Not started | 0/5 | Epic 7 (algebra and executor), Epic 13 (authorization) |
| **102** | [`102-read-write-partitions.md`](102-read-write-partitions.md) | Not started | 0/2 | Epic 4 (the flake table), Epic 37a (the measurement) |
| **103** | [`103-in-process-traversal.md`](103-in-process-traversal.md) | Not started | 0/2 | Epic 7a (the `TraversalEngine` port), Epic 37a (the trigger) |
| **104** | [`104-ontology-alignment.md`](104-ontology-alignment.md) | Not started | — | Epic 33 (ontology packs — supplies the vocabularies), Epic 100 (profile detecti… |

## Slices, per epic

### Epic 1 — API Conventions & Contract

- [x] A
- [x] B
- [x] C
- [x] D
- [x] E
- [x] F
- [x] G
- [x] H
- [x] I
- [x] J
- [x] K
- [x] It earned its keep immediately.

### Epic 2 — Entity Hierarchy & Columns

- [x] `Asset` + `AssetKind` for all five levels, one type not five
- [x] FQN derivation (`fqn::derive`, `fqn::child_of`, `parent`, `leaf`)
- [x] Containment rule in one place (`AssetKind::parent_kind`)
- [x] Hierarchy endpoints: roots, children, ancestors, search, stats
- [x] `PATCH /assets/{id}` and `DELETE /assets/{id}` (soft, cascading to the subtree) — shippe
- [x] Containment cascade characterised
- [ ] Non-database services (dashboard, pipeline, ML) → deferred to Epic 34

### Epic 3 — Envelope, Versioning, Soft Delete & Change Events

- [x] `EntityEnvelope` on every asset: version, `updatedAt`, `updatedBy`, `changeDescription`
- [x] Major/Minor version arithmetic; a no-op update produces no version
- [x] Field-level `ChangeDescription` diffs (added/updated/deleted); breaking-change classific
- [x] `PATCH /assets/{id}` with server-computed diffs
- [x] Soft delete cascading to the subtree, with restore; a connector re-run does not resurrec
- [x] `GET /assets/{id}/versions` — snapshot per version, newest first
- [x] Console: trust bar shows version and last editor; History tab with a field-level diff ti
- [x] `If-Match`/`412` optimistic concurrency — a stale precondition is refused and names the 
- [x] `EventSink` port + `ChangeEvent`
- [x] Emission wired into the facade
- [x] Create and re-ingest announce
- [ ] `HardDeleted` has no producer
- [~] Nothing subscribes.

### Epic 4 — Triple Storage & Time-Travel ★

- [x] `Flake` in `graph-owl-core`; ten pinned `FlakeValue` variants *(Slice A)*
- [x] Namespace code registry — constants allocated and range-tested; runtime namespaces persi
- [x] Four index orderings: SPOT, PSOT, POST, OPST — each verified by `EXPLAIN` naming the ind
- [x] `op = false` is a retraction, not a delete — assert/retract/assert/retract verified in b
- [x] Entity → flake projection — wired into both write paths; a catalogue run of the 124-asse
- [x] Reified relationships — each edge is a node of its own carrying `rdf:type`, both endpoin
- [x] As-of query API
- [x] Reconciliation and drift metric — drift computed by comparison rather than from a queue 
- [~] Runtime predicate registry — define/lookup/list, duplicate refused, core vocabulary seed
- [ ] `rdf:reifies` + triple terms → **Epic 94**
- [ ] Language-tag side table → **Epic 94**, and it needs three components not two: `rdf:dirLa

### Epic 5 — Constraint Validation

- [x] Shape and constraint types; four target kinds
- [x] Compile-once, evaluate-many
- [x] Shapes compile from graph triples
- [x] Continuous validation with violation reports, not write-time rejection
- [x] Severity classification; repair suggestions never auto-applied
- [x] `GET /validation/report`
- [x] All six target kinds
- [x] Seed shapes ship
- [~] Pending in this epic

### Epic 6 — Reasoning Overlay

- [x] Eight OWL 2 RL axioms as built-in rules
- [x] Semi-naive fixpoint, `CappedReason` on every limit
- [x] Derived facts in `graph:reasoning`, never persisted into the base
- [x] `GET /reasoning/explain` derivation chains
- [x] Reasoning is skipped on historical queries
- [x] Classification propagates along `feeds`, opt-in per classification
- [x] Lineage is projected into the graph
- [~] Pending in this epic

### Epic 7 — Graph Query — SPARQL ★

- [x] `QueryableDataset` over flakes
- [x] Pattern pushdown
- [x] `as_of` — the dataset is constructed at a transaction time *(Slice B)*
- [x] Authorization applied before the dataset exists, so the evaluator only ever sees permitt
- [x] Fact budget — **nothing adopted enforces budgets; this is ours**
- [x] Freshness stamping on the result (Epic 4 decision 8) *(Slice B)*
- [x] ~~Parser~~ — `spargebra`, full SPARQL 1
- [x] ~~Planner / execution~~ — `spareval` *(adopted)*
- [ ] `sparopt` is not yet in the path — pushdown reads the parsed algebra directly

### Epic 7a — Graph Traversal Engine

- [x] One frontier primitive (recursive CTE, one statement); `neighbours` and `subgraph` over 
- [x] Budgeted, cycle-safe, truncation always visible and farthest-first
- [x] Reified two-hop edges hidden — five logical edges reports distance five
- [x] `as_of` on every walk, so time-travelling traversal is free
- [x] `shortest_path`, `all_paths`, `detect_cycles` *(Slices C, D)* — deterministic tiebreak, 

### Epic 8 — Vector & Hybrid Search ★

- [x] Facets by kind and schema, computed over the **visible** set
- [x] Result counts consistent with authorization filtering
- [x] Full-text, stemmed, prefix-matched and ranked
- [x] Relevance is asserted as an order, not as membership
- [x] A user cannot reach the query language.
- [x] The rank key *is* the pagination cursor
- [ ] Decision 5's full relevance ordering (exact FQN > exact name > prefix > fuzzy > descript
- [ ] Snippets: a hit shows the asset, not the matched fragment of its description

### Epic 10 — Operability & Resource Budget ★

- [x] `/health` (checks nothing, so a dependency blip cannot restart-loop the fleet)
- [x] `/ready`, three-valued: required vs optional checks, `200 degraded` when auth is off
- [x] Graceful shutdown draining in-flight requests
- [x] Startup states its security posture — an accidentally-open server must not look identica
- [x] `BIND_ADDR` configurable
- [x] Structured logging and request-id propagation
- [x] `DATABASE_URL` is redacted wherever it is logged
- [x] `/metrics`
- [x] Unauthenticated `/metrics`, for the same reason as `/health` — a scrape that depends on 
- [x] Memory budget, itemized and defended
- [x] Two totals, because only one cache exists.
- [x] The cgroup limit is a guard, never a sizing input.
- [x] Invalid configuration is refused **naming the variable**, per this epic's first acceptan
- [x] Admission control
- [x] Permits available, held and rejections are exported per class, so "overloaded" and "brok
- [x] Spans across port boundaries
- [x] `db_pool_connections{state}` and `catalog_entities_total{entity_type}`

### Epic 11 — Users, Teams & Ownership

- [x] `User` with roles; auto-provisioned on first sight
- [x] `owner_id` on assets (nullable, so the gap is visible rather than prevented)

### Epic 12 — Authentication & Authorization

- [x] JWT verification (HS256, shared secret); a forged token is rejected
- [x] The `Principal` extractor swap
- [x] Auto-provision a `User` on first sight, with no roles
- [x] Open mode when no secret is configured, logged as such at startup
- [x] JWKS / RS256 against an OIDC issuer
- [x] A heterogeneous JWKS does not break authentication
- [x] The refetch an unknown `kid` triggers is rate-limited
- [x] OIDC beats a shared secret when both are configured
- [x] `GRAPH_OWL_ADMIN_SUBJECTS`
- [x] Roles can come from the token
- [x] Sign-in verified end to end against the live tenant
- [x] A bug that only end-to-end could find
- [x] Verified against the live tenant
- [ ] A page refresh signs the user out
- [ ] The authorization redirect, consent and code exchange are **untested end to end** — they

### Epic 13 — Authentication & Authorization

- [x] `AccessPredicate` in `graph-owl-authz` — pure, zero surviving mutants
- [x] Lowered to SQL for list, search, children and counts
- [x] Deny-overrides, order-independent; an unmatched request denies
- [x] `MetadataOperation` vocabulary, append-only
- [x] Row-level filtering — the PII demo
- [x] Counts filtered through the same predicate, so a total cannot leak what it hid
- [x] Hidden reads as `404`, not `403` — a `403` on an id confirms the id exists
- [x] Decision cache
- [x] Invalidated by epoch, never by TTL
- [ ] `invalidate_authorization()` has no caller yet

### Epic 14 — MCP + Outbound Events ★

- [x] Protocol, authentication and policy together, with one tool
- [x] Trust summaries and gaps
- [~] Not wired to anything yet
- [ ] The remaining six read tools — search, lineage, governance, graph query *(Slices C, D)*
- [ ] Token-budgeted responses *(Slice E)*
- [ ] Outbound webhooks, HMAC-signed, at-least-once *(Slice F)*
- [ ] The thesis test: an agent with only MCP access answers a real question *(Slice G)*

### Epic 15 — Source Connectors

- [x] `Connector` trait, `SourceRecord`, `RunScope`
- [x] Postgres reference connector reading `information_schema`
- [x] Parents-before-children ordering as a connector contract
- [x] Re-runs converge (FQN is the identity, not the generated id)
- [x] Run report names each failure and its reason
- [x] System schemas excluded; views catalogued and marked
- [x] Deletion detection with a threshold guard — off by default; a refusal deletes nothing at
- [x] Run history persisted
- [x] `GET /connectors/runs`, newest first, and the console's Connectors page shows it — a run
- [ ] ~~Scheduled runs~~ — **refused by decision 5**, not missing: *"graph-owl does not become
- [x] `source_hash` fingerprinting
- [x] The fingerprint covers source-owned fields only.
- [x] A skipped record still counts as reported by the source
- [x] The run reports `skipped` alongside `created`: a run that wrote nothing because nothing 
- [ ] Deletion detection, threshold guard
- [ ] `source_hash` fingerprinting
- [ ] Python connector protocol + one non-Postgres source

### Epic 16 — Ingestion APIs, SDKs, Batch & Custom Adapters

- [ ] Push API with partial success and idempotency keys
- [ ] Batch file ingestion
- [ ] Generated TypeScript and Python SDKs

### Epic 17 — Entity Resolution & Deduplication

- [ ] Deterministic + probabilistic matching
- [ ] Reversible `sameAs` merge
- [ ] Merge adjudication queue (Epic 42)

### Epic 18 — Inbound Events & Webhooks

- [ ] Webhook registry, signature verification, replay
- [ ] Broker consumption with consumer-group rebalancing

### Epic 19 — Streaming Ingestion

- [ ] Webhook registry, signature verification, replay
- [ ] Broker consumption with consumer-group rebalancing

### Epic 20 — Metadata-as-Code ★

- [ ] `plan` / `apply` / `diff` with scoped authority
- [ ] Drift reported, never auto-corrected

### Epic 21 — Document & Conversation Ingestion

- [ ] Python worker: PDF/OCR/chunking → extraction named graph
- [ ] Extraction review queue with source-span evidence

### Epic 22 — Custom Properties

- [ ] 22
- [ ] 23
- [ ] 24
- [ ] 25
- [ ] 26
- [ ] 27
- [ ] 28
- [ ] 29
- [ ] 30

### Epic 29 — Lineage

- [x] A
- [x] A
- [x] B
- [x] B
- [x] C
- [ ] D
- [ ] E
- [ ] F

### Epic 31 — Organizational Memory ★

- [ ] Memory objects: kind, content, authorship, confidence, `as_of`
- [ ] Supersession and contradiction detection
- [ ] Retrieval with reranking

### Epic 32 — Agent Capabilities ★

- [ ] Write-back with agent authorship
- [ ] Investigation and remediation proposals

### Epic 33 — Domain Ontology Packs

- [ ] 33
- [ ] 34
- [ ] 35
- [ ] 36
- [ ] 37a
- [ ] 37b
- [ ] 37c
- [ ] 38

### Epic 39 — Console Foundation, Discovery & Entity Pages

- [x] SPA embedded in the binary via `rust-embed`, one process
- [x] Hierarchy tree with lazy children
- [x] Entity page: breadcrumb, properties, children table
- [x] Search across name and FQN
- [x] Empty-database first-run state that offers the next action
- [x] Trust bar that states what it does not know yet
- [x] Deep-linkable selection (`?asset=`)
- [x] OIDC/PKCE login
- [x] Generated API client
- [x] Hierarchy tree, asset detail, and the five-level service → column navigation
- [x] Trust bar: version, last editor, and honest "not captured yet" for certification and lin
- [x] Version history tab with the diff viewer
- [x] Inline description editing writing straight through `PATCH`
- [x] Connectors catalogue page; Postgres available, the rest listed as unavailable rather tha
- [x] Light/dark theme, light by default, deep-linkable via `?theme=`
- [x] Search box over name and FQN
- [x] Facet rail over kind and schema
- [x] Keyboard navigation
- [ ] Nothing outstanding that is not blocked on Epic 12's OIDC/PKCE, below
- [x] OIDC/PKCE sign-in
- [x] Three outcomes, three screens
- [x] The token has **one owner**
- [ ] Owner and team display → blocked on Epic 11's teams

### Epic 40 — Graph Explorer, Lineage & Time Travel ★

- [x] Renderer-agnostic `GraphView` — the shape a WebGL canvas will consume unchanged
- [x] Graph tab with a deterministic radial layout, hop selector, truncation shown
- [x] Time control
- [x] Non-visual equivalent: the same neighbourhood as a keyboard-navigable table, expansion i
- [x] Expand-on-click
- [x] Diff mode
- [x] Cytoscape canvas, WebGL above 256 nodes
- [x] The model was untouched by the swap
- [x] Diff compares the *expanded* model, not the seed walk
- [x] Lineage DAG
- [ ] Derived edges visually distinct — nothing derives edges until Epic 6

### Epic 41 — Query Workbench, Governance & Admin

- [x] Violations queue
- [x] Both engines triggerable from the console
- [ ] SPARQL editor with plan display
- [ ] Results as table ⇄ graph
- [ ] Violations as an assignable **workflow** with waivers — the queue displays; assignment a
- [x] The derivation chain rendered beside a derived fact
- [ ] Admin: policies with dry-run, connectors, jobs

### Epic 42 — Semantic Browse, Review Queues & Agent Activity

- [ ] One vocabulary browser over glossary, tags, domains, packs
- [ ] One review queue over four proposal sources
- [ ] Agent activity audit

### Epic 43 — Agent Framework Integrations

- [ ] LangChain retriever preserving provenance and confidence
- [ ] LangGraph toolkit, manifest-parity with MCP
- [ ] Checkpointer over Epic 31, retraction not deletion
- [ ] Zero graph-owl crate changes, asserted

### Epic 94 — RDF 1.2 Alignment

- [ ] A
- [ ] B
- [ ] C
- [ ] C (console)
- [ ] D
- [ ] Slices B, C and D share one `oxrdf/rdf-12` feature gate — one decision, taken once for t
- [ ] Export dialog offers RDF 1

### Epic 95 — OWL 2 RL Completion

- [ ] The remaining RL axioms beyond Epic 6's eight
- [ ] Explanation panel extends to the new axioms — same surface, more rules *(UI → Epic 41)*

### Epic 96 — SHACL-SPARQL

- [ ] SPARQL-based constraint components
- [ ] The violations workflow is unchanged; **authoring gains a second language**, so the cons

### Epic 97 — Incremental & Parallel Reasoning

- [ ] Incremental maintenance rather than full recomputation
- [ ] Overlay staleness is visible

### Epic 98 — OWL 2 EL Reasoning

- [ ] EL and QL reasoners alongside RL
- [ ] No UI of their own

### Epic 99 — OWL 2 QL Reasoning

- [ ] EL and QL reasoners alongside RL
- [ ] No UI of their own

### Epic 100 — Ontology Profile Detection & Routing

- [ ] Detection across RL, EL, QL; incomparable profiles not reported as supersets
- [ ] Out-of-profile reasoning refused, naming the first offending axiom
- [ ] Profile badge + the reasoner that produced each derivation
- [ ] Out-of-profile and override-partial results marked

### Epic 101 — SPARQL Federation — `SERVICE`

- [ ] `SERVICE` against an allow-listed endpoint; unlisted refused by name
- [ ] Bindings denied by policy never transmitted — asserted on the outbound request, the only
- [ ] Remote rows attributed to their endpoint in the result grid
- [ ] A `SILENT` failure is visible in the result.
- [ ] Allow-list admin with dry-run

### Epic 102 — Read/Write Partition Split

- [ ] The split itself
- [ ] Partition health and replication lag in admin *(UI → Epic 41 Slice G)*

### Epic 103 — In-Process Traversal

- [ ] The traversal path
- [ ] No UI

## Epics with slice marks and no plan file

None — every epic tracked in `DEMOS.md` has a plan.
