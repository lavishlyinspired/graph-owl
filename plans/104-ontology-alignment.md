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

- [x] UMLS RRF ingests: CUIs land as identities in a reserved namespace; `MRCONSO` atoms attach to their CUI; source-vocabulary codes (SNOMED, RxNorm) align to the CUI with `source = Curated`. (Slice B)
- [ ] A SNOMED concept and an RxNorm concept sharing a CUI are reachable from each other **without any computed matching having run**.
- [x] A computed alignment never asserts `owl:equivalentClass` — asserted structurally, so the type system refuses it rather than a validation rule catching it. (Slice A)
- [ ] An alignment at 0.62 confidence appears in a review queue and **not** in query results that do not opt into unreviewed alignments.
- [ ] A human-confirmed alignment records **who** confirmed it and when; a later automated run does not overwrite it.
- [ ] A lossy reverse mapping is marked, and a query traversing it in the lossy direction can tell.
- [x] Alignment ingestion is budgeted and resumable — UMLS is millions of rows and a failure at 80% must not mean starting again. (Slice B — resumable by construction; "budgeted" in the sense of bounded per-call cost is inherited from the caller owning batch size, not yet wired to a job/error-cap harness like Epic 16's `graph-owl-connectors::job`)
- [ ] **Console**: an alignment review queue *(Epic 42)*, and on any cross-vocabulary result the alignment that made it reachable is inspectable — a result that crossed an approximate match must be distinguishable from one that did not, and not by colour alone.

## Slices

### Slice A: The alignment fact and its store — **shipped, 7 August 2026**
**RED**: the `equivalentClass`-from-computed test — asserting the type system refuses it, not that a validator rejects it. Mutator watch: widening `MatchPredicate` to permit it must fail to compile or fail the test.

**Shipped.** `graph_owl_ontology::alignment` gained `MatchPredicate`
(`ExactMatch`/`CloseMatch`/`BroadMatch`/`NarrowMatch` — no `EquivalentClass`
variant at all), `AlignmentSource` (`Curated`/`Computed`/`Human`), a
*narrower* `AssertableSource` (`Curated`/`Human` — no `Computed`), and
`Alignment` as an enum of `Match { predicate: MatchPredicate, source:
AlignmentSource, .. }` / `EquivalentClass { source: AssertableSource, .. }`.
The refusal is structural exactly as the RED test demands: there is no
value of `AssertableSource` that represents "computed", so
`Alignment::EquivalentClass { source: AssertableSource::Computed { .. },
.. }` does not typecheck — pinned by a `compile_fail` doctest on
`Alignment` itself (the actual mechanical proof; a `#[test]` cannot assert
a compile failure) plus a positive `#[test]` proving `Curated`/`Human`
*do* construct one, so the doctest is shown to be testing the refusal and
not a typo.

**Real namespace verification, not assumed IRIs.** `graph_owl_core::flake
::namespace` gained `SKOS`, `CUI`, `SNOMED_CT`, and `RXNORM` — each
checked against a live source before being hardcoded: the W3C SKOS
Reference §10 for the four mapping properties (confirming `exactMatch` is
a sub-property of `closeMatch`, not a sibling — irrelevant to storage but
wrong to assert blind), NLM's own UMLS concept browser
(`https://uts.nlm.nih.gov/uts/umls/concept/{CUI}`, fetched live and
confirmed to resolve) as the CUI namespace since NLM is the issuing
authority, SNOMED International's own URI standard
(`http://snomed.info/id/{SCTID}`, and specifically *not*
`.../sct/{SCTID}`, which names a whole edition), and NLM's `RxNav` REST
resolver for `RxNorm`. `CUI`/`SNOMED_CT`/`RXNORM` sit in the
"vocabularies this project introduces later" range (512+), not beside
RDF/OWL, since none has a W3C spec.

**Flake shape**: `alignment_to_flakes` writes the *direct* semantic triple
(`left {predicate} right`, e.g. `left skos:exactMatch right` — an ordinary
flake a plain SPARQL query traverses with no special handling) plus a
reified metadata node (`Alignment::subject()`) carrying source, confidence
and `lossyReverse`. The reified subject is **deterministic** — derived
from `(left, predicate, right)` alone, never the source — which is what
makes re-ingesting an identical row idempotent (Slice B's resumability
needs this) and lets a later curated ingestion find and supersede an
earlier computed guess of the *same* alignment (decision 1), rather than
the two coexisting as unrelated facts. Pinned directly:
`the_same_left_predicate_right_always_names_the_same_subject_regardless_of_source`
and `ingesting_the_same_alignment_twice_produces_identical_flakes`.

**Confidence-band gating (decision 4) is explicitly not this slice's
concern** — `alignment_to_flakes` always emits both the direct triple and
the metadata node; Slice D decides whether to call it at all for a
sub-0.8 computed match.

Mutation report: `flake.rs`'s diff — 8/8 viable mutants caught, 1
unviable. `alignment.rs` — 7/7 viable mutants caught, 11 unviable (all
`Default::default()` substitutions against types with no `Default` impl —
a compile failure, not a coverage gap).

### Slice B: UMLS RRF ingestion, resumable — **shipped, 7 August 2026**
**RED**: interrupt at 80% and resume; the result must equal the uninterrupted run. Mutator watch: a resume that restarts from zero must fail a row-count-and-timing assertion.

**Shipped.** `graph_owl_connectors::umls` gained `parse_mrconso_line`
(verified against the UMLS Reference Manual, NCBI Bookshelf `NBK9685`, 7
August 2026: 18 pipe-delimited fields, no header, a trailing `|` row
terminator — a hand-rolled split rather than reconfiguring the `csv`
crate's delimiter, since RRF carries no quoting and CSV's escaping rules
would be a mismatch, not a convenience), `source_namespace` (`SAB` →
namespace code, verified real for exactly `SNOMEDCT_US` and `RXNORM` —
UMLS names ~190 source vocabularies and only these two have a namespace
this system has checked; every other `SAB` is an honest, counted skip,
never a guessed IRI), `atom_to_alignment` (builds Slice A's `Alignment`
directly, `source = Curated { authority: "UMLS" }`, confidence `1.0`),
and `ingest_mrconso` — the resumable driver.

**Resumable by construction, not by tracked cross-row state.** Every
MRCONSO row maps to its own self-contained alignment; nothing about one
row's projection depends on another's. So "skip `N` lines, continue" and
"process from the start" produce the identical union of flakes once every
row has been seen exactly once between the two calls — the function does
not know or care whether it is a first call or a resume, and the caller
owns persisting the skip count between calls (this function is pure, no
I/O, matching `graph-owl-reasoning`'s own pure-core / impure-shell split).

**The RED test proves the thing the plan's mutator note warns is easy to
fake.** Flake-set equality between "ingest 0..80 then resume 80..100" and
"ingest 0..100 uninterrupted" is *necessary* but not *sufficient* — a
buggy resume that silently restarted from row 0 would happen to pass it
too, since re-asserting an identical alignment's flakes a second time is
idempotent and invisible in the final set. `resuming_after_an_
interruption_at_80_percent_equals_an_uninterrupted_run` therefore also
asserts the **row count the resume call itself reports**:
`resume_progress.rows_processed == 20`, not `100` — the assertion a
zero-restart bug fails and a flake-only comparison would have missed
entirely.

**Real UMLS text, not invented.** The parser test fixture
(`REAL_MRCONSO_LINE`) is the UMLS Reference Manual's own published
example row, fetched live rather than typed from memory — and it
happens to name `SAB=MSH` (MeSH), which doubled as the fixture for
"an unsupported source vocabulary is skipped, not guessed" once the
verification confirmed MeSH has no checked namespace here.

Mutation report: first pass found 4 real gaps — `progress.errors`/
`progress.skipped`'s own increment operators were never exercised by any
test that put a malformed or unsupported row through `ingest_mrconso`
itself (only through `parse_mrconso_line`/`atom_to_alignment` directly).
Added `ingest_counts_skipped_and_errored_rows_separately_from_aligned_
ones`; re-run: 20/20 viable mutants caught, 3 unviable.

### Slice C: Cross-vocabulary traversal
**RED**: SNOMED → RxNorm via CUI with no computed matcher present. Second RED: the lossy-direction test.

### Slice D: Computed alignment, confirmation, and the review queue
**RED**: a 0.62 match is invisible to default queries and present in review. Mutator watch: a threshold comparison flipped to `>=` on the wrong bound must fail a boundary test at exactly 0.5 and 0.8.

## Explicitly deferred (with destination)

- **Automatic ontology merging** → never. Decision 2; alignment is traversal, not merge.
- **Alignment algorithm research (embeddings, structural matching)** → out of process per `00j`. This epic stores and governs alignments; producing computed ones is a `ClaimExtractor`-shaped port, as in Epic 21.
- **OAEI benchmark conformance** → only if a customer requires a published matching quality figure.
