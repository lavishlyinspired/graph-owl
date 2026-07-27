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

## Deliberate non-goals

Recorded so nobody re-proposes them as oversights.

| Not doing | Why |
|---|---|
| Full SPARQL 1.1 or 1.2 | The subset in Epic 7 is chosen against the queries a metadata catalog actually receives. Full conformance is a project, and the tail of it — entailment regimes, service descriptions — serves tooling this product does not target |
| `SERVICE` federated query | Revisit when a user wants to join graph-owl against an external endpoint. Nobody has |
| OWL 2 EL | Polynomial classification for very large taxonomies. Metadata ontologies are thousands of classes, not the hundreds of thousands EL exists for |
| OWL 2 QL | Query rewriting instead of materialisation. Epic 6 materialises deliberately, because explainability is a requirement and rewriting hides the derivation |
| RDF/XML | Nothing new emits it. Turtle and JSON-LD cover interoperation |
| Being a general-purpose triple store | The scope decision in `00a`. Flakes exist to make *this catalog* a graph, not to compete with a store whose only job is triples |

## What would change these answers

- **RDF 1.2 reaching Recommendation** — raises emitting `rdf:reifies` from
  optional to expected, and makes the export decision in Epic 9 urgent.
- **A user asking to point Protégé or a SPARQL client at graph-owl** — that is
  the concrete trigger for full SPARQL 1.1, and it has not happened.
- **SHACL 1.2 reaching CR** — reduces the churn cost of building against it.
- **An ontology of more than ~50k classes** — the first real argument for EL.
