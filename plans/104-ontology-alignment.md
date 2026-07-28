# Plan: Ontology Alignment & Curated Mappings (Epic 104)

**Branch**: feat/ontology-alignment
**Status**: Not started — **new, 28 July 2026**. Created because `00n-large-ontology-reality.md` §2.5 found it genuinely uncovered
**Depends on**: Epic 33 (ontology packs — supplies the vocabularies), Epic 100 (profile detection), Epic 17 (entity resolution — the *instance* analogue this must not be confused with)
**Crates**: `graph-owl-ontology` · `graph-owl-connectors` (RRF reader) · `graph-owl-resolution` (shares the confidence machinery, not the algorithm)

## Goal

Answer *"is SNOMED's `Myocardial infarction` the same **class** as ICD-10's `I21`?"* — and record the answer with its provenance and confidence, so a query can cross vocabularies without a human translating first.

## Why this is a separate epic and not part of Epic 17 or 33

`00n` §2.5 records three problems that keep being called "reconciliation". Two are covered and one was not:

| Problem | Question | Epic |
|---|---|---|
| Instance resolution (ABox) | Are these two **records** the same thing? | 17 ✅ |
| Vocabulary annotation | What does this column **mean**, in FIBO terms? | 33 ✅ |
| **Ontology alignment (TBox)** | Is this **class** the same as that class? | **this one** |

Folding it into Epic 17 would be a category error with a practical cost: entity resolution's blocking, scoring and merge semantics are built for *individuals with attributes*, and classes have neither. Folding it into Epic 33 would let a pack — which supplies vocabulary — start asserting equivalences between vocabularies, which is the coupling `33-ontology-packs.md` explicitly prevents.

## Resolved decisions

1. **Curated before computed. Always.** UMLS's **CUI** is a cross-vocabulary alignment across SNOMED CT, RxNorm and 200+ source vocabularies, curated by the NLM over decades. Ingesting it is cheaper *and* more accurate than computing it. A system that ran a matching algorithm where a curated mapping already exists would be spending compute to produce a worse answer. **Computed alignment is the fallback, not the default.**

2. **An alignment is a first-class fact with provenance, not a merge.** Alignments are stored as flakes carrying `skos:exactMatch` / `skos:closeMatch` / `owl:equivalentClass`, each with its source (curated, computed, human) and confidence. **Nothing is merged.** Two classes that are aligned remain two classes; queries traverse the alignment. Merging would destroy the ability to answer "what did the source actually say", which is the same argument `00b` decision 14 makes for the reasoning overlay.

3. **`owl:equivalentClass` is not asserted from a computed match, ever.** It has logical force: a reasoner will draw conclusions from it, and a wrong one poisons the inference set with no obvious symptom. Computed matches assert `skos:closeMatch` at most. Promotion to `equivalentClass` requires either a curated source or human confirmation.

4. **Confidence bands from `00c` govern, and the thresholds are not new.** ≥0.8 asserts, 0.5–0.8 surfaces for review, <0.5 is not recorded. A computed alignment at 0.62 goes to a review queue; it does not quietly become a graph edge.

5. **A CUI is a first-class identifier, not a string in a custom property.** `00n` §2.6: a CUI stored untyped is a CUI nobody can join on. It gets a reserved namespace in the Epic 4 registry, exactly as other external identifier schemes do.

6. **Alignment is directional-aware even when the predicate is symmetric.** "SNOMED → ICD-10" mappings are frequently many-to-one and lossy in one direction; recording only a symmetric edge loses the fact that the reverse is an approximation. The source's own directionality is preserved.

## Implementation reference

```rust
pub struct Alignment {
    pub left: Sid,                 // e.g. SNOMED concept
    pub right: Sid,                // e.g. ICD-10 code
    pub predicate: MatchPredicate, // ExactMatch | CloseMatch | BroadMatch | NarrowMatch | EquivalentClass
    pub source: AlignmentSource,   // Curated { authority } | Computed { method } | Human { principal }
    pub confidence: f64,
    pub lossy_reverse: bool,       // decision 6
}
```

**The RRF reader is a connector module, not a serialization format.** UMLS ships pipe-delimited `MRCONSO` / `MRREL` / `MRSTY` files, not OWL — so it belongs in `graph-owl-connectors` beside every other external system, and **not** in `09-engine-rdf-io.md`, whose job is W3C serializations. Putting it there would make "RDF I/O" mean "any file we can read", which is how a crate boundary dissolves.

## Acceptance criteria

- [ ] UMLS RRF ingests: CUIs land as identities in a reserved namespace; `MRCONSO` atoms attach to their CUI; source-vocabulary codes (SNOMED, RxNorm) align to the CUI with `source = Curated`.
- [ ] A SNOMED concept and an RxNorm concept sharing a CUI are reachable from each other **without any computed matching having run**.
- [ ] A computed alignment never asserts `owl:equivalentClass` — asserted structurally, so the type system refuses it rather than a validation rule catching it.
- [ ] An alignment at 0.62 confidence appears in a review queue and **not** in query results that do not opt into unreviewed alignments.
- [ ] A human-confirmed alignment records **who** confirmed it and when; a later automated run does not overwrite it.
- [ ] A lossy reverse mapping is marked, and a query traversing it in the lossy direction can tell.
- [ ] Alignment ingestion is budgeted and resumable — UMLS is millions of rows and a failure at 80% must not mean starting again.
- [ ] **Console**: an alignment review queue *(Epic 42)*, and on any cross-vocabulary result the alignment that made it reachable is inspectable — a result that crossed an approximate match must be distinguishable from one that did not, and not by colour alone.

## Slices

### Slice A: The alignment fact and its store
**RED**: the `equivalentClass`-from-computed test — asserting the type system refuses it, not that a validator rejects it. Mutator watch: widening `MatchPredicate` to permit it must fail to compile or fail the test.

### Slice B: UMLS RRF ingestion, resumable
**RED**: interrupt at 80% and resume; the result must equal the uninterrupted run. Mutator watch: a resume that restarts from zero must fail a row-count-and-timing assertion.

### Slice C: Cross-vocabulary traversal
**RED**: SNOMED → RxNorm via CUI with no computed matcher present. Second RED: the lossy-direction test.

### Slice D: Computed alignment, confirmation, and the review queue
**RED**: a 0.62 match is invisible to default queries and present in review. Mutator watch: a threshold comparison flipped to `>=` on the wrong bound must fail a boundary test at exactly 0.5 and 0.8.

## Explicitly deferred (with destination)

- **Automatic ontology merging** → never. Decision 2; alignment is traversal, not merge.
- **Alignment algorithm research (embeddings, structural matching)** → out of process per `00j`. This epic stores and governs alignments; producing computed ones is a `ClaimExtractor`-shaped port, as in Epic 21.
- **OAEI benchmark conformance** → only if a customer requires a published matching quality figure.
