# 00n — Large Ontologies at Scale: an honest assessment

**Status**: standing reference. Written 28 July 2026 in response to a stated requirement change: **millions to billions of triples**, with **FIBO, UMLS, SNOMED CT, RxNorm and DBpedia** as ontologies to reconcile against.

**Read this before `00a-product-position.md` is quoted in an argument about scope.** It does not overrule `00a`; it states what the new requirement costs, so the choice between them is made deliberately rather than by accretion.

---

## 1. The headline: this is a different product, and pretending otherwise is the expensive option

`00a-product-position.md` positions graph-owl as an **enterprise metadata catalog** — a system that describes *data assets*. `37a-scale.md` targets **100,000 entities ≈ 1M flakes**. Several decisions across the plans were made *because* of that target, and are correct only under it:

| Decision | Where | Made because the target was a catalog |
|---|---|---|
| OWL 2 RL only, EL/QL "not needed" | `06-engine-reasoning.md` | Metadata ontologies are shallow; RL covers them |
| Postgres only, single instance | `00a`, `37a` decision 4 | 1M flakes fits comfortably |
| Reasoning overlay, **never persisted**, wholesale replaced per run | `00b` decision 14 | Re-deriving 1M flakes is seconds |
| Reasoning budget: 100k facts, 30s, 512MB | `06-engine-reasoning.md` | Generous at catalog scale |
| No NER; extraction behind a port | `21-document-ingestion.md`, `00j` | Metadata is structured at the source |

The new requirement — SNOMED CT, UMLS, DBpedia, at billions — is a **biomedical and general-purpose knowledge graph**. That is a different product with different physics. It is *reachable* from here, because the flake model, the time-travel design and the authorization model are all sound at any scale. But **five of the decisions above stop being right**, and this document says which and why.

**The honest framing is a fork, not a stretch.** Either:

- **(A)** graph-owl stays a metadata catalog and these ontologies are *vocabularies it annotates with* — FIBO terms on financial columns, SNOMED codes on clinical fields. Scale stays ~1M flakes. Almost nothing below is needed.
- **(B)** graph-owl becomes a knowledge graph engine that *hosts* these ontologies and reasons over them. Scale becomes 10⁸–10⁹. Most of this document applies.

**These are different products and the same codebase can serve both**, but only if the difference is explicit. The dangerous middle is claiming (B) while having planned (A) — which is where the project is today.

---

## 2. What breaks, specifically

### 2.1 OWL 2 RL cannot classify SNOMED CT. This is correctness, not performance.

The most important finding in this document.

**SNOMED CT is the ontology OWL 2 EL was designed for.** [W3C's profiles document](https://www.w3.org/TR/owl2-profiles/) says EL's constructors "are sufficient to express the very large biomedical ontology SNOMED CT". SNOMED defines concepts with **existential restrictions** — *a `Bacterial pneumonia` is a `Pneumonia` that `hasCausativeAgent some Bacterium`* — and derives its subsumption hierarchy from them.

**OWL 2 RL cannot draw those conclusions.** RL deliberately restricts `owl:someValuesFrom` in the consequent position, because permitting it breaks the property that made RL rule-expressible. RL and EL are **incomparable** profiles, not a subset relationship. So an RL reasoner over SNOMED does not produce a smaller inference set — it produces a **wrong** one, silently, by failing to infer the subsumptions that are the whole point of the ontology.

The existing analysis rates "No OWL 2 EL support" as **Low severity — not needed for metadata**. Under requirement (B) that rating inverts to **Critical**, and the plans already anticipated it: `98-owl-el-reasoning.md` says it is "**scheduled**, on the medical-ontology requirement". **That requirement has now been stated. The trigger has fired.**

`100-profile-detection-and-routing.md` becomes load-bearing rather than a nicety: with three profiles in play, "which reasoner ran and what could it therefore not conclude" is the difference between a trustworthy answer and a confident wrong one.

### 2.2 UMLS is not an OWL ontology, and treating it as one will not work

UMLS is a **Metathesaurus**: ~4.5M concepts integrating 200+ source vocabularies (SNOMED CT and RxNorm among them), distributed in **RRF** (Rich Release Format) pipe-delimited files, not OWL. Its core structure is:

- **CUI** — a Concept Unique Identifier, the thing that says "these twelve strings from nine vocabularies all mean one concept"
- **AUI/SUI/LUI** — atom, string and lexical identifiers beneath it
- **MRREL / MRCONSO / MRSTY** — relationship, concept-name and semantic-type tables

**The CUI is a pre-computed cross-vocabulary alignment.** That is not a reasoning output; it is curated data, and it is the single most valuable thing UMLS provides. A system that tried to *derive* it with an OWL reasoner would be reimplementing decades of manual curation, badly.

Consequence: UMLS needs an **ingestion path**, not a reasoner — an RRF reader that lands CUIs as first-class identity anchors. Nothing in `09-engine-rdf-io.md` reads RRF, and it should not: this is a connector-shaped problem (`15-connectors.md`), not a serialization one.

### 2.3 DBpedia is the QL case, and its shape is the opposite of SNOMED's

DBpedia is a **huge ABox with a thin TBox** — hundreds of millions of instance triples against a shallow class hierarchy. That is precisely what **OWL 2 QL** exists for: query rewriting rather than materialization, because materializing inferences over that ABox produces more data than the source. `99-owl-ql-reasoning.md` covers it and is likewise "scheduled".

So the five named ontologies need **three different reasoning strategies**:

| Ontology | Shape | Profile | Strategy |
|---|---|---|---|
| SNOMED CT | Deep TBox, existentials | **EL** | Consequence-based classification |
| RxNorm | Moderate taxonomy + relations | EL/RL | Classification, then rules |
| UMLS | Not OWL — curated alignment | **n/a** | RRF ingestion, CUI as anchor |
| DBpedia | Thin TBox, vast ABox | **QL** | Query rewriting, do not materialize |
| FIBO | Rich TBox with constructs outside RL | **DL-ish** | Profile-detect, then refuse or approximate **by name** |

**One reasoner cannot serve this table, and the current plan has one.**

### 2.4 The reasoning overlay's central design assumption fails at 10⁸+

`00b-architecture.md` decision 14: reasoning is *"a queryable overlay, never persisted"*, with `06-engine-reasoning.md` adding **wholesale replacement per run**. At 1M flakes that is elegant — re-derive in seconds, no staleness, no invalidation logic, base data provably untouched.

At 10⁸–10⁹ base triples it fails on arithmetic, not on design taste. Materialised RL inference typically produces inferences on the order of the base set; re-deriving that per run is hours, and the memory required is not 512MB. The industry evidence is unambiguous and includes negative results: mature commercial engines materialize *once* and maintain **incrementally** thereafter, and at least one well-known engine is documented as failing outright on full OWL 2 RL at the 100M-triple mark.

So at requirement (B):

- **Wholesale replacement must become incremental maintenance.** `97-incremental-parallel-reasoning.md` exists and is "deliberately unscheduled until measurement demands it". Measurement now demands it — it moves from optional to **prerequisite**.
- **"Never persisted" has to be re-examined.** The property worth keeping is *derived facts are distinguishable from asserted ones and are never confused with them* — which the named-graph split gives. The property that fails is *recomputed from scratch on demand*. Those are separable, and only the second must go.
- **The budgets are 10³–10⁴× too small.** 100k facts and 512MB were calibrated for a catalog. They are not defaults to raise casually — a budget is a promise about the worst case, and re-deriving it means re-deriving the promise.

### 2.5 Ontology *reconciliation* is a capability nothing in the plans covers

This deserves its own heading because it is the requirement most likely to be assumed covered and is not.

**Three different problems keep getting called "reconciliation":**

| Problem | Question | Covered by |
|---|---|---|
| **Instance resolution (ABox)** | Are these two *records* the same thing? | `17-entity-resolution.md` ✅ |
| **Vocabulary annotation** | What does this column *mean*, in FIBO terms? | `33-ontology-packs.md` ✅ |
| **Ontology alignment (TBox)** | Is SNOMED's `Myocardial infarction` the same *class* as ICD-10's `I21`? | **Nothing** ❌ |

The third is **ontology matching** — a research field with its own literature, benchmarks (OAEI) and tooling. It produces `owl:equivalentClass` / `skos:exactMatch` alignments between *class hierarchies*, and it is neither entity resolution (which resolves individuals) nor pack installation (which supplies vocabulary without relating it to anything).

**The good news is that for the named set, most of it is a data problem rather than an algorithm problem.** UMLS's CUIs *are* an alignment across SNOMED, RxNorm and 200+ others, curated by the NLM. Ingesting them is enormously cheaper and more accurate than computing alignments. The algorithmic problem remains only where no curated mapping exists — FIBO↔DBpedia, or a customer's private ontology↔anything — and there the honest first move is **assisted alignment with human confirmation**, not automatic merging. `00c-domain-model.md`'s confidence bands already give the vocabulary for that: propose below the assert threshold, never auto-merge.

### 2.6 NER and NED: the port exists, the identifiers do not

`21-document-ingestion.md` has a `ClaimExtractor` port and `17-entity-resolution.md` handles linking; `00j-language-boundaries.md` correctly puts the ML out of process in Python. **That boundary is right and should not move.** What is missing is narrower: biomedical NED normalizes a mention to a **CUI** or a SNOMED concept id, and nothing in the current model gives those a first-class home. A CUI stored as an untyped string in a custom property is a CUI nobody can join on.

---

## 3. What has to change, in dependency order

| # | Change | Epic | Currently |
|---|---|---|---|
| 1 | **Profile detection before reasoning** — never run RL over an EL ontology and call the result complete | 100 | Not started, "prerequisite for 98/99" ✅ correct |
| 2 | **OWL 2 EL reasoner** — adopt, do not build (`00l`); `whelk-rs` is BSD-3 and already named | 98 | "Scheduled, on the medical-ontology requirement" — **trigger fired** |
| 3 | **Incremental reasoning maintenance** — wholesale replacement stops being viable | 97 | "Unscheduled until measurement demands it" — **measurement demands it** |
| 4 | **OWL 2 QL query rewriting** for DBpedia-shaped ABoxes | 99 | Scheduled |
| 5 | **UMLS/RRF ingestion with CUI as a first-class identity anchor** | 15 + **new** | **Nothing** |
| 6 | **Ontology alignment (TBox), curated-first** | **new** | **Nothing** |
| 7 | **Partitioning + write-path measurement at 10M+** | 4, 37a | Trigger stated, measurement pending |
| 8 | **Reasoning budgets re-derived for the new scale** | 6 | Calibrated for 1M flakes |

Items 5 and 6 have no epic. Everything else exists and is correctly sequenced — **the plans anticipated this requirement better than they anticipated its scale.**

---

## 4. End to end: one clinical question, all the way through

The requirement is easiest to judge against a concrete path. This is **requirement (B)**, and it is written as the target state — not as what runs today.

**The question**: *"Which of our data assets contain information about patients treated for a bacterial pneumonia, and which regulation governs them?"*

Nothing in that sentence matches a string in the estate. `Bacterial pneumonia` is not a column name; the tables say `dx_cd`, `rx_ndc`, `pt_id`.

### Step 0 — Load the ontologies (batch, out of the request path)

```
SNOMED CT  ──▶ horned-owl parse ──▶ profile detect (Epic 100) ──▶ "EL"
                                                                   │
UMLS RRF   ──▶ RRF connector (Epic 15) ──▶ CUIs + cross-vocabulary alignments
                                                                   │
RxNorm     ──▶ ingest, aligned to SNOMED *via UMLS CUIs*, not computed
```

**Design point:** the SNOMED↔RxNorm alignment is *read from UMLS*, not derived. Deriving it would be reimplementing NLM curation. This is item 6's "curated-first" principle doing the work.

### Step 1 — Classify, once (Epic 98, EL)

`whelk-rs` classifies SNOMED. This is where an RL reasoner would silently fail:

```
Asserted:  BacterialPneumonia ⊑ Pneumonia ⊓ ∃hasCausativeAgent.Bacterium
           StreptococcalPneumonia ⊑ Pneumonia ⊓ ∃hasCausativeAgent.Streptococcus
           Streptococcus ⊑ Bacterium

Inferred:  StreptococcalPneumonia ⊑ BacterialPneumonia      ← EL derives this
                                                              RL cannot
```

That single inference is the query's entire value. A user asking for bacterial pneumonia **must** get streptococcal pneumonia patients, and an RL-only engine returns them nothing while reporting success.

Materialised into `graph:reasoning` with provenance (rule + premises), maintained **incrementally** thereafter (Epic 97) — because reclassifying SNOMED on every write is a non-starter.

### Step 2 — Link the estate to the ontology (Epics 21, 17)

An out-of-process NER/NED service (Python, per `00j`) reads column descriptions, sample values and glossary terms, and emits **claims**:

```
column dx_cd  ──▶ mention "pneumonia dx code" ──▶ CUI C0032285 (Pneumonia)  conf 0.91
column rx_ndc ──▶ mention "antibiotic NDC"    ──▶ RxNorm 723 (Amoxicillin)  conf 0.78
```

Confidence bands from `00c` apply: **0.91 asserts, 0.78 surfaces for review, nothing auto-merges.** The 0.78 lands in Epic 42's extraction review queue, where a human confirms or rejects it. This is the difference between a catalog people trust and one they learn to ignore.

### Step 3 — Ask the question (Epic 7, SPARQL)

```sparql
SELECT ?asset ?regulation WHERE {
  ?asset  dsc:meaning     ?concept .
  ?concept rdfs:subClassOf* sct:BacterialPneumonia .   # ← needs Step 1's inference
  ?asset  dsc:inDomain    ?domain .
  ?domain dsc:governedBy  ?regulation .
}
```

What has to be true for this to work:

1. `rdfs:subClassOf*` is a **property path**, evaluated by `graph-owl-traversal` (Epic 7a decision 2a) — bounded and budgeted, not an unbounded walk of SNOMED.
2. The transitive closure comes from **Step 1's materialised EL inference**, not from computing it per query.
3. **Authorization is compiled into the query** (Epic 13), so a principal who cannot see the clinical schema gets no rows — not filtered rows, no rows, with counts and facets consistent.
4. The result carries **freshness** (Epic 4 decision 8) and, per Epic 100, **which reasoner produced the inference** — so the caller can weigh a conclusion drawn under EL differently from one drawn under RL approximation.

### Step 4 — Explain it (Epic 6)

```
GET /reasoning/explain?fact=<pt_records subClassOf BacterialPneumonia>

  StreptococcalPneumonia ⊑ BacterialPneumonia
    ├── rule: EL existential subsumption
    ├── premise: StreptococcalPneumonia ⊑ ∃hasCausativeAgent.Streptococcus  [SNOMED, asserted]
    ├── premise: Streptococcus ⊑ Bacterium                                  [SNOMED, asserted]
    └── reasoner: whelk-rs (OWL 2 EL), classified 2026-07-20T09:14:00Z
```

**Every step above is an existing epic except the UMLS ingestion and the alignment store.** The architecture is right. What is wrong is the *calibration* — budgets, scale triggers, and which reasoners are considered optional.

---

## 5. What this document does not claim

- **It does not claim the fork has been taken.** Requirement (B) is stated here as a requirement, not as an adopted position. Adopting it is a `00a` change and belongs there.
- **It does not claim billions is reachable on the current stack unchanged.** Section 2.4 says the opposite.
- **It does not schedule anything.** `ROADMAP.md` sequences; this document supplies the reasoning it should sequence against.
- **It does not repeat the licensing rules.** `00i` binds here as everywhere: `horned-owl` is LGPL and `whelk-rs` is BSD-3 (`00l` has the current readings), and no reasoner is adopted without that check.
