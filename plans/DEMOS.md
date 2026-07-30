# graph-owl — Demo Tracer

**Purpose**: sequence every epic and slice into demos that are *cumulative* — Demo N contains everything from Demo N−1 and adds to it. Each demo is a state the whole application can be run in and shown, not a milestone on paper.

**How to read the marks**: `[x]` shipped and tested · `[~]` partially shipped, gap named · `[ ]` not started.

**Domain**: Indian retail and corporate banking. Chosen because it exercises the parts of this system that a toy schema does not — PII classification, regulatory lineage, data residency, and the difference between an asset that is *wrong* and one that is *unreported*.

---

> **Per-epic status across all 62 epics** — slice counts, state and
> dependencies — is generated into [`EPIC-STATUS.md`](EPIC-STATUS.md) *from this
> file*, so it cannot disagree with it. Regenerate with
> `python3 scripts/epic-status.py`.

## What is left for Demos 1–3

*Maintained in the same commit as the work, per rule 4. Each line is a gap in
the code, checked against the code — not copied forward from the previous
revision (rule 0's corollary).*

**Nothing.** Demos 1, 2 and 3 are complete — see the table below.


**Deferred *by design*, and not Demo 1–3 debt** — listing them as gaps would be
scope confusion: `rdf:reifies` and the language-tag side table → Epic 94 ·
non-database services → Epic 34 · derived edges drawn distinctly → Epic 6 ·
column-level masking → Epic 25 · teams and ownership inheritance → Demo 7 ·
vector embeddings → out of process per `00j`.

## Demo status

| Demo | Theme | Epics | State |
|---|---|---|---|
| **1** | A source becomes a browsable catalog | 1, 2, 15, 39 (partial) | **Complete** — deletion detection, `source_hash` skip, run history, and a generated client round-tripped against a live service |
| **2** | A governed catalog people can trust | +3, 8, 10, 11, 12, 13 | **Complete** — `If-Match`/412, OIDC sign-in, ranked search, and Epic 10 at 17/17 (budget, admission control, spans, gauges) |
| **3** ★ | It is a graph engine | +4, 7, 7a, 40, 93, **29** | **Complete** — Epic 4 A–H, 7 A–C, 7a core, 40 A/B/D, 93 Overview, a Cytoscape/WebGL explorer, and the lineage DAG. **Epic 29 Slices A–C were pulled forward from Demo 7** to supply the lineage the DAG draws |
| **4** | It reasons, and it validates | +5, 6, 41 | |
| **5** ★ | Agents can use it | +14, 31, 32, 43 | |
| **6** | It fills itself | +16, 17, 18, 19, 20, 21 | |
| **7** | Business meaning and trust signals | +22–30, 42 | |
| **8** | Property graph and open interop | +7b, 7c, 7d, 9, 9a | |
| **9** | Breadth, scale, and the proof | +33–38, 36, 37a–c | |
| **10** | Standards depth | +94–97 | Not started — see `00k-standards-conformance.md`. Epic 94 is a **vocabulary** change, not a model one: the reified edges already shipped *are* RDF 1.2's reifier shape, so the flake count must not move |
| **11** | Full semantics | +98–103 | Not started — three OWL profiles, federation, storage split |
| **12** | Large ontologies, honestly | +104, and the recalibration of 6/97/98/99/100 | Not started — **the fork was taken 28 Jul 2026**; `00a` adopts the engine framing. See `00n-large-ontology-reality.md` |

**Demo 12 exists because a requirement was stated that the other eleven do not serve**: FIBO, UMLS, SNOMED CT, RxNorm and DBpedia at 10⁸–10⁹ triples. `00n-large-ontology-reality.md` is the honest assessment. The short version: **OWL 2 RL cannot classify SNOMED CT** — RL and EL are incomparable profiles, so an RL run yields a *wrong* hierarchy rather than a smaller one — and Demo 4's reasoning claim silently stops being true the moment a clinical ontology is loaded. Three epic triggers fired as a result (97, 98, 100), one epic was created (104), and the reasoning budgets in Epic 6 are calibrated three orders of magnitude below the stated requirement. **That decision has been taken** — `00a` now commits to EL + RL + QL, profile-routed, delivered in three phases. Note the sequencing correction it carries: **Epic 100 ships in Phase 1**, with the catalog, because profile detection is what makes RL-only honest; waiting until a clinical ontology loads is one load too late.

### Demo 12 — Large ontologies, honestly

**The claim**: it loads a real clinical ontology, classifies it with a reasoner that can, and says which reasoner answered.

**What you can show**: load SNOMED CT; watch profile detection report **EL, not RL**, and watch the RL engine *refuse* rather than produce a confidently wrong hierarchy. Classify with the EL reasoner. Ask for bacterial pneumonia and get streptococcal pneumonia patients back — the inference RL cannot draw. Then open the explanation and read which reasoner drew it, from which two asserted SNOMED axioms. Load UMLS and reach RxNorm from SNOMED through a curated CUI, with no matching algorithm having run.

- [ ] **100** Profile detection reports EL for SNOMED and refuses RL routing, naming the first out-of-profile axiom
- [ ] **98** EL classification via an adopted reasoner (`whelk-rs`, BSD-3 — `00l`), `StreptococcalPneumonia ⊑ BacterialPneumonia` derived
- [ ] **97** Incremental maintenance — reclassifying SNOMED per write is a non-starter, which is why this trigger fired
- [ ] **97** The overlay carries `maintained_to` — with incremental maintenance it **lags the base even at "now"**, so "current inferences" means current as of that watermark. A third time coordinate beside `as_of` and the projection lag, and it arrives with this epic
- [ ] **99** QL query rewriting for a DBpedia-shaped ABox: vast instances, thin TBox, **do not materialise**
- [ ] **104** UMLS RRF ingestion; CUI as a first-class identifier; SNOMED → RxNorm with **no computed matching**
- [ ] **104** A computed alignment cannot assert `owl:equivalentClass` — refused by the type system, not by a validator
- [ ] **6** Reasoning budgets re-derived for this scale — 100k facts and 512MB were calibrated for a 1M-flake catalog
- [ ] **4 / 37a** Partitioning trigger and write-path latency measured at 10M+ flakes
- [ ] **Console**: profile badge, reasoner attribution on every derivation, alignment review queue *(Epics 41, 42)*

**Every demo carries its console half.** A backend capability with no surface is a capability nobody can be shown, and `00a-product-position.md` sells differentiators that are seen rather than described. Where a demo line needs UI, it names the receiving UI epic in *(italics)*; where a capability deliberately has **no** UI, it says so with the reason, per `00h-ui-design-system.md`'s completeness requirement.

★ = the demo that carries a differentiator. Cutting it is a positioning decision.

## Epic coverage index — all 60, checkable

**Audited 28 July 2026: every epic with a plan file appears in a demo. None is orphaned.** (Re-audited the same day when Epic 104 was created; it is in Demo 12.) This index exists because that was true but *not verifiable* — epics live under grouped headings (`### Epics 22–30`, `### Epics 18, 19`, `### Epic 98 / 99`), so a mechanical search for "Epic 19" finds nothing and coverage could only be confirmed by reading the whole file. The next epic added would have gone missing silently. **Add a row here when adding an epic; a plan file with no row is the thing this table is for.**

| Demo | Epics covered |
|---|---|
| 1 | 1, 2, 15, 39 |
| 2 | 3, 8, 10, 11, 12, 13 |
| 3 ★ | 4, 7, 7a, 40, 93 |
| 4 | 5, 6, 41 |
| 5 ★ | 14, 31, 32, 43 |
| 6 | 16, 17, 18, 19, 20, 21 |
| 7 | 22, 23, 24, 25, 26, 27, 28, 29, 30, 42 |
| 8 | 7b, 7c, 7d, 9, 9a |
| 9 | 33, 34, 35, 36, 37a, 37b, 37c, 38 |
| 10 | 94, 95, 96, 97 |
| 11 | 98, 99, 100, 101, 102, 103 |
| 12 | 104, and the recalibration of 6, 97, 98, 99, 100 |

**Epics deliberately carrying no demo moment of their own**, rather than being missing:

| Epic | Why it has no demo beat |
|---|---|
| 34 Entity expansion | Adds five entity families and **no UI or demo step** — the composable entity page absorbs them. A demo beat here would mean Epic 39 decision 4 had failed |
| 36 Reference apps | The demo *is* someone else's application; it is listed in Demo 9 but has no beat inside this console |
| 43 Framework integrations | Same shape — the artefact runs in a user's repo, so the demo is external |
| 103 In-process traversal | A performance path. Its success looks identical to its absence, which is why its entry condition is a measurement in `37a` |

`90-` and `91-` are completed historical records, not epics, and are excluded by design.

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
- [x] **J** OpenAPI 3.1 generated from code, served at `/openapi.json`, committed and drift-guarded. **One route table, two consumers**: the spec is built from it and the router is asserted against it, so a route cannot be documented without existing *or* exist without being documented. Schemas are `ToSchema` derives on the domain types, so a field added to `Asset` reaches the contract without anyone remembering
- [x] **K** A TypeScript client is generated from `openapi.json` and driven against a live server: create → read → list → patch → delete, a typed problem+json error, and pagination following a cursor. `scripts/verify-generated-client.sh` regenerates the contract, regenerates the client, starts Postgres and a server, and runs it. The client is **not committed** — it is derived from the spec, and a committed copy is a second thing to keep in step
- [x] **It earned its keep immediately.** The contract said `DELETE /assets/{id}` returns `204`; the server returns `200` with the cascade count, deliberately — *"a delete that silently tombstoned 400 columns and returned 204 would leave an operator unable to tell whether it did what they meant"*. A spec can be valid, drift-guarded, and still describe a service that does not exist; only a client running against the real thing catches that

### Epic 2 — Entity hierarchy & columns
- [x] `Asset` + `AssetKind` for all five levels, one type not five
- [x] FQN derivation (`fqn::derive`, `fqn::child_of`, `parent`, `leaf`)
- [x] Containment rule in one place (`AssetKind::parent_kind`)
- [x] Hierarchy endpoints: roots, children, ancestors, search, stats
- [x] `PATCH /assets/{id}` and `DELETE /assets/{id}` (soft, cascading to the subtree) — shipped with Epic 3
- [x] **Containment cascade characterised** — a hard delete takes the whole subtree, leaves ancestors alone, and does not reach a sibling branch; a soft delete removes no rows at all. Nothing hard-deletes an asset today, which is why it was untested; it gets a caller when `00g`'s erasure path lands, and a constraint found to be wrong *then* is found while deleting somebody's personal data
- [ ] Non-database services (dashboard, pipeline, ML) → deferred to Epic 34

### Epic 15 — Source connectors
- [x] `Connector` trait, `SourceRecord`, `RunScope`
- [x] Postgres reference connector reading `information_schema`
- [x] Parents-before-children ordering as a connector contract
- [x] Re-runs converge (FQN is the identity, not the generated id)
- [x] Run report names each failure and its reason
- [x] System schemas excluded; views catalogued and marked
- [x] Deletion detection with a threshold guard — off by default; a refusal deletes nothing at all; a source reporting almost nothing is caught by the threshold and names what it saw *(Epic 15)*
- [x] **Run history persisted** — a run opens its row *before* the work and closes it after, so a run that dies mid-flight leaves a row with no `finishedAt` rather than leaving nothing. A history that only records completions cannot show a crash, which is what it is most needed for. Recording never fails the run it records: a catalogue that refused to sync because its own audit row would not write would be trading the thing for the record of the thing
- [x] `GET /connectors/runs`, newest first, and the console's Connectors page shows it — a run that leaves a record nobody can see is half a feature. The table distinguishes **did not finish** from **failed** from **refused**, and shows "unchanged (n skipped)" rather than a bare zero
- [ ] ~~Scheduled runs~~ — **refused by decision 5**, not missing: *"graph-owl does not become a scheduler. Runs are triggered by an API call or external cron. Scheduling is a solved problem owned by other software."* The tracker line was asking for something the plan rejects on purpose. What it was really missing was the history above
- [x] **`source_hash` fingerprinting** *(decision 7)* — a re-run reads, compares and skips. Decision 3 already made a re-run *converge*; this makes it cheap. Three outcomes decided before any write: create if the FQN is unknown, patch if the fingerprint differs, **patch if there is no fingerprint at all** — absent evidence is not evidence of sameness, and skipping on it would freeze every pre-fingerprinting asset invisibly
- [x] **The fingerprint covers source-owned fields only.** A description edited in the console is catalog-owned; including it would make every human edit look like a source change and the connector would helpfully overwrite it. Framed with lengths rather than concatenated, because `["ab","c"]` and `["a","bc"]` are different assets that a naive join gives identical bytes
- [x] **A skipped record still counts as reported by the source**, so deletion detection does not tombstone it — getting that wrong would delete the entire catalog on the first run that used fingerprinting
- [x] The run reports `skipped` alongside `created`: a run that wrote nothing because nothing changed and one that wrote nothing because it was broken are otherwise indistinguishable

### Epic 39 — Console foundation
- [x] SPA embedded in the binary via `rust-embed`, one process
- [x] Hierarchy tree with lazy children
- [x] Entity page: breadcrumb, properties, children table
- [x] Search across name and FQN
- [x] Empty-database first-run state that offers the next action
- [x] Trust bar that states what it does not know yet
- [x] Deep-linkable selection (`?asset=`)
- [x] **OIDC/PKCE login** — verifier parked in `sessionStorage` across the redirect, because the navigation destroys the JS context and a verifier held in memory is `null` by the time the callback needs it. The **access token stays in memory only**; the *refresh* token moved to `sessionStorage` on 30 July 2026 so a reload restores the session rather than bouncing the user through a login they had already completed. `localStorage` was rejected — it survives a browser restart at the cost of leaving a long-lived credential on disk
- [x] **Generated API client** — a TypeScript client generated from `openapi.json` and driven against a live server *(Epic 1 Slice K)*. Not committed: it is derived from the spec, and a committed copy is a second thing to keep in step

**Known issues carried forward**
- ~~`/assets/{id}` is an API namespace; prefixing the API is the fix.~~ **Withdrawn 28 July 2026 — the proposed fix contradicts `00d-api-conventions.md`**, which keeps the URL unversioned and unprefixed *precisely so that adding a prefix later carries a signal*. Nothing is broken: the console routes entirely in query parameters (`?asset=`, `?asOf=`, `?compareTo=`, `?expand=`, `?tab=`), which cannot collide with an API route by construction. `00d` now states that as a constraint to keep rather than a coincidence to rediscover.
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
- [x] **`EventSink` port + `ChangeEvent`** — the port, the five event kinds, and the payload *(Slice J, part 1)*. Two rules are structural rather than a caller's duty: `updated()` returns `Option` and yields `None` on an empty diff, so a facade that forgot to check still cannot emit an empty event; and `emit()` returns `()`, so "emission failure must not fail the request" is enforced by the signature rather than by every call site remembering to swallow an error
- [x] **Emission wired into the facade** *(Slice J, part 2)* — `Catalog::with_events(sink)`; update, soft delete and restore announce after the write returns. Ordering is structural: every `announce` call sits past an early return on failure, so a change that did not commit cannot reach it. A no-op update emits nothing because `ChangeEvent::updated` returns `None`, not because the call site checks. A catalog with no sink still mutates — a missing subscriber is not an outage
- [x] **Create and re-ingest announce** *(Slice J, part 3)* — `upsert_asset` is create-or-update behind one method and the caller never says which it meant, so prior state is the signal: no `before` is a `Created`, a `before` is an `Updated`. Storage neither versions nor diffs an upsert (a connector re-run is a mechanical sync, not a curated edit), so the facade computes the diff from `syncable_fields` — **name, description, parentId, properties and nothing else**. A whole-entity diff would include `updatedAt`, which is rewritten on every upsert whether or not anything moved, and would therefore republish the entire estate on every nightly run
- [ ] **`HardDeleted` has no producer**, and this is not an oversight to close here: assets are soft-deleted by design (deletion is a governance decision, and a tombstone is what stops a connector re-run resurrecting one). The kind exists for the erasure path in `00g-operations.md` §data retention & erasure, which is not built. `delete_table` and `delete_relationship` are genuine hard deletes but their subjects are not `Asset`s, so `EventSubject` — which carries an `AssetKind` — does not describe them
- [~] **Nothing subscribes.** The sink exists and the facade calls it; Epic 8's `TextIndex` is what should be listening

### Epic 8 — Search
- [x] Facets by kind and schema, computed over the **visible** set
- [x] Result counts consistent with authorization filtering
- [x] **Full-text, stemmed, prefix-matched and ranked** *(Slice A)* — a `GENERATED … STORED` `tsvector` on `assets` with a GIN index, weighted name (A) / FQN (B) / description (C). Identifiers are split on `._-` before tokenising, so `upi_transactions` is findable by either half and a description word is findable at all — neither was possible under `LIKE`
- [x] **Relevance is asserted as an order, not as membership** — a name match outranks a description match, with *both* returned, so the test cannot pass on an unranked query
- [x] **A user cannot reach the query language.** Everything outside `[A-Za-z0-9]` is a separator, so `!upi`, `a & b` and `x:*` are searches rather than syntax errors or inverted intent. A query with no searchable terms is an empty result, not a 500
- [x] **The rank key *is* the pagination cursor**, inverted so descending relevance is ascending string order and the existing keyset comparison works unchanged

**Decision, and the reason it is not a shortcut**: the `TextIndex` port and the
event-driven indexer are **not built, and no longer blocked** — a generated
column is written in the same transaction as the row it describes, so there is
no derived store to drift, reindex or reconcile. Decisions 1–3 of
`08-engine-search.md` were written assuming a detached engine and remain correct
for one; they bind again when OpenSearch or a vector index lands. Epic 3's
`EventSink` therefore has no subscriber, and that is now a fact about the design
rather than a gap in it.

**Pending in this epic**
- Decision 5's full relevance ordering (exact FQN > exact name > prefix > fuzzy > description > column names > tags) is three weights, not seven tiers. No fuzzy matching, no tag or column-name search, and `ts_rank_cd` is not BM25 → Slice D
- Snippets: a hit shows the asset, not the matched fragment of its description

**Deferred**
- Vector index and embeddings → generated out of process, per `00j-language-boundaries.md`

### Epic 10 — Operability
- [x] `/health` (checks nothing, so a dependency blip cannot restart-loop the fleet)
- [x] `/ready`, three-valued: required vs optional checks, `200 degraded` when auth is off
- [x] Graceful shutdown draining in-flight requests
- [x] Startup states its security posture — an accidentally-open server must not look identical to a secured one
- [x] `BIND_ADDR` configurable

- [x] **Structured logging and request-id propagation** *(Slice C)* — `tracing` with `LOG_LEVEL` and `LOG_FORMAT=json`; one line per request carrying `request_id`, `method`, `route`, `status`, `duration_ms`. A client-supplied `X-Request-Id` is **propagated**, not replaced, and echoed back on every response including error paths — the error path is where an operator most needs the correlation. A supplied id is validated first: it lands in a response header and in every log line, so an unvalidated one is header injection and log forging in the same field
- [x] **`DATABASE_URL` is redacted wherever it is logged** — the password only, so the host, port, database and user that make the line worth logging survive. The rightmost `@` is the split point, because a password containing `@` split on the first one leaves its tail in the output, which is this function's whole failure mode delivered quietly
- [x] **`/metrics`** *(Slice D)* — `graph_owl_http_requests_total{method,route,status}` and `graph_owl_http_request_duration_seconds{method,route}`, base units per the observability contract. Route labels are the **template**: three requests to three asset ids produce one series, and the test asserts no concrete id appears anywhere in the exposition. `/metrics` is excluded from its own counters
- [x] Unauthenticated `/metrics`, for the same reason as `/health` — a scrape that depends on the identity provider goes blind during exactly the outage it should be reporting

**Found by the tests, worth recording**: the recorder was installed lazily on
first scrape, so every metric taken before Prometheus first polled was silently
dropped — including the whole startup window. It is now installed when the app
is built. The bug was invisible when tests ran in parallel, because another
test's scrape had already installed the recorder.

- [x] **Memory budget, itemized and defended** — six lines, each a `GRAPH_OWL_BUDGET_*` variable with a stated default, summed at startup and exported as `graph_owl_memory_budget_bytes{cache}`. Absolute, never a percentage: `00a` states a footprint, and "runs in 2 GB" cannot coexist with "takes 35% of whatever you give it"
- [x] **Two totals, because only one cache exists.** 32 MB is *allocated* by this build (Epic 13's decisions); 1168 MB is *planned* when the other five land. Reporting the plan alone would tell an operator to size a container for memory nothing will use
- [x] **The cgroup limit is a guard, never a sizing input.** Too small for what is allocated → refuse to start with both numbers and exit `78`. Too small for the plan → warn, because nothing is reserved yet and refusing over a future line item would block a deployment that works. cgroup v2 preferred, v1 fallback, and **v1's near-`u64::MAX` sentinel is treated as no-limit** — it parses cleanly, and taking it literally makes every budget fit a nine-exabyte container, which is a guard that always passes
- [x] Invalid configuration is refused **naming the variable**, per this epic's first acceptance criterion, and a value large enough to overflow is refused rather than wrapping into a small budget that sails through the check

**Pending in this epic**
- [x] **Admission control** — a bounded semaphore per class fronting the expensive paths; a request that cannot get a permit is rejected **immediately** with `503` and `Retry-After`, never queued. An unbounded queue converts overload into latency collapse: everyone waits, everyone times out, everyone retries, and the queue grows. A fast `503` lets a well-behaved client back off while the requests already in flight finish at normal speed
- [x] Permits available, held and rejections are exported per class, so "overloaded" and "broken" — which look identical from outside and demand opposite responses — are distinguishable
- [x] **Spans across port boundaries** — the middleware opens `http.request` carrying `request_id`, and the handler runs *inside* it, so `catalog.*`, `storage.*` and `engine.*` spans are its children and inherit the id. That inheritance is the whole mechanism: without a parent span those are roots, and a slow request is attributable to the process rather than to a layer. Every `#[instrument]` uses `skip_all` — the default records each argument as a span field, and these arguments are FQNs, search strings and SPARQL, which is customer metadata that must not reach a tracing backend
- [x] **`db_pool_connections{state}` and `catalog_entities_total{entity_type}`**, sampled at scrape time rather than on a timer: a background task publishes numbers up to its interval old and keeps running when nobody is scraping, where sampling on the scrape is exact and free when nobody asks. `in_use` is derived from `connections - idle` rather than counted separately, because two independently-sampled numbers publish a pair that does not sum

### Epic 11 — Users, teams, ownership
- [x] `User` with roles; auto-provisioned on first sight
- [x] ~~`owner_id` on assets~~ — **superseded by Slice C**, which replaced it with an `asset_owners` join table; `V16` dropped the column. Kept as a line rather than deleted because the *intent* held throughout: nullable ownership so a gap is visible rather than prevented, which is now the `?unowned=true` report
- [x] **Teams exist** *(30 July 2026)* — a `teams` table with membership, and `owner_team_id` on assets **beside** `owner_id`, not instead of it. Two columns rather than a polymorphic `(kind, id)` pair, because the two reference different tables and a polymorphic key cannot be a foreign key: losing referential integrity to save a column would let a deleted team stay named as an owner. **Both may be set** — "the platform team owns this, and Priya is the person to ask" is the normal case, and forcing a choice pushes one into a description field where nothing can query it. Membership is **replaced, not merged**: a partial update cannot express "remove everybody", and a team somebody has left is an owner who no longer exists. A member must be a known user, checked at the facade for the message and by a foreign key for the guarantee
- [~] **Was marked Shipped while teams did not exist** *(found 30 July 2026)*. This epic read **Shipped** at 2/2 while its own title names teams and no table, type or port method for one had ever been written. The marking was wrong, not the plan: two slices covering users and `owner_id` were counted as the whole epic. Two other lines are correctly blocked on it and looked like *their* problem — Epic 39's owner-and-team display, and Epic 41's violation assignment, which now assigns to a `users.id` because a team is not addressable. **An epic marked complete while a thing it is named for is missing is the failure this index exists to prevent**, and it survived because the count came from the slice marks rather than from the plan's own acceptance criteria

- [x] **Entities have owners — plural, and of two kinds** *(Slice C, 30 July 2026)*. `owners: Vec<EntityReference>` on the envelope, backed by an `asset_owners` join table, aggregated into every asset read in SQL so a list of N assets costs one query rather than N+1. `00c`: "**single-owner models fail immediately** — every real asset has a producing team and an accountable individual". **`V16` drops `assets.owner_id` and `assets.owner_team_id`**, added by `V5` and `V13` and never once read or written — two columns that looked like the answer to "who owns this", held nothing, and could not express the plural model
- [x] **Ownership inherits down `contains`** *(Slice D, 30 July 2026)*. An asset with no owner of its own reports the **nearest** owned ancestor's owners, every entry flagged `inherited: true`. One recursive CTE inside the same correlated subquery that already projected owners, so the dedicated endpoint and the asset read cannot drift — `asset_owners` was rewritten to select that projection rather than keep a second implementation of it, which deleted the hand-rolled row mapping. `ORDER BY hops LIMIT 1` is what makes it *stop* at the nearest ancestor: "who do I ask about this table" has one answer, and a list that grows with tree depth answers "who might conceivably care" instead.

  **The flag is the slice, not the walk.** Inheriting without saying so turns a 5,000-table catalog with no owner below the database into one that reads as fully owned, which is worse than not inheriting — the ownership-gap report would have nothing to report. It is serialized even when `false`, because a console reading its absence cannot tell "owned here" from "a server that predates inheritance". Per *entry* rather than per list: today the list is homogeneous, but the flag states a fact about one owner and belongs beside it.

  **This was previously deferred to Demo 7 "needs Epic 23's domains", and that reasoning was wrong.** Inheritance walks **containment**, which has existed since Epic 2; domains are a second, *orthogonal* grouping axis and give inheritance nothing it was waiting for. The deferral cost nothing except the delay, but it is the kind of dependency that gets asserted once and then believed

- [x] **Assets are filterable by owner** *(Slice E, 30 July 2026)*. `GET /assets?owner={id}` matches **effective** ownership — direct *and* inherited — so "show me everything my team owns" answers with the whole estate rather than the handful somebody remembered to tag. Combines with `?kind=` and with keyset pagination; an unknown principal is an **empty page, not a `404`**, because a filter is a question and "nothing" is a valid answer; an absent parameter is unfiltered rather than match-nothing, which would otherwise have emptied every existing list.

  **Written over the same SQL expression the read path uses, not as a second walk.** `OWNERS_JSON` became `OWNERS_EXPR` and the filter selects from it, so the filter and the header cannot disagree about who owns a thing — two copies of a nearest-owned-ancestor rule would have agreed right until somebody edited one. `a_nearer_owner_shadows_a_further_one_for_the_filter_too` is the test that would catch the drift: a service owned by one team and the database below it by another means the table's effective owner is the *database's*, and filtering by the service's team must not match it.

  Query-time walk rather than a maintained effective-owner projection, per the plan's own REFACTOR note: a projection buys speed and owes an invalidation problem, and containment is at most five levels deep. Revisit on measurement, not on principle.

  Filtering by a **team** does not expand to its members — a steward asking what their team owns must not be shown a colleague's personal assets.

- [x] **The ownership-gap report** *(30 July 2026)*. `GET /assets?unowned=true` — assets with **no effective owner anywhere up their chain**, which is the query Slice D's `inherited` flag exists to make answerable. Written over the same `OWNERS_EXPR` as the owner filter, and `the_gap_report_is_the_inverse_of_the_owner_filter` asserts every asset falls in exactly one of the two sets: if they ever disagree, one is lying about effective ownership. A separate parameter rather than `owner=none`, because a sentinel would collide with a principal actually called `none`; `?owner=` **and** `?unowned=true` together is a `400`, since "owned by X and owned by nobody" has no answer and an empty page would read as "X owns nothing"

- [x] **Teams nest, with cycles refused at any depth** *(Slice B's second half, 30 July 2026)*. `parent_team_id` as a self-referential nullable foreign key, not a join table: a team has at most one parent, which is what makes this a hierarchy rather than a graph, and a join table would make two parents representable for the cycle checks to defend against forever. `GET /teams/{id}/children`. `ON DELETE RESTRICT`, so a deleted parent cannot silently orphan its children into roots — making that refusal *visible* is Slice G's job.

  **Depth 3 is the test that matters, and it was verified by breaking the code.** Slice B's mutator watch says "a check that only compares immediate parent passes depth-1 and fails depth-3"; degrading `would_cycle` to exactly that comparison left depths 1 and 2 passing and failed only the depth-3 test. `parentTeamId` is always serialized, `null` for a root — a console reading its absence cannot tell "top of the hierarchy" from "a server that does not know about nesting".

- [x] **Users can be created before they ever sign in** *(Slice A's missing half, 30 July 2026)*. `PUT /users/{id}` — `PUT` on the id because the caller supplies it (the identity-provider subject), so a retry is a rename rather than a second user. **Users previously existed only by auto-provisioning on authentication**, which meant a person who had never logged in could not be named as an owner at all: exactly backwards for onboarding, and the reason Slice C's own tests had to own assets with the seeded `system` user. Roles are deliberately *not* settable here — `set_user_roles` is the one path that grants them and the one that invalidates the authorization cache, and a second writer would eventually forget.

- [x] **Users can follow assets** *(Slice F, 30 July 2026)*. `PUT/DELETE /assets/{id}/followers/{userId}`, `GET /users/{id}/follows` paginated with the same keyset cursor as every other asset page. **Idempotent, and `200` rather than `201` or `409`**: following what you already follow is the state you asked for, so a retried request must not look like a conflict. The primary key is the idempotency and `ON CONFLICT DO NOTHING ... RETURNING` is what makes it race-free — a read-then-write would let two concurrent follows both see "not following". Following a soft-deleted asset is a `400`, because recording interest in a tombstone is a subscription to something nobody can read.

- [x] **Deleting a principal does not orphan assets** *(Slice G, 30 July 2026)*. `DELETE /users/{id}` and `/teams/{id}` refuse with `409` while the principal still owns assets or parents teams, **reporting counts by kind** — "you still own 400 things" is not actionable, "1 service, 3 schemas, 396 columns" says reassign the service and let inheritance do the rest. `?reassignTo=&reassignToKind=` transfers and deletes in one transaction, bumping each moved asset's version Minor, because a silent transfer makes the audit trail claim nothing happened.

  **`reassignToKind` is required, never defaulted.** A user and a team can share an id, and guessing would transfer an estate to the wrong principal — a mistake no response body would reveal.

  **`ConflictKind::PrincipalStillHolds` exists because reusing `AssignmentExists` silently ate the counts.** The server renders a canned sentence per conflict kind, so the `409` said "this finding is already assigned" and told a steward nothing about what they were about to strand. Found by an HTTP test, and it is exactly what that enum's own doc warns about: two conflicts sharing one identity is a contract bug.

**Pending in this epic**
- **Principals have no soft-delete state, so two criteria are vacuous rather than unmet.** Slice C's "owner referencing a soft-deleted principal → `400`" and Slice G's "reassigning to a soft-deleted principal → `400`" both presume a `deleted_at` on `users`/`teams` that does not exist: deletion is hard, guarded by Slice G's `409` and reassignment. Adding soft-delete is a real decision with authentication consequences — a soft-deleted user must not be able to sign in — and belongs with Epic 12 rather than being smuggled in here. **Both endpoints do refuse a *nonexistent* target**, which is the reachable half of each criterion
- Slice B's `GET /teams/{id}/members` was not added separately: membership already rides on every team read (`members` is aggregated in SQL), so a second endpoint would be a second place for the same list to be wrong

**Deferred**
- Notification delivery to followers → needs a transport and a consumer

### Epic 12 — Authentication
- [x] JWT verification (HS256, shared secret); a forged token is rejected
- [x] **The `Principal` extractor swap** — one function changed, no handler touched
- [x] Auto-provision a `User` on first sight, with no roles
- [x] Open mode when no secret is configured, logged as such at startup

- [x] **JWKS / RS256 against an OIDC issuer** — keys fetched from `{issuer}/.well-known/jwks.json`, cached an hour, refreshed on an unknown `kid`. Key rotation is what JWKS gives for free, so Slice B lands with Slice A
- [x] **A heterogeneous JWKS does not break authentication** — keys are parsed loosely and narrowed after, so an EC key beside the RSA ones costs nothing. Parsing into a struct that required `n`/`e` failed the *whole* document, losing every usable key with the one that was not
- [x] **The refetch an unknown `kid` triggers is rate-limited** — `kid` comes from the token, so without a floor a stream of forged ones becomes one outbound request to the identity provider per inbound request
- [x] **OIDC beats a shared secret when both are configured**, and the ambiguity is logged. Checking the cheaper secret first silently downgrades the one deployment where the downgrade is invisible: mid-migration, with the old secret still live
- [x] **`GRAPH_OWL_ADMIN_SUBJECTS`** — without it the *first* sign-in renders an empty catalog, because identities auto-provision with no roles and authorization denies by default. Two correct decisions producing the one screen `00f` forbids. Applied after resolution and never written back, so removing the variable revokes it

- [x] **Roles can come from the token** via `OIDC_ROLES_CLAIM` — opt-in and off by default, because a provider deciding what this catalog authorizes is a reasonable arrangement and a terrible default: it is invisible to anyone reading the policies
- [x] **Sign-in verified end to end against the live tenant** — Auth0 → Google → consent → callback → token exchange → catalog. The access log shows the turn: `principal="-" status=401` on the requests fired before the exchange, then `principal="google-oauth2|…" status=200` in the same second
- [x] **A bug that only end-to-end could find**: the console fired its opening requests on mount, which on the callback page is *before* the exchange completes. They 401'd, set `refused`, and **nothing ever cleared it** — so a completely successful sign-in kept showing the sign-in screen, indistinguishable from one that failed. The load is now gated on the sign-in having settled and re-runs when it does
- [x] **Verified against the live tenant** — discovery confirms the issuer carries a trailing slash (`iss` is an exact compare), the JWKS holds a two-key RS256 rotation pair, and the tenant advertises `plain` as a challenge method, so pinning S256 is load-bearing. An unknown `kid` returns `401 unknown KID`, which proves the fetch reaches Auth0; the **first** such request took 492 ms and the next three took 0.02 ms, which is the refetch floor measured rather than asserted

**Pending in this epic**
- [x] **A page refresh keeps the session** *(decided 30 July 2026)*. The line above named three options — custom domain, persisted tokens, or sessions lasting as long as the tab — and **the third was taken**: the refresh token is parked in `sessionStorage`, so a reload, a navigation and a crash-restore all keep the session and closing the tab ends it. `localStorage` was rejected: it survives a browser restart at the cost of leaving a long-lived credential on disk for any future XSS. The **access token stays in memory only**, unchanged — this is the smallest persistence that makes a reload work, not a relaxation of `00f`'s rule. Rotation is handled, because Auth0 rotates on use and keeping the spent token fails one reload later as an unexplained sign-out. The iframe flow was **not** built, for the reason given: it is reliable only behind a custom domain
- [x] **The full flow is confirmed at a browser** *(30 July 2026)*. Redirect, consent, code exchange and the authenticated session were exercised against the live Auth0 tenant with a human completing the credential step, and a reload afterwards kept the user signed in. It also surfaced something no test could: the **consent screen was appearing on every sign-in**, which is tenant configuration (first-party application + "allow skipping user consent" on the API) and not this code — a diagnosis worth recording, because the symptom is indistinguishable from a broken client

### Epic 13 — Authorization
- [x] `AccessPredicate` in `graph-owl-authz` — pure, zero surviving mutants
- [x] Lowered to SQL for list, search, children and counts
- [x] Deny-overrides, order-independent; an unmatched request denies
- [x] `MetadataOperation` vocabulary, append-only
- [x] **Row-level filtering — the PII demo**: two principals, one search, different results
- [x] Counts filtered through the same predicate, so a total cannot leak what it hid
- [x] Hidden reads as `404`, not `403` — a `403` on an id confirms the id exists

- [x] **Decision cache** — compiled predicates held in a bounded LRU keyed by the **role set**, not the principal, so it scales with policy shape rather than headcount and a thousand analysts sharing one role warm the entry once between them. `is_admin` is part of the key because `compile` short-circuits on it
- [x] **Invalidated by epoch, never by TTL** (`00g`). A TTL makes staleness the normal case: a revoked role keeps working until a clock says otherwise, and the window is invisible to whoever revoked it. `Catalog::invalidate_authorization()` is the trigger
- [x] **`PUT /users/{id}/roles` is the caller** *(30 July 2026)*. `invalidate_authorization()` previously had none, because no endpoint changed a role. **A revoked role now stops working on the very next request** — the cache has no TTL, deliberately, because a revocation whose window is invisible to whoever performed it is one nobody can reason about. The whole cache is cleared rather than one subject's entries: roles compile against policies naming *other* subjects, so one person's change can alter what a group rule grants everybody, and clearing selectively would be right most of the time — the worst property a security control can have. `PUT` rather than `PATCH`, because a partial update could not express "remove every role", which is the operation that most needs expressing. **Still blind to SQL**: an operator editing `role_policies` directly is not noticed, and policy *administration* remains Epic 41 Slice F's

**Pending in this epic**

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

- [x] **Facet rail over kind and schema**, filtering the returned page client-side against the server's counts, with a visible "filtered from N" and a clear control
- [x] **Keyboard navigation** — arrow keys move a cursor through the results and Enter opens one; the listener is on the document rather than the input, so the cursor survives the search box losing focus

**Pending in this epic**
- [x] **Unblocked and clear** *(30 July 2026)*. This line existed only to point at Epic 12's OIDC/PKCE, which has now shipped — including the reload behaviour that was the visible half of it. Nothing else in this epic is outstanding.

> **Tracker correction, 28 July 2026.** Facets and keyboard navigation were
> listed here as pending until a check against `ui/src/App.tsx` found both
> shipped (`FacetGroup`, and the `keydown` handler at the document level). The
> same sweep confirmed `PATCH` and `DELETE` on assets exist, contradicting Epic
> 2's gap line. Entries copied forward from a previous revision were the cause;
> rule 0 below now requires a check against the code, not against the previous
> revision.

- [x] **OIDC/PKCE sign-in**, S256 challenge, tokens in memory only. The state and verifier are parked in `sessionStorage` **because they must survive a full-page redirect** — holding them in memory made the callback's guard unsatisfiable and login could never complete. That is not a weakening of the token rule: a verifier is single-use, lives for seconds, and is worthless without the matching authorization code
- [x] **Three outcomes, three screens** — signed out, denied (`403`, signed in without a role), and mid-exchange each render distinctly. A failed sign-in shows *why*, rather than returning silently to the panel the user just used
- [x] The token has **one owner**. `api.ts` reads it through an injected source rather than holding a second copy; two modules each holding "the" token meant every request went out unauthenticated after a successful sign-in
- [x] **Owner and team display** *(30 July 2026)*. Every asset read carries `owners` with denormalized names and kinds; the header renders them as chips, users and teams distinguished, capped at three with a `+N more` overflow whose tooltip names the rest. Three states, not two: `[]` says "no owner recorded" plainly, an **absent** field renders nothing (claiming "no owner" about an estate the server never mentioned is a claim, not a blank), and Slice D's inherited ownership says "inherited" / "partly inherited" in words — **a dashed border was verified invisible at real size**, which defeated the whole point of the flag. Found by the browser check and nothing else: `tsc`, 232 tests, 100% mutation and a clean production build all passed while an older server omitting `owners` took the entire asset page down with `Cannot read properties of undefined`

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
- **SPARQL works** — `POST /sparql`, authorization-scoped and budgeted. What is *not* done is pattern pushdown: the scan materialises the permitted fact set rather than narrowing per pattern, which is fine at demo scale and will not be at 100k assets.
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
- [~] Runtime predicate registry — define/lookup/list, duplicate refused, core vocabulary seeded by migration and immutable, cardinality recorded per predicate; **an unregistered predicate is refused on assert and named**, so the vocabulary is a constraint rather than documentation. Retraction is deliberately not gated — refusing one would strand the fact it withdraws *(Slice H)*; **gap**: cardinality and value type are recorded but not yet *enforced* on write (both are constraints → Epic 5)
- [ ] `rdf:reifies` + triple terms → **Epic 94**. The reified edges already shipped *are* RDF 1.2's reifier shape; only the vocabulary is missing (`04-engine-triples.md` finding 5). **Today a SPARQL query using `rdf:reifies` returns zero rows, not an error** — indistinguishable from an empty graph. Epic 94 decision 7 fixes it by synthesising the reifying quad at the query surface, leaving the store and the flake count untouched
- [ ] Language-tag side table → **Epic 94**, and it needs three components not two: `rdf:dirLangString` carries a base direction (the RDF-namespace datatype — there is no `xsd:langString`)

### Epic 7 — SPARQL ★

**Scope changed 28 July 2026** (`07` decisions 8–9, `00l`). Parsing, optimisation,
join execution, expressions, aggregates and result serialisation are **adopted**
from permissively-licensed crates. What this epic builds is the
`QueryableDataset` implementation over flakes — which is where the three things
no library can supply actually live.

- [x] **`QueryableDataset` over flakes** — the whole of this epic's own content *(Slice A)*. Real SPARQL runs: BGP, two-hop join, numeric FILTER, OPTIONAL, ASK, named-graph isolation
- [x] **Pattern pushdown** — the query's patterns become narrowed flake scans; a one-predicate query reads 124 facts instead of 1,234 on the demo estate. A property path or an unrecognised algebra node forces a full scan rather than a guess *(Slice C)*
- [x] `as_of` — the dataset is constructed at a transaction time *(Slice B)*
- [x] Authorization applied before the dataset exists, so the evaluator only ever sees permitted rows; edges hidden unless *both* endpoints are visible *(Slice B)*
- [x] Fact budget — **nothing adopted enforces budgets; this is ours**. 50k facts, server-side, truncation always reported *(Slice B)*
- [x] Freshness stamping on the result (Epic 4 decision 8) *(Slice B)*
- [x] ~~Parser~~ — `spargebra`, full SPARQL 1.1 *(adopted)*
- [x] ~~Planner / execution~~ — `spareval` *(adopted)*
- [ ] `sparopt` is not yet in the path — pushdown reads the parsed algebra directly. Adding the optimizer is a measurement question, not a gap

### Epic 7a — Traversal
- [x] One frontier primitive (recursive CTE, one statement); `neighbours` and `subgraph` over it *(Slices A, B, E)*
- [x] Budgeted, cycle-safe, truncation always visible and farthest-first
- [x] Reified two-hop edges hidden — five logical edges reports distance five
- [x] `as_of` on every walk, so time-travelling traversal is free
- [x] `shortest_path`, `all_paths`, `detect_cycles` *(Slices C, D)* — deterministic tiebreak, hard path cap, cycles normalised so rotations are one cycle

### Epic 40 — Graph explorer ★
- [x] Renderer-agnostic `GraphView` — the shape a WebGL canvas will consume unchanged
- [x] Graph tab with a deterministic radial layout, hop selector, truncation shown
- [x] **Time control** — `?asOf=` on the walk and on the asset read
- [x] Non-visual equivalent: the same neighbourhood as a keyboard-navigable table, expansion included
- [x] **Expand-on-click** *(Slice B)* — one hop per click, budgeted server-side; dedup by node id and by `(from, to, relationship)`; truncation **sticky and per-node**, so the marker sits on the node hiding something; `?expand=` replays the expansions, so a pasted link restores the sender's picture
- [x] **Diff mode** *(Slice D)* — two instants, `?compareTo=`. A node removed between them is **still drawn**, marked by sigil, strikethrough and dash pattern rather than colour alone. A truncated side marks the comparison `partial`, because a partial comparison shown as complete invents deletions that never happened
- [x] **Cytoscape canvas, WebGL above 256 nodes** *(Slice A, renderer)* — the hand-drawn SVG is gone. `breadthfirst` rooted at the seed, `animate: false`, `maximal`/`grid` pinned: a force layout settles differently every run, so the same neighbourhood never looks the same twice and nobody can say "the node on the left". WebGL is chosen **once, at creation** — `00f` rejects a hybrid that swaps mid-session, because the swap discards the layout at the moment a reader most needs it
- [x] **The model was untouched by the swap**, which is the payoff of `GraphView` having been renderer-agnostic from the start. Everything decidable — which elements exist, what classes they carry, whether the layout is deterministic — lives in `graph/cytoscape.ts` and is tested there; the component is mount, feed, listen
- [x] **Diff compares the *expanded* model, not the seed walk** — the baseline replays the reader's expansions at the earlier instant. A node expanded today that did not exist then is skipped rather than treated as empty, and the skip does not mark the comparison `partial` (which would suppress the deletions the screen exists to show)
- [x] **Lineage DAG** — React Flow with a layered left-to-right layout, upstream and downstream from any table or column. `00f` keeps two renderers on purpose: exploration is an arbitrary cyclic graph at scale where WebGL is the point, lineage is a DAG of modest size where the *layering* is the point and a force layout is actively wrong for it
- [x] **Derived edges visually distinct** — Epic 6 now derives them, so this unblocked. Provenance rides on the edge (`derived`) from the traversal SQL through the port and the wire, because a consumer cannot recover it from the endpoints or the relationship name. Drawn **dashed and tinted**, not tinted alone: `00h` requires a state to survive being unable to tell two hues apart, and this is a state somebody acts on. `00b` decision 2 keeps conclusions in their own graph precisely so nobody mistakes one for an asserted fact, and a picture that drew both alike would undo that in front of the reader most likely to act. An **absent** flag reads as asserted — understating what the reasoner did rather than overstating it

### Epic 29 — Lineage *(pulled forward from Demo 7)*

Demo 3's lineage DAG had nothing to draw: `Feeds` was already in the
relationship vocabulary, but `create_relationship` was hardcoded to the
pre-`Asset` `EntityKind::Table`, so lineage between the assets the catalog
actually holds could not be asserted at all.

- [x] **A** `POST /lineage` and `DELETE /lineage/{id}` between assets, with the SQL that produced the edge. The **same pair asserted by a person and by a connector are two edges** — automation is often wrong about lineage a human knows, and a human is often out of date about what automation observes; collapsing them makes one silently overwrite the other with run order deciding the winner
- [x] **A** Self-lineage refused (a cycle of length one), a missing endpoint is `404`, and lineage across levels is refused — table-to-table or column-to-column, never mixed, or "what is downstream of this table" returns a set whose members are not comparable
- [x] **B** A bounded walk in both directions, each spending its **own** budget: a merged frontier would let an upstream hop spend the downstream allowance, so `upstream=1&downstream=3` would return something that is neither
- [x] **B** A diamond yields the shared node once with both inbound edges; a tombstoned node stays in the picture, flagged, because "nothing downstream" and "the downstream was deleted" are opposite conclusions
- [x] **C** A cycle terminates. The graph is called acyclic because it should be, not because anything stops somebody asserting otherwise
- [ ] **D** Column-level lineage — the legality table already admits `column feeds column`; nothing asserts it yet
- [ ] **E** Connector-asserted lineage reconciles with curated edges → the two-source model above is the foundation, the reconciliation is not built
- [ ] **F** Lineage survives entity deletion → today a hard delete cascades the edge away; a tombstone keeps it

---

## Demo 4 — It reasons, and it validates

**The claim**: it tells you what is broken and why it believes what it believes.

**What you can show**: a SHACL-style shape says "every table in `regulatory` must have an owner and a retention tag"; the violations queue fills; classify one table as PII and watch the classification propagate along lineage as a *derived* fact, visibly marked, with its derivation chain.

### Epic 5 — Constraint validation
- [x] **Shape and constraint types; four target kinds** *(Slice A)* — twelve constraint components plus `not`/`and`/`or`, each with a conforming case and a violating one. `Class`, `Subjects`, `SubjectsOf`, `ObjectsOf`; `LiteralsOf` and `ImplicitClass` are not built — see below
- [x] **Compile-once, evaluate-many** *(Slices A, D)* — regexes parsed at compile time, so a broken shape is refused before it meets data rather than failing halfway through a pass over a live estate. Cached on the newest `t` among the shape facts, which is exactly what a shape edit moves
- [x] **Shapes compile from graph triples** *(Slice B)* — SHACL vocabulary, `graph:shapes`. A shape is data: versioned, time-travellable, auditable. **An unknown term is refused, never skipped** — a silently dropped constraint is a shape that looks like it checks something and does not
- [x] **Continuous validation with violation reports, not write-time rejection** *(Slice C)* — `POST /validation/runs` reads, validates and writes *nothing* back to the graph
- [x] **Severity classification; repair suggestions never auto-applied** *(Slices A, E)* — a warning does not fail conformance; `MinCount` suggests `AssertMissing`, `MaxCount` suggests `RetractExcess`, a wrong type suggests `RetypeValue`, and everything else suggests nothing rather than restating the violation
- [x] **`GET /validation/report`** *(Slice E)* — paged, filterable by severity, shape and focus node, and **carrying the `t` it reflects**, because a queue whose currency is unknown is unactionable
- [x] **All six target kinds** — `Class`, `Subjects`, `SubjectsOf`, `ObjectsOf`, `LiteralsOf`, `ImplicitClass`. A literal target focuses the node that *holds* the value, because a literal has no identity to report a violation against
- [x] **Seed shapes ship** — `TableShape`, `ColumnShape` and `ConfidenceShape`, written through `POST /validation/shapes/seed` as flakes rather than by a migration: a migration would hand-encode each object into the flake table's text encoding, which is computed in Rust and would then exist in two places. Explicit rather than automatic on startup, so a server cannot re-impose a rule somebody removed
- [x] **`sh:not`/`sh:and`/`sh:or` stated as triples** *(30 July 2026)* — a combinator's branches are **property shapes with their own `sh:path`**, because the useful case is two *paths* ("an owner **or** a steward") and not two node shapes whose targeting could then disagree with the parent's. One reader for the top level and every nested one, so a branch is read by exactly the same rules — two readers would be two vocabularies. A shape that references itself is **refused, not recursed into**: a cycle here is a hang, and a hang in shape compilation is a server that will not start, so indirect cycles are caught too. `sh:not` over several branches is refused rather than guessed at — "not any" and "not all" are different statements. An **empty branch** is refused, because an empty branch in an `or` is satisfied by everything and silently disables the whole combinator
- [~] **Pending in this epic**: `sh:in` as an RDF list rather than a repeated predicate — a **stated departure** recorded in `00k`, not an omission; and `RelationshipShape`/`EnvelopeShape` from the seed table, which need Epic 2's relationship projection to have anything to target

### Epic 6 — Reasoning overlay
- [x] **Eight OWL 2 RL axioms as built-in rules** *(Slice A)* — subClassOf, subPropertyOf, transitive, symmetric, inverseOf, domain, range, sameAs. Eight named functions, not a rule interpreter. Iterates to fixpoint with dedup, so depth-3 chains resolve and a symmetric property terminates instead of ping-ponging. A derived fact carries the **max premise `t`**, so it can never be visible at an instant before the facts implying it. Retracted flakes derive nothing
- [x] **Semi-naive fixpoint, `CappedReason` on every limit** *(Slices B, C)* — each iteration joins against the previous one's output, proved by a join counter rather than asserted. Four limits, four reasons: `Facts`, `Duration`, `Iterations`, `Memory`, each stopping the run and returning what it had. **A new axiom widens the delta**, because a rule that needs a new axiom and an old fact is otherwise silently dropped — the completeness hole a naive reading of semi-naive evaluation leaves
- [x] **Derived facts in `graph:reasoning`, never persisted into the base** *(Slice E)* — a run replaces the overlay wholesale and leaves the default graph byte-identical. Withdrawal precedes assertion at a strictly earlier `t`, or current-state resolution drops the fact and the overlay empties from the second run onwards
- [x] **`GET /reasoning/explain` derivation chains** *(Slice D)* — recursive to the assertions underneath, every route when a fact follows more than one, `404` when nothing supports it. Confidence is the **minimum** of the premises, not the product
- [x] **Reasoning is skipped on historical queries** — `as_of` reads the default graph specifically. Inferring from premises that did not hold at `t` would be a wrong answer carrying right-looking provenance
- [x] **Classification propagates along `feeds`, opt-in per classification** *(Slice F)* — the opt-in is stated on the classification itself, as a fact, so "why did this spread" is answerable from the graph rather than from a config file. Epic 25 made non-propagation the default deliberately: `pii` follows the data and `deprecated` does not, and a blanket rule turns the estate one colour within a run. It rides the ordinary fixpoint, so a chain carries the whole way down and a diamond concludes once
- [x] **Lineage is projected into the graph** — the prerequisite nobody had noticed: lineage lived only in `lineage_edges`, so nothing could reason over it and no SPARQL question could reach it. Asserted and withdrawn alongside the relational row, failure never propagated, per decision 6
- [~] **Pending in this epic**: ownership and domain inheritance down `contains`, and the differential test asserting they agree with Epics 11 and 23 — **those epics are not built**, so there is nothing yet to differ from and a rule written now would be a second implementation of a first that does not exist; certification invalidation on an upstream Major bump (Epic 26)

### Epic 41 — Workbench & governance
### Epic 41 — Workbench & governance
- [x] **Violations queue** — grouped by asset rather than by finding, because forty rows about one table is one piece of work; worst-first and stable, so it can be worked from the top; each row carries the shape's own message, the offending value, and the suggested repair. The header states how far behind the graph the report is, which is the only thing that makes an empty queue trustworthy
- [x] **Both engines triggerable from the console** — validation and reasoning, with the refused-shape count shown rather than hidden
- [x] **SPARQL editor with plan display** — the engine now reports the scans pushdown decided on, and the workbench shows them. A lone `? ? ?` means nothing could be bounded and the whole graph was read, which is the entry worth seeing: without it an author cannot tell a query that is inherently expensive from one a single triple pattern away from being cheap. Truncation is called out above the results, because a truncated answer that looks complete is the failure this project refuses everywhere else
- [x] **Results as table ⇄ graph** — the graph view is offered **only when the results are triples**. Drawing two arbitrary columns as nodes and an edge asserts a relationship the query never returned, and a reader believes a picture more readily than a table. A single solution short of a triple disables it, because a graph missing rows the table shows is worse than no graph. Unbound variables render as `unbound`, not blank: `OPTIONAL` produces exactly that, and blank makes "no value" indistinguishable from "the empty value"
- [x] **Violations carry waivers** — a violation can be accepted on the record, with a **required reason** and a **required expiry**. Without a reason a waiver is a violation deleted with extra steps: the next reader cannot tell an accepted risk from a forgotten one. Without an expiry it is a rule switched off where nobody will see it again. A waived finding stays **visible and marked, never hidden** — removing it makes the acceptance invisible, and with it the fact that it is about to lapse. An **expired** waiver reads as expired rather than absent, because a lapsed acceptance and one nobody ever gave look identical otherwise and only the first is somebody's to answer for
- [x] **A waiver survives the next pass** — keyed on the finding's *identity* (shape, node, path, constraint), not its row id. Results are replaced wholesale each run and every row gets a fresh id, so a waiver keyed on one would work once and then point at nothing — a failure that reads as the waiver having been forgotten
- [x] **Violations are assignable** — to a `users.id`, enforced by a foreign key rather than accepted as free text. A finding assigned to a name nobody can resolve is a queue row that *looks worked and is not*, and "what is on my plate" stops being answerable the first time somebody types a nickname. One assignment per finding, because two owners is no owner. Assignment and acceptance are **independent**: "somebody is fixing this" and "somebody accepted this" are different statements, either can hold alone, and collapsing them would make an accepted finding look unowned
- [x] **The derivation chain rendered beside a derived fact** — a Reasoning tab on every asset lists what the reasoner concluded about it, each expandable to an indented chain down to the assertions underneath. Conclusions are marked as conclusions: `00b` decision 2 keeps them in their own graph so nobody mistakes one for something a person asserted, and the console honours that where it matters most
- [x] **Policy dry-run** — `POST /policies/dry-run` reports what a policy *would* do to the estate as it stands, and **writes nothing**. It reports counts and a bounded sample: "admits 4,231 assets" is what a reader acts on, and five names is how they check the count means what they think. It never simulates an administrator — an admin bypasses policy entirely, so a dry-run against one would report that every policy admits everything, always saying the same reassuring thing. And it calls out **`admitsEverything`** explicitly, because a policy that denies nothing is almost always a mistake and is indistinguishable from a correct one in the counts alone
- [x] **Connector configuration with write-only secrets** *(Slice F, the plan's first RED)* — a credential is **unrepresentable** in what a reader gets: `ConnectorConfig` has no field for one. A `redacted: bool` beside the value, or a `secret: Option<String>` a handler is trusted to skip, is one `Debug` derive away from a password in a log line; making it impossible is the only version that cannot be got wrong later by somebody who does not know the rule exists. One method sees credentials — `connector_secret(id)` — so a reviewer auditing where they go has one signature to grep for. **Absent means keep, not clear**: an edit form cannot resend what it was never given, and treating absence as "clear it" would break a connector every time somebody changed a port. A blank secret is refused, because `""` would set `hasSecret` and then fail at connection time with an error nobody can explain. Encryption at rest is stated as the deployment's, not implied — a key managed beside its own ciphertext protects nothing
- [~] **Admin: the section exists** *(Epic 41 Slice F, 30 July 2026)*. An `Admin` nav section with two panels: **people & teams**, and **connector config rendered from the connector's own JSON Schema**. The dry-run and the write-only secret — the two security-critical halves — were already built; connector *runs* already had a history page. **Schedules are refused, not missing** (Epic 15 decision 5).

  **`SchemaForm` is the only form renderer, and a structural test enforces it.** The failure guarded is not a wrong pixel — it is somebody adding a second hand-written connector form because it was quicker, after which the two drift until a field goes missing from one. No behavioural test catches that: both forms would work. **Verified by planting the regression** (a local `snowflakeFields` array) and watching the test fail.

  The team hierarchy is built by a pure `admin/hierarchy.ts` (95.83% mutation): an **orphan** — a team whose parent is not in the list — is reported rather than promoted to a root, because drawing it top-level asserts it *is* top-level when the truth is usually a policy filter or a parent deleted since. A **cycle** is reported rather than walked; the server refuses to create one, so seeing one means something else wrote it, and a hang leaves nothing in the console to report. Mutation also found dead code: seeding the `seen` set with `start.id` looked like what catches self-parenting and was not — the loop records each parent as it walks.

  **Policy authoring with dry-run** *(30 July 2026)*. Compose a rule — effect, operations, and a coarse resource matcher — and preview it against a set of roles before anything is applied. The plan's own reason: "a policy saved without preview is a production access change made blind, and this is the one screen where a mistake is a security incident." `admin/policy.ts` decides what the preview *means* (100% mutation over 73 mutants): a policy that **admits everything** and a correct one are identical in a count, and one that **admits nothing** looks like a working filter until somebody cannot do their job. An **empty estate** is reported as uninformative rather than alarming, or the preview would blame the policy for an empty catalog.

  **Connection test before save** *(30 July 2026)*. `POST /connectors/{connector}/test` runs the connector's own `test_connection` against settings that have **not been saved** — testing after the write would confirm the credential once the mistake was already made. A refused connection is `200 {ok:false}`, not a `5xx`: the request succeeded and the answer is "no", and an error status would make a wrong password indistinguishable from the catalog being down. The driver's message is passed through because "could not connect" tells an admin nothing while `password authentication failed for user "catalog"` names the wrong field — with the secret redacted server-side before it leaves the process, since sqlx puts the connection string in some errors.

**Pending in this epic**
- **Policies can be previewed but not saved, and the console says so.** Nothing in the API inserts a policy — the `policies` table is written by nothing, so a write path is an **Epic 13** surface that does not exist. The panel carries a standing notice rather than a Save button that would lie
- **Job and schedule management** is listed in Slice F's criteria and is **refused, not missing** (Epic 15 decision 5). Connector *runs* have a history page
- **The admin panels are not visually verified.** `tsc`, 262 tests, the planted-regression check and a production build all pass, but the section was never seen signed-in — and last time exactly that combination passed while an older server took the asset page blank. A runtime shape guard was added at the `/teams` boundary for that reason, rather than trusting the `Team[]` annotation

---

## Demo 5 — Agents can use it ★

**The claim**: an agent asks "is `upi_transactions` safe to build a fraud model on?" and gets a policy-filtered, provenance-carrying answer — plus the institutional memory of why the schema changed last quarter.

### Epic 14 — MCP + outbound events ★
- [x] **Protocol, authentication and policy together, with one tool** *(Slice A)* — `graph-owl-mcp` is pure, reaching the catalog through a `ContextSource` port, so the security decisions are testable without a database. **Denied and absent are one answer**: a refusal naming an asset the caller cannot see tells them it exists, which is the fact the policy withholds. Authentication is checked *before* the tool name, because "no such tool" tells an unauthenticated caller which tools exist. `policy_filtered` rides on every context — an agent that cannot tell a complete answer from a filtered one presents the filtered one as complete
- [x] **Trust summaries and gaps** *(Slice B)* — lifecycle, certification with expiry *evaluated* (tested at exactly the boundary), quality, and named gaps. **An asset nobody tested reports `Unknown`, never `Healthy`**; `None` and `Some(false)` are kept apart because "never ran" and "ran and failed" are opposite statements. An unrecognised lifecycle is `Unknown` rather than `Production`, and a blank description counts as missing. `gaps` is empty only when genuinely complete
- [x] **The adapter over `Catalog`** *(30 July 2026)* — `CatalogContext` implements `ContextSource`, and every context now carries its trust summary. Two decisions it rests on: the catalog's fields reach `Observed` as `Option`s **without defaulting on the way in**, because a field that lost its `None` earlier would have thrown away the distinction `trust` protects and the loss is invisible; and **visibility reuses the facade's filtered read** rather than restating the rule — two implementations of "may this principal see it" is one more than can be kept in step, and this is the copy an agent drives. `policy_filtered` is the difference between what exists and what the caller may see, which is the only way to know something was hidden: a filtered read cannot report what it removed
- [~] **No transport yet** — nothing speaks MCP over stdio or HTTP, so the surface is reachable from Rust and not from an agent. That is the remaining half of Slice A
- [ ] The remaining six read tools — search, lineage, governance, graph query *(Slices C, D)*
- [ ] Token-budgeted responses *(Slice E)*
- [ ] Outbound webhooks, HMAC-signed, at-least-once *(Slice F)*
- [ ] The thesis test: an agent with only MCP access answers a real question *(Slice G)*

### Epic 31 — Organizational memory ★
- [x] Memory objects: kind, content, authorship, confidence, `as_of`
- [x] Supersession and contradiction detection
- [~] Retrieval with reranking

**Pending in this epic**
- The semantic ranking term. Blocked on Epic 8, and deliberately: embeddings are
  generated out of process (`00j`), so the port that matters does not exist —
  fabricating a lexical similarity and labelling it semantic would destroy the
  distinction `Score.semantic: Option<f64>` exists to preserve between "measured,
  not similar" and "never measured".
- A person cannot author a memory over HTTP yet. `Principal::system()` is the
  placeholder until Epic 12, so every memory written through the API today is
  recorded as agent-authored — honestly, but it means the human-confidence default
  is unit-tested only.

### Epic 32 — Agent capabilities
- [ ] Write-back with agent authorship
- [ ] Investigation and remediation proposals

### Console half
- [ ] **14** **Agent activity — sessions, reads, writes, webhooks** *(Epic 42)*. An agent writing to the catalog with no visible audit is the single scariest thing in this demo
- [ ] **31** Memory panel and memory administration *(Epic 41)*
- [ ] **32** Agent capabilities; **write-back audit** *(Epic 42)*

### Epic 43 — Framework integrations
- [ ] LangChain retriever preserving provenance and confidence
- [ ] LangGraph toolkit, manifest-parity with MCP
- [ ] Checkpointer over Epic 31, retraction not deletion
- [ ] Zero graph-owl crate changes, asserted

---

## Demo 6 — It fills itself

**The claim**: the catalog is populated from every shape of source without duplicating anything.

### Epic 16 — Ingestion APIs & SDKs
- [~] **Push API with partial success** *(Slice A, 30 July 2026)*. `POST /ingest` takes a batch and answers **`207`** with a per-item status — `200` would claim the whole push succeeded when item 42 did not, `400` that it failed when 999 landed, and a pusher branching on the status needs the one that means "read the results". One bad item costs only itself: an all-or-nothing batch makes a pusher re-send everything to fix one typo, and at that size somebody stops retrying.

  **Order is computed, not demanded.** A pusher walking a source emits what it finds when it finds it, so `graph-owl-connectors::ingest::apply_order` sorts the batch so parents land before children — a child submitted first is the normal case, and requiring dependency order would push the catalog's model onto every adapter author (decision 1). Pure, 5/5 mutants caught, and **stable**: independent items keep their submitted order so a `207` reflects the batch back rather than a permutation a client has to re-derive. Indexes are the *submitted* positions, which is the one place a wrong number silently sends somebody to the wrong item.

  Two whole-batch refusals, because neither has a partial success to report: a **duplicate FQN** states two intents for one entity and applying both would make the result depend on submission order; a **containment cycle** would otherwise be an infinite walk. Over **1000 items** is a `400` — a request is not a job (decision 2).

- [x] **Idempotency** *(Slice B, 30 July 2026)*. `Idempotency-Key` on `POST /ingest`: a replay returns the original body and status and creates nothing; the same key with **different content** is a `409`, because a key identifies a request rather than a slot and serving the first answer would silently drop a push the client believes landed. Decision 4 calls this mandatory, not optional — at-least-once transport (Epic 18) duplicates without it.

  **The claim *is* the insert.** `ON CONFLICT DO NOTHING ... RETURNING` returns a row only to the caller that actually inserted, so two concurrent identical pushes cannot both believe they are first; a read-then-write would let exactly that happen, which is the concurrency criterion. The response is recorded **after** the work, so a replay returns what happened rather than what was intended. Keys are swept on write — this project refuses a scheduler (Epic 15 decision 5), and a table that only grows is a leak nobody notices until it is large. Matching is on **content, not bytes**: two requests differing only in key order are the same request, and a `409` over a client's serializer would be wrong.

- [x] **Boundary validation** *(Slice D, 30 July 2026)*. Epic 5's shapes run on a draft **before** it is persisted, so a malformed push is rejected per entity rather than corrupting the graph. Cheaper than expected because `constraint::validate` takes *facts*, not a database: a draft is projected to flakes and checked by the identical code that checks a stored entity, with no second implementation to drift.

  It runs **before the write and therefore before the FQN uniqueness check**, which is the ordering criterion: a draft that is both shape-invalid and FQN-conflicting reports the shape violation, because that is the actionable one — a conflict tells a pusher to rename, the wrong fix for a malformed entity. Only `Violation` rejects; a `Warning` lands, since refusing a push over one would make every shape author's judgement call a hard gate. A shape this server cannot compile is ignored here rather than blocking an unrelated push, because one bad shape should not be an outage.


- [x] **Relationships and lineage in a push** *(Slice A completed, 30 July 2026)*. Edges are applied **after every entity**, because an endpoint may be an item submitted after the edge that names it — ordering entities among themselves is a containment problem with one answer, ordering an edge against them is not. Endpoints resolve against the batch first and the catalog second, and edge indexes continue past the entity range so one numbering addresses the whole request.

  **Every pushed edge is lineage, and the alternative was removed rather than left as a trap.** `Relationship` operates on the `tables` relation; a push creates *assets*, so a plain relationship between two pushed assets can never resolve — it failed with `NotFound`, which reads as a missing entity rather than a mismatched model. It was offered as an option until a test proved it always failed. An option that can never succeed is worse than no option.

  The ceiling counts entities and edges together, or a caller could double the cost by splitting work across two fields; edges are part of the idempotency fingerprint, since two pushes with the same entities and different edges are different requests.
**Pending in this epic**
- **Batch file ingestion** (Slice C) — jobs, multipart, per-row errors, a reaper for crashed jobs. The largest remaining piece
- **Generated TypeScript and Python SDKs** (Slice E) and the **custom adapter guide** (Slice F)

- **`ConflictKind` eats a conflict's detail unless it has its own variant.** `AppError` renders a fixed sentence per kind, so borrowing an existing one silently replaces whatever the facade wrote. It cost Slice G its counts and Slice B its key before `PrincipalStillHolds` and `IdempotencyConflict` were added — a design trap, not two slips

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


### Console half
- [ ] **16** Admin: ingestion tokens, batch job status *(Epic 41)*
- [ ] **17** **Merge adjudication queue** *(Epic 42)* — a resolution decision made by a machine and never reviewed is a merge nobody can undo
- [ ] **18, 19** Admin: webhook registry with deliveries; consumer lag and throughput *(Epic 41)*
- [ ] **20** **Drift view — declared vs actual** *(Epic 42)*. Metadata-as-code without a drift screen means the repo and the catalog disagree silently, which is the failure the epic exists to prevent
- [ ] **21** **Extraction review queue** *(Epic 42)* — extracted claims carry confidence, and a claim below the assert band that lands unreviewed is a guess wearing a fact's clothes
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


### Console half
- [ ] **7b** Cypher in the workbench as a second language, same result surface *(Epic 41)*
- [ ] **7c** **Triple ⇄ property-graph toggle** on the Knowledge tab *(Epic 42)* — the same facts in both shapes, because "is it RDF or a property graph" is the question this epic answers and a toggle answers it faster than prose
- [ ] **7d** Admin: Bolt endpoint status, active sessions *(Epic 42)*
- [ ] **9** **Export dialog** — format, scope, and a preview before download *(Epic 42)*. RDF 1.2 output lands here (Epic 94)
- [ ] **9a** Same dialog; projection-target administration *(Epic 42)*
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


### Console half
- [ ] **33** Vocabulary browser + pack install *(Epic 42)*. A pack annotates **meaning**, never flow — see `33-ontology-packs.md`
- [ ] **34** **Nothing, deliberately** — the composable entity page absorbs five new entity families without UI work. If this needed a screen, Epic 39 decision 4 failed
- [ ] **35** Threads and proposals *(Epic 42)*
- [ ] **37a** Admin: budget headroom against measured limits *(Epic 41)*
- [ ] **37b** Admin: export, restore, verify *(Epic 41)*
- [ ] **37c** **Nothing** — a library that shipped a console would be the opposite of embeddable
- [ ] **38** Governance reports: orphans, silos, blast radius *(Epic 41)*
---

## Demo 10 — Standards depth

**The claim**: the standards alignment survives inspection by someone who knows the standards.

**What you can show**: export a lineage edge and point at `rdf:reifies << … >>` in the Turtle — then run the *same* query with the standard vocabulary in the workbench and get rows back, not zero. Add an Arabic-labelled term and watch it render right-to-left in the entity header, the search results and the graph node. Author a constraint in SPARQL rather than in the shape language and see it land in the same violations queue.

### Epic 94 — RDF 1.2 alignment
- [ ] **A** `FlakeValue::TripleTerm` at discriminant 10, pinning test extended
- [ ] **B** `rdf:reifies` + triple term on export; store flake count unchanged
- [ ] **C** `rdf:dirLangString` — lexical form, language tag, base direction in `flake_meta`
- [ ] **C (console)** — every user-supplied label renders with `dir` from the data; asserted with real Arabic or Hebrew, not a placeholder. **Without this half, the slice makes the product worse**: the store would know a label is right-to-left while the screen renders it left-to-right
- [ ] **D** `rdf:reifies` synthesised at the query surface, so the standard vocabulary returns rows rather than zero — store and flake count untouched
- [ ] Slices B, C and D share one `oxrdf/rdf-12` feature gate — one decision, taken once for the workspace
- [ ] Export dialog offers RDF 1.2 output and previews it *(UI → Epic 42)*

### Epic 95 — OWL 2 RL completion
- [ ] The remaining RL axioms beyond Epic 6's eight
- [ ] Explanation panel extends to the new axioms — same surface, more rules *(UI → Epic 41)*

### Epic 96 — SHACL-SPARQL
- [ ] SPARQL-based constraint components
- [ ] The violations workflow is unchanged; **authoring gains a second language**, so the constraint editor gains a second mode *(UI → Epic 41 Slice G)*

### Epic 97 — Incremental & parallel reasoning
- [ ] Incremental maintenance rather than full recomputation
- [ ] **Overlay staleness is visible** — a derived fact whose age is invisible is a derived fact nobody can weigh *(UI → Epic 41 Slice G)*

---

## Demo 11 — Full semantics

**The claim**: three OWL profiles, federation, and a storage split — with the console honest about which one answered and how completely.

**What you can show**: load an ontology and watch the profile badge resolve to EL rather than RL, with the reasoner that will run named next to it. Load one in *no* profile and watch reasoning refuse, naming the offending axiom. Override it and watch the result come back **marked partial**. Then run a federated query and see each remote row attributed to the endpoint that produced it — and a `SILENT` failure appear *in the result* rather than only in a log.

### Epic 98 / 99 — OWL EL and QL reasoning
- [ ] EL and QL reasoners alongside RL
- [ ] **No UI of their own** — which profile ran is a routing question, surfaced by Epic 100. This is the design working, like Epic 34

### Epic 100 — Profile detection & routing
- [ ] Detection across RL, EL, QL; incomparable profiles not reported as supersets
- [ ] Out-of-profile reasoning refused, naming the first offending axiom
- [ ] **Profile badge + the reasoner that produced each derivation** *(UI → Epic 41 Slice G)*. Epics 98 and 99 add reasoners with **different completeness guarantees**, so an unlabelled conclusion is one whose strength cannot be assessed
- [ ] **Out-of-profile and override-partial results marked**, not by colour alone

### Epic 101 — SPARQL federation
- [ ] `SERVICE` against an allow-listed endpoint; unlisted refused by name
- [ ] Bindings denied by policy never transmitted — asserted on the outbound request, the only way to prove a leak did not happen
- [ ] **Remote rows attributed to their endpoint in the result grid** *(UI → Epic 41 Slice G)* — an unattributed remote row is this epic's own named danger, rendered
- [ ] **A `SILENT` failure is visible in the result.** An empty region of a grid reads as "no such data" rather than "we could not ask"
- [ ] **Allow-list admin with dry-run** — adding an endpoint lets the query engine make outbound calls carrying the caller's bindings, which is a policy decision in a configuration costume

### Epic 102 — Read/write partition split
- [ ] The split itself
- [ ] Partition health and replication lag in admin *(UI → Epic 41 Slice G)*

### Epic 103 — In-process traversal
- [ ] The traversal path
- [ ] **No UI** — a performance path with no user-visible behaviour change

---

## Rules for this tracer

0. **This file is the single source of truth for what is built.** Per-epic
   progress lives here, in the `[x]` / `[~]` / `[ ]` marks. Each plan's
   `**Status**:` line is a *summary* of these marks and is derived from them —
   never the other way round.

   **Recorded because they had drifted badly.** On 28 July 2026, twelve plans
   said "Not started" for epics this file marked shipped — including Epics 1, 2,
   3, 15 and 39, whose work is demonstrable in Demo 1 and Demo 2, and Epic 7,
   which had three slices in `git log`. Rule 4 below is why: this file is
   updated in the same commit as the slice, and the status lines were not, so
   they drifted exactly as rule 4 predicts a separately-updated tracker will.
   The lines are corrected; the rule exists so the correction is not needed
   again. **If the two disagree, this file is right and the status line is
   stale** — the same relationship the `00*` documents have with the code.

   **Corollary, added 28 July 2026 after a second drift in the other
   direction.** A pending item must be re-checked against the code before it is
   worked on, not copied forward. Three entries here — Epic 39's facets, Epic
   39's keyboard navigation, Epic 2's `PATCH`/`DELETE` — sat as gaps for
   revisions after the code shipped, because each revision copied the previous
   revision's list. This file is authoritative over the plans' status lines; it
   is **not** authoritative over the code. Rule 4 keeps it true going forward,
   and a grep keeps it honest when it has not been.

1. **Cumulative, always.** Demo N runs everything Demo N−1 ran. A regression in an earlier demo blocks the later one.
2. **A demo is a runnable state**, not a checklist. If it cannot be shown end to end, it is not done regardless of how many boxes are ticked.
3. **`[~]` requires a named gap.** A partial tick without a stated hole is a full tick pretending to be honest.
4. **Update this file in the same commit** as the slice it records. A tracer updated separately drifts within a week.
