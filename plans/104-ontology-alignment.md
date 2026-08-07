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
- [x] A SNOMED concept and an RxNorm concept sharing a CUI are reachable from each other **without any computed matching having run**. (Slice C — found and fixed three real bugs to make this true: `scope_facts` had no notion of vocabulary content, `Alignment::subject()` produced an invalid IRI, and `scoped_facts`'s own flake dedup missed a real duplication class)
- [x] A computed alignment never asserts `owl:equivalentClass` — asserted structurally, so the type system refuses it rather than a validation rule catching it. (Slice A)
- [x] An alignment at 0.62 confidence appears in a review queue and **not** in query results that do not opt into unreviewed alignments. (Slice D)
- [x] A human-confirmed alignment records **who** confirmed it and when; a later automated run does not overwrite it. (Slice D — "when" is the flake's own transaction time `t`, not a separate timestamp field)
- [x] A lossy reverse mapping is marked, and a query traversing it in the lossy direction can tell. (Slice C)
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

### Slice C: Cross-vocabulary traversal — **shipped, 7 August 2026**
**RED**: SNOMED → RxNorm via CUI with no computed matcher present. Second RED: the lossy-direction test.

**No new query-layer code, by design** — decision 2's whole point is that
an alignment stored as `left {predicate} right` is an ordinary flake a
plain SPARQL query already traverses, unlike Epic 94's `rdf:reifies`
(synthesized, never stored). This slice is almost entirely the two RED
tests plus fixing what they found — and they found real bugs in three
different places, none of them in `graph_owl_ontology::alignment` itself:

**1. `scope_facts` had no notion of vocabulary content.** The very first
run of the traversal test returned zero rows. `scope_facts`'s asset
visibility check requires every flake's subject to be either a real
catalog asset (`list_assets_under_fqn`) or a `fromEntity`/`toEntity`
relationship endpoint — a CUI or a SNOMED/`RxNorm` code is neither, and
can never appear in `visible` no matter how permissive the policy is.
Fixed by adding `is_vocabulary_namespace` (true for `CUI`/`SNOMED_CT`/
`RXNORM`) as a third admission path, and extending the existing
`fromEntity`/`toEntity` endpoint-tracking loop to also recognise
`alignmentLeft`/`alignmentRight` — the alignment analogue, with an
endpoint counting as "permitted" when it is a visible asset *or* a
vocabulary identifier. Four new `scope_facts_tests` pin this, including
the negative: an alignment endpoint naming a real, hidden catalog asset
is still dropped — the carve-out is for vocabulary namespaces specifically,
not a blanket bypass for the predicate shape.

**2. `Alignment::subject()` produced an invalid IRI.** Once (1) was fixed,
a *different* test (querying the reified metadata directly) failed with
`namespace 1 has no IRI` — a genuinely confusing error for a namespace
(`DSC`) that plainly has one. The real cause: `subject()` built its local
name from `left.to_iri()`/`predicate.to_iri()`/`right.to_iri()` — each
already a real IRI carrying its own `#` — concatenated inside another
`dsc:`-namespaced IRI. Two-plus fragment delimiters is not valid IRI
syntax, and this surfaces only when something actually tries to convert
the subject to an RDF term, which no `alignment_to_flakes` unit test ever
does. Fixed by using `Sid`'s own compact `Display` (`namespace_code:id`)
instead of `to_iri()` for all three components — pinned both by a
regression unit test in `graph-owl-ontology`
(`the_reified_subject_is_itself_a_valid_iri`) and by the fact that the
end-to-end query now succeeds.

**3. `scoped_facts`'s own flake dedup missed a real duplication class.**
With (1) and (2) fixed, the traversal test *still* failed — not with zero
rows this time, but with the same correct row duplicated. Traced to
`scoped_facts`'s sort-then-`Vec::dedup()` step: the sort key was `(s.id,
p.id, t)`, omitting the object and both namespace codes. Two flakes
sharing a subject and predicate but differing only in object — exactly
what two *overlapping* pushdown scans on the same predicate produce, one
bound to a specific object and one unbound — sort as equal, so a third
flake with the identical key can land between two copies of the same
duplicate after a stable sort. `Vec::dedup` only removes *adjacent*
repeats, so the duplicate survived untouched, and the evaluator faithfully
emitted the surviving row twice. This is not alignment-specific — it is a
latent bug in the shared query path any two overlapping scans on one
predicate could have triggered — found here because this slice's own
traversal query is exactly `?cui p <bound> . ?cui p ?free`, the shape that
exposes it. Fixed by extracting the sort/dedup into its own function,
`dedup_flakes`, with a key that includes the object (rendered via `Debug`,
the same reason `graph-owl-reasoning`'s own dedup key does — `FlakeValue`
has no `Ord`, a `Float` NaN is not `Eq`) and both full `Sid`s rather than
their local names alone. Four new `dedup_flakes_tests` pin it directly,
including the exact non-adjacent shape that exposed the bug.

**Acceptance criteria, verified**:
`a_snomed_concept_reaches_its_rxnorm_counterpart_via_the_shared_cui`
constructs only `Curated` alignments (no `Computed` value appears
anywhere in the test) and reaches RxNorm from SNOMED through their shared
CUI via a real two-hop `Catalog::sparql()` join.
`a_lossy_reverse_alignment_is_distinguishable_by_a_query` reads
`lossyReverse` for two alignments straight off the reified metadata via
an ordinary SPARQL pattern (`?alignment dsc:alignmentLeft ?left ;
dsc:alignmentRight ?right ; dsc:lossyReverse ?lossy`), no query-layer
support needed.

### Slice D: Computed alignment, confirmation, and the review queue — **backend shipped, 7 August 2026**
**RED**: a 0.62 match is invisible to default queries and present in review. Mutator watch: a threshold comparison flipped to `>=` on the wrong bound must fail a boundary test at exactly 0.5 and 0.8.

**No new thresholds invented.** `graph_owl_core::extraction::Disposition`
already implements `00c-domain-model.md`'s exact generic confidence bands
(`ASSERT_THRESHOLD = 0.8`, `SURFACE_THRESHOLD = 0.5`, both boundaries
inclusive on their upper side, a `NaN` input treated as `Ignore` rather
than silently passing `>=`) — built for Epic 21's extraction claims and
reused directly rather than re-derived, matching decision 4's own framing
("the thresholds are not new"). Checked before reuse: `graph_owl_
resolution::bands::ConfidenceBands` looked like the obvious fit by name
but uses Epic 17's own `0.9`/`0.6` entity-resolution bands — a different,
wrong pair of numbers for this decision specifically.

**`alignment_to_flakes` split into `metadata_flakes` + `direct_triple`**,
composed by a new `alignment_to_flakes_gated`: `Disposition::Assert`
writes both (current behaviour, unchanged for Slice B's curated
ingestion — `AlignmentSource::Curated` is definitionally trustworthy per
decision 1, so nothing about it is gated against a numeric threshold),
`Disposition::Surface` writes only the metadata (so a review-queue
listing can find it, but the direct triple never "quietly becomes a
graph edge" — the plan's own words), `Disposition::Ignore` writes
nothing.

**`Catalog::upsert_alignment`** (`graph-owl-api`) is the stateful half:
retract-then-assert of `Alignment::subject()`'s reified node, mirroring
`run_reasoning`'s own withdraw-before-write pattern — necessary because a
later call updating this alignment's confidence must also retract the
*old* direct triple, whose subject is `left`, not the reified node, so a
confidence drop from `0.95` to `0.62` doesn't leave a stale graph edge
standing (pinned directly:
`upserting_a_lower_confidence_retracts_the_stale_direct_triple`). Before
writing, it checks whether the *existing* stored alignment at that
subject was human-confirmed (`alignmentSourceKind = "human"`, read from
what is actually stored, not re-derived) and refuses an automated
overwrite — `UpsertAlignmentOutcome::RefusedHumanConfirmed` — while
still allowing a second human call through (a person correcting or
re-affirming is not "an automated run").

**`Catalog::pending_alignment_review`** returns the raw `confidence`
flakes in the `Surface` band — the backend surface a review-queue UI
would read (every other field of a pending alignment is reachable from
its own subject via an ordinary query), not the UI itself.

**Honestly deferred, not silently dropped**: the **console** half
(review queue UI, and making the alignment behind a cross-vocabulary
result inspectable) is Epic 41/42 territory and is not attempted here —
matching this epic's own acceptance criterion wording, which names those
epics explicitly. `pending_alignment_review`'s raw-flake return type is a
deliberately minimal backend contract, not a finished API — a richer DTO
is exactly the kind of thing a real console consumer should drive the
shape of, not a guess made without one.

**Mutation testing on the `graph-owl-api` diff (`upsert_alignment` +
`pending_alignment_review`) first surfaced 6 real MISSED mutants**, all
"delete a field from a `TriplePattern`/`Flake` expression" — a class this
project's own `RecordingGraph` double calls out by name
(`queried: Mutex<Vec<TriplePattern>>` — "some obligations narrow the
scan...and are unobservable from the result alone"), and the same reason
the double exists as it does. On a fresh, empty `RecordingGraph` dropping
an `s`/`p`/`o` narrowing constraint returns the *same rows* — nothing else
is present to wrongly sweep in — so the gap was invisible to every
result-shaped assertion already written; only the *pattern actually sent*
distinguishes a narrowed query from an unnarrowed one that got lucky.
Fixed with 4 new tests reading `RecordingGraph::patterns()` and
`::retracted_flakes()` directly (the idiom this crate's own
`shapes_and_estate_are_read_from_different_graphs` test already
established): `upsert_alignment_narrows_both_lookups_to_their_own_identity`
pins both the metadata query's `s` and the direct-triple query's full
`(s, p, o)`; `withdrawing_a_stale_alignment_uses_a_time_after_the_original_
assertion` pins that a retraction's `t` is strictly later than the flake it
supersedes, not reused from `f.clone()` (which would make the withdrawal
and the assertion it withdraws simultaneous — ambiguous under
`RecordingGraph`'s own documented tie-break, and under real Postgres);
`pending_alignment_review_narrows_its_query_to_the_confidence_predicate`
pins its `p` filter, using `.patterns()` rather than a crafted Float-valued
decoy under a different predicate. Re-run: 16 caught, 2 unviable, 0
missed.

**Acceptance criteria, verified**: `a_review_band_alignment_is_absent_
from_a_plain_query_but_present_in_review` and its mirror-case sibling for
a confident alignment; `a_human_confirmed_alignment_is_not_overwritten_
by_a_later_automated_run` and `a_second_human_confirmation_is_written_
not_refused` (the boundary the refusal is actually about — automation
overriding a person, not a second write ever being possible).

## Explicitly deferred (with destination)

- **Automatic ontology merging** → never. Decision 2; alignment is traversal, not merge.
- **Alignment algorithm research (embeddings, structural matching)** → out of process per `00j`. This epic stores and governs alignments; producing computed ones is a `ClaimExtractor`-shaped port, as in Epic 21.
- **OAEI benchmark conformance** → only if a customer requires a published matching quality figure.
