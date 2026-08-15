# Plan: GST canonical Invoice model — the B+C semantic-ingestion migration

**Status**: Planned, not started. Blocking design decisions confirmed with the
user; the exact ontology wiring below is this session's synthesis of those
decisions and needs confirmation before code.

**Origin**: `plans/ingestion_extraction.md` (a design-consultation transcript,
first-party architectural reasoning — verified not to reference any
proprietary third-party material) proposed widening
`packs/gst/ontology.ttl`'s three source-shaped invoice classes
(`PurchaseInvoice`/`Gstr1Invoice`/`Gstr2bInvoice`) into one canonical
`gst:Invoice`. `plans/105c-gst-causal-graph.md` — the real, already-partially-
shipped in-repo design — reviewed this exact merge earlier, shipped a first
step (`gst:Supplier`/`issuedBy`), and **explicitly deferred the rest**, naming
two open questions verbatim: *"what does an Invoice with only one side filed
look like? how does a Filing node interact with per-source named graphs?"*
This plan answers both, per the user's explicit direction (this session), and
implements Slices B and C of the transcript's own split-candidates table
(`recorded here for provenance, not as the source of the design`: B —
"Canonical `gst:Invoice` replaces the 3 source classes"; C — "Books & GSTR-1
ingestion emit the canonical shape"). Slice D (ITC as its own claim object)
is explicitly out of scope — gated on this plan shipping green.

**Scope**: two layers, sequenced as two slices. A **platform** layer — a new,
domain-agnostic entity-resolution/attachment capability (`graph_owl_core`,
`graph-owl-resolution`, `graph-owl-storage(-postgres)`, `graph-owl-api`,
possibly `graph-owl-server`) — and a **pack-content** layer on top of it:
`packs/gst/` (ontology, fixtures, all 13 registered SPARQL queries,
`pack.toml`'s `[console.reconciliation]` config) plus all four ingestion
surfaces that currently emit the three source classes
(`connectors/python/graph_owl_packs/gstr2b.py`,
`ui/src/features/packs/{gstr2b,books,gstr1}.ts`). This plan's first draft
assumed the platform layer already existed and could be skipped — corrected
by the user; see "Entity-resolution/attachment capability" below.

## Resolved decisions (this session)

1. **`gst:Invoice` is the canonical business entity.** One per real invoice,
   not one per source that reported it.

2. **`gst:Filing`, period-scoped, per the user's explicit instruction** — one
   node per (supplier, return period, filing type) tuple, carrying
   `filingType` (`"GSTR1"` / `"GSTR2B"`, open string, same trade as
   `extractor`/`extractor_version` elsewhere in this codebase), `period`,
   `filedDate`, and a `filedBy` edge to `gst:Supplier`. Deduplicated across
   every invoice line the same filing declares — today `gstr1.ts`/`gstr2b.py`
   repeat `period`/`filedDate` identically on every line from the same
   supplier's same filing; Filing is what removes that repetition and gives
   "was this invoice reflected in *any* filing across periods" somewhere to
   be asked from later (105c's own stated reason for wanting this node).

3. **The purchase register stays a separate source/record concept, not a
   Filing** — per the user's explicit instruction. A purchase-register entry
   is the taxpayer's own bookkeeping, not a government submission; forcing it
   into the Filing shape (which the user's own wording distinguishes) would
   invent a filing type that does not exist. `gst:PurchaseInvoice` (the
   existing type `books.ts` already produces) keeps that name and role.

4. **Source-record provenance is preserved, not collapsed** — per the user's
   explicit instruction ("entity resolution must create/attach the canonical
   gst:Invoice, not erase the source representations"; "preserve source
   records and provenance rather than collapsing them destructively"). This
   is the one place this plan's wiring **departs from 105c's original
   sketch**: 105c drew `Invoice --appearsIn/reflectedIn--> Filing` directly,
   with no per-line record surviving the merge. That sketch predates the
   user's provenance-preservation requirement and (105c says so itself) never
   answered "what does a Filing interact with per-source named graphs look
   like." This plan's answer, synthesized from the user's constraint:

   ```
   gst:Invoice (canonical, NEW)
       --recordedIn-->  gst:PurchaseInvoice   (existing type — books.ts's own per-line record)
       --appearsIn-->   gst:Gstr1Invoice      (existing type — gstr1.ts's own per-line record)
       --reflectedIn--> gst:Gstr2bInvoice     (existing type — gstr2b.py/.ts's own per-line record)

   gst:Gstr1Invoice  --filedIn--> gst:Filing   (filingType = "GSTR1")
   gst:Gstr2bInvoice --filedIn--> gst:Filing   (filingType = "GSTR2B")
   gst:Filing --filedBy--> gst:Supplier
   ```

   `appearsIn`/`reflectedIn` keep 105c's own names (the user confirmed them
   explicitly) but point at the **preserved per-line evidence record**, not
   directly at Filing — the per-line record is what still lives inside that
   source's own named graph (`GRAPH ?g { gst:g1-INV-1001 gst:invoiceKey ... }`
   is unchanged), and `filedIn` is the one new hop connecting it to the
   Filing it came from. **This is my synthesis, not something either source
   document states verbatim, and it is the piece of this plan most likely to
   need adjustment — confirm before implementation starts.**

5. **Per-line facts stay on the per-line record, not on canonical Invoice or
   Filing.** `itcAvailable`, `reverseCharge`, `invoiceType`, `placeOfSupply`,
   the tax component breakdown (`igst`/`cgst`/`sgst`/`cess`) vary per invoice
   *within* one filing — they cannot move to Filing (which is shared across
   many invoices) and Slice D (deferred) is what gives `itcAvailable`
   specifically its own claim object rather than moving it to Invoice now.

6. **Identity: deterministic first — refined per the user's explicit
   correction to include document type and a period/date safeguard.**
   `gst:Invoice`'s deterministic key is
   `(normalized supplierGstin, normalized document number, document type,
   period/date safeguard)` — not just `(gstin, invoiceKey)` as this plan
   first drafted. Two reasons the extra fields are load-bearing, not
   defensive padding: **document type** future-proofs against Slice D's
   deferred CreditNote/DebitNote, which can plausibly share a numbering
   series with an Invoice at the same supplier; **the period/date safeguard**
   guards against a supplier reusing invoice numbering across periods or
   years (`INV-001` every January is an ordinary numbering scheme) — with no
   date component, two genuinely different invoices years apart would
   collide onto one canonical subject. Confirmed by the codebase's own
   `INV-2001`/`INV-2002` fixture pair (2020) sitting beside `INV-2001`-shaped
   2026 numbers in spirit, even though this particular fixture doesn't
   collide today — the safeguard is for the case it plausibly will once real
   uploads arrive.

   **Entity resolution for ambiguous records is genuinely new platform
   work — corrected by the user after this plan's first draft treated the
   existing read-only near-miss display as sufficient. It is not.** See the
   capability section below.

## Entity-resolution/attachment capability — genuinely new platform work

**What already exists, checked rather than assumed, and how much of it is
actually reusable:**

- `graph_owl_resolution::bands::decide(score, &ConfidenceBands) -> Decision`
  is a **pure, already domain-agnostic function** — no `Uuid`, no `Sid`, just
  a score in and a `New`/`Existing(confidence)`/`Ambiguous` band decision
  out. Fully reusable as-is.
- `graph_owl_resolution::score` (jaro-winkler) and `rule_match` (n-gram
  Jaccard, already what `gst:GstinTransposition`'s similarity band runs) are
  likewise pure and reusable as-is — this pack already calls the second one
  today.
- What is **not** reusable as-is: `graph_owl_core::resolution`'s
  `MergeRecord`/`Candidate`/`Resolution` are typed on catalog-asset `Uuid`s
  (`MergeRecord.canonical: Uuid`, `.merged: Uuid`; `Candidate.entity: Uuid`),
  and the storage/API machinery around them (`graph-owl-storage`'s merge
  tables, `Catalog`'s merge-decision endpoints) is wired specifically to the
  catalog `Asset`/`Table` domain. Domain-pack subjects are `Sid`s (IRIs), not
  catalog-asset UUIDs, and have no row in that storage shape at all.
  Confirmed by reading `resolution.rs` directly rather than assuming the
  types would happen to fit.

**What this plan adds, generalizing the *shape* Epic 17 already proved —
`New`/`Existing`/`Ambiguous` bands, a reversible audit record, scored
candidates with evidence for review — to a domain-pack `Sid`, not reusing
the `Uuid`-typed types themselves:**

- A `Sid`-typed attachment/audit record (name TBD during implementation,
  e.g. `SubjectAttachment`) in `graph_owl_core`, parallel to `MergeRecord`:
  which canonical `gst:Invoice` subject a source record was attached to,
  what evidence justified it, who/what decided it (`Auto`/`Human`/`Agent`,
  reusing `MergeDecidedBy` as-is — it is already identifier-agnostic),
  reversible the same way `MergeRecord.split_at` is.
- Storage for these records — new table(s), not a retrofit of the
  asset-merge ones, since a `Sid` is an IRI string with no catalog row
  behind it.
- A **domain-agnostic platform capability** in `graph-owl-resolution` (its
  existing charter per `plans/00e-crate-architecture.md`'s crate table is
  literally "Entity resolution, coreference, temporal" — this is squarely
  inside it, not a reason for a new crate) that takes a caller-supplied
  identity-key policy and a set of candidate subjects, and produces exactly
  Epic 17's three bands: auto-attach on an exact deterministic key match
  (`New`/`Existing` collapse to one path here, since an exact key match
  *is* the deterministic identity, not a scored candidate), or an
  `Ambiguous` result — candidates only, **nothing written, nothing
  attached** — for anything resolved only by score. This is the load-bearing
  property the user's instruction names directly: *"ambiguous matches must
  remain reviewable rather than being auto-attached solely from a fuzzy
  score."*
- **GST supplies an identity policy, not identity logic.** Declared in
  `pack.toml`, the same pattern `[[matching.blocking]]` already uses for
  supplier matching: which predicates form the deterministic key
  (`supplierGstin`, `invoiceKey`, a document-type marker, a period/date
  field for the safeguard above), and which existing blocking strategy
  (`ngram`, already configured for GSTIN and invoice-number transposition)
  supplies ambiguous candidates when the deterministic key does not match
  exactly. No GST-specific Rust.
- **Open design question, not resolved by either source document — flag for
  confirmation**: does the review step reuse the pack's *existing* findings
  queue (an ambiguous match surfaces as a `gst:GstinTransposition`-shaped
  finding, and *accepting* it is what writes the attachment record — reusing
  UI, decision endpoint, and audit trail the console already has), or does
  it need its own review surface? Reusing findings is smaller and this
  plan's default; a separate resolution queue is real new console/API
  surface with no forcing fixture behind it yet.

This is real, multi-layer platform engineering — `graph-owl-core` (new
types), `graph-owl-storage`/`graph-owl-storage-postgres` (new persistence),
`graph-owl-resolution` (the generalized band/attach logic, extending its
existing charter), `graph-owl-api` (orchestration), `graph-owl-server`
(routes if the review step is not reused from findings) — not a pack-content
change, and not something 105c Slice 1's "zero Rust changed" precedent
applies to.

## What this plan does NOT do (explicitly deferred)

- **Slice D — ITC as its own claim object.** Separate plan, gated on this one
  passing `scripts/verify-gst-reconciliation.sh` unchanged.
- **CreditNote/DebitNote/Amendment, a richer ~30-field Invoice.** Named in the
  transcript, not driven by any fixture scenario this pack has today —
  inventing one to justify the ontology would be designing for a hypothetical
  (the same standing rule 105c already applied to the same classes).
- **A shared cross-language wire schema (JSON Schema → Rust + TypeScript
  codegen)**, floated in the transcript's later section. Real idea, separate
  concern from "the ontology has the right shape" — not needed to ship this.
- **Cross-period linkage / evidence-chain walk (P7).** 105c's own scoping:
  `Filing` nodes existing is a prerequisite for that work, not the work
  itself.

## Acceptance Criteria

### Slice 1 — the entity-resolution/attachment capability (platform)

- [ ] `graph_owl_core` gains a `Sid`-typed attachment/audit record type,
      parallel to `MergeRecord` (canonical subject, attached subject,
      evidence, confidence, `decided_by: MergeDecidedBy`, reversible).
- [ ] A domain-agnostic resolution function/service in `graph-owl-resolution`
      (or orchestrated from `graph-owl-api`, exact placement decided during
      implementation) takes a caller-supplied deterministic-key policy plus
      a blocking-strategy-sourced candidate set, and returns exactly one of:
      auto-attach (exact key match), or `Ambiguous` (candidates, evidence,
      confidence — nothing written). No code path writes an attachment from
      an `Ambiguous` result.
- [ ] Storage for the new attachment records, exercised by a real test
      against Postgres (this project's standing testcontainers pattern).
- [ ] A round-trip test: two GST source records with an exact deterministic
      key match attach automatically; the pack's own planted transposition
      pair (INV-1004, `…1MZ` vs `…1ZM`) and PAN-mismatch pair (INV-1015)
      produce `Ambiguous` with the correct candidates and evidence, and no
      attachment record exists until a decision is made.
- [ ] The review-surface question (reuse findings vs a new queue) is
      resolved and, if findings are reused, an accepted
      `gst:GstinTransposition`/`gst:SupplierPanMismatch` finding is what
      writes the attachment record — exercised end-to-end.

### Slice 2 — the B+C ontology/ingestion migration (uses Slice 1)

- [ ] `packs/gst/ontology.ttl` declares `gst:Invoice`, `gst:Filing`, and the
      new predicates (`recordedIn`, `appearsIn`, `reflectedIn`, `filedIn`,
      `filedBy`, `filingType`, `filedDate` moved off the per-line classes'
      doc comments onto Filing's). `PurchaseInvoice`/`Gstr1Invoice`/
      `Gstr2bInvoice` remain declared, documented as the per-source evidence
      layer rather than the business entity.
- [ ] `packs/gst/pack.toml` registers the new predicates and updates
      `[console.reconciliation.sources]`'s three `class = "..."` entries
      (currently `PurchaseInvoice`/`Gstr1Invoice`/`Gstr2bInvoice` — confirm
      whether the console's reconciliation-page rendering needs to read
      `gst:Invoice` instead, or continue reading the per-line classes; this
      is a real fifth blast-radius item found during verification that
      neither source document named).
- [ ] All 13 registered SPARQL query files (`packs/gst/queries/*.sparql`
      referenced by a `[[findings]]` entry, per the earlier grep) are
      rewritten from "two `rdf:type`s + exact key match" joins to traversal
      through `gst:Invoice`. Every query's existing guard logic (the
      `missing-in-gstr2b.sparql` global GSTR-1-handover guard, the
      `!BOUND`-not-`NOT EXISTS` pushdown workaround, the PAN-mismatch/
      transposition similarity bands) is preserved, not just the join shape.
- [ ] `packs/gst/fixtures/{purchase-register,gstr1,gstr2b,gstr2b-2026-08}.ttl`
      are rewritten by hand to the new shape (matching 105c Slice 1's own
      method), preserving every planted scenario's numbers and every comment
      explaining why that scenario exists.
- [ ] All four ingestion surfaces (`gstr2b.py`, `gstr2b.ts`, `books.ts`,
      `gstr1.ts`) emit the new shape — `gst:Invoice` (deterministic subject
      from `supplierGstin` + `invoiceKey`), the unchanged per-line evidence
      record, and (for the two GSTR-1/GSTR-2B surfaces) a deduplicated
      `gst:Filing` subject per (supplier, period, filing type) with the
      `filedIn` edge from the per-line record. No importer is left emitting
      the old shape when this plan is done — the release constraint the user
      stated directly.
- [ ] `gstr2b.py` and `gstr2b.ts` stay pinned to identical output on the same
      fixture, per their existing tests (`test_gstr2b.py`/`gstr2b.test.ts`) —
      this pinning is a pre-existing regression guard, not new to this plan.
- [ ] `scripts/verify-gst-reconciliation.sh` passes with **zero changes to
      its assertions** — same finding counts, same cited evidence values, for
      every one of its ~70 checks. This is the regression bar both source
      documents named and the one the codebase actually enforces.
- [ ] `scripts/verify-pack-load.sh` (the "no pack ships code" check) stays
      green — this migration is pack content plus ingestion-surface changes,
      never new Rust.
- [ ] `cargo test -p graph-owl-server --test reconcile` and `--test
      evidence_graph` stay green (they seed GST scenarios via HTTP against
      the real pack manifest — confirm they don't hard-code the old class
      names anywhere the grep didn't catch).

## Slices

**Two slices, split on a different axis than "ontology vs ingestion."** The
user's instruction — *"do not implement B in isolation and leave the
importers producing the old source-shaped classes"* — forbids splitting
ontology from ingestion, and Slice 2 below still lands ontology, fixtures,
queries and all four importers together as one PR-sized unit, per the
`planning` skill's named exception for inherently-coupled changes.

What *is* split out is Slice 1, the entity-resolution/attachment
capability — this is horizontal platform work, and it qualifies for the
`planning` skill's own horizontal-work exception on all four counts: it
names the vertical slice it unlocks (Slice 2, which needs it to construct
`gst:Invoice` subjects), it is independently verifiable (the round-trip test
in its own acceptance criteria needs no ontology change at all — it runs
against the pack's *existing* transposition/PAN-mismatch fixtures), it
leaves the codebase deployable on its own, and building it inline inside
Slice 2 would be strictly larger than building it first and consuming it.

### Slice 1: A domain-agnostic subject can be resolved and attached to a canonical entity, reviewably

**Value**: any future pack (GST first) can ask "is this candidate subject the
same real-world entity as an existing one" and get either a written,
auditable attachment (exact match) or a reviewable set of candidates
(ambiguous) — never a silent merge — which is the platform primitive 105c's
own "genuinely open design question" and this plan's user-directed
correction both name as missing.

**Path**: `graph_owl_core` (new `Sid`-typed attachment/audit type) →
`graph-owl-resolution` (band decision + candidate scoring, reusing
`bands::decide`/`score`/`rule_match` as-is) → `graph-owl-storage` +
`graph-owl-storage-postgres` (new persistence, tested against real Postgres)
→ `graph-owl-api` (orchestration: policy in, candidates out, decision
in, attachment written) → the review-surface wiring (reusing `findings`
decision endpoints, per the open question above, resolved during this
slice) → a test run against the GST pack's *own* already-shipped
transposition/PAN-mismatch fixtures, with no ontology change required to
prove it.

**Required implementation skills**: load `tdd`, `testing`,
`mutation-testing`, and `refactoring` before any code changes.

**Acceptance criteria**: this plan's Slice 1 Acceptance Criteria above.
**Present to human and get explicit confirmation — including on the
review-surface open question — before any file is edited.**

**RED**: A failing test asserting that two candidate subjects sharing an
exact deterministic key attach automatically with a written, reversible
record, and that two candidates related only by an `ngram`-scored near-miss
(the pack's own INV-1004/INV-1015 shapes) produce an `Ambiguous` result with
**no** attachment record written.
Mutator watch: a mutant that treats a high-but-not-exact score as "close
enough to auto-attach" must be caught — the test asserting *zero* attachment
records exist after an `Ambiguous` resolution is the one that catches it,
not merely asserting the candidates list is non-empty.

**GREEN**: The new type, the generalized band/attach logic, storage, and
orchestration, together — this is the platform primitive Slice 2 depends on
existing in full.

**MUTATE**: Scoped to the new resolution/attach logic and its storage
adapter, `--lib` where the mutated code has unit coverage, `--re` for the
storage-adapter shell per this project's documented `cargo mutants`
practice.

**KILL MUTANTS**: Address survivors; ask when ambiguous — expect the
auto-attach-vs-ambiguous boundary to be where this matters most, matching
the standing project finding that surviving mutants are almost always a
missing *negative* test.

**REFACTOR**: Assess only after MUTATE confirms test strength.

**Done when**: Slice 1's Acceptance Criteria are met, mutation report
reviewed, human approves commit.

### Slice 2: Canonical `gst:Invoice` + `gst:Filing`, all four ingestion surfaces, zero regression

**Value**: "Which invoices did Supplier X issue, across the register, GSTR-1
and GSTR-2B" becomes one traversal from one canonical subject instead of
three separately-typed subjects joined at query time — with every existing
finding rule producing byte-identical output on the same fixtures, so
shipping this is a pure internal restructuring from the product's point of
view, not a behavior change a CA using the reconciliation page would notice.

**Path**: `packs/gst/ontology.ttl` (new vocabulary) → `packs/gst/pack.toml`
(new predicates registered, the new identity-policy config Slice 1's
capability consumes, console config updated) → `packs/gst/fixtures/*.ttl`
(hand-rewritten to the new shape) → `packs/gst/queries/*.sparql` (13 files,
rewritten join patterns) → `connectors/python/graph_owl_packs/gstr2b.py` +
`ui/src/features/packs/{gstr2b,books,gstr1}.ts` (all four emit the new
shape, computing the deterministic key directly for the exact-match path;
ambiguous pairs are left as two separate `gst:Invoice` subjects and reach
one canonical subject only through Slice 1's review/attach flow) →
`scripts/verify-gst-reconciliation.sh` (the observable proof, run against a
real Postgres + real server + the real pack).

**Depends on**: Slice 1, shipped and green. The deterministic (exact-match)
path is simple enough that each importer could compute it directly without
calling Slice 1's capability at all — but the ambiguous path (transposition,
PAN mismatch) is exactly what Slice 1 exists to handle correctly, so this
slice's queries and fixtures are written against Slice 1 existing, not
against a stub.

**Required implementation skills**: load `tdd`, `testing`,
`mutation-testing`, and `refactoring` before any code changes — the
per-file work still goes through RED/GREEN per touched normalizer function
and per touched query's own test coverage (`test_gstr2b.py`, `gstr2b.test.ts`,
`books.test.ts`, `gstr1.test.ts`, plus any Rust tests touching the pack).

**Acceptance criteria**: this plan's Slice 2 Acceptance Criteria above.
**Present to human and get explicit confirmation — including on decision 4's
proposed wiring specifically, since it's this session's synthesis rather
than a literal instruction — before any file is edited.**

**RED**: For each of the four ingestion surfaces, a failing test asserting
the new Turtle shape (a `gst:Invoice` subject present, `recordedIn`/
`appearsIn`/`reflectedIn` edges present, a deduplicated `gst:Filing` subject
for the GSTR-1/GSTR-2B surfaces) against a small fixture payload — extending
the existing `test_gstr2b.py`/`gstr2b.test.ts`/`books.test.ts`/`gstr1.test.ts`
files rather than replacing their existing assertions outright, since the
per-line evidence record's own shape is unchanged.
Mutator watch: a mutant that computes the canonical `gst:Invoice` subject
from `invoiceNumber` instead of the normalized `invoiceKey` must be caught —
this is exactly the "one invoice, three printed formats" case the fixtures
already plant (INV/1014 vs INV 1014 vs INV-1014), so a test asserting all
three inputs produce the *same* canonical subject is required, not just that
a subject exists.

**GREEN**: Ontology + pack.toml + all four normalizers + all 13 queries +
all 4 fixture files rewritten together, since none of it is independently
correct without the rest.

**MUTATE**: `graph_owl_packs.gstr2b`'s new subject-computation logic
(Python, no `cargo mutants` — check whether this project has a Python
mutation tool; if not, note the gap rather than skip verification silently)
and the TypeScript twins' equivalent logic (Stryker, scoped to the four
changed files per this project's TS mutation practice).

**KILL MUTANTS**: Address survivors; ask before accepting one whose value is
ambiguous — expect this to matter most on the deterministic-subject
computation and the Filing-deduplication-by-(supplier,period,type) logic.

**REFACTOR**: Assess only after `scripts/verify-gst-reconciliation.sh` is
green — a rewrite this size will have real duplication across four
ingestion surfaces (the Filing-construction logic in particular) worth a
second look once behavior is proven, not before.

**Done when**: every Acceptance Criteria item is checked, mutation reports
for the four ingestion surfaces reviewed, `scripts/verify-gst-reconciliation.sh`
and `scripts/verify-pack-load.sh` both green, human approves commit.

## Pre-PR quality gate

1. `scripts/verify-gst-reconciliation.sh` — the real regression bar, ~70
   assertions, zero changed.
2. `scripts/verify-pack-load.sh` — no pack code check.
3. `cargo test -p graph-owl-server --test reconcile --test evidence_graph`.
4. Python: `pytest connectors/python/tests/` (at least `test_gstr2b.py` and
   whatever `test_loader.py` coverage touches `packs/gst`).
5. TypeScript: `books.test.ts`, `gstr1.test.ts`, `gstr2b.test.ts`,
   `gstText.test.ts`, `packSurfaces.test.ts` (the last for the
   `console.reconciliation.sources` class-name question above).
6. `cargo fmt`/`clippy` on every crate Slice 1 touches
   (`graph-owl-core`, `graph-owl-resolution`, `graph-owl-storage`,
   `graph-owl-storage-postgres`, `graph-owl-api`, `graph-owl-server` if the
   review surface is new). Slice 2 is not expected to touch any Rust —
   confirm nothing did.
7. `cargo mutants` scoped to Slice 1's new resolution/attachment logic —
   0 missed, per this plan's own Slice 1 acceptance criteria.

---
*Delete this file when the plan is complete.*
