# 00k — Standards Conformance

**Status**: standing reference. Binds Epics 5, 6, 7, 9 and anything else that
claims to speak a W3C vocabulary.

## Why this document exists

Four epics each implement "a subset of" a W3C standard, and each plan says so
in its own words. That produces a product whose overall conformance nobody can
state — a buyer asks "do you support SHACL" and the honest answer is scattered
across four files in four vocabularies.

This is the single place that answers it. **Every row states what is
implemented, what is not, and against which published document at which
maturity.** A claim without a spec reference is not a claim.

## Verification date

Statuses below were checked against w3.org on **28 July 2026**. Anything not
checked on a given pass keeps its previous date, so a stale row is visible as a
stale row rather than passing as current.

Standards move. A conformance table that is not dated is a conformance table
that is wrong, and the failure mode is confident and silent.

## The specification landscape, as published

| Specification | Status | Dated |
|---|---|---|
| RDF 1.2 Concepts and Abstract Syntax | Candidate Recommendation | 7 Apr 2026 |
| RDF 1.2 Semantics | Candidate Recommendation | 7 Apr 2026 |
| RDF 1.2 Turtle / TriG / N-Triples / N-Quads / XML | Working Draft | Jun 2026 |
| SPARQL 1.2 Federated Query | Candidate Recommendation | 7 Apr 2026 |
| SPARQL 1.2 Query Language | Working Draft | 2026 |
| SPARQL 1.2 Protocol · Service Description · Entailment | Working Draft | 2026 |
| SHACL 1.2 Core | Working Draft | 2026 |
| SHACL 1.2 SPARQL Extensions | Working Draft | 24 Jul 2026 |
| OWL 2 RL (profile) | Recommendation | 2012, unchanged |

**Read this before treating any of it as a deadline.** RDF 1.2 and SPARQL 1.2
Federated Query are the only two at CR; CR exit requires two independent
implementations passing each test in the suite. Everything else is Working
Draft and may change. Building against a Working Draft is a decision to accept
churn, not a decision to be early.

**RDF 1.2 Concepts did not advance on schedule, and that is the useful fact.**
Its CR snapshot stated it would not become a Recommendation earlier than
**5 May 2026**. That date has passed; re-checked at w3.org on 28 July 2026, it
is still a Candidate Recommendation Snapshot dated 7 April 2026. Recorded
explicitly because "CR, 7 Apr 2026" read in late July invites the reasonable
but wrong inference that it must have advanced by now. It has not, so
`94-rdf12-alignment.md` decision 5 still binds: the claim is "aligned with
RDF 1.2 CR of 7 April 2026", never "RDF 1.2 compliant". The earliest-possible
date in a CR is a floor on the wait, not a forecast of it.

OWL 2 RL is the exception and the reason Epic 6 targets it: it has been a
Recommendation since 2012 and is not moving.

## Where graph-owl actually stands

`—` means not started, not "partially works".

| Capability | State | Where |
|---|---|---|
| Triple store with time-travel | **Shipped** | Epic 4 |
| Four index orderings | **Shipped** | Epic 4 |
| Reified edges — structurally RDF 1.2's reifier | **Shipped** | Epic 4 slice E |
| `rdf:reifies` + triple terms emitted | — | Epic 9 decides default vs export-only |
| `rdf:dirLangString` (language + base direction) | — | Epic 4's `flake_meta`, unbuilt |
| Predicate registry with cardinality recorded | **Shipped** | Epic 4 slice H |
| Cardinality *enforced* on write | — | Epic 4, named gap |
| Bounded graph traversal | **Shipped** | Epic 7a |
| SPARQL — any of it | — | Epic 7 |
| SHACL Core validation | — | Epic 5 |
| SHACL-SPARQL constraints and rules | — | Epic 5, was not previously scoped |
| OWL 2 RL reasoning | — | Epic 6 |
| JSON-LD | — | Epic 9 |
| Turtle / N-Triples / TriG serialization | — | Epic 9 |

## The four conformance decisions

### 1. RDF 1.2 alignment is additive, not a migration

Established in `04-engine-triples.md` finding 5. graph-owl's reified
relationship node *is* an RDF 1.2 reifier; what is missing is the `rdf:reifies`
predicate and a triple-term value. One appended `FlakeValue` variant, which the
pinned discriminant already makes safe.

**This is the single most valuable correction in this document**, because the
previous framing — "we chose reification over RDF-star" — implied a divergence
that would have to be defended, and there is none to defend.

### 2. JSON-LD before Turtle

Both live in Epic 9 and JSON-LD goes first. Not because it is more standard but
because it is what the *inputs* are shaped like: DCAT, PROV-O, ODCS and
OpenLineage are all published as JSON-LD, so JSON-LD is an ingestion capability
and Turtle is an export convenience. Expand and compaction first; framing later,
where it can replace per-endpoint DTOs.

### 3. SHACL means Core **and** SPARQL Extensions

Epic 5 previously scoped SHACL Core only. SHACL 1.2 SPARQL Extensions adds
three things this project specifically wants:

- `sh:SPARQLConstraint` — arbitrary SPARQL as a constraint, which is the escape
  hatch that stops every unusual rule becoming a feature request;
- SPARQL-based constraint *components*, parameterised and reusable;
- SPARQL-based **inference rules** — shapes-driven derivation.

The third matters more than it looks: one shapes graph then serves as both the
data-quality gate and a derivation engine, which is a smaller system than a
validator plus a separate rule language. It also overlaps Epic 6, and the
overlap needs deciding rather than discovering — see decision 4.

**Both are Working Draft.** Implementing against them is accepting churn.

### 4. SHACL rules and OWL 2 RL overlap, and that is a decision not an accident

Both derive triples. OWL 2 RL derives from *ontology axioms* — subclass,
transitivity, inverse — and answers "what follows from the model". SHACL rules
derive from *shapes* and answer "what should be filled in for data of this
shape". They will produce overlapping facts on a real catalog.

The rule: **derived facts carry which engine derived them**, and the reasoning
overlay (`graph:reasoning`) holds both. Epic 6's explainability requirement then
covers both, because a derivation chain that cannot say which engine produced a
step is not an explanation.

## The recommendation ledger

Both source analyses proposed concrete changes. Every one is accounted for
below, because a recommendation silently dropped is indistinguishable from one
that was never read.

**Adopted** — went into a plan:

| Recommendation | Where |
|---|---|
| Prioritise JSON-LD | `09` decision 3 — before Turtle, because the inputs are JSON-LD |
| SHACL beyond Core | `05` deferred section + Epic 96 |
| Design for RDF-star | Epic 94 — and the model understanding was corrected in the process |
| Complete OWL 2 RL | Epic 95, scoped: facts here, contradictions in Epic 5 |
| Incremental reasoning (DRed) | Epic 97, with a measured entry condition |
| Parallel reasoning | Epic 97, same |
| SHACL-SPARQL + rules | Epic 96, blocked on the spec leaving Working Draft |
| Authorization-aware reasoning | `06` — derived facts inherit their least-visible premise |
| Six index permutations | `04` finding 7 — decided *against* for now, with a trigger |
| JSON-LD native vs compatible | `09` decision 5 — compatible, with the three failure modes named |

**Declined** — with the condition that would reverse it:

| Recommendation | Why not | Reverses when |
|---|---|---|
| Full SPARQL **evaluation** | Parsing is now full SPARQL 1.1 via `spargebra` (`07` decision 8); only evaluation is subset. "Do you support SPARQL" is answered per algebra node, not yes/no | Per node, as each is evaluated — no longer an all-or-nothing decision |
| ~~`SERVICE`~~ | **Reversed 28 Jul 2026 — now Epic 101.** Cheap (the parser handles it; SPARQL 1.2 Federated Query is at CR) and the epic is really about three dangers: outbound calls from inside a query, bindings leaving the process, and unattributable remote results | Already reversed |
| ~~Aggregates, `GROUP BY`, subqueries~~ | **Reversed 28 Jul 2026.** Epic 93's Overview computes counts-by-kind and coverage in hand-written SQL precisely because SPARQL cannot — the project is itself the user that needed them | Already reversed; promoted to Epic 7 v2 |
| A second triple-store backend | See *An embedded RDF store as a backend* below — the deferral in `00e` is right but its stated reason is the weak one | Only under the conditions named there |
| ~~OWL 2 EL~~ | **Reversed 28 Jul 2026 — now Epic 98.** The deferral assumed metadata ontologies only; the stated use includes medical ontologies, and SNOMED CT at 400k+ classes is exactly what EL exists for | Already reversed by new information about the use case |
| ~~OWL 2 QL~~ | **Reversed 28 Jul 2026 — now Epic 99**, with a correction: QL gives a *different* explanation (the rewritten query), not none. It does however **forbid** property chains, keys and functional properties, so it cannot replace RL for Epics 17 and 29 | Already reversed |
| RDF/XML | Nothing new emits it | Never, plausibly |
| Split read/write partitions | **Now Epic 102 — planned, entry condition unchanged.** The design is recorded so it can be built correctly; the trigger is still a measurement, because building it early adds a merge path, a second read path and a compaction schedule for a problem not yet observed here | Epic 37a shows index maintenance exceeding the ingestion budget |
| Bulk-load path distinct from the API | Already effectively met — Epic 4 batches at the bind-parameter ceiling inside one transaction | A load arrives that the batching path cannot absorb |

### An embedded RDF store as a backend, examined properly

The recommendation was to consider a Rust-native, permissively-licensed,
RocksDB-backed RDF store as an alternative engine backend, on the strength of
being several times faster than a JVM store in query benchmarks. `00e` already
defers it with "a second backend before the first is proven adds no
information", which is true and is not the interesting reason. The interesting
reasons are three, and they point the other way.

**1. It does not do what this store exists to do.** graph-owl's flake store is
not a generic triple store that happens to hold metadata. It is a
*time-travelling, authorization-filterable* store, and those two properties are
the product's differentiators. A general RDF store offers neither: no
`as_of`, and no way to compile an access predicate into the scan. Substituting
one would mean discarding `?asOf=` and the `AccessPredicate` lowering — replacing
the parts that make this distinct with a faster version of the parts that do
not.

**2. The benchmark compares something this project has not built.** "N× faster"
is a *SPARQL query* number. graph-owl has no SPARQL. Its current hot paths are
pattern lookups over four composite indexes and a recursive-CTE frontier walk,
and no published benchmark speaks to either. Adopting a number measured on a
different workload is how a project acquires someone else's scale assumptions.

**3. It would be a second datastore, not a replacement.** Relational is the
source of truth and that is Postgres. Adding a second engine means two stores
to operate, back up, restore and keep consistent, against a deployment model
that is explicitly single-node with a budgeted footprint (`00a`).

**But there is a real use, and it is a better one than the recommendation
made.** If Epic 7 implements a SPARQL subset, a permissively-licensed Rust
SPARQL implementation is available *as a differential-test oracle*: run the same
query against both, require identical results on the subset, and a divergence is
a bug in graph-owl's planner rather than an opinion about it. This project
already uses differential testing internally — Epic 6 against hand-coded
inheritance, Epic 7's fast paths against the general path — and an external
oracle is strictly stronger, because it cannot share a misunderstanding with the
code under test.

That is a **test dependency**, not a runtime one. Licence permits it; nothing
ships. Recorded here as the form this recommendation should take if it is taken
at all.

**Reverses when**: a deployment is demonstrably read-heavy SPARQL over a static
graph, where time-travel and per-principal filtering are not required. That is a
different product, and saying so is cleaner than pretending the swap is cheap.

**Not a recommendation, but worth recording**: the analyses' benchmark numbers
(load times, p95 latencies) are from other systems on other hardware. They are
not targets for this project, whose budgets live in `00a` and are stated
against its own footprint claim.

## Deliberate non-goals

Recorded so nobody re-proposes them as oversights.

| Not doing | Why |
|---|---|
| ~~Full SPARQL~~ | **No longer a non-goal.** `07` decision 0: full SPARQL 1.1 is the target, delivered in stages. Parsing is already total; the algebra is a closed set, so completion is a finite list rather than an open commitment |
| ~~`SERVICE`~~ | **Now Epic 101** |
| ~~OWL 2 EL~~ | **Now Epic 98**, on the medical-ontology requirement |
| ~~OWL 2 QL~~ | **Now Epic 99.** Its explanation is the rewritten query, which is a different shape rather than an absence |
| RDF/XML | Nothing new emits it. Turtle and JSON-LD cover interoperation |
| Being a general-purpose triple store | The scope decision in `00a`. Flakes exist to make *this catalog* a graph, not to compete with a store whose only job is triples |

## What would change these answers

- **RDF 1.2 reaching Recommendation** — raises emitting `rdf:reifies` from
  optional to expected, and makes the export decision in Epic 9 urgent.
- **A user asking to point Protégé or a SPARQL client at graph-owl** — that is
  the concrete trigger for full SPARQL 1.1, and it has not happened.
- **SHACL 1.2 reaching CR** — reduces the churn cost of building against it.
- **An ontology of more than ~50k classes** — the first real argument for EL.
