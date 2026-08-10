# Plan: Domain Neutrality — one platform, any domain, no per-domain code (Epic 105)

**Branch**: main (committed directly, per `CLAUDE.md`)
**Status**: **In progress, 10 August 2026.** DN-1 in progress; DN-2 and DN-3 planned.
**Depends on**: Epic 4 (flake model), Epic 9 (RDF I/O), Epic 17 (resolution), Epic 33 (ontology packs), Epic 36 (reference apps)
**Crates**: `graph-owl-core` (namespace resolution, blocking strategies) · `graph-owl-engine-postgres` (namespace registry table) · `graph-owl-rdf-io` (IRI resolution) — **no new crates**

## Why this epic exists

The domain-pack platform design asks one question of the implementation: **can a domain be added as configuration and data, with no code change anywhere?** The design says yes. The implementation, checked on 10 August 2026, says **not yet — and there is one hard blocker plus one soft one.**

This epic makes the answer true, and — more importantly — makes it **provable** rather than asserted, by adding a domain deliberately unlike every domain the platform was designed against.

## The finding that motivated it

`Sid::from_iri` (`graph-owl-core/src/flake.rs`) resolves an IRI by iterating **a fixed, compile-time array of sixteen namespace codes**, and `namespace_iri(code)` is a `match` returning `&'static str`. An IRI in any other namespace resolves to `None`, which surfaces as `RdfError::UnrecognisedIri`.

**The proof this is a live problem, not a theoretical one, is already in the tree**: `namespace::CUI = 0x0200`, `SNOMED_CT = 0x0201`, `RXNORM = 0x0202` were added to `graph-owl-core` for Epic 104's UMLS work. **The last domain that needed namespaces got them by editing core Rust.** That is precisely the per-domain hardcoding this epic exists to eliminate, and it happened in the exact place a design review would predict.

Epic 33 Slice A hit the same wall from another direction and worked around it rather than fixing it — its own account says the parser *"resolves every subject and predicate IRI against a **fixed, hardcoded namespace registry**"*, and its fix was to keep SKOS concept IRIs as plain `String`s, never `Sid`s. That was correct for glossary terms, which are not `Sid`-addressed. **It does not generalize to a domain pack's ontology, whose classes and predicates must be real graph subjects and predicates.**

Consequence, stated plainly: **without DN-1, `POST /graph/import/rdf` cannot import any domain pack's ontology, so zero packs work.** It blocks the platform's own P0.

## What is genuinely domain-neutral already (verified, not assumed)

Checked against `crates/` and `ui/` on 10 August 2026. These need no work, and saying so is as important as naming the gaps:

| Capability | Why it is neutral |
|---|---|
| **Flake model** `{s, p, o, cx, t, op}` | A tuple of identifiers. It has no opinion about what the identifiers mean. |
| **Predicate registry** (`predicates` table, `V3`) | Its own migration comment: *"Predicates definable at runtime, so an organisation can extend the vocabulary without a release."* Keyed `(namespace, name)`, with `core` protecting shipped predicates from redefinition. **Already runtime-extensible.** |
| **`Sid` itself** | `{ namespace_code: u16, id: String }` — arbitrary by construction. The *data model* was always neutral; only the *resolution functions* are not. |
| **`namespace::RUNTIME_START = 1024`** | Already reserved, with the comment *"First code the predicate registry may hand out at runtime."* The design anticipated DN-1; nothing consumed the reservation. |
| **Traversal, analytics, reasoning, SPARQL/Cypher, SHACL** | Graph algorithms over arbitrary predicates. A shortest path does not know it is walking invoices rather than referrals. |
| **Entity resolution scoring/bands** | `ScoreWeights`, `ConfidenceBands`, near-miss policy — all numeric configuration over comparison outputs. |
| **Console review queue** | `ReviewQueue.tsx` *"never names a queue"*; five queues ship as `queues.ts` config. Adjudicating a drug-interaction finding and a GST mismatch are the same interaction. |
| **Console vocabulary browser** | Same split (`vocabularies.ts`); four vocabularies ship as config. |
| **Ontology packs** (Epic 33) | Versioning, licence, overrides, upgrade diff, removal — all keyed by source IRI, no domain assumptions. |

## The four chokepoints — two to build, two to state as boundaries

### DN-1 · Runtime namespace registry — **BLOCKER, build it**

A pack declares its own namespace (`hosp:`, `auto:`, `gst:`) and gets a runtime code from the reserved `1024+` range. `Sid::from_iri` and `namespace_iri` resolve against shipped **and** registered namespaces.

**Design constraint that shapes the whole slice:** `namespace_iri` currently returns `&'static str` from a `match`. Runtime namespaces cannot be `&'static str` without leaking. The resolution boundary therefore moves behind a **resolver** the caller supplies, with the compile-time set as its always-present base — so the shipped path keeps working unchanged and stays allocation-free, and only the runtime path pays for a lookup. **The pure-core rule holds: `graph-owl-core` gains no I/O.** It gains a trait and an in-memory implementation; the storage adapter loads the table into it.

### DN-2 · Generic blocking strategies — build it

`graph-owl-core::blocking` ships `normalized_fqn_key`, `name_parent_key`, `soundex`, `column_hash_key` — all catalog-shaped. Hospitality needs phone-number keys, medicine NPI+DOB composites, auto VIN and part-number keys.

**The rule that keeps this neutral: a strategy is named after its algorithm, never its domain, and reads whichever predicates `matching.yaml` names.** `exact`, `normalized`, `phonetic`, `ngram`, `numeric_bucket`, `date_window`, `composite` are neutral; `gstin_key` would be the hardcoding this epic exists to remove. A pack that needs a genuinely new *algorithm* has found a generic-capability proposal — reviewed on its merits, added as an algorithm, never as a domain.

### DN-3 · Domain entities are graph subjects, never catalog asset kinds — **a boundary, not a gap**

`AssetKind` is a fixed Rust enum with a `parent_kind()` containment chain. A pack **cannot** add `hosp:Property` or `med:Patient` as a catalog asset kind, and **must not try**: Epic 33 already refused exactly this (*"Domain-specific entity types from packs → a much larger change to the fixed entity model; packs supply vocabulary, not schema"*), and the platform plan reached the same rule independently as *"no new SQL schema for any pack."*

**The correct model, stated once so no pack rediscovers it wrongly:**

> **Catalog assets are the metadata-about-data layer** — tables, columns, services, dashboards, the things a data catalog describes. **Domain entities are graph subjects** — invoices, patients, rooms, parts, cases — living as flakes in the pack's own named graph, under the pack's own namespace, described by the pack's own ontology.

An invoice is not a table. A patient is not a column. The two layers meet where a *column* is said to *mean* a domain concept — which is Epic 24's glossary link and Epic 33's stated purpose, not a widening of `AssetKind`.

### DN-4 · The console covers graph-shaped work only — **a boundary, not a gap**

The five patterns plus the proposed obligations calendar are domain-neutral, but they are **knowledge-graph surfaces**. A domain wanting a map (property locations), a time-series chart (sensor telemetry) or a BI dashboard is asking for something `00f-ui-architecture.md` deliberately excludes ("Dashboard builder", "Notebook / BI features"). That is a positioning boundary, not a neutrality failure — and it applies identically to every domain, which is what makes it a boundary rather than a bias.

## The acceptance test: a domain deliberately unlike the seven

The platform was designed against seven Indian financial-compliance domains. **Seven samples from one family prove nothing about neutrality** — they share vocabulary shape, a legal spine, deadline arithmetic and identifier-based matching. A design can be accidentally fitted to all seven and still be useless for a hospital.

So the acceptance test is **hospitality**, chosen precisely because it shares nothing with tax compliance: no statute, no filing deadline, no government identifier, no reconciliation of two authorities' records.

> **DN-3 acceptance: a hospitality pack loads its own namespace, resolves duplicate guest/vendor records, produces findings, and renders in the console — with ZERO changes to any `.rs` file and ZERO changes to any `.tsx` file.**
>
> If it needs either, the neutrality claim is **false**, and the design is corrected rather than the pack special-cased. A green test here is the only evidence that "any domain" is a property of the platform and not of the seven domains it was drawn from.

## Checklist — tracked as it is implemented

Legend: `[ ]` not started · `[~]` in progress · `[x]` done and verified

### DN-1 — Runtime namespace registry

- [x] **1.1** RED: `Sid::from_iri` returns `None` for an unregistered namespace (characterises today's behaviour before changing it)
- [x] **1.2** RED: a resolver carrying a runtime namespace resolves an IRI in it; the shipped sixteen still resolve unchanged
- [x] **1.3** `NamespaceResolver` trait + `StaticNamespaces` (shipped set) in `graph-owl-core`, no I/O added
- [x] **1.4** `Sid::from_iri_with` / `namespace_iri_with` resolving against shipped ∪ registered
- [x] **1.5** Round-trip property: `from_iri_with(iri_of(sid)) == sid` for both shipped and runtime namespaces
- [x] **1.6** Collision + validation rules: a runtime namespace may not shadow a shipped IRI, may not re-register a different IRI for a live code, longest-prefix wins
- [x] **1.7** `namespaces` registry table (`V14`) + `NamespaceRegistry` port + Postgres adapter, mirroring `predicates`' shape and its `core` protection. 8 integration tests against real Postgres: a declaration persists and rebuilds a resolver that makes a domain IRI resolvable; redeclaring an identical pair is idempotent (a reload must not fail a restart); a code is never repointed; one IRI never gets a second code; a reserved code is refused as `CoreImmutable`; allocation starts at `RUNTIME_START` and is **monotonic — an abandoned code is never reissued**, because flakes written while it was live still carry it
- [x] **1.8** Mutation run over the resolution logic; survivors triaged — **25 mutants, 0 missed** (22 caught, 3 unviable). The first run left **6 survivors, every one the same missing-negative pattern this project keeps rediscovering**: `len()` survived returning a constant `1` because every assertion happened to be against a one-namespace registry; `is_empty()` survived returning `true` because only the empty case was asserted; `StaticNamespaces::iri` survived three separate mutations (`None`, `Some("")`, `Some("xyzzy")`) because it was reachable only through `pairs()` and never called directly, though `sid_to_iri(.., &StaticNamespaces)` is a real path for a binary that registers nothing; and `Display for RegisterError` survived rendering nothing at all, which would leave an operator with "registration failed" and no reason. Killed by asserting zero/one/many counts, the non-empty negative, the forward direction with two distinct codes, and that each refusal names what it protects.
- [ ] **1.9** `00k`/`00b` decision-log entry: the resolution boundary moved behind a resolver, and why

### DN-2 — Generic blocking strategies

- [x] **2.1** RED: a strategy named after an algorithm produces the same key for two records a domain calls equal, and a different key for two it does not
- [x] **2.2** `Exact`, `Normalized`, `Phonetic`, `NGram`, `NumericBucket`, `DateWindow`, `Composite` over caller-named fields (`graph-owl-core::blocking_strategy`)
- [x] **2.3** The shipped catalog keys expressed as configurations of the generic set — `normalized_fqn_key` is `Normalized` over one field, `name_parent_key` over two, asserted equal so the new module generalizes the old rather than duplicating it
- [x] **2.4** Mutation run — **79 mutants, 79 caught, 0 survivors.** Three real gaps found, none of which inspection would have: the `NGram` length guard had no `len == n` boundary test (`<` vs `<=` decides whether a shortest-valid identifier is blockable at all); and the civil-from-days era term survived twice because the first attempt to kill it asserted `is_some()` on a year-0 date — **which passes under both mutations**. The negative-era branch turned out to be *reachable*, not dead as first assumed: January and February shift into the previous year before the era is computed, so `0000-01-01` computes with year `-1`. Killed by asserting the exact day number.

**Two real defects found while building it, both by the compiler and linter rather than by design.** `f64` is not `Eq`, and chasing that derive error surfaced that a **NaN or infinite bucket width passes every `<= 0.0` guard** (all NaN comparisons are false) while an infinite one divides every amount to zero — either way the entire corpus lands under one key, which is the single failure a blocking stage must never have. Separately, clippy flagged `as i64` on the bucket: it **saturates rather than failing**, so every amount past `i64::MAX` would collapse onto one key. Fixed by formatting the floored float instead of casting, which has no ceiling — and `+ 0.0` normalizes `-0.0`, without which `-0` and `0` are one bucket with two key strings (a *missed* match, and correspondingly harder to notice than a wrong one).

### P0 — `POST /graph/import/rdf` (the platform plan's P0, unblocked by DN-1)

- [x] **P0.1** The route, admin-gated, with `?source=`/`?format=`/`?dryRun=`/`?base=`
- [x] **P0.2** `Class::Ingestion` admission entry — a pack load is exactly the burst that class sheds
- [x] **P0.3** Committed OpenAPI contract regenerated (one path added, none removed) **with its query parameters** — Epic 36 finding 4 found the contract had no mechanism for documenting these at all, and a route a generated client cannot pass `source` to is a route it cannot call
- [x] **P0.4** 9 integration tests: lands and names its subjects, re-import skips rather than duplicates, dry run writes nothing (proven by a real import *after* it still landing), unparsable body is a 400, every documented format accepted, non-admin refused as 404, a `source` that could forge a graph name refused, two sources land in their own graphs
- [x] **P0.5** Mutation — **11 mutants, 0 missed** (10 caught, 1 unviable)

**Three findings worth carrying.**

- **`Catalog::import_rdf` was already complete and had no callers.** Parsing, SHACL validation before any write, per-subject transactionality, dedup, dry run — all shipped in Epic 9 Slice E, reachable from nothing. The only import path on the wire was the admin `/ontology-editor/save`, which edits *this catalog's own* ontology rather than landing a pack's. So P0 was a routing slice over a finished capability, exactly as the platform plan predicted, and the reason nothing could ship without it.
- **The facade takes no principal**, unlike every other write method on `Catalog`. An import writes straight to a named graph, bypassing the asset-level authorization every other write path applies — so the admin gate has to live at the route, and if it did not exist this would be the one unauthenticated write in the system.
- **A first version of the non-admin test asserted `404` and got `200`, which looked like a missing gate and was not.** `test_app` runs every caller as an admin; `authorization_fixture` + `asha` is the fixture every other admin-gate test in the crate already uses. Mixing both fixtures in one binary then exposed a latent fragility — `authorization_fixture` provisions `asha` with an *unasserted* HTTP call, so under parallelism the user is never created and the failure surfaces as an opaque foreign-key violation naming neither the call nor the reason. Serialised in `.config/nextest.toml` with that written down; the real fix (assert the provisioning call) belongs with whoever next touches `common/mod.rs`.

**One refactor the route forced, and it was worth it.** `rdf_format_of` is now shared by import and export, so the two cannot drift into accepting different spellings — a document this server exported as `ntriples` and refused to import under the same word would be an absurd contract, and two independent `match`es is precisely how that happens. And `is_usable_import_source` was extracted as a free function so a unit test can reach it: the route around it is only reachable end-to-end, where a container-backed mutation run costs ~60s per mutant against ~0 for a pure predicate. The first attempt mutated the whole diff and **timed out after 10 minutes** — the same crate-placement argument `00e` makes, one level down.

### P0b — `POST /namespaces` (the route DN-1 was missing)

DN-1 built a table, a port and an adapter, and **nothing exposed them** — the same shape `Catalog::import_rdf` was in before P0. A capability nothing can reach is not a capability.

- [x] **P0b.1** `Catalog::declare_namespace` / `Catalog::namespaces`, behind a third optional field (`namespaces`) beside `graph` and `traversal` — the precedent the `Catalog` already sets, and for the same stated reason: declaring a vocabulary and storing flakes are genuinely separate contracts
- [x] **P0b.2** `POST /namespaces` (admin) and `GET /namespaces` (not admin — a prefix list is the vocabulary this deployment understands, which anyone writing a query needs; it carries no data)
- [x] **P0b.3** Wired into all three composition roots — `main.rs`, the stdio binary, and the test harness. A route whose registry is `None` returns a 500 saying so, which is a worse failure than not shipping it
- [x] **P0b.4** 8 integration tests; contract regenerated (one path added, none removed)
- [x] **P0b.5** Mutation — **5 mutants, 0 missed** (2 caught, 3 unviable), after a real gap the first run exposed

**The mutation run found something the tests genuinely missed, and the first explanation was wrong.** Two mutants survived: the idempotence check (`==` → `!=`) and `namespaces()` → `Ok(vec![])`. The tempting reading was `--lib` blindness — this project already records that `--lib` cannot see integration-only coverage. **That reading was wrong here**: `cargo mutants -p graph-owl-api` runs *`graph-owl-api`'s own* tests and would never have seen `graph-owl-server/tests/namespaces.rs` with or without `--lib`. The logic is pure, unit-testable, and simply was not unit-tested. Fixed with an in-memory `NamespaceRegistry` double and 7 unit tests — including the negative that matters most (a check returning the existing entry for *every* IRI would pass an idempotence test and collapse every vocabulary onto one code) and the "no registry configured" case, where reporting "none declared" would be a lie that reads as a working empty system.

**The general lesson: before blaming a survivor on `--lib`, check which crate's test suite the mutation run actually invokes.** A survivor in crate A covered only by crate B's tests is not a tooling artifact — it is uncovered logic.

**The load-bearing design decision: the caller names an IRI and never a code.** A pack manifest carrying a number would make two deployments that installed packs in different orders disagree about what `1024` means — and a `Sid` is stored as a bare `(code, local)` pair, so that disagreement is unfixable after the fact rather than a migration. `deny_unknown_fields` makes the point enforceable: a caller who sends `{"code": 5}` believing it chose one is told it did not, rather than silently getting an allocated code and a false belief about what its manifest controls.

**Idempotent by IRI, and that is the normal case rather than the edge one.** A pack is reloaded far more often than it is first installed, so re-declaring returns the existing code. A conflict would make every second `demo.sh` run fail; a second allocation would make the pack's own IRIs resolve to two different `Sid`s depending on when they were written. Returns `200`, not `201` — a `201` would claim creation on every reload.

**Two compiler-found requirements worth recording.** `AppJson<T>` requires `T: ValidateBody`, not merely `DeserializeOwned` — the project's own body-validation contract, and the `Handler` trait bound fails opaquely without it (axum's `macros` feature is off, so `#[debug_handler]` is unavailable to diagnose it). And `graph-owl-server` has no direct dependency on `graph-owl-engine`, so `NamespaceDef` is re-exported through `graph-owl-api`: the facade is what the server is meant to speak to.

### P1 — the pack format and loader

- [x] **P1.1** `pack.toml` (TOML, for `tomllib` — the stdlib has parsed it since 3.11 and has never parsed YAML, and a YAML manifest would put PyYAML in the dependency tree of everything that loads a pack, including the reference apps that may import nothing but stdlib + `graph_owl_sdk`)
- [x] **P1.2** `connectors/python/graph_owl_packs` — manifest reader, loader, `graph-owl-load-pack` CLI. No runtime dependencies; `urllib` only
- [x] **P1.3** `POST /predicates` — **the third registry that existed with no route.** A pack cannot assert a single fact without it: `reject_unregistered_predicates` refuses any flake whose predicate is unknown
- [x] **P1.4** 30 tests (17 manifest, 13 loader against a real local HTTP double)

### DN-3 — Hospitality proof-pack

- [x] **3.1** `packs/hospitality/` — own namespace, ontology, predicates, matching config, findings, console block
- [x] **3.2** Loads and resolves with zero `.rs` changes, **asserted against `git ls-files` in `scripts/verify-pack-load.sh`** rather than by inspection
- [x] **3.3** Ships no `.tsx`, `.ts` or `.css` — same assertion
- [x] **3.4** `packs/gst/` alongside it, sharing no vocabulary, no legal spine, no identifier scheme and no subject matter — **and configuring the identical blocking strategies**, which is the surprising half of the claim

**Run, not asserted** — `scripts/verify-pack-load.sh` loads both packs into one real graph-owl: hospitality lands 15 subjects under code 1024, GST lands 19 under 1025, both reload as no-ops keeping their codes, and no pack file is code.

**Four gates found only by running it, each a genuine design requirement nobody had written down:**

1. **The namespace was declared and stored, and the parser did not use it.** DN-1 built the registry; nothing plugged it into `Sid::from_iri`. Fixed with a process-wide runtime table that resolution falls through to — the mapping is a property of the deployment, not of a call, so threading a resolver through the parser, serializer, SPARQL and export would have been threading the same value everywhere.
2. **A restart would have silently un-resolved every pack.** The rows survive; the in-process table does not. `Catalog::prime_namespaces` at startup, because without it a restarted server stops understanding the vocabulary of every pack installed before it — a total failure that looks like the packs were never loaded.
3. **Predicates must be defined before documents are imported.** The first load rejected all 15 subjects. Hence `POST /predicates` and `[[predicates]]` in the manifest, and the loader's three-phase order: namespace → predicates → documents.
4. **A pack may not assert `rdfs:label`.** `rdfs:` is a shipped namespace whose predicates this store does not register, and a pack may not define terms in somebody else's vocabulary. Both packs now own every predicate they assert (`hosp:label`, `gst:label`).

### P2 — the discovering connector, and the strongest neutrality evidence so far

`packs/hospitality` and `packs/gst` are weak evidence in one specific way: **this project wrote them**, so they fit the platform by construction. `connectors/python/graph_owl_packs/erpnext.py` reads a schema from a live Frappe/ERPNext instance and derives the vocabulary from what the instance reports — no mapping table, no per-doctype branch, nothing named after accounting.

**Proven end to end against a real graph-owl**: a DocType invented in the test (`Rescue Mission` — deliberately not an invoice) became 6 landed subjects under namespace 1024 with 4 discovered predicates, and answered a SPARQL query. Its `Link` field resolved as a **real edge** (`<…#MV_Resolute>`, a subject reference rather than a string), with two missions pointing at one vessel — traversable structure derived entirely from metadata.

It drives exactly the three routes this epic built: `POST /namespaces` → `POST /predicates` (all of them, before any document — the ordering the pack loader learned by running) → `POST /graph/import/rdf`.

**Licensing, recorded in `plans/00l-build-vs-adopt.md`.** ERPNext is GPL-3.0 and `india-compliance` (where GST actually lives — *not* core ERPNext) is too; `frappe` and `frappe_docker` are MIT. The copyleft gate is not engaged because graph-owl never links and never vendors — it speaks HTTP to a separate process, the same shape as the OCR model endpoint and the whelk-rs sidecar, and the surface actually used is Frappe's MIT one. **The rule that keeps it that way: never vendor a doctype definition or fixture derived from ERPNext.** Discovery at run time is both licence-safe and better engineering — a vendored schema drifts silently.

Design decisions worth keeping: layout fieldtypes (section breaks, HTML) are **dropped rather than imported and ignored**, because a predicate defined for a section break is a permanent registry entry meaning nothing; every non-`Link` value lands as a **string**, because a currency parsed to a float at the graph boundary loses the exactness a monetary figure needs and a date parsed to an instant invents a timezone the source never stated; and an absent or empty value is **omitted rather than written blank**, because "not recorded" and "recorded as blank" are different facts and a graph that cannot tell them apart cannot answer a question about missing data — which is most of what a reconciliation asks.

### Closed — the GST reconciliation is visible

`packs/gst/queries/` returns the planted scenarios against a real server, asserted in `verify-pack-load.sh`: **INV-1003** never filed, **INV-1004** unmatched, **INV-1002** claiming ₹18,000 against ₹17,100 filed, and INV-1001 correctly producing nothing. Three defects stood between "loaded data" and "visible reconciliation", and none was findable by reading:

1. **The same per-domain hardcoding, in a second place.** `scope_facts` admits a flake only when its subject is a visible catalog *asset* — or when `is_vocabulary_namespace` says so, and that was **a hardcoded list of exactly three medical namespaces** (CUI, SNOMED_CT, RXNORM). The identical Epic 104 hardcoding this epic removed from `graph-owl-core`, living again in the authorization filter. A pack's subjects are graph subjects with no asset row *by design* (DN-3's boundary), so every pack fact was filtered out of SPARQL. Generalized to any runtime-declared namespace — the same argument that already admitted a SNOMED-coded clinical fact.
2. **Pushdown does not descend into `FILTER NOT EXISTS`.** The GSTR-2B facts were never loaded, so the filter was trivially true and *all four* invoices reported as missing — including the two that match perfectly. The queries use `OPTIONAL` + `!BOUND` instead, which pushdown does handle. **Teaching pushdown to walk `NOT EXISTS` is the engine-side follow-up**; a query that silently reports every row as unmatched is a bad failure to leave reachable.
3. **Every pattern must be inside `GRAPH ?g`.** Imports land in `graph:import:{source}`, never the default graph, so a query without a `GRAPH` clause matches nothing — silently, which reads exactly like "the reconciliation found no problems".

**INV-1004 is a true finding about the method, not a bug.** Its GSTIN is transposed by two characters, so an exact join cannot pair it even though the supplier filed it. That is why `pack.toml` configures an `ngram` strategy, and the gap between these two answers is the concrete argument for the fusion engine.

**The authorization limitation this leaves, stated plainly**: a pack's facts are now readable by any principal who can query. That matches how the three medical namespaces were already treated, so it is consistent with the system's existing posture rather than a new weakening — but it is not right for a pack carrying real invoices. **Per-named-graph policy**, so access to `graph:import:{source}` is a policy decision rather than a namespace one, is the follow-up. It needs a policy model that does not exist and is a deliberate design decision, not something to infer.

### GST console surface — blocked on the above, not on the console

The `[console]` block in `packs/gst/pack.toml` declares its review queue (`gst-reconciliation`, the two finding labels, side-by-side evidence). **No console change is needed to render it** — `ui/src/features/review/queues.ts` is already a config registry behind one generic `ReviewQueue.tsx`, which is exactly §13's claim. What is missing is something for it to *fetch*: the findings runtime (the platform plan's P5) does not exist, and the SPARQL fallback above returns nothing. Wiring a queue to an empty source would be a screen that always says "nothing to review", which is indistinguishable from a working one.

### Cross-cutting

- [x] **X.1** `scripts/check-namespace-neutrality.py`, wired into `scripts/gate.sh`. **Verified in both directions rather than written**: a deliberately-added `namespace::HOSPITALITY` constant fails it with the constant named, and the tree passes clean at 18 constants. Carries its own negative guard too — if the allowlist names constants the source no longer has, the check reports *itself* as broken rather than silently matching nothing and passing whatever arrives. CUI/SNOMED_CT/RXNORM are allowlisted as **grandfathered, not endorsed**, with a comment saying so; a genuinely general vocabulary (a new W3C standard) passes by an explicit allowlist edit in the same commit, which is the reviewable act that was missing when the medical namespaces arrived
- [ ] **X.2** `CLAUDE.md` gains the domain-entity-vs-catalog-asset distinction as a fifth conflated pair
- [ ] **X.3** DEMOS.md checkbox + `EPIC-STATUS.md` regenerated

## Acceptance criteria

- [ ] A domain declares its own namespace and its IRIs become real graph subjects and predicates, with no core Rust change.
- [ ] The shipped sixteen namespaces resolve exactly as before, on the same allocation-free path.
- [ ] A blocking strategy is selected and parameterized by configuration; no strategy is named after a domain.
- [ ] The hospitality pack — a domain sharing nothing with the seven the platform was designed against — works end to end with zero `.rs` and zero `.tsx` changes.
- [ ] A CI check fails the build if a domain namespace is added to `graph-owl-core`.

## Explicitly deferred (with destination)

- **Domain entity types as catalog `AssetKind`s** → refused, permanently. DN-3's boundary above; Epic 33 refused it first.
- **Per-domain console components, themes or routes** → refused. §13 of the platform plan; a pack configures, never extends.
- **Domain-specific visualizations** (maps, time-series, BI) → `00f`'s existing exclusions apply to every domain equally.
- **Automatic namespace discovery from an imported document** → a namespace is *declared* by a pack, never inferred from content; inferring it would let a malformed import mint namespaces silently.

## Pre-PR quality gate

1. `cargo mutants` on the resolution and blocking logic — survivors triaged, equivalents justified.
2. `cargo test/clippy/fmt` on touched crates.
3. The hospitality pack proves neutrality mechanically (zero-diff assertion), not by inspection.
4. The regression guard (X.1) verified against a deliberately-added domain namespace.
