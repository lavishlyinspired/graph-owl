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

### DN-3 — Hospitality proof-pack

- [ ] **3.1** `packs/hospitality/` — own namespace, ontology, shapes, matching config, findings
- [ ] **3.2** Loads and resolves with zero `.rs` changes (asserted by a diff check, not by inspection)
- [ ] **3.3** Renders with zero `.tsx` changes (same assertion)
- [ ] **3.4** A guest-duplicate finding lands with provenance and a `governedBy` citation

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
