# graph-owl — Demo Tracer

**Purpose**: sequence every epic and slice into demos that are *cumulative* — Demo N contains everything from Demo N−1 and adds to it. Each demo is a state the whole application can be run in and shown, not a milestone on paper.

**How to read the marks**: `[x]` shipped and tested · `[~]` partially shipped, gap named · `[ ]` not started.

**Domain**: Indian retail and corporate banking. Chosen because it exercises the parts of this system that a toy schema does not — PII classification, regulatory lineage, data residency, and the difference between an asset that is *wrong* and one that is *unreported*.

---

## Demo status

| Demo | Theme | Epics | State |
|---|---|---|---|
| **1** | A source becomes a browsable catalog | 1, 2, 15, 39 (partial) | **Shipped** |
| **2** | A governed catalog people can trust | +3, 8, 10, 11, 12, 13 | Next |
| **3** ★ | It is a graph engine | +4, 7, 7a, 40 | |
| **4** | It reasons, and it validates | +5, 6, 41 | |
| **5** ★ | Agents can use it | +14, 31, 32, 43 | |
| **6** | It fills itself | +16, 17, 18, 19, 20, 21 | |
| **7** | Business meaning and trust signals | +22–30, 42 | |
| **8** | Property graph and open interop | +7b, 7c, 7d, 9, 9a | |
| **9** | Breadth, scale, and the proof | +33–38, 36, 37a–c | |

★ = the demo that carries a differentiator. Cutting it is a positioning decision.

---

## Demo 1 — A source becomes a browsable catalog · **SHIPPED**

**The claim**: point graph-owl at a bank's core-banking Postgres and get a navigable inventory of every schema, table and column, from one binary.

**What you can show**: run the connector, watch 34 assets appear, expand `service → database → schema → table → column`, click a column, read its type and nullability, follow the breadcrumb back to the service.

### Epic 1 — API conventions & contract
- [x] **A** Errors are RFC 9457 problem+json with stable `type` URIs
- [x] **B** Validation reports every field violation at once, not the first
- [x] **C** Cursor pagination, keyset not offset
- [x] **D** camelCase on the wire; conflict taxonomy split by kind
- [x] **E** One `CatalogError` across the facade
- [x] **F** Closed relationship vocabulary with a legality table
- [x] **G** `Principal` seam through every mutating handler
- [x] **H** Unknown query parameters rejected and named
- [x] **I** `Location` header on creates, asserted against the returned id
- [ ] **J** OpenAPI generated from code, committed, diffed in CI
- [ ] **K** Generated client round-trips against a running service

### Epic 2 — Entity hierarchy & columns
- [x] `Asset` + `AssetKind` for all five levels, one type not five
- [x] FQN derivation (`fqn::derive`, `fqn::child_of`, `parent`, `leaf`)
- [x] Containment rule in one place (`AssetKind::parent_kind`)
- [x] Hierarchy endpoints: roots, children, ancestors, search, stats
- [~] **Gap**: no PATCH/DELETE on assets; cascade delete is a DB constraint with no test
- [ ] Non-database services (dashboard, pipeline, ML) → deferred to Epic 34

### Epic 15 — Source connectors
- [x] `Connector` trait, `SourceRecord`, `RunScope`
- [x] Postgres reference connector reading `information_schema`
- [x] Parents-before-children ordering as a connector contract
- [x] Re-runs converge (FQN is the identity, not the generated id)
- [x] Run report names each failure and its reason
- [x] System schemas excluded; views catalogued and marked
- [ ] Deletion detection with a threshold guard
- [ ] Scheduled runs, run history persistence
- [ ] `source_hash` fingerprinting to skip unchanged records

### Epic 39 — Console foundation
- [x] SPA embedded in the binary via `rust-embed`, one process
- [x] Hierarchy tree with lazy children
- [x] Entity page: breadcrumb, properties, children table
- [x] Search across name and FQN
- [x] Empty-database first-run state that offers the next action
- [x] Trust bar that states what it does not know yet
- [x] Deep-linkable selection (`?asset=`)
- [ ] OIDC/PKCE login, tokens in memory only
- [ ] Generated API client (blocked on Epic 1 Slice J)

**Known issues carried forward**
- `/assets/{id}` is an API namespace; any client-side route under it is unreachable. Prefixing the API is the fix, and belongs with Epic 1 Slice J.
- The trust bar is empty because Epic 3 has not landed.

---

## Demo 2 — A governed catalog people can trust · **NEXT**

**The claim**: the catalog knows *who changed what, when, and why you should believe it* — and only shows you what you are allowed to see.

**What you can show**: edit a table's description; see the version go `0.1 → 0.2` with a field-level diff and your name on it; soft-delete it and restore it; search `"upi"` and get ranked results with facets; log in as a risk analyst and watch PII columns disappear from the same search.

### Epic 3 — Envelope, versioning, soft delete, change events
- [x] `EntityEnvelope` on every asset: version, `updatedAt`, `updatedBy`, `changeDescription`
- [x] Major/Minor version arithmetic; a no-op update produces no version
- [x] Field-level `ChangeDescription` diffs (added/updated/deleted); breaking-change classification
- [x] `PATCH /assets/{id}` with server-computed diffs
- [x] Soft delete cascading to the subtree, with restore; a connector re-run does not resurrect a tombstone
- [x] `GET /assets/{id}/versions` — snapshot per version, newest first
- [x] Console: trust bar shows version and last editor; History tab with a field-level diff timeline; inline description editing
- [ ] `EventSink` port + `ChangeEvent` emission
- [ ] `If-Match`/`412` optimistic concurrency

### Epic 8 — Search
- [ ] `TextIndex` port; lexical BM25 over name, FQN, description
- [ ] Event-driven incremental indexing (never a dual write in the request path)
- [ ] Facets by kind, schema, owner
- [ ] Search result counts consistent with authorization filtering
- [ ] Vector index deferred; embeddings generated out of process (`00j`)

### Epic 10 — Operability
- [ ] Typed config from environment, validated at startup
- [ ] `/health` vs `/ready`, three-valued with required/optional checks
- [ ] Structured JSON logs, request-id propagation
- [ ] `/metrics` conforming to the observability contract
- [ ] Graceful shutdown draining in-flight requests
- [ ] Itemized memory budget reported at startup

### Epic 11 — Users, teams, ownership
- [x] `User` with roles; auto-provisioned on first sight
- [x] `owner_id` on assets (nullable, so the gap is visible rather than prevented)
- [~] **Gap**: no teams, no ownership inheritance, no gap report — deferred to Demo 7 where domains land

### Epic 12 — Authentication
- [x] JWT verification (HS256, shared secret); a forged token is rejected
- [x] **The `Principal` extractor swap** — one function changed, no handler touched
- [x] Auto-provision a `User` on first sight, with no roles
- [x] Open mode when no secret is configured, logged as such at startup
- [~] **Gap**: JWKS and key rotation not implemented; the swap point is `signing_secret()`
- [ ] OIDC/PKCE in the console; tokens in memory only

### Epic 13 — Authorization
- [x] `AccessPredicate` in `graph-owl-authz` — pure, zero surviving mutants
- [x] Lowered to SQL for list, search, children and counts
- [x] Deny-overrides, order-independent; an unmatched request denies
- [x] `MetadataOperation` vocabulary, append-only
- [x] **Row-level filtering — the PII demo**: two principals, one search, different results
- [x] Counts filtered through the same predicate, so a total cannot leak what it hid
- [x] Hidden reads as `404`, not `403` — a `403` on an id confirms the id exists
- [~] **Gap**: no decision cache; every request recompiles. Correct but not yet fast
- [ ] Column-level (as opposed to row-level) masking — needs Epic 25 classifications

### Epic 39 — Console, completed
- [ ] Login, session, denied-vs-empty states
- [ ] Version history tab with the diff viewer
- [ ] Search with facets and keyboard navigation
- [ ] Owner and team display

**The demo moment**: two logins, one search, different results — and the count is consistent, so the restricted user cannot infer what was hidden.

---

## Demo 3 — It is a graph engine ★

**The claim**: this is not a catalog with a lineage feature. It is a graph with time travel, and you can see the estate as it stood on any past date.

**What you can show**: open the explorer on `upi_transactions`, expand two hops, drag the time slider back to before a schema migration and watch a column reappear.

### Epic 4 — Triple storage & time travel ★
- [ ] `Flake` in `graph-owl-core`; ten pinned `FlakeValue` variants
- [ ] Namespace code registry
- [ ] Four index orderings: SPOT, PSOT, POST, OPST
- [ ] `op = false` is a retraction, not a delete
- [ ] Entity → flake projection; reified relationships
- [ ] As-of query API
- [ ] Reconciliation job and drift metric
- [ ] Language-tag side table

### Epic 7 — SPARQL subset ★
- [ ] Parser for BGP, FILTER, OPTIONAL, UNION, MINUS, (NOT) EXISTS, paths
- [ ] Planner with index selection and filter pushdown
- [ ] Batched pull execution model
- [ ] Authorization compiled into the query
- [ ] Resource tracking (`Tracker`)
- [ ] Fast-path routing for the five common shapes

### Epic 7a — Traversal
- [ ] One frontier primitive; neighbours, shortest path, all paths, cycles, subgraph
- [ ] Budgeted, cycle-safe, truncation always visible

### Epic 40 — Graph explorer ★
- [ ] Renderer-agnostic `GraphModel`
- [ ] Sigma/WebGL exploration canvas, expand-on-click
- [ ] React Flow + ELK lineage DAG
- [ ] **Time slider and diff mode**
- [ ] Derived edges visually distinct, not by colour alone
- [ ] Non-visual keyboard-navigable equivalent

---

## Demo 4 — It reasons, and it validates

**The claim**: it tells you what is broken and why it believes what it believes.

**What you can show**: a SHACL-style shape says "every table in `regulatory` must have an owner and a retention tag"; the violations queue fills; classify one table as PII and watch the classification propagate along lineage as a *derived* fact, visibly marked, with its derivation chain.

### Epic 5 — Constraint validation
- [ ] Shape and constraint types; six target kinds
- [ ] Compile-once, evaluate-many
- [ ] Continuous validation with violation reports, not write-time rejection
- [ ] Severity classification; repair suggestions never auto-applied

### Epic 6 — Reasoning overlay
- [ ] Eight OWL 2 RL axioms as built-in rules
- [ ] Semi-naive fixpoint, `CappedReason` on every limit
- [ ] Derived facts in `graph:reasoning`, never persisted into the base
- [ ] `GET /reasoning/explain` derivation chains
- [ ] Standard rule set: classification along lineage, ownership down containment

### Epic 41 — Workbench & governance
- [ ] SPARQL editor with plan display
- [ ] Results as table ⇄ graph
- [ ] Violations as an assignable workflow with waivers
- [ ] Admin: policies with dry-run, connectors, jobs

---

## Demo 5 — Agents can use it ★

**The claim**: an agent asks "is `upi_transactions` safe to build a fraud model on?" and gets a policy-filtered, provenance-carrying answer — plus the institutional memory of why the schema changed last quarter.

### Epic 14 — MCP + outbound events ★
- [ ] MCP server in Rust (`rmcp`), same `AccessPredicate` as HTTP
- [ ] Seven read tools; trust summaries and gaps
- [ ] Token-budgeted responses
- [ ] Outbound webhooks, HMAC-signed, at-least-once

### Epic 31 — Organizational memory ★
- [ ] Memory objects: kind, content, authorship, confidence, `as_of`
- [ ] Supersession and contradiction detection
- [ ] Retrieval with reranking

### Epic 32 — Agent capabilities
- [ ] Write-back with agent authorship
- [ ] Investigation and remediation proposals

### Epic 43 — Framework integrations
- [ ] LangChain retriever preserving provenance and confidence
- [ ] LangGraph toolkit, manifest-parity with MCP
- [ ] Checkpointer over Epic 31, retraction not deletion
- [ ] Zero graph-owl crate changes, asserted

---

## Demo 6 — It fills itself

**The claim**: the catalog is populated from every shape of source without duplicating anything.

### Epic 16 — Ingestion APIs & SDKs
- [ ] Push API with partial success and idempotency keys
- [ ] Batch file ingestion
- [ ] Generated TypeScript and Python SDKs

### Epic 17 — Entity resolution
- [ ] Deterministic + probabilistic matching
- [ ] Reversible `sameAs` merge
- [ ] Merge adjudication queue (Epic 42)

### Epics 18, 19 — Inbound events, streaming
- [ ] Webhook registry, signature verification, replay
- [ ] Broker consumption with consumer-group rebalancing

### Epic 20 — Metadata-as-code ★
- [ ] `plan` / `apply` / `diff` with scoped authority
- [ ] Drift reported, never auto-corrected

### Epic 21 — Document ingestion
- [ ] Python worker: PDF/OCR/chunking → extraction named graph
- [ ] Extraction review queue with source-span evidence

### Epic 15 — Connectors, completed
- [ ] Deletion detection, threshold guard
- [ ] `source_hash` fingerprinting
- [ ] Python connector protocol + one non-Postgres source

---

## Demo 7 — Business meaning and trust signals

**The claim**: the catalog carries what the business means, not just what the database contains.

### Epics 22–30
- [ ] **22** Custom properties, JSON-Schema validated
- [ ] **23** Domains and data products
- [ ] **24** Glossary with SKOS relations; metrics as entities
- [ ] **25** Classifications with mutual exclusivity — the PII taxonomy
- [ ] **26** Lifecycle and certification with issuer and expiry
- [ ] **27** Data contracts and compatibility
- [ ] **28** Usage and popularity signals
- [ ] **29** Lineage: table, column, with SQL and pipeline payload
- [ ] **30** Quality: test definitions, suites, results, incidents

### Epic 42 — Semantic surfaces
- [ ] One vocabulary browser over glossary, tags, domains, packs
- [ ] One review queue over four proposal sources
- [ ] Agent activity audit

---

## Demo 8 — Property graph and open interop

**The claim**: connect with the driver you already have, run the Cypher you already know, and get time travel the database you think you are talking to does not have.

### Epics 7b, 7c, 7d, 9, 9a
- [ ] **7c** Bidirectional flake ⇄ LPG projection, losses enumerated
- [ ] **7b** openCypher lowering onto the same plan (ships *after* 7c)
- [ ] **7d** Bolt server: PackStream, handshake, state machine (ships after Epic 12)
- [ ] **9** JSON-LD, Turtle, DCAT, PROV-O, OpenLineage
- [ ] **9a** GraphML, bulk CSV, projection targets

---

## Demo 9 — Breadth, scale, and the proof

### Epics 33–38, 36, 37a–c
- [ ] **33** Domain ontology packs — a banking/BFSI pack
- [ ] **34** Entity expansion: dashboards, pipelines, topics, models, APIs
- [ ] **35** Collaboration: threads and proposals
- [ ] **36** Reference applications (Python, published surfaces only)
- [ ] **37a** 100k-entity scale validation
- [ ] **37b** Backup, export, restore
- [ ] **37c** Embeddable library, `graph-owl-storage-memory` published
- [ ] **38** Analytics: degree, components, orphans, silos

---

## Rules for this tracer

1. **Cumulative, always.** Demo N runs everything Demo N−1 ran. A regression in an earlier demo blocks the later one.
2. **A demo is a runnable state**, not a checklist. If it cannot be shown end to end, it is not done regardless of how many boxes are ticked.
3. **`[~]` requires a named gap.** A partial tick without a stated hole is a full tick pretending to be honest.
4. **Update this file in the same commit** as the slice it records. A tracer updated separately drifts within a week.
