# Plan: Document & Conversation Ingestion (Epic 21)

**Branch**: feat/document-ingestion
**Status**: In progress — the Rust domain, ports and the markdown/text adapter are built; every external worker (PDF, OCR, LLM, multimodal) remains out of scope by decision 0
**Depends on**: Epic 16 (ingestion), Epic 17 (mention resolution)
**Feeds**: Epic 31 (organizational memory)
**Crates**: `graph-owl-connectors` (DocumentParser + ClaimExtractor ports, optional feature-gated adapters) · `graph-owl-resolution` (mention resolution) · `graph-owl-core` (Claim, Provenance) — no new crates

## Goal

Get knowledge out of runbooks, notebooks, incident tickets, decision records, and chat threads — and link it to the assets it discusses.

## Why extraction quality is a correctness concern

This epic is the input path for organizational memory (Epic 31). A confidently-wrong extraction becomes a confidently-wrong memory that an agent later cites. Extraction confidence is therefore recorded on every claim, and below-threshold claims are surfaced for confirmation rather than asserted.

## Resolved decisions

1. **Ontology-constrained extraction, not open information extraction.** Free-form triple extraction produces a graph nothing can query. Extraction targets the Epic 1 entity and relationship model; anything that does not fit is discarded with a reason, not stored as an untyped triple.

0. **This epic runs out of process, in Python.** PDF layout analysis, OCR, chunking, and LLM-based extraction have a Python ecosystem Rust does not come close to matching, and none of it is on graph-owl's read path. The worker parses and extracts; extracted facts arrive through Epic 16's ingestion API and land in the `graph:extraction` named graph (`04-engine-triples.md` decision 5) so a bad run is deletable wholesale. See `00j-language-boundaries.md`.
2. **Everything lands in `graph:extraction`, never the default graph.** Epic 6's reasoning does not run over it by default. Unconfirmed machine output must not silently feed inference.
3. **Confidence bands from `00c-domain-model.md` apply**: ≥0.8 assert, 0.5–0.8 surface for confirmation, <0.5 discard.
4. **Parsers and extractors are ports with optional adapters.** No Python runtime is required for core operation — that would break the operational-simplicity budget (`00a-product-position.md`).
5. **The source document is retained and linked.** A claim without its source is unverifiable. `capturedAs` links source → extracted memory.

## What is built, and the constraint that shaped it

**Scope, decided deliberately:** the Rust domain (`Claim`, `Provenance`, `TextSpan`, `ParsedDocument`, `ExtractionResult`, `Disposition`), the two ports (`DocumentParser`, `ClaimExtractor`), the markdown/text adapter, the confidence bands, the confirmation queue, `graph:extraction`, and idempotent re-ingestion. **Every external worker — PDF, OCR, LLM, multimodal — is out of scope**, per decision 0.

**The binding requirement on that split: adding a worker later must not change the Rust domain model.** That ruled out three things which would otherwise be the natural Rust choice, and each exclusion is load-bearing rather than stylistic:

- **No enum naming the kind of extractor.** `ExtractorKind { Rules, Llm, Ocr }` would need a new variant for every worker anyone writes, and each variant is a breaking change to a type that has already been persisted. `Provenance` carries the extractor's *identity as data* (`extractor` + `extractor_version`), so adding a worker is a deployment, not a migration.
- **No Rust-specific document representation.** `ParsedDocument` is text plus spans — a shape a Python worker produces as readily as a Rust one. An AST would be rich enough for markdown and wrong for OCR, and only Rust could speak it.
- **No claim only in-process code could build.** Subject and predicate are strings, so a worker that has never heard of `AssetKind` can emit a claim and be told it was wrong.

Everything is `Serialize + Deserialize` with round-trip tests, because the boundary these types cross is a *process* boundary and a type that cannot survive JSON cannot cross it.

**The policy stays in graph-owl, not in the worker.** `Disposition::for_confidence` decides what a proposed confidence buys; `constrain` decides whether a predicate is in the vocabulary. Both run on every claim from every source — *including* the in-process extractor, which gets no exemption for being local. A worker proposes; graph-owl disposes. That is what stops a mis-tuned or compromised extractor from writing straight into the graph by asserting its own certainty, and it is only true because the check is on this side of the boundary.

**The rule-based extractor claims 0.6 on purpose.** A name matched in prose is evidence, not proof, so it lands in the *surface* band and waits for a human — an extractor claiming 0.9 for substring matching would be asserting into the graph on the strength of a string match. A test asserts it can never reach `Assert`.

**A bug the gate caught that no design review would have: FQNs contain periods.** The sentence splitter ended a sentence at every `.`, which tore `svc.db.orders` into `The svc.`, `db.` and `orders table is append-only.` — so no sentence ever contained the subject and the extractor silently found *nothing at all*. A period now ends a sentence only when whitespace or the end of the text follows it. Worth recording because the failure was total and silent: the extractor returned an empty result rather than a wrong one, which reads identically to "this document mentions nothing".

## The out-of-process half, and what it proved about the ports

The Rust half was written first, deliberately shaped so that adding a worker
later would need no change to it. Writing the worker is the only way to find out
whether that was true. **It was, with one exception, and the exception was in
the transport rather than the domain**: `GraphOwlClient._send` narrowed every
response to a dict, which turns the review queue's JSON array into `{}` — and an
empty queue looks exactly like "nothing is waiting for you". The fix was a
public `request()` returning the body as it is; no domain type changed.

Three things the split forced into the open:

- **Byte offsets are a cross-language contract, not a detail.** Python indexes
  strings by character and Rust by byte. A worker computing spans character-wise
  would point at the wrong words in any document containing an accent — silently,
  and only in the documents most likely to be interesting. Both sides now assert
  it, and the Python `TextSpan.resolve` returns `None` on a span that splits a
  multi-byte character rather than raising.
- **The fingerprint has to be pinned to a literal, because neither side can
  assert the other agrees.** If Python hashed the wire JSON or used a different
  encoding, every re-submission would look like a new document, idempotence
  would never fire, and an OCR pass would re-run over an unchanged corpus
  forever *while reporting success*. Both suites now assert the same two hex
  strings, one of them non-ASCII, because that is the case where a plausible
  alternative still passes the ASCII one.
- **The sentence-splitter bug reappeared in the second language.** FQNs contain
  periods; splitting on every period tears `svc.db.orders` into three fragments
  and the extractor finds nothing at all. The Python implementation has the same
  rule and a test whose name says why.

**The policy did not move, and that is the result worth keeping.** The worker
proposes a confidence, a predicate and a subject; graph-owl decides what each
one buys, for every claim from every source, including its own in-process
extractor. A worker cannot opt out because the checks are not in it.

## Implementation reference

```rust
pub trait DocumentParser: Send + Sync {
    async fn parse(&self, input: DocumentInput) -> Result<ParsedDocument, ParserError>;
}

pub struct ParsedDocument {
    pub text: String,                    // markdown
    pub blocks: Vec<Block>,              // heading/paragraph/table/code, with positions
    pub metadata: DocumentMetadata,      // title, author, created, source URL
}

pub trait ClaimExtractor: Send + Sync {
    /// Ontology-constrained: `schema` bounds what may be produced.
    async fn extract(&self, doc: &ParsedDocument, schema: &ExtractionSchema)
        -> Result<Vec<Claim>, ExtractError>;
}

pub struct Claim {
    pub kind: ClaimKind,                 // EntityMention|Relationship|Decision|Assumption|Incident
    pub mentions: Vec<TextMention>,      // resolved via Epic 17
    pub content: String,
    pub confidence: f64,
    pub provenance: Provenance,          // document id + block index + char range
}
```

### Parser adapters

Two, both optional and behind the port. A cloud OCR service (accuracy leader, no data locality) and a local CLI subprocess adapter (free, self-hosted, lower accuracy). Selected at the composition root; neither is compiled in by default. Plain-text and markdown inputs need no adapter at all — that path always works.

### Pipeline

```
DocumentInput → parse → ParsedDocument → extract (ontology-constrained) → Vec<Claim>
  → resolve mentions (Epic 17) → confidence band → assert | surface | discard
  → link source via capturedAs → Memory draft (Epic 31)
```

Each stage is separately testable; only parse and extract touch external services.

## Acceptance criteria

- [x] Markdown and plain text ingest with no optional adapter installed.
- [x] PDF ingests when an adapter is configured; absent one, a clear "no parser configured" error.
- [x] Extraction is schema-bounded — off-ontology output is discarded with a reason, never stored untyped.
- [x] Every claim carries provenance to document, block, and character range.
- [x] Mentions resolve via Epic 17; unresolved mentions are recorded as unresolved, not dropped. **Both endpoints, through one scorer.** `Catalog::best_mention_candidate` is now the single candidate-scoring path in the system, shared with `POST /memories/{id}/mentions` — extraction has no identity logic of its own, which is what stops the two disagreeing about what "the orders table" refers to. An exact fully-qualified name short-circuits it (a worker that emits one is stating an identity, not guessing); everything else is scored and held to Epic 17's threshold, and a scored resolution is written to `mention_resolutions` against the run that caused it. A claim's *object* goes through the same path when the predicate is reference-shaped, because half an edge is not an edge.
- [x] Confidence bands are applied; the 0.5–0.8 band queues for confirmation.
- [x] Everything lands in `graph:extraction`; a failed run is deletable wholesale. **Deletable wholesale** is `ON DELETE CASCADE` from `extraction_runs` — a schema guarantee rather than something a method must remember. **The named-graph projection now happens**: an asserted claim is written as a flake with `cx = Sid::dsc("graph:extraction")`, and confirming a surfaced claim writes exactly the same flake. Rejecting writes nothing and has nothing to retract, because a pending claim was never in the graph.
- [x] Re-ingesting the same document is idempotent.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first.

### Slice A: Parse markdown and plain text

**Acceptance criteria**: `DocumentParser` port with a built-in markdown/text adapter; blocks carry type and character positions; headings establish a hierarchy usable for context; a code block is marked and not extracted from as prose; document metadata captured; a 10MB document parses without unbounded memory.
**RED**: Position-accuracy test asserting a known phrase's character range is exact — provenance depends on it. A code-block test asserting prose extraction skips it. Mutator watch: off-by-one positions must fail the range assertion; treating code as prose must fail.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice B: Ontology-constrained extraction

**Acceptance criteria**: `ExtractionSchema` derived from the Epic 1 entity and relationship model; extractor output validated against it; a claim naming an unknown entity type is discarded with a reason recorded; a relationship whose triple is illegal (Epic 1's validation table) is discarded; extraction is deterministic for a fixed input and model version; the schema is versioned so re-extraction under a new schema is distinguishable.
**RED**: A fixture document plus a golden claim set. An off-ontology test: an extractor producing `Table isFriendsWith Table` must be discarded with a reason, not stored. Mutator watch: unconstrained extraction must fail the off-ontology test — this is decision 1's guarantee, and violating it produces an unqueryable graph.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice C: Confidence bands and confirmation queue

**Acceptance criteria**: ≥0.8 asserts into `graph:extraction`; 0.5–0.8 queues with the claim, provenance, and source excerpt shown; <0.5 discarded and counted; confirmation promotes the claim and sets confidence 1.0; rejection records the decision so re-ingestion does not re-queue it; band boundaries tested at exactly 0.8 and 0.5.
**RED**: Boundary tests at exactly 0.8 and 0.5. A rejection-persistence test: reject a claim, re-ingest the document, assert it is not re-queued. Mutator watch: `>` for `>=` must fail the boundary; a rejection that only clears the queue must fail the persistence test.
**Done when**: criteria met, mutation report reviewed, commit approved.

**Extended, 6 August 2026 — a real "source excerpt shown" and a third outcome, driven by Epic 42's review-queue pattern.** The excerpt this slice originally shipped was the matched phrase alone (e.g. "orders service"), not a sentence around it — usable for confirm/reject but not for judging the extraction *in context*, which Epic 42 decision 2 requires ("source passage with the extracted span highlighted"). `graph_owl_api::extraction::windowed_passage` widens the stored byte span to the sentence it sits in (scanning outward to the nearest `.`/`!`/`?`/`\n`, capped at 400 chars either direction so an unpunctuated document — a transcript, a log dump — cannot return itself whole); `PendingClaim` now carries `passage: String` + `span: (usize, usize)` (byte offsets into `passage`) instead of a bare `evidence: String`. 100% mutation score on the windowing logic (69/69 on the first pass after two real gaps were found and fixed by strengthening assertions to exact-value rather than substring/length checks — see the function's own doc comment for the specific mutants that survived weak assertions and why).

A third decision joined confirm/reject: **Edit**, matching Epic 42's Accept/Edit/Reject pattern. `graph_owl_core::extraction::ReviewDecision` (`Accept | Edit { subject, predicate, object } | Reject { reason }`) replaces the old `confirmed: bool` throughout the stack — storage trait, Postgres (`COALESCE` in the `UPDATE ... RETURNING`, so Accept leaves the extractor's own values and Edit overwrites all three atomically), the in-memory fake, the facade, and `POST /extraction/claims/{id}/decision`'s wire shape (`{"outcome": "accept" | "edit" | "reject", ...}`, `#[serde(tag = "outcome")]`). **Reject now requires a `reason`** (`400` on missing or empty, matching Epic 17's identical rule for merge rejection) — a rejected claim was previously recorded with no explanation at all. `extraction_claims` gained a nullable `reason` column (`V50`), the same shape as `resolution_queue`'s (`V49`, Epic 17).

Verified: `cargo check --workspace` clean; 34 facade unit tests, 24 server HTTP tests (4 new/modified: editing projects the correction not the original, rejecting without a reason is refused at both the single and bulk endpoints), all green; clippy clean on every touched crate; 0 missed mutants on both the new windowing logic and the new HTTP validation branches.

### Slice D: Provenance and source linking

**Acceptance criteria**: every asserted claim links to its source document via `capturedAs`; provenance resolves to the exact excerpt, verified by re-reading the document at the recorded range; the source document is retained as an entity with its own envelope; deleting the source flags dependent claims as unverifiable rather than deleting them; an excerpt is returned with each claim for review.
**RED**: A round-trip test: assert a claim, then use its provenance to re-extract the excerpt from the stored document and compare. This catches position drift, which would make every claim unverifiable. Mutator watch: storing only a document reference without a range must fail the excerpt round-trip.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice E: Optional PDF adapter

**Acceptance criteria**: an adapter behind the port, selected at the composition root, not compiled in by default; absent configuration, PDF input returns a clear "no parser configured for application/pdf" error rather than a generic failure; the adapter's output goes through the identical downstream pipeline; adapter failure (service down, subprocess crash) is a typed error, not a panic; a timeout bounds the call; **the core workspace builds and all non-PDF tests pass with the adapter feature disabled**.
**RED**: A build test with the feature off asserting the workspace compiles and text ingestion still works — decision 4's guarantee, and the thing that keeps the dependency footprint honest. A timeout test. Mutator watch: a hard dependency on the adapter must fail the feature-off build.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice F: Idempotent re-ingestion

**Acceptance criteria**: re-ingesting an unchanged document produces no new claims and no new versions; a changed document supersedes prior claims from it rather than duplicating; document identity is content-hash plus source URL, not filename; confirmed claims survive re-ingestion of their source; the run is scoped to `graph:import:{document}` so it is deletable wholesale.
**RED**: The confirmed-claim survival test: confirm a claim, re-ingest the document, assert the confirmation is not reset. Human curation must not be destroyed by re-processing — the same rule as Epic 15's hand-edit preservation. Mutator watch: wholesale replacement must fail it.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice G: Resolution, projection, and the boundary that proves both

**Value**: the epic's two open criteria closed, and decision 2 demonstrated
rather than assumed.

**What it did**:

- **One identity path.** `Catalog::best_mention_candidate` is now the only
  candidate-scoring code in the system; extraction calls it, and so does Epic
  17's mention endpoint. An exact FQN short-circuits it — a worker emitting one
  is stating an identity, not guessing — and everything else clears Epic 17's
  threshold or resolves to nothing.
- **Both endpoints of a claim.** A reference-shaped predicate (`feeds`,
  `derivedFrom`, `dependsOn`, `owner`, `term`) has its *object* resolved by the
  same path. An unresolvable object is discarded with a reason, by the same rule
  `constrain` already applied to the subject: half an edge is not an edge, and a
  lineage fact reachable from neither end is worse than no fact.
- **Projection into `graph:extraction`.** An asserted claim becomes a flake with
  `cx = Sid::dsc("graph:extraction")`; confirming a surfaced claim writes exactly
  the same flake; rejecting writes nothing. Literal-shaped predicates project a
  string, reference-shaped ones a `Ref` — a string in a ref position stores
  cleanly and is unreachable by reverse traversal, which is how a lineage edge
  disappears with no error anywhere.
- **`Catalog::reasoning_base`.** `derive_within` already filtered its input on
  `include_graphs`, but filtering a base that never contained the named graph is
  a no-op: `graph:extraction` was invisible to reasoning whether or not a
  deployment asked for it. That looked exactly like the containment rule working
  and was really the facts never arriving — safe by accident, and in a way that
  also made the opt-in impossible. Loading the included graphs is what makes
  both directions real.

**The magic numbers, and where they come from**: `MENTION_CANDIDATES = 50` is
one page of name-search hits, so resolving a mention costs a constant rather
than a scan — and a candidate outside the search engine's first page was never
going to clear the threshold on name similarity anyway. Nothing else new is a
number; the bands and the threshold are Epic 17's and Epic 21's existing ones.

**RED**: the two-directional boundary test, which is the point of the slice —
a surfaced claim reaches no conclusion *even for a deployment that opted in*
(it is absent, not filtered), and an asserted one reaches a conclusion *only*
for a deployment that opted in. Plus the confirmation transition and its
negative: rejecting puts nothing in the graph. Mutator watch: a projection that
ran on every decision rather than on a confirmation must fail the rejection
test; a `cx` of `None` must fail the containment test.

**A standing invariant, tested**: every predicate in `CATALOG_PREDICATES` must
exist in the engine's `predicates` table. Nothing in the type system connects
the two lists, and a predicate in the first and not the second passes every
policy check and then fails at `assert_flakes` — where the only symptom is a
logged line and a fact that is not there. `term` and `dependsOn` were exactly
that, and `V7__extraction_predicates.sql` fixes it.

**Known-loose, deliberately**: `owner` and `term` are reference-shaped, and
their objects are users and glossary terms — neither of which the *asset*
resolver can see. So an extracted `owner` claim resolves to nothing and is
discarded with a reason. That is honest rather than silent, and it is the same
outcome as before this slice (the claim reached no graph either way), but it
means owner and term extraction needs a resolver per namespace before it does
anything. Destination: a slice of its own, once a worker actually emits them —
today both extractors emit only `description`.

## Explicitly deferred (with destination)

- **Owner and glossary-term resolution** → both are reference-shaped predicates
  whose objects live outside the asset tree; each needs its own resolver. See
  Slice G's "known-loose".
- **Conversation platform connectors** (chat, email) → a source-specific adapter each; the pipeline is shared. Add per platform on demand.
- **Coreference resolution within a document** → Epic 17 resolves mentions to entities; mention-to-mention chaining is a further step.
- **Multilingual extraction** → single-language assumed.
- **Table extraction from documents into entities** → the parser marks tables; turning a document table into catalog entities is a separate, riskier capability.
- **Layout-model-based parsing** (vision models) → the port allows it; no adapter planned until accuracy on real documents justifies the dependency.

## Pre-PR quality gate

1. `cargo mutants` — 0 missed. 2. Refactoring assessment. 3. `cargo test/clippy/fmt`.
4. **Workspace builds and passes with every optional parser feature disabled** (Slice E).
5. Provenance excerpt round-trip verified (Slice D).
6. Off-ontology discard verified (Slice B) — this is what keeps the graph queryable.
