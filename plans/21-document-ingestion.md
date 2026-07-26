# Plan: Document & Conversation Ingestion (Epic 21)

**Branch**: feat/document-ingestion
**Status**: Not started
**Depends on**: Epic 16 (ingestion), Epic 17 (mention resolution)
**Feeds**: Epic 31 (organizational memory)
**Crates**: `graph-owl-connectors` (DocumentParser + ClaimExtractor ports, optional feature-gated adapters) · `graph-owl-resolution` (mention resolution) · `graph-owl-core` (Claim, Provenance) — no new crates

## Goal

Get knowledge out of runbooks, notebooks, incident tickets, decision records, and chat threads — and link it to the assets it discusses.

## Why extraction quality is a correctness concern

This epic is the input path for organizational memory (Epic 31). A confidently-wrong extraction becomes a confidently-wrong memory that an agent later cites. Extraction confidence is therefore recorded on every claim, and below-threshold claims are surfaced for confirmation rather than asserted.

## Resolved decisions

1. **Ontology-constrained extraction, not open information extraction.** Free-form triple extraction produces a graph nothing can query. Extraction targets the Epic 1 entity and relationship model; anything that does not fit is discarded with a reason, not stored as an untyped triple.
2. **Everything lands in `graph:extraction`, never the default graph.** Epic 6's reasoning does not run over it by default. Unconfirmed machine output must not silently feed inference.
3. **Confidence bands from `00c-domain-model.md` apply**: ≥0.8 assert, 0.5–0.8 surface for confirmation, <0.5 discard.
4. **Parsers and extractors are ports with optional adapters.** No Python runtime is required for core operation — that would break the operational-simplicity budget (`00a-product-position.md`).
5. **The source document is retained and linked.** A claim without its source is unverifiable. `capturedAs` links source → extracted memory.

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

- [ ] Markdown and plain text ingest with no optional adapter installed.
- [ ] PDF ingests when an adapter is configured; absent one, a clear "no parser configured" error.
- [ ] Extraction is schema-bounded — off-ontology output is discarded with a reason, never stored untyped.
- [ ] Every claim carries provenance to document, block, and character range.
- [ ] Mentions resolve via Epic 17; unresolved mentions are recorded as unresolved, not dropped.
- [ ] Confidence bands are applied; the 0.5–0.8 band queues for confirmation.
- [ ] Everything lands in `graph:extraction`; a failed run is deletable wholesale.
- [ ] Re-ingesting the same document is idempotent.

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

## Explicitly deferred (with destination)

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
