# graph-owl — Demo Tracer

**Purpose**: sequence every epic and slice into demos that are *cumulative* — Demo N contains everything from Demo N−1 and adds to it. Each demo is a state the whole application can be run in and shown, not a milestone on paper.

**How to read the marks**: `[x]` shipped and tested · `[~]` partially shipped, gap named · `[ ]` not started.

**Domain**: Indian retail and corporate banking. Chosen because it exercises the parts of this system that a toy schema does not — PII classification, regulatory lineage, data residency, and the difference between an asset that is *wrong* and one that is *unreported*.

---

## Demo status

| Demo | Theme | Epics | State |
|---|---|---|---|
| **1** | A source becomes a browsable catalog | 1, 2, 15, 39 (partial) | **Shipped** — + deletion detection |
| **2** | A governed catalog people can trust | +3, 8, 10, 11, 12, 13 | **Shipped** — + `If-Match`/412, console sign-in, facets (gaps named per epic) |
| **3** ★ | It is a graph engine | +4, 7, 7a, 40, 93 | **Mostly shipped** — Epic 4 A–G, 7a core, 40 core, 93 Overview. **Epic 7 (SPARQL) not started** |
| **4** | It reasons, and it validates | +5, 6, 41 | |
| **5** ★ | Agents can use it | +14, 31, 32, 43 | |
| **6** | It fills itself | +16, 17, 18, 19, 20, 21 | |
| **7** | Business meaning and trust signals | +22–30, 42 | |
| **8** | Property graph and open interop | +7b, 7c, 7d, 9, 9a | |
| **9** | Breadth, scale, and the proof | +33–38, 36, 37a–c | |
| **10** | Standards depth | +94–97 | Not started — see `00k-standards-conformance.md` |
| **11** | Full semantics | +98–102 | Not started — three OWL profiles, federation, storage split |

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
- [x] Deletion detection with a threshold guard — off by default; a refusal deletes nothing at all; a source reporting almost nothing is caught by the threshold and names what it saw *(Epic 15)*
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

## Demo 2 — A governed catalog people can trust · **SHIPPED**

**The claim**: the catalog knows *who changed what, when, and why you should believe it* — and only shows you what you are allowed to see.

**What you can show**: edit a table's description; see the version go `0.1 → 0.2` with a field-level diff and your name on it; soft-delete it and restore it; search `"upi"`; then run the same search as a risk analyst and watch the PII schema vanish from the results, the counts *and* the facets.

**Status**: the demo moment below is verified live. Every epic below lists what
shipped, what is still **pending** in that epic, and what was **deferred** with
its destination — so "Demo 2 is done" never has to mean "Epic 13 is finished".

### Epic 3 — Envelope, versioning, soft delete, change events
- [x] `EntityEnvelope` on every asset: version, `updatedAt`, `updatedBy`, `changeDescription`
- [x] Major/Minor version arithmetic; a no-op update produces no version
- [x] Field-level `ChangeDescription` diffs (added/updated/deleted); breaking-change classification
- [x] `PATCH /assets/{id}` with server-computed diffs
- [x] Soft delete cascading to the subtree, with restore; a connector re-run does not resurrect a tombstone
- [x] `GET /assets/{id}/versions` — snapshot per version, newest first
- [x] Console: trust bar shows version and last editor; History tab with a field-level diff timeline; inline description editing
- [x] `If-Match`/`412` optimistic concurrency — a stale precondition is refused and names the current version; absent the header, last-write-wins as documented

**Pending in this epic**
- `EventSink` port + `ChangeEvent` emission — nothing consumes change events yet, which is also what blocks Epic 8's incremental indexing

### Epic 8 — Search
- [x] Facets by kind and schema, computed over the **visible** set
- [x] Result counts consistent with authorization filtering

**Pending in this epic**
- Still `LIKE` over name and FQN, not a real index: no BM25, no relevance ranking, no description search. Adequate at 124 assets, not at 100k
- `TextIndex` port and event-driven incremental indexing — blocked on Epic 3's `EventSink`

**Deferred**
- Vector index and embeddings → generated out of process, per `00j-language-boundaries.md`

### Epic 10 — Operability
- [x] `/health` (checks nothing, so a dependency blip cannot restart-loop the fleet)
- [x] `/ready`, three-valued: required vs optional checks, `200 degraded` when auth is off
- [x] Graceful shutdown draining in-flight requests
- [x] Startup states its security posture — an accidentally-open server must not look identical to a secured one
- [x] `BIND_ADDR` configurable

**Pending in this epic**
- Structured JSON logs and request-id propagation
- `/metrics`
- Memory budget report (the input `00a`'s footprint claim needs to stay honest)

### Epic 11 — Users, teams, ownership
- [x] `User` with roles; auto-provisioned on first sight
- [x] `owner_id` on assets (nullable, so the gap is visible rather than prevented)

**Deferred**
- Teams, ownership inheritance, and the ownership gap report → Demo 7, where domains land and give inheritance something to inherit along

### Epic 12 — Authentication
- [x] JWT verification (HS256, shared secret); a forged token is rejected
- [x] **The `Principal` extractor swap** — one function changed, no handler touched
- [x] Auto-provision a `User` on first sight, with no roles
- [x] Open mode when no secret is configured, logged as such at startup

**Pending in this epic**
- JWKS and key rotation — the swap point is `signing_secret()`, so this is a function body, not a refactor

**Deferred**
- OIDC/PKCE in the console, tokens in memory only → paired with Epic 39's login, below; neither is useful without the other

### Epic 13 — Authorization
- [x] `AccessPredicate` in `graph-owl-authz` — pure, zero surviving mutants
- [x] Lowered to SQL for list, search, children and counts
- [x] Deny-overrides, order-independent; an unmatched request denies
- [x] `MetadataOperation` vocabulary, append-only
- [x] **Row-level filtering — the PII demo**: two principals, one search, different results
- [x] Counts filtered through the same predicate, so a total cannot leak what it hid
- [x] Hidden reads as `404`, not `403` — a `403` on an id confirms the id exists

**Pending in this epic**
- No decision cache; every request recompiles the predicate. Correct, and not yet fast

**Deferred**
- Column-level (as opposed to row-level) masking → needs Epic 25's classifications to know *which* columns carry what

### Epic 39 — Console
- [x] Hierarchy tree, asset detail, and the five-level service → column navigation
- [x] Trust bar: version, last editor, and honest "not captured yet" for certification and lineage
- [x] **Version history tab with the diff viewer** — field-level, before and after, newest first
- [x] Inline description editing writing straight through `PATCH`
- [x] Connectors catalogue page; Postgres available, the rest listed as unavailable rather than hidden
- [x] Light/dark theme, light by default, deep-linkable via `?theme=`
- [x] Search box over name and FQN

**Pending in this epic**
- **Facets are returned by the API and not rendered by the console** — `GET /assets/search` already computes them over the visible set (`graph-owl-server/src/lib.rs`), so this is a console-side gap only, and it is the cheapest visible win left in Demo 2
- Keyboard navigation through results — `00f`'s non-negotiable, currently unmet

**Deferred**
- Login, session, and the denied-vs-empty distinction → blocked on Epic 12's OIDC/PKCE. The console currently runs against an open server or a token supplied out of band; "denied" and "empty" therefore look identical to it, which is exactly the state `00f` says is not acceptable to ship to a user
- Owner and team display → blocked on Epic 11's teams

**The demo moment — verified live against the 124-asset bank estate:**

```
        stats_total  listed  search 'customers'
root         124       124          13
asha          93        93           0
```

`core_banking` — the schema holding PAN, Aadhaar and CKYC — is absent from
Asha's rows, her counts, *and* her facets. `stats_total == listed` for both, so
the count cannot leak what it hid. A hidden asset reads `404` for her and `200`
for root: a `403` would have confirmed the id exists.

**What Demo 2 does not claim.** Authorization is enforced in SQL on every read
path listed above, and is *not* enforced over the flake projection — by design
(`04-engine-triples.md` decision 7). Demo 3 adds that projection, so the
predicate has to reach it before any graph query is exposed to a real user.

---

## Demo 3 — It is a graph engine ★

**The claim**: this is not a catalog with a lineage feature. It is a graph with time travel, and you can see the estate as it stood on any past date.

**What you can show**: open the Graph tab on `upi_transactions`, switch between 1/2/3 hops and watch the neighbourhood grow; set the time chip to before an edit and see the asset come back at its older version with its older description.

**What this demo cannot yet show**, stated plainly:
- **No SPARQL yet.** Epic 7 is not started. Its scope collapsed on 28 July 2026 — the evaluator is adopted and only the flake-side dataset is ours — so this is now the nearest large win rather than the largest remaining build.
- **The time control moves the asset read and the graph walk, not the tree or search.** `?asOf=` is answered per asset and per traversal; the hierarchy and the search index still show the present.
- **Nothing declares lineage yet.** The graph is real, but its edges are the hierarchy plus any relationship a client creates by hand — no connector infers `feeds`.

**The column-reappears moment now works.** Epic 15's deletion detection landed: drop a column from the source, re-run with `detectDeletions`, and it is tombstoned now and live at an instant before the run — reconstructed from the graph, not read from a snapshot.

### Epic 4 — Triple storage & time travel ★
- [x] `Flake` in `graph-owl-core`; ten pinned `FlakeValue` variants *(Slice A)*
- [x] Namespace code registry — constants allocated and range-tested; runtime namespaces persisted by the predicate registry *(Slice H)*
- [x] Four index orderings: SPOT, PSOT, POST, OPST — each verified by `EXPLAIN` naming the index over a 100k-flake table *(Slice B)*
- [x] `op = false` is a retraction, not a delete — assert/retract/assert/retract verified in both directions, scoped by value, predicate and graph; the assertion row survives *(Slice C)*
- [x] Entity → flake projection — wired into both write paths; a catalogue run of the 124-asset estate produces 1,234 flakes over 124 subjects *(Slices D, G)*
- [x] Reified relationships — each edge is a node of its own carrying `rdf:type`, both endpoints as `Ref` (so OPST reverse traversal reaches them), its type and both endpoint kinds; deleting an edge retracts every one of its flakes *(Slice E)*
- [x] **As-of query API** — `GET /assets/{id}?asOf=<rfc3339>` reconstructs the entity from flakes; 5 HTTP tests *(Slice F)*
- [x] Reconciliation and drift metric — drift computed by comparison rather than from a queue (a queue can be lost; comparison cannot miss); `POST /graph/reconcile` repairs and reports; one-directionality asserted structurally by a fake that panics on any relational write *(Slice G)*
- [~] Runtime predicate registry — define/lookup/list, duplicate refused, core vocabulary seeded by migration and immutable, cardinality recorded per predicate *(Slice H)*; **gap**: cardinality is recorded but not yet *enforced* on write (enforcement is a constraint → Epic 5), and an unregistered predicate is not yet rejected
- [ ] `rdf:reifies` + triple terms → **Epic 94**. The reified edges already shipped *are* RDF 1.2's reifier shape; only the vocabulary is missing (`04-engine-triples.md` finding 5)
- [ ] Language-tag side table → **Epic 94**, and it needs three components not two: `rdf:dirLangString` carries a base direction

### Epic 7 — SPARQL ★

**Scope changed 28 July 2026** (`07` decisions 8–9, `00l`). Parsing, optimisation,
join execution, expressions, aggregates and result serialisation are **adopted**
from permissively-licensed crates. What this epic builds is the
`QueryableDataset` implementation over flakes — which is where the three things
no library can supply actually live.

- [x] **`QueryableDataset` over flakes** — the whole of this epic's own content *(Slice A)*. Real SPARQL runs: BGP, two-hop join, numeric FILTER, OPTIONAL, ASK, named-graph isolation
- [ ] Index selection across the four orderings, inside the scan
- [ ] `as_of` — the dataset is constructed at a transaction time
- [ ] Authorization compiled into the scan, so the evaluator only ever sees permitted rows
- [ ] Resource tracking (`Tracker`) — **nothing adopted enforces budgets; this is ours**
- [ ] Freshness stamping on the result (Epic 4 decision 8)
- [x] ~~Parser~~ — `spargebra`, full SPARQL 1.1 *(adopted)*
- [x] ~~Planner / execution~~ — `spareval` *(adopted)*
- [ ] Verify: does `sparopt`'s generic rewriting fight index selection? Measure before trusting

### Epic 7a — Traversal
- [x] One frontier primitive (recursive CTE, one statement); `neighbours` and `subgraph` over it *(Slices A, B, E)*
- [x] Budgeted, cycle-safe, truncation always visible and farthest-first
- [x] Reified two-hop edges hidden — five logical edges reports distance five
- [x] `as_of` on every walk, so time-travelling traversal is free
- [x] `shortest_path`, `all_paths`, `detect_cycles` *(Slices C, D)* — deterministic tiebreak, hard path cap, cycles normalised so rotations are one cycle

### Epic 40 — Graph explorer ★
- [x] Renderer-agnostic `GraphView` — the shape a Sigma canvas would consume unchanged
- [x] Graph tab with a deterministic radial layout, hop selector, truncation shown
- [x] **Time control** — `?asOf=` on the walk and on the asset read
- [x] Non-visual equivalent: the same neighbourhood as a keyboard-navigable table
- [ ] Sigma/WebGL canvas with expand-on-click — the hand-drawn SVG is honest at demo scale and will not survive 10k nodes
- [ ] React Flow + ELK lineage DAG
- [ ] Diff mode (two instants side by side)
- [ ] Derived edges visually distinct — nothing derives edges until Epic 6

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
