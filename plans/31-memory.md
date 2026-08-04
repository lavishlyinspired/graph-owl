# Plan: Organizational Memory (Epic 31) ★

**Branch**: feat/memory
**Status**: **In progress** — Slices A, B, C, D and E have their domain core and
their persistence, 30 Jul 2026. `graph-owl-core::memory` / `::recall` /
`::contradiction`, `V15__memories.sql`, six `Storage` port methods with a
Postgres adapter and 15 integration tests, and the `Catalog` facade
(`create_memory`, `memory`, `recall`, `supersede_memory`,
`contradictions_about`, `dismiss_contradiction`). The HTTP surface is live —
`POST /memories`, `GET /memories/{id}`, `POST /memories/{id}/supersede`,
`GET /assets/{id}/memories`, `GET /assets/{id}/contradictions`,
`POST /contradictions/reviews` — with end-to-end tests and the contract
regenerated.

**The MCP `recall` tool shipped** (`graph-owl-mcp`, 30 Jul 2026), which is what
makes this epic reachable by an agent rather than only by the console. Epic 14
still has no *transport* — nothing speaks MCP over stdio or HTTP — so the tool is
declared, dispatched and tested but not yet served, exactly as `get_asset_context`
has been since Epic 14 Slice B.

Four flags travel with every recalled memory, and each exists to stop an agent
overstating what it read:

| Flag | What goes wrong without it |
|---|---|
| `humanAuthored` | An agent reads **its own earlier guess** back as institutional knowledge and compounds it, confidence growing each retrieval because it keeps finding "the catalog says so" |
| `staleness` | The same argument as `policyFiltered`: an agent that cannot tell current from stale presents stale as current |
| `contradicted` | The agent picks one of two conflicting memories and presents it as the answer — adjudicating institutional disagreement **by omission** |
| `confidence` | Facts with no weight attached get reported all alike |

Two distinctions the dispatcher keeps that `Outcome` did not previously cover:
`Recalled([])` is **not** `NotFound`, because "nothing has been written down" and
"there is no such table" are opposite statements and an agent that conflates them
fills the silence; and superseded memories are **excluded rather than flagged**,
because a withdrawn memory arriving as an unmarked peer of its own correction is
not stale but retracted, and no flag makes presenting both defensible.

**What is genuinely left**: Slice C's semantic term, and any console surface.

**Why the semantic term is not merely unfinished.** `graph-owl-search`'s own
module docs already record that embeddings are generated **out of process**
(`00j`), so the port that matters sits at that process boundary and does not
exist. Implementing the term today would mean either fabricating a lexical
similarity and labelling it semantic — which destroys the distinction
`Score.semantic: Option<f64>` exists to preserve, since a reader could no longer
tell "measured, not similar" from "never measured" — or inventing a seam with no
implementation on either side, which `00e`'s growth trigger and that module's docs
both refuse in writing. It waits for Epic 8.

**Closed 4 August 2026**: a request resolving to a *person* rather than
`Principal::system()`. The note above was written before Epic 12 shipped and
was never revisited once it did — `resolve_principal` already maps any
auto-provisioned, non-bot JWT subject to `PrincipalKind::User`, and
`create_memory`'s `authorship_of` already had the correct split; nothing in
the domain or HTTP layer needed to change. What was missing was proof: the
"a human defaults to confidence 1.0" rule was unit-tested only, never
exercised through a real JWT-authenticated request over
`POST /memories`. Two HTTP tests now do
(`crates/graph-owl-server/tests/memory.rs`,
`a_real_person_authors_a_memory_and_it_defaults_to_full_confidence` and
`a_real_persons_stated_confidence_overrides_the_human_default`). Mapping
`system` to human authorship to make the fixtures read nicely would still be
exactly the relabelling the trust model refuses — the fix was proving the
real path works, not changing what `system` means.

**Two schema decisions worth reading before extending this.**

- **Links keep referential integrity by splitting the target column.** A link
  points at either an asset or another memory. A single polymorphic `target UUID`
  could not be a foreign key, and `V13` already refused that trade for owners:
  losing referential integrity to save a column lets a deleted asset stay named
  as a subject. `MemoryLink` still carries one `Uuid` because the domain does not
  care which it is — the adapter resolves it, and that resolution is needed
  anyway to answer "a link to a nonexistent target → 400".
- **Reviews are one table with a verdict column, not a dismissals table beside a
  confirmations table.** Two tables would make "confirmed *and* dismissed"
  representable, and a reviewer changing their mind would be a delete from one
  plus an insert into the other with nothing making the pair atomic. One row per
  pair makes the contradictory state unrepresentable and a change of mind a plain
  `UPDATE`. This replaced a dismissals-only table that was already written and
  tested — caught while implementing *confirm*, which is when the wrong shape
  became visible.
- **`ON DELETE RESTRICT` on both supersession columns.** Deleting a memory
  somebody corrected would destroy the record of what people believed *before*
  they were corrected, which is most of the reason to keep a record. Refusing the
  delete is the honest failure. Deleting the *subject*, by contrast, cascades the
  link and keeps the memory — `Staleness::SubjectUnknown` exists precisely so
  that state can be reported rather than deleted.

**Slice E resolved "overlapping `as_of`" without a magic number.** The criterion
reads "two `Decision` memories about the same asset with overlapping `as_of`",
but `as_of` is an instant, so "overlapping" needs an interpretation — and the
obvious one ("within N days") would be a number with no derivation available,
which `00i` rule 4 forbids. Supersession says it exactly and needs no number: a
superseded decision is **history**, not a competing claim, and a decision that
supersedes another is a **correction**, which is the opposite of a contradiction.
So two decisions overlap precisely when both are current and neither corrects the
other.
**Depends on**: Epic 3 (envelope), Epic 11 (people), Epic 14 (MCP surface to serve it)
**Benefits from**: Epic 21 (extraction), Epic 30 (incidents)
**Crates**: `graph-owl-core` (Memory, MemoryLink, Authorship, **pure ranking function**) · `graph-owl-search` (embeddings for the semantic term) · `graph-owl-storage-postgres` · `graph-owl-api` · `graph-owl-mcp` (recall tool)

## Goal

Capture the knowledge that currently evaporates into chats, tickets, and notebooks — *why* a metric changed, *why* a pipeline failed, *why* a dashboard was deprecated — as first-class linked objects that humans and agents can reuse.

## Why this is the headline differentiator

Technical metadata says **what exists**. Memory says **why it is the way it is**.

**A correction to an earlier claim here**: this file previously said *"neither reference architecture models it"*. The catalog reference does not, but the engine reference ships a **~4,400-line memory crate** whose core type is `Memory { kind, scope, severity, … }` with `MemoryKind { Fact, Decision, Constraint }`, a recall path with reranking, and a vocabulary including `rationale` and `alternatives`. That is close enough to this plan's `MemoryKind { Decision, Incident, Assumption, … }` and `Authorship`/`confidence`/`supersedes` model to be worth stating plainly.

**What differs is the scope, and the difference is the whole point.** That implementation's `Scope` enum is `{ Repo, User }`, and its vocabulary carries `branch` and `artifactRef` — it is memory for coding agents working on one codebase. This epic's memory attaches to **enterprise metadata assets**: why *this table* was deprecated, what *this pipeline's* owner assumed, which alternatives were rejected for *this contract*.

So the honest positioning: the memory *model* is convergent and therefore probably right — two independent designs reaching `Fact`/`Decision` kinds with confidence and supersession is evidence, not coincidence. The *application* to a metadata estate is what nothing surveyed does.

It is also the only layer that **compounds**: every investigation recorded makes the next one cheaper. Connector breadth accumulates linearly; memory compounds. An agent that answers "why was this deprecated" from institutional knowledge offers something nothing else does.

## Resolved decisions

1. **Memory is an entity, not an annotation.** It has an envelope, a version, an owner, and a lifecycle. A comment field on a table cannot be searched, linked, superseded, or attributed.
2. **Three authorship classes, distinguished permanently**: `Human`, `Extracted` (Epic 21), `Agent` (Epic 32). Provenance and confidence differ by class, and a reader must be able to tell an agent's inference from a domain expert's statement.
3. **Memories supersede rather than update.** A corrected memory creates a new one linked by `supersedes`; the original stays readable. Overwriting institutional knowledge destroys the record of what people believed and when.
4. **Links are typed and multi-target.** One memory links to assets, people, incidents, policies, contracts, and domains simultaneously. A single `about` field forces the interesting cases into free text.
5. **Confidence is required, not optional.** Extracted and agent-written memories carry it; human-written memories are 1.0. Below 0.5 is not stored (`00c-domain-model.md`).
6. **Memory does not expire, but it can go stale.** A memory about a table that has since changed is flagged stale by comparing its `as_of` against the asset's current version — surfaced, never deleted.

## Implementation reference

```rust
pub struct Memory {
    pub envelope: EntityEnvelope,
    pub kind: MemoryKind,
    pub content: String,                 // markdown
    pub summary: String,                 // one line, for retrieval
    pub authorship: Authorship,
    pub confidence: f64,
    pub links: Vec<MemoryLink>,
    pub as_of: DateTime<Utc>,            // the state of the world it describes
    pub supersedes: Option<Uuid>,
    pub superseded_by: Option<Uuid>,
}

pub enum MemoryKind {
    Decision,          // "we renamed this column because..."
    Incident,          // investigation and remediation
    Assumption,        // "this analysis assumes orders are deduplicated"
    Explanation,       // domain-expert knowledge
    Deprecation,       // why, and what to use instead
    Investigation,     // an agent's findings
    Convention,        // "we always suffix staging tables with _stg"
}

pub enum Authorship {
    Human   { principal: EntityReference },
    Extracted { source: EntityReference, extractor: String },  // doc/conversation
    Agent   { principal: EntityReference, session: Option<String> },
}

pub struct MemoryLink {
    pub target: EntityReference,
    pub relation: MemoryRelation,
}

pub enum MemoryRelation {
    About,          // primary subject
    Mentions,       // referenced in passing
    CausedBy,       // this incident caused by that asset
    Resolves,       // this memory resolves that incident
    Contradicts,    // disagrees with another memory
    SupportedBy,    // evidence
}
```

### Graph projection

`capturedAs` links the source (document, conversation, incident) to the memory; `documents` / `mentionedIn` link the memory to assets. This is why those relations exist in the Epic 1 taxonomy.

### Retrieval ranking

Memory retrieval is not plain search — recency and authorship matter more than lexical match:

```
score = 0.4 · semantic_similarity      (Epic 8 embeddings over `summary` + `content`)
      + 0.2 · link_proximity           (direct About > Mentions > 2-hop via lineage)
      + 0.2 · recency_decay            (half-life 180 days, configurable)
      + 0.1 · authorship_weight        (Human 1.0, Agent 0.7, Extracted 0.5)
      + 0.1 · confirmation_count       (how many people marked it useful)
      − staleness_penalty              (asset changed since `as_of`)
```

Weights are configurable and the formula is a pure function — testable against a fixed corpus with expected orderings, which is how Epic 8 Slice D tests relevance.

### Contradiction detection

Two memories linked `Contradicts`, or two `Decision` memories about the same asset with overlapping `as_of` and opposing content, are surfaced for review. Detection is deliberately shallow — flag the pair, let a human adjudicate. Automatic resolution of contradictory institutional knowledge is not a thing software should attempt.

## Acceptance criteria

- [ ] Memory has full CRUD with the envelope, linked to any number of targets with typed relations.
- [ ] Authorship class is recorded and immutable after creation.
- [ ] Correction creates a superseding memory; the original stays readable.
- [ ] Retrieval ranks by the formula above, tested against a fixed corpus.
- [ ] A memory whose asset changed since `as_of` is flagged stale.
- [ ] Contradictions are surfaced, never auto-resolved.
- [ ] `recall_memory` (Epic 14) returns memories with provenance and confidence.
- [ ] An agent can write a memory (Epic 32) attributed to itself.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first.

## Ranking weights and where each number comes from

`00i` rule 4: every magic number must be derivable from a stated reason in a
plan. These are the ranking defaults (`graph-owl-core::recall::Weights`), and the
derivation is the tiering rather than the individual values.

**Three tiers, and the tiering is the decision.**

| Term | Weight | Why this tier |
|---|---|---|
| `anchor` | 3.0 | A memory explicitly `About` this asset is on-topic regardless of wording; every other term is a *guess* at topicality. Highest so no accumulation of weaker signals promotes a memory about a different asset above one about this one. |
| `lexical` | 2.0 | Evidence of topic. |
| `semantic` | 2.0 | The same job done better (Epic 8). Equal to `lexical` because it replaces it rather than adding to it. |
| `staleness` | 2.0 | The anti-signal. **Set equal to `lexical` on purpose**: a stale memory matching the words perfectly is the confident, well-worded, wrong answer this feature exists to prevent, so being stale must be able to cancel a perfect match. |
| `recency` | 1.0 | Qualifier — breaks ties among memories that are equally on-topic. |
| `authorship` | 1.0 | Qualifier. |
| `confidence` | 1.0 | Qualifier. |

Within a tier the values are **equal because there is nothing to distinguish
them**, and inventing a gap would be inventing precision. What is claimed is the
ordering each term produces, not an exchange rate between terms — which is
exactly why every acceptance test isolates one term and holds the rest equal.

**The derived sub-values:**

- `recency_half_life_days = 180`. A memory about a data asset written half a year
  ago is worth about half one written today, because that is roughly the cadence
  at which the pipelines, owners and column meanings this catalogs turn over.
  Configurable precisely so an estate that moves faster says so.
- Staleness multipliers: `Stale = 1.0` (full penalty, per the weight above);
  `PossiblyStale = SubjectUnknown = 0.4`. **Derived from the same principle**:
  possibly-stale should cancel a *marginal* lexical match but not a strong one,
  and "marginal" is fewer than half the query's words — so the multiplier must
  sit below `0.5`. `SubjectUnknown` shares it because there is no evidence the
  memory is wrong, only no way to check; treating it as fully stale would condemn
  every memory about an asset a connector stopped reporting.
- Authorship: `Human = 1.0`, `Agent = 0.5`. An agent's claim is worth reviewing,
  not trusting. Halving is the coarsest honest statement of "counts, but less";
  anything finer is invented precision about how much less.
- Anchor strengths `About 1.0 > Affects 0.6 > Evidence 0.5 > Follows 0.4 >
  Mentions 0.2`. **Ordinal only.** `Affects` is causal so it sits just below
  being about it; `Evidence` makes the subject proof *for* the memory rather than
  its topic; `Follows` points at another memory, so the subject is rarely
  something a person queries; `Mentions` is named in passing — real, and the
  weakest thing that counts.

**What mutation testing taught us about these weights**, worth keeping because it
generalises to every weighted score in this codebase: `weight * term` and
`weight + term` are **indistinguishable at any non-zero weight**, since adding a
constant to every candidate reorders nothing. Only at zero does multiplication
erase the term while addition passes it straight through. So the "zeroing a
weight removes its effect" test is the *only* test that pins down **how** a
weight is applied rather than merely that it is — and there must be one per
weight. Three were missing on the first pass and mutation found all three.

### Slice A: Memory exists and links

**Value**: Knowledge has a home.
**Acceptance criteria**: create with kind, content, summary, authorship, confidence, and ≥1 link; a link to a nonexistent target → `400` naming the index; multiple links with different relations; `About` is required (at least one); confidence outside `[0,1]` → `400`; human-authored defaults to confidence 1.0; authorship is immutable on PATCH.
**RED**: The immutable-authorship test — an agent memory relabelled as human-authored destroys the trust model. A multi-link test with three relation types. Mutator watch: a mutable authorship field must fail; allowing zero `About` links must fail, since an unanchored memory is unretrievable.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice B: Correction supersedes

**Value**: The record of what people believed survives being corrected.
**Acceptance criteria**: `POST /memories/{id}/supersede` creates a new memory with `supersedes` set and marks the original `superseded_by`; the original remains readable and versioned; retrieval returns only the current memory by default, superseded ones with `?include=superseded`; a chain of three supersessions is traversable end to end; superseding an already-superseded memory → `409` pointing at the current one.
**RED**: The three-deep chain test asserting the full history is traversable. The `409` test asserting it names the current memory so a client can retry correctly. Mutator watch: overwriting content in place must fail the original-readable assertion; a one-level chain must fail depth 3.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice C: Retrieval ranks correctly

**Value**: The right memory surfaces first, which is the whole capability.
**Path**: pure ranking function; Epic 8 embeddings for the semantic term.
**Acceptance criteria**, against a fixed corpus:
- A directly-`About` memory outranks a `Mentions` memory with identical text.
- A recent memory outranks an older one with identical relevance.
- A human-authored memory outranks an agent-authored one with identical relevance and recency.
- A stale memory is penalized below a fresh one with lower lexical match.
- Ranking is a **pure function** of (memory, query, asset state) — no I/O, exhaustively testable.
- Weights are configurable; a config change changes ordering predictably.
**RED**: A fixed corpus with a table of (query → expected ordered ids). This test *is* the specification. Each criterion isolates one term by holding the others equal — the only way to prove a weight is actually applied. Mutator watch: a zeroed weight must fail its isolating test; ignoring staleness must fail the staleness case.
**REFACTOR**: assess sharing the ranking harness with Epic 8 Slice D. Yes — one relevance-testing fixture, two consumers.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice D: Staleness is detected

**Value**: A memory about a table that has since changed twice should not read as current truth.
**Acceptance criteria**: a memory's `as_of` compared against its `About` target's current version; a Major bump since `as_of` flags `stale`; a Minor bump flags `possibly_stale`; no change flags fresh; staleness is computed on read, not stored (it changes without the memory changing); stale memories are returned but flagged, never hidden; the flag names what changed.
**RED**: Major-vs-Minor differentiation test. A test asserting staleness is recomputed after the asset changes without the memory being touched — a stored flag would go wrong silently. Mutator watch: storing staleness must fail the recompute test; treating Minor as stale must fail the differentiation.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice E: Contradictions surface

**Shipped 30 Jul 2026.** Two notes on how the criteria were met:

- "Overlapping `as_of`" was resolved **without a time window**, because any
  window would be a number with no derivation, which `00i` rule 4 forbids.
  Supersession says it exactly: a superseded decision is history, and a decision
  that supersedes another is a correction — the opposite of a contradiction.
- **Confirming is not resolving.** A confirmed pair stays in the queue flagged
  `confirmed`; only a dismissal removes it, and neither memory is ever hidden or
  picked as the winner. `ContradictionKind` keeps `Confirmed` distinct from
  `Declared` because the provenance differs in a way somebody will want to query:
  a confirmed candidate is evidence the heuristic works, a declared one says
  nothing about it. And a confirmation cannot **resurrect** a pair detection no
  longer finds — if one side has since been superseded, the disagreement was
  settled by correction, and re-raising it would reopen a question the record
  already answers.

**Value**: Conflicting institutional knowledge is visible rather than silently inconsistent.
**Acceptance criteria**: an explicit `Contradicts` link surfaces both memories in a review queue; two `Decision` memories about the same asset with overlapping `as_of` are flagged as *candidate* contradictions; a human can confirm or dismiss a candidate; dismissal is recorded so the pair is not re-flagged; contradictions are **never** auto-resolved and neither memory is hidden.
**RED**: A test asserting both memories remain readable and neither is suppressed after a contradiction is flagged. A dismissal test asserting no re-flagging. Mutator watch: hiding either memory must fail; auto-selecting a winner must fail — that is software adjudicating institutional disagreement, which decision 6 forbids.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice F: Memory reaches agents

**Value**: The compounding loop closes — an agent reads what a previous investigation found.
**Acceptance criteria**: `recall_memory` (Epic 14) returns ranked memories with authorship, confidence, staleness, and links; results are policy-filtered like every other MCP response; a memory about an asset the principal cannot read is withheld and `policy_filtered` set; token budget applies, truncating content before dropping memories; recall by topic (not just by asset) works via the semantic term.
**RED**: A policy test asserting a memory about a denied asset is withheld — memory is a side channel that could leak asset existence otherwise. A truncation-order test asserting content shortens before memories drop. Mutator watch: unfiltered memory recall must fail the policy test — this is the leak path that bypasses Epic 13.
**Done when**: criteria met, mutation report reviewed, commit approved.

## Explicitly deferred (with destination)

- **Automatic memory extraction from documents** → Epic 21 produces the input; this epic consumes it.
- **Agent-written memory** → Epic 32.
- **Memory summarization / consolidation** ("merge these five incident notes") → needs a quality signal for summaries; revisit once there is volume.
- **Cross-organization memory sharing** → single-tenant assumption.
- **Memory expiry** → deliberately not planned (decision 6). Stale is surfaced, not deleted; institutional knowledge about a decommissioned system is still the reason it was decommissioned.
- **Retrieval-weight learning from feedback** → the confirmation count is captured; learning from it needs volume.

## Pre-PR quality gate

1. `cargo mutants` — 0 missed. The ranking function is the highest-stakes pure logic here.
2. Refactoring assessment.
3. `cargo test/clippy/fmt`.
4. Ranking verified against the fixed corpus with one isolating test per weight (Slice C).
5. Policy filtering on `recall_memory` verified (Slice F) — memory must not become an existence side channel.
