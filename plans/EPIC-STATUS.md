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

**Tracked items are not the plan's slices, and the difference matters.**
This column counts `DEMOS.md` checkboxes *plus* the bullets under a
**Pending in this epic** heading — which is deliberate, so an epic
cannot read complete while the file lists outstanding work. The
consequence is that the ratio **understates** an epic whose remaining
work is itemised: Epic 31 shows 2/5 while all five of its plan slices
(A–E) have domain logic, persistence and an HTTP surface, because two
of those five items are *pending notes* rather than slices. Read a low
ratio as "outstanding work is written down", not as "slices unwritten",
and read the plan for slice-level state.

`—` means the plan exists and `DEMOS.md` tracks no marks for it yet,
which is a different thing from zero of them done.

**Demo** is which demo an epic serves, from `DEMOS.md`'s coverage
index. An epic serving more than one shows both — Epic 6 is Demo 4's
reasoning *and* is recalibrated in Demo 12, and a single number would
quietly drop the later work. `—` means the epic is in no demo, which
is the condition that index exists to catch.

| Epic | Demo | Plan | State | Tracked items | Depends on |
|---|---|---|---|---|---|
| **1** | 1 | [`01-api-conventions.md`](01-api-conventions.md) | **Shipped** | 12/12 | nothing |
| **2** | 1 | [`02-entity-hierarchy.md`](02-entity-hierarchy.md) | **Shipped** | 8/8 | Epic 1 (conventions, relationship taxonomy) |
| **3** | 2 | [`03-versioning.md`](03-versioning.md) | In progress | 11/13 (+1 partial) | Epic 2 (four entity types to apply the envelope to) |
| **4** | 3 | [`04-engine-triples.md`](04-engine-triples.md) | **Shipped** | 11/11 | Epic 3 (four entity types with an envelope to project) |
| **5** | 4 | [`05-engine-constraints.md`](05-engine-constraints.md) | In progress | 9/10 (+1 partial) | Epic 4 (triples to validate) |
| **6** | 4, 12 | [`06-engine-reasoning.md`](06-engine-reasoning.md) | In progress | 7/8 (+1 partial) | Epic 4 (triples), Epic 5 (ontology types) |
| **7** | 3 | [`07-engine-query.md`](07-engine-query.md) | In progress | 8/9 | Epic 4 (triples), Epic 13 (authorization to compile into queries), **Epic 7a**… |
| **7a** | 3 | [`07a-engine-traversal.md`](07a-engine-traversal.md) | **Shipped** | 5/5 | Epic 4 (triples, SPOT/POST/OPST indexes) |
| **7b** | 8 | [`07b-engine-cypher.md`](07b-engine-cypher.md) | In progress | 0/1 (+1 partial) | Epic 7 (SPARQL plan is the lowering target), Epic 7a (traversal), **Epic 7c (LP… |
| **7c** | 8 | [`07c-engine-lpg.md`](07c-engine-lpg.md) | **Shipped** | 1/1 | Epic 4 (flakes), Epic 1 (relationship taxonomy) |
| **7d** | 8 | [`07d-engine-bolt.md`](07d-engine-bolt.md) | **Shipped** | 1/1 | Epic 7b (Cypher), Epic 7c (LPG projection), Epic 12 (auth), Epic 13 (authorizat… |
| **8** | 2 | [`08-engine-search.md`](08-engine-search.md) | In progress | 9/10 | Epic 3 (change events to subscribe to), Epic 2 (FQNs to rank on), Epic 25 (tags… |
| **9** | 8 | [`09-engine-rdf-io.md`](09-engine-rdf-io.md) | **Shipped** | 1/1 | Epic 4 (triples to serialize) — shipped. ~~Epic 7 (CONSTRUCT produces Turtle)~~… |
| **9a** | 8 | [`09a-lpg-interchange.md`](09a-lpg-interchange.md) | **Shipped** | 1/1 | Epic 7c (LPG projection) — shipped, and its `FlakeValue::Ref`-vs-`String` kind… |
| **10** | 2 | [`10-operability.md`](10-operability.md) | **Shipped** | 17/17 | Epic 1 (a server, an error model, and a contract to instrument) |
| **11** | 2 | [`11-people-and-ownership.md`](11-people-and-ownership.md) | In progress | 11/14 (+1 partial) | Epic 3 (envelope carries `owners`) |
| **12** | 2 | [`12-13-security.md`](12-13-security.md) | **Shipped** | 15/15 | Epic 11 (`Principal` seam), Epic 11 (users and teams to attach roles to) |
| **13** | 2 | [`12-13-security.md`](12-13-security.md) | **Shipped** | 10/10 | Epic 11 (`Principal` seam), Epic 11 (users and teams to attach roles to) |
| **14** | 5 | [`14-mcp-activation.md`](14-mcp-activation.md) | In progress | 7/8 (+1 partial) | Epic 7a (subgraph retrieval for agent context), Epic 13 (authorization — **hard… |
| **15** | 1 | [`15-connectors.md`](15-connectors.md) | In progress | 13/15 | Epic 2 (hierarchy to populate), Epic 3 (versioning to make re-runs observable) |
| **16** | 6 | [`16-ingestion-apis.md`](16-ingestion-apis.md) | **Shipped** | 6/6 | Epic 1 (contract), Epic 15 (upsert semantics) |
| **17** | 6 | [`17-entity-resolution.md`](17-entity-resolution.md) | **Shipped** | 4/4 | Epic 4 (`sameAs` in the graph), Epic 15 + 16 (two write paths make this necessa… |
| **18** | 6 | [`18-inbound-events.md`](18-inbound-events.md) | **Shipped** | 5/5 | Epic 16 (ingestion contract), Epic 17 (resolution, so pushes do not duplicate) |
| **19** | 6 | [`19-streaming.md`](19-streaming.md) | In progress | 5/6 (+1 partial) | Epic 16 (ingestion contract), Epic 18 (dedup and ordering machinery) |
| **20** | 6 | [`20-metadata-as-code.md`](20-metadata-as-code.md) | **Shipped** | 3/3 | Epic 15 (idempotent upsert and reconciliation machinery) |
| **21** | 6 | [`21-document-ingestion.md`](21-document-ingestion.md) | **Shipped** | 4/4 | Epic 16 (ingestion), Epic 17 (mention resolution) |
| **22** | 7 | [`22-custom-properties.md`](22-custom-properties.md) | **Shipped** | 5/5 | Epic 3 (the envelope's `extension` field) |
| **23** | 7 | [`23-domains.md`](23-domains.md) | **Shipped** | 7/7 | Epic 11 (domains and products are owned) |
| **24** | 7 | [`24-business-semantics.md`](24-business-semantics.md) | In progress | 8/9 (+1 partial) | Epic 2 (FQN derivation and the hierarchy terms attach to), Epic 11 (term review… |
| **25** | 7 | [`25-classification.md`](25-classification.md) | **Shipped** | 9/9 | Epic 3 (envelope carries `tags`), Epic 11 (term reviewers are users) |
| **26** | 7 | [`26-lifecycle-certification.md`](26-lifecycle-certification.md) | In progress | 9/10 (+1 partial) | Epic 11 (issuers are principals), Epic 24 (metrics are certifiable) |
| **27** | 7 | [`27-contracts.md`](27-contracts.md) | In progress | 4/6 (+1 partial) | Epic 2 (schemas to guarantee), Epic 3 (version diffs detect breakage) |
| **28** | 7 | [`28-usage.md`](28-usage.md) | In progress | 5/6 | Epic 16 (push ingestion), Epic 11 (consumers are principals) |
| **29** | 7 | [`29-lineage.md`](29-lineage.md) | **Shipped** | 14/14 | Epic 15 (connectors assert lineage), Epic 2 (columns for column-level lineage),… |
| **30** | 7 | [`30-quality-results.md`](30-quality-results.md) | In progress | 7/8 | Epic 29 (lineage, for propagating trust signals) |
| **31** | 5 | [`31-memory.md`](31-memory.md) | In progress | 2/4 (+1 partial) | Epic 3 (envelope), Epic 11 (people), Epic 14 (MCP surface to serve it) |
| **32** | 5 | [`32-agent-capabilities.md`](32-agent-capabilities.md) | **Shipped** | 2/2 | Epic 14 (read surface, validated by real usage), Epic 31 (memory to write into) |
| **33** | 9 | [`33-ontology-packs.md`](33-ontology-packs.md) | **Shipped** | 3/3 | Epic 24 (glossary and taxonomy model), Epic 9 (standards import) |
| **34** | 9 | [`34-entity-expansion.md`](34-entity-expansion.md) | **Shipped** | 1/1 | Epic 8 (each new type indexes for free — the property being demonstrated), Epic… |
| **35** | 9 | [`35-collaboration.md`](35-collaboration.md) | In progress | 0/1 (+1 partial) | Epic 11 (users), Epic 12 (real identity on posts) |
| **36** | 9 | [`36-reference-apps.md`](36-reference-apps.md) | **Shipped** | 1/1 | Epic 14 (MCP), Epic 16 (SDKs), Epic 29 (graph API) |
| **37a** | 9 | [`37a-scale.md`](37a-scale.md) | In progress | 0/1 (+1 partial) | Epic 34 (a realistic entity mix to generate) — shipped |
| **37b** | 9 | [`37b-portability.md`](37b-portability.md) | **Shipped** | 1/1 | Epic 3 (history is part of what must survive) — shipped |
| **37c** | 9 | [`37c-embeddable.md`](37c-embeddable.md) | In progress | 0/1 (+1 partial) | Epic 1 (stable contract); benefits from Epic 34 (wide surface to validate again… |
| **38** | 9 | [`38-graph-analytics.md`](38-graph-analytics.md) | In progress | 0/1 (+1 partial) | Epic 7a (traversal), Epic 4 (flakes), Epic 28 (usage signals, for comparison) |
| **39** | 1 | [`39-ui-foundation.md`](39-ui-foundation.md) | In progress | 25/26 (+1 partial) | Epic 1 (API conventions + OpenAPI), Epic 8 (search), Epic 12 (authn), Epic 13 (… |
| **40** | 3 | [`40-ui-graph-explorer.md`](40-ui-graph-explorer.md) | **Shipped** | 11/11 | Epic 39 (console shell, trust components), Epic 7a (traversal), Epic 4 (flakes,… |
| **41** | 4 | [`41-ui-workbench-governance.md`](41-ui-workbench-governance.md) | **Shipped** | 12/12 | Epic 39 (shell, trust components), Epic 40 (graph model and renderers), Epic 5… |
| **42** | 7 | [`42-ui-semantic-surfaces.md`](42-ui-semantic-surfaces.md) | In progress | 3/4 (+1 partial) | Epic 39 (shell, patterns, trust components), Epic 41 (admin section, schema-dri… |
| **43** | 5 | [`43-framework-integrations.md`](43-framework-integrations.md) | **Shipped** | 6/6 | Epic 14 (MCP), Epic 13 (authorization), Epic 31 (memory), Epic 16 (Python SDK),… |
| **93** | 3 | [`93-console-overview.md`](93-console-overview.md) | **Shipped** | 2/2 | Epic 2 (hierarchy), Epic 3 (envelope), Epic 4 (graph), Epic 13 (authorization) |
| **94** | 10 | [`94-rdf12-alignment.md`](94-rdf12-alignment.md) | In progress | 6/8 (+1 partial) | Epic 4 (flakes, reified relationships), Epic 9 (serialization) |
| **95** | 10 | [`95-owl-rl-completion.md`](95-owl-rl-completion.md) | **Shipped** | 2/2 | Epic 6 (the eight rules and the fixpoint that runs them) |
| **96** | 10 | [`96-shacl-sparql.md`](96-shacl-sparql.md) | In progress | 2/4 | Epic 5 (SHACL Core), Epic 7 (SPARQL) |
| **97** | 10, 12 | [`97-incremental-parallel-reasoning.md`](97-incremental-parallel-reasoning.md) | In progress | 2/3 | Epic 6 (semi-naive fixpoint), Epic 37a (the measurement) |
| **98** | 11, 12 | [`98-owl-el-reasoning.md`](98-owl-el-reasoning.md) | In progress | 3/4 | Epic 6 (overlay, budgets, explainability), Epic 24 (ontologies as entities) |
| **99** | 11, 12 | [`99-owl-ql-reasoning.md`](99-owl-ql-reasoning.md) | In progress | 3/4 | Epic 7 (query algebra to rewrite), Epic 6 (explanation contract) |
| **100** | 11, 12 | [`100-profile-detection-and-routing.md`](100-profile-detection-and-routing.md) | In progress | 5/7 | Epic 6 (RL engine — shipped), ~~Epic 24 (ontologies as entities)~~ — **phantom*… |
| **101** | 11 | [`101-sparql-federation.md`](101-sparql-federation.md) | **Shipped** | 5/5 | Epic 7 (algebra and executor), Epic 13 (authorization) |
| **102** | 11 | [`102-read-write-partitions.md`](102-read-write-partitions.md) | In progress | 2/3 | Epic 4 (the flake table), Epic 37a (the measurement) |
| **103** | 11 | [`103-in-process-traversal.md`](103-in-process-traversal.md) | In progress | 4/5 | Epic 7a (the `TraversalEngine` port), Epic 37a (the trigger) |
| **104** | 12 | [`104-ontology-alignment.md`](104-ontology-alignment.md) | In progress | 5/6 | Epic 33 (ontology packs — supplies the vocabularies), Epic 100 (profile detecti… |

## Tracked items, per epic

### Epic 1 — API Conventions & Contract *(Demo 1)*

- [x] Errors are RFC 9457 problem+json with stable `type` URIs
- [x] Validation reports every field violation at once, not the first
- [x] Cursor pagination, keyset not offset
- [x] camelCase on the wire; conflict taxonomy split by kind
- [x] One `CatalogError` across the facade
- [x] Closed relationship vocabulary with a legality table
- [x] `Principal` seam through every mutating handler
- [x] Unknown query parameters rejected and named
- [x] `Location` header on creates, asserted against the returned id
- [x] OpenAPI 3.1 generated from code, served at `/openapi.json`, committed and drift-guarded.
- [x] A TypeScript client is generated from `openapi.json` and driven against a live server: c
- [x] It earned its keep immediately.

### Epic 2 — Entity Hierarchy & Columns *(Demo 1)*

- [x] `Asset` + `AssetKind` for all five levels, one type not five
- [x] FQN derivation (`fqn::derive`, `fqn::child_of`, `parent`, `leaf`)
- [x] Containment rule in one place (`AssetKind::parent_kind`)
- [x] Hierarchy endpoints: roots, children, ancestors, search, stats
- [x] `PATCH /assets/{id}` and `DELETE /assets/{id}` (soft, cascading to the subtree) — shippe
- [x] Containment cascade characterised
- [x] Non-database services (dashboard, pipeline, ML) → deferred to Epic 34, which shipped all
- [x] Cascade-on-rename for `Asset`, closed 8 August 2026 (Phase 3 item 3.3)

### Epic 3 — Envelope, Versioning, Soft Delete & Change Events *(Demo 2)*

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

### Epic 4 — Triple Storage & Time-Travel ★ *(Demo 3)*

- [x] `Flake` in `graph-owl-core`; ten pinned `FlakeValue` variants *(Slice A)*
- [x] Namespace code registry — constants allocated and range-tested; runtime namespaces persi
- [x] Four index orderings: SPOT, PSOT, POST, OPST — each verified by `EXPLAIN` naming the ind
- [x] `op = false` is a retraction, not a delete — assert/retract/assert/retract verified in b
- [x] Entity → flake projection — wired into both write paths; a catalogue run of the 124-asse
- [x] Reified relationships — each edge is a node of its own carrying `rdf:type`, both endpoin
- [x] As-of query API
- [x] Reconciliation and drift metric — drift computed by comparison rather than from a queue 
- [x] Runtime predicate registry — define/lookup/list, duplicate refused, core vocabulary seed
- [x] `rdf:reifies` + triple terms → **Epic 94**, shipped (Slices A, B, D)
- [x] Language-tag side table → **Epic 94**, shipped (Slice C) via a deliberate design pivot: 

### Epic 5 — Constraint Validation *(Demo 4)*

- [x] Shape and constraint types; four target kinds
- [x] Compile-once, evaluate-many
- [x] Shapes compile from graph triples
- [x] Continuous validation with violation reports, not write-time rejection
- [x] Severity classification; repair suggestions never auto-applied
- [x] `GET /validation/report`
- [x] All six target kinds
- [x] Seed shapes ship
- [x] `sh:not`/`sh:and`/`sh:or` stated as triples
- [~] Pending in this epic

### Epic 6 — Reasoning Overlay *(Demo 4, 12)*

- [x] Eight OWL 2 RL axioms as built-in rules
- [x] Semi-naive fixpoint, `CappedReason` on every limit
- [x] Derived facts in `graph:reasoning`, never persisted into the base
- [x] `GET /reasoning/explain` derivation chains
- [x] Reasoning is skipped on historical queries
- [x] Classification propagates along `feeds`, opt-in per classification
- [x] Lineage is projected into the graph
- [~] Pending in this epic

### Epic 7 — Graph Query — SPARQL ★ *(Demo 3)*

- [x] `QueryableDataset` over flakes
- [x] Pattern pushdown
- [x] `as_of` — the dataset is constructed at a transaction time *(Slice B)*
- [x] Authorization applied before the dataset exists, so the evaluator only ever sees permitt
- [x] Fact budget — **nothing adopted enforces budgets; this is ours**
- [x] Freshness stamping on the result (Epic 4 decision 8) *(Slice B)*
- [x] ~~Parser~~ — `spargebra`, full SPARQL 1
- [x] ~~Planner / execution~~ — `spareval` *(adopted)*
- [ ] `sparopt` is not yet in the path — pushdown reads the parsed algebra directly

### Epic 7a — Graph Traversal Engine *(Demo 3)*

- [x] One frontier primitive (recursive CTE, one statement); `neighbours` and `subgraph` over 
- [x] Budgeted, cycle-safe, truncation always visible and farthest-first
- [x] Reified two-hop edges hidden — five logical edges reports distance five
- [x] `as_of` on every walk, so time-travelling traversal is free
- [x] `shortest_path`, `all_paths`, `detect_cycles` *(Slices C, D)* — deterministic tiebreak, 

### Epic 7b — Cypher Query Support *(Demo 8)*

- [~] openCypher lowering onto the same plan (ships *after* 7c) — **Slices A, B, C, D, E, F bu

### Epic 7c — Labelled Property Graph Projection ★ *(Demo 8)*

- [x] Bidirectional flake ⇄ LPG projection, losses enumerated — nodes, edges, element ids and 

### Epic 7d — Bolt Protocol Server ★ *(Demo 8)*

- [x] Bolt server: PackStream, handshake, state machine, `graph-owl-bolt`, behind an off-by-de

### Epic 8 — Vector & Hybrid Search ★ *(Demo 2)*

- [x] Facets by kind and schema, computed over the **visible** set
- [x] Result counts consistent with authorization filtering
- [x] Full-text, stemmed, prefix-matched and ranked
- [x] Relevance is asserted as an order, not as membership
- [x] A user cannot reach the query language.
- [x] The rank key *is* the pagination cursor
- [x] Column-name search — shipped 8 August 2026
- [ ] Decision 5's full relevance ordering (exact FQN > exact name > prefix > fuzzy > descript
- [x] Snippets — shipped 8 August 2026
- [x] A popularity term folded into ranking — shipped 8 August 2026

### Epic 9 — RDF Interop & Open Standards *(Demo 8)*

- [x] JSON-LD, Turtle, DCAT, PROV-O, OpenLineage — **all six slices shipped, 6 August 2026.** 

### Epic 9a — Property-Graph Interchange & External Store Sync *(Demo 8)*

- [x] GraphML, bulk CSV, projection targets — **all slices shipped, 5–6 August 2026; the epic-

### Epic 10 — Operability & Resource Budget ★ *(Demo 2)*

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

### Epic 11 — Users, Teams & Ownership *(Demo 2)*

- [x] `User` with roles; auto-provisioned on first sight
- [x] ~~`owner_id` on assets~~ — **superseded by Slice C**, which replaced it with an `asset_o
- [x] Teams exist
- [~] Was marked Shipped while teams did not exist
- [x] Entities have owners — plural, and of two kinds
- [x] Ownership inherits down `contains`
- [x] Assets are filterable by owner
- [x] The ownership-gap report
- [x] Teams nest, with cycles refused at any depth
- [x] Users can be created before they ever sign in
- [x] Users can follow assets
- [x] Deleting a principal does not orphan assets
- [ ] Principals have no soft-delete state, so two criteria are vacuous rather than unmet.
- [ ] Slice B's `GET /teams/{id}/members` was not added separately: membership already rides o

### Epic 12 — Authentication & Authorization *(Demo 2)*

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
- [x] A page refresh keeps the session
- [x] The full flow is confirmed at a browser

### Epic 13 — Authentication & Authorization *(Demo 2)*

- [x] `AccessPredicate` in `graph-owl-authz` — pure, zero surviving mutants
- [x] Lowered to SQL for list, search, children and counts
- [x] Deny-overrides, order-independent; an unmatched request denies
- [x] `MetadataOperation` vocabulary, append-only
- [x] Row-level filtering — the PII demo
- [x] Counts filtered through the same predicate, so a total cannot leak what it hid
- [x] Hidden reads as `404`, not `403` — a `403` on an id confirms the id exists
- [x] Decision cache
- [x] Invalidated by epoch, never by TTL
- [x] `PUT /users/{id}/roles` is the caller

### Epic 14 — MCP + Outbound Events ★ *(Demo 5)*

- [x] Protocol, authentication and policy together, with one tool
- [x] Trust summaries and gaps
- [x] The adapter over `Catalog`
- [x] The transport
- [x] The remaining six read tools — search, lineage, governance, graph query *(Slices C, D)*
- [x] Token-budgeted responses *(Slice E)*
- [~] Outbound webhooks, HMAC-signed, at-least-once *(Slice F)* — the decisions are built and 
- [x] The thesis test: an agent with only MCP access answers a real question *(Slice G)* — aga

### Epic 15 — Source Connectors *(Demo 1)*

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
- [ ] Python connector protocol + one non-Postgres source

### Epic 16 — Ingestion APIs, SDKs, Batch & Custom Adapters *(Demo 6)*

- [x] Push API with partial success
- [x] Idempotency
- [x] Boundary validation
- [x] Relationships and lineage in a push
- [x] Batch file ingestion
- [x] Generated TypeScript and Python SDKs, and the custom adapter guide

### Epic 17 — Entity Resolution & Deduplication *(Demo 6)*

- [x] Deterministic + probabilistic matching
- [x] Reversible `sameAs` merge
- [x] Merge adjudication queue
- [x] Mention resolution

### Epic 18 — Inbound Events & Webhooks *(Demo 6)*

- [x] Endpoint registration and signature verification
- [x] Dedup and ordering
- [x] Declarative mapping
- [x] Dead-letter and replay, with real out-of-order protection
- [x] Abuse resistance

### Epic 19 — Streaming Ingestion *(Demo 6)*

- [x] Consume and apply
- [x] Offsets commit only after apply
- [x] Lag and health
- [x] Poison messages and backpressure
- [x] Rebalancing and replay
- [~] Pulsar parity

### Epic 20 — Metadata-as-Code ★ *(Demo 6)*

- [x] `plan` / `apply` / `diff` with scoped authority
- [x] Drift reported, never auto-corrected
- [x] Drift made HTTP-queryable, for a review queue with no filesystem access

### Epic 21 — Document & Conversation Ingestion *(Demo 6)*

- [x] Python worker: PDF/OCR/chunking → extraction submission
- [x] The Rust domain and both ports
- [x] Extraction review queue with source-span evidence
- [x] A real passage-plus-highlighted-span, and a third Edit outcome

### Epic 22 — Custom Properties *(Demo 7)*

- [x] Custom properties — typed definitions, per-key PATCH merge, guarded evolution, indexed f
- [x] Typed, per-entity-type property definitions
- [x] Values validated on write
- [x] Definitions evolve safely
- [x] Custom properties are queryable

### Epic 23 — Domains & Data Products *(Demo 7)*

- [x] Domains and data products — accountability axis with inheritance, consumable bundles *(3
- [x] Domains nest, and the paths move with them
- [x] One asset, one domain, resolved by walking up
- [x] The cascade is free, and the plan's criteria for it did not survive
- [x] Data products bundle across boundaries
- [x] Both axes filter list and search
- [x] Deleting a domain does not orphan

### Epic 24 — Business Semantics *(Demo 7)*

- [x] Glossary and term CRUD — `POST/GET /glossaries`, `GET/DELETE
- [x] Synonyms and abbreviations are string lists, both indexed by the
- [x] Deleting a glossary with terms is a `409` naming the count,
- [x] Every term is created `Draft`; the core review workflow (`can_transition`,
- [x] SKOS relations at the wire — `POST/GET/DELETE
- [x] Review workflow at the wire — `PUT/GET
- [x] Terms attach to assets and columns — `POST/GET/DELETE
- [x] `Metric` as a first-class entity — full CRUD on
- [~] Metric lineage reconciliation — `PUT

### Epic 25 — Tags & Classification *(Demo 7)*

- [x] Classifications with mutual exclusivity — the PII taxonomy, with provenance and a reject
- [x] Three of nine slices were already built.
- [x] Provenance from day one
- [x] Exclusivity is scoped to one classification
- [x] A rejection is a row, not an absence
- [x] Columns are the point
- [x] A governance label cannot vanish by accident
- [x] Propagation never downgrades a manual label
- [x] `?tags=` filtering — shipped 8 August 2026

### Epic 26 — Lifecycle & Certification *(Demo 7)*

- [x] Lifecycle and certification with issuer and expiry, status computed on read *(3 August 2
- [x] Two orthogonal axes
- [x] The state machine refuses the shortcuts
- [x] A successor is a reference, not prose
- [x] Evidence is enforced, and named when missing
- [x] Status is computed on every read
- [x] Renewal re-checks
- [~] Discoverable
- [x] `?lifecycle=` filtering — shipped 8 August 2026
- [x] `?certification=` filtering — shipped 8 August 2026

### Epic 27 — Data Contracts *(Demo 7)*

- [x] Data contracts and compatibility — the 24-cell matrix, breaches that report rather than 
- [x] The compatibility matrix, written out cell by cell
- [x] Two rules outside the matrix, applied first.
- [x] A breach reports and never blocks
- [~] Every SLA reports `Unknown`
- [ ] ODCS interop

### Epic 28 — Usage & Popularity *(Demo 7)*

- [x] Usage and popularity signals — rollups, trend with a volume floor, query text dropped at
- [x] Observations ingest, rollups fold in incrementally
- [x] Popularity computed on read
- [x] Query text is dropped at the boundary
- [x] The most recent observation survives pruning
- [ ] Ranking integration

### Epic 29 — Lineage *(Demo 7)*

- [x] `POST /lineage` and `DELETE /lineage/{id}` between assets, with the SQL that produced th
- [x] Self-lineage refused (a cycle of length one), a missing endpoint is `404`, and lineage a
- [x] A bounded walk in both directions, each spending its **own** budget: a merged frontier w
- [x] A diamond yields the shared node once with both inbound edges; a tombstoned node stays i
- [x] A cycle terminates. The graph is called acyclic because it should be, not because anythi
- [x] Column-level lineage — shipped 3 August 2026; see "Epic 29 — Lineage, the column half" b
- [x] Connector-asserted lineage reconciles with curated edges — shipped; see "Epic 29 — Linea
- [x] Lineage survives entity deletion — shipped (soft delete retains edges and they return on
- [x] Lineage: table and column, with source-scoped reconciliation *(Slices A–C 29 Jul, D–F 3 
- [x] Column-level mappings, many-to-one
- [x] Source-scoped reconciliation
- [x] Edges survive soft delete and return on restore
- [x] Rename propagation
- [x] The node budget Slice C's own acceptance criteria specified — built 8 August 2026, not 2

### Epic 30 — Quality Signals & Incidents *(Demo 7)*

- [x] Quality: definitions, suites, results, derived health *(3 August 2026)*
- [x] The boundary held
- [x] Definitions, cases and suites
- [x] Results are history
- [x] Health is derived, and refuses to lie twice
- [x] The latest result survives pruning
- [x] Upstream health is reported separately, never merged
- [ ] Health filtering and facets

### Epic 31 — Organizational Memory ★ *(Demo 5)*

- [x] Memory objects: kind, content, authorship, confidence, `as_of`
- [x] Supersession and contradiction detection
- [~] Retrieval with reranking
- [ ] The semantic ranking term

### Epic 32 — Agent Capabilities ★ *(Demo 5)*

- [x] Write-back with agent authorship — grants, the closed capability set, propose-by-default
- [x] Investigation and remediation proposals — `record_investigation` refuses a finding with 

### Epic 33 — Domain Ontology Packs *(Demo 9)*

- [x] Import, licence gating, extend-without-fork overrides, upgrade diffing, and cross-pack-g
- [x] A first real BFSI pack import (FIBO) — shipped 8 August 2026
- [x] Domain ontology packs — a banking/BFSI pack — **shipped 8 August 2026**, one real FIBO m

### Epic 34 — Entity Expansion *(Demo 9)*

- [x] Entity expansion: dashboards, pipelines, topics, models, storage — **Shipped** *(5 Augus

### Epic 35 — Collaboration *(Demo 9)*

- [~] Collaboration: threads and proposals — **backend Slices A–F shipped, 6 August 2026**: `T

### Epic 36 — Reference Applications *(Demo 9)*

- [x] Reference applications (Python, published surfaces only) — **Shipped, 8 August 2026: all

### Epic 37a — Scale Validation *(Demo 9)*

- [~] 100k-entity scale validation — **Slices A–F shipped, 8 August 2026 — epic complete, with

### Epic 37b — Backup & Portability *(Demo 9)*

- [x] Backup, export, restore — **Slices A–E shipped, F's documentation shipped** *(5 August 2

### Epic 37c — Embeddable Library ★ *(Demo 9)*

- [~] Embeddable library — **Slices A–D shipped** *(4 August 2026)*, **Slice E shipped partial

### Epic 38 — Graph Analytics *(Demo 9)*

- [~] Analytics: degree, components, orphans, silos — **Slices A–D shipped, 8 August 2026**: `

### Epic 39 — Console Foundation, Discovery & Entity Pages *(Demo 1)*

- [x] SPA embedded in the binary via `rust-embed`, one process
- [x] Hierarchy tree with lazy children
- [x] Entity page: breadcrumb, properties, children table
- [x] Search across name and FQN
- [x] Empty-database first-run state that offers the next action
- [x] Trust bar that states what it does not know yet
- [x] Deep-linkable selection (`?asset=`)
- [x] OIDC/PKCE login
- [x] Generated API client
- [x] Slice E — shared trust component set
- [~] Slice F — states, budgets, and journeys
- [x] Hierarchy tree, asset detail, and the five-level service → column navigation
- [x] Trust bar: version, last editor, and honest "not captured yet" for certification and lin
- [x] Version history tab with the diff viewer
- [x] Inline description editing writing straight through `PATCH`
- [x] Connectors catalogue page; Postgres available, the rest listed as unavailable rather tha
- [x] Light/dark theme, light by default, deep-linkable via `?theme=`
- [x] Search box over name and FQN
- [x] Facet rail over kind and schema
- [x] Keyboard navigation
- [x] Unblocked and clear
- [x] A quiet gap found in the completion audit, fixed 8 August 2026
- [x] OIDC/PKCE sign-in
- [x] Three outcomes, three screens
- [x] The token has **one owner**
- [x] Owner and team display

### Epic 40 — Graph Explorer, Lineage & Time Travel ★ *(Demo 3)*

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
- [x] Derived edges visually distinct

### Epic 41 — Query Workbench, Governance & Admin *(Demo 4)*

- [x] Violations queue
- [x] Both engines triggerable from the console
- [x] SPARQL editor with plan display
- [x] Results as table ⇄ graph
- [x] Violations carry waivers
- [x] A waiver survives the next pass
- [x] Violations are assignable
- [x] The derivation chain rendered beside a derived fact
- [x] Policy dry-run
- [x] Connector configuration with write-only secrets
- [x] Admin: the section exists
- [x] Memory panel and memory administration

### Epic 42 — Semantic Browse, Review Queues & Agent Activity *(Demo 7)*

- [x] One vocabulary browser over glossary, tags, domains, packs — **Slices A–B shipped, 7 Aug
- [x] One review queue over four proposal sources — **Slices C–D shipped, 7 August 2026, propo
- [~] Agent activity audit — **shipped 7 August 2026, write-backs only**: see the full account
- [x] A text-first ontology editor, graph as feedback — **Slice G shipped, 7 August 2026**: a 

### Epic 43 — Agent Framework Integrations *(Demo 5)*

- [x] LangChain retriever preserving provenance and confidence — `_core/rendering
- [x] LangGraph toolkit, manifest-parity with MCP — `tools
- [x] Checkpointer over Epic 31, retraction not deletion — `memory
- [x] Zero graph-owl crate changes, asserted structurally — `tests/test_no_crate_change
- [x] The core client
- [x] Live-service CI (`langchain-integration` job, `scripts/verify-langchain

### Epic 93 — Console Overview *(Demo 3)*

- [x] `GET /overview` — one request for the whole landing page, authorization-filtered through
- [x] Graph tile reports `nodes`/`edges`, not only a flake total

### Epic 94 — RDF 1.2 Alignment *(Demo 10)*

- [x] `FlakeValue::TripleTerm` at discriminant 10, pinning test extended
- [x] `rdf:reifies` + triple term on export; store flake count unchanged
- [x] `rdf:dirLangString` — shipped as a new `FlakeValue::LangString` variant, a deliberate de
- [~] C (console)
- [x] `rdf:reifies` synthesised at the query surface, so the standard vocabulary returns rows 
- [x] Slices B, C and D share one `oxrdf/rdf-12` feature gate — one decision, taken once for t
- [x] `GET /graph/export/rdf` — found and fixed 8 August 2026
- [ ] Export dialog offers RDF as one of its format choices and previews it *(UI → Epic 42)* —

### Epic 95 — OWL 2 RL Completion *(Demo 10)*

- [x] The four RL axioms in scope beyond Epic 6's eight
- [x] Explanation panel extends to the new axioms — same surface, more rules *(UI → Epic 41, c

### Epic 96 — SHACL-SPARQL *(Demo 10)*

- [x] `sh:sparql`/`sh:SPARQLConstraint`, the bare constraint — shipped 8 August 2026
- [x] The bare constraint is now actually reachable — found and fixed 8 August 2026
- [ ] SPARQL-based constraint components (`sh:SPARQLConstraintComponent`, `sh:parameter`, `sh:
- [ ] The violations workflow is unchanged; **authoring gains a second language**, so the cons

### Epic 97 — Incremental & Parallel Reasoning *(Demo 10, 12)*

- [x] Incremental maintenance rather than full recomputation
- [x] `POST /reasoning/runs` can now actually take the incremental path over HTTP — found and 
- [ ] Overlay staleness is visible

### Epic 98 — OWL 2 EL Reasoning *(Demo 11, 12)*

- [x] EL and QL reasoners alongside RL — **Both shipped, 5 August 2026
- [x] `qlRewrite`/`refusedAxioms` now actually reach `/sparql`'s response — found and fixed 8 
- [ ] No UI of their own
- [x] Epic 98's EL classification is now actually reachable — found and fixed 8 August 2026

### Epic 99 — OWL 2 QL Reasoning *(Demo 11, 12)*

- [x] EL and QL reasoners alongside RL — **Both shipped, 5 August 2026
- [x] `qlRewrite`/`refusedAxioms` now actually reach `/sparql`'s response — found and fixed 8 
- [ ] No UI of their own
- [x] Epic 98's EL classification is now actually reachable — found and fixed 8 August 2026

### Epic 100 — Ontology Profile Detection & Routing *(Demo 11, 12)*

- [x] Detection across RL, EL, QL; incomparable profiles not reported as supersets
- [x] Out-of-profile reasoning refused, naming the first offending axiom
- [x] Override marks the result partial, carrying what was ignored
- [x] Detection over 400k axioms completes in seconds
- [ ] Profile badge + the reasoner that produced each derivation
- [ ] Out-of-profile and override-partial results marked
- [x] Routing is now actually enforced on the live reasoning path — found and fixed 8 August 2

### Epic 101 — SPARQL Federation — `SERVICE` *(Demo 11)*

- [x] An unlisted `SERVICE` endpoint is refused by name
- [x] `SERVICE` against an allow-listed endpoint joins correctly, bounded by a timeout
- [x] `SILENT` is honoured; endpoints that answered or failed are named
- [x] Bindings denied by policy never transmitted
- [x] Remote endpoints named at the result level, `SILENT` failures visible, allow-list admin 

### Epic 102 — Read/Write Partition Split *(Demo 11)*

- [x] The split itself.
- [x] Compaction trigger + partition-health metric — found and fixed 8 August 2026
- [ ] A partition-health panel in admin *(UI → Epic 41 Slice G)* — the metric now exists to bu

### Epic 103 — In-Process Traversal *(Demo 11)*

- [x] The traversal path — `graph-owl-traversal-memory::InMemoryTraversalEngine`, a second `Tr
- [x] Authorization holds through the real adapter, not just a mock — `graph-owl-api`'s new `t
- [x] Measured, not assumed
- [x] A real, independent production bug found and fixed getting to that number
- [ ] No UI

### Epic 104 — Ontology Alignment & Curated Mappings *(Demo 12)*

- [x] The alignment fact and its store — `Alignment::Match`/`EquivalentClass`, stored as flake
- [x] UMLS RRF ingestion, resumable — `graph_owl_connectors::umls` parses real `MRCONSO.RRF` t
- [x] Cross-vocabulary traversal — SNOMED reaches its `RxNorm` counterpart through a shared CU
- [x] Computed alignment, confirmation, and the review queue — `graph_owl_core::extraction::Di
- [x] `POST /alignments` and `GET /alignments/review` — found and fixed 8 August 2026
- [ ] Console

## Epics with slice marks and no plan file

None — every epic tracked in `DEMOS.md` has a plan.
