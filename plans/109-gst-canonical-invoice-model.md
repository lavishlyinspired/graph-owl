# Plan: GST canonical Invoice model — the B+C semantic-ingestion migration

**Status**: **Approved for implementation, pending two final refinements
landed in this revision.** Two review passes: the first caught a real
semantic error (GSTR-2B modeled as supplier-filed rather than
recipient-generated) plus three smaller gaps, all corrected. The second
approved the architecture and added two closing refinements — a new,
domain-agnostic resolution-finding type rather than overloading
`gst:GstinTransposition`/`gst:SupplierPanMismatch` as the attachment
decision, and a collision guard that also gates the *exact*-match
auto-attach path, not only the ambiguous one. Both are folded in below.
**Next step: begin Slice 1**, per the user's explicit instruction not to do
another architecture pass after this one.

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
`graph-owl-resolution`, `graph-owl-storage(-postgres)`, `graph-owl-api` — no
new `graph-owl-server` routes; the review surface reuses the existing
findings decision endpoint) — and a **pack-content** layer on top of it:
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

2. **Two separate period-scoped classes, not one generic `gst:Filing` —
   corrected by the user after this plan's first draft got the GSTR-2B side
   semantically wrong.** The first draft modeled GSTR-2B as
   `Supplier --filedBy--> Filing`, i.e. one supplier's filing. That is wrong:
   GSTR-1 is a supplier's own outward-supply declaration, but **GSTR-2B is an
   auto-drafted statement the authority generates *for the recipient*,
   aggregating many suppliers' filings** — a July 2B belongs to the taxpayer
   running this deployment, not to any one supplier. Corrected:

   - **`gst:Gstr1Filing`** — one per (supplier, return period), carrying
     `period` and `filedDate`, with a `filedBy` edge to `gst:Supplier`.
   - **`gst:Gstr2bStatement`** — one per (recipient, return period), carrying
     `period` and (when the source actually reports it — no current fixture
     or API payload field has been confirmed to carry this, so it is
     optional, following this pack's existing "absent is omitted, not
     blank" convention) `generatedDate`, with a `generatedFor` edge to
     `gst:Recipient`.

   Two concrete classes rather than one `gst:Filing` with a `filingType`
   discriminator, because `rdf:type` already carries that distinction once
   the classes are separate — a redundant string property duplicating what
   the type already says is exactly the kind of two-sources-of-truth risk
   this pack's own `invoiceKey`-vs-`invoiceNumber` split already exists to
   avoid elsewhere. Both are deduplicated across every invoice line they
   cover — today `gstr1.ts`/`gstr2b.py` repeat `period`/`filedDate`
   identically on every line from the same supplier's same filing; this is
   what removes that repetition and gives "was this invoice reflected in
   *any* statement across periods" somewhere to be asked from later (105c's
   own stated reason for wanting a Filing-shaped node, now split correctly
   across the two source types it actually needs to cover).

   **`gst:Recipient` is new scope this plan's first draft didn't name.**
   Nothing in the pack today tracks the taxpayer's own GSTIN (confirmed by
   grep — `supplierGstin` exists, no `recipientGstin`/`taxpayerGstin`
   equivalent does), and this pack is single-tenant: every reconciliation is
   one taxpayer's own data, so there is exactly one `gst:Recipient` subject
   ever needed. **Approved, with an explicit rule attached** — a single
   well-known subject:

   ```turtle
   gst:recipient-self rdf:type gst:Recipient .
   ```

   every `Gstr2bStatement`'s `generatedFor` pointing at it, meaning **"the
   recipient/taxpayer whose data this GST reconciliation deployment
   represents"** — not a taxpayer identity system.

   **Standing architectural rule, added per the user's explicit
   instruction, to record now rather than let drift**: `gst:recipient-self`
   is a single-tenant pack context anchor, not a globally reusable
   identity. It must not be used as the basis for cross-tenant identity
   resolution. If multi-tenant GST support is introduced later, the
   recipient subject becomes tenant-scoped and can carry the taxpayer
   GSTIN. This is written down here specifically so nobody six months from
   now reads `gst:recipient-self` as an accidental global singleton and
   builds on that assumption.

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
                            Supplier
                               │
                             issuedBy
                               │
                               ▼
                          gst:Invoice (canonical, NEW)
                               │
              ┌────────────────┼────────────────┐
              │                │                │
          recordedIn        appearsIn        reflectedIn
              │                │                │
              ▼                ▼                ▼
       gst:PurchaseInvoice  gst:Gstr1Invoice  gst:Gstr2bInvoice
       (existing type,      (existing type,   (existing type,
        books.ts)            gstr1.ts)         gstr2b.py/.ts)
                               │                │
                             filedIn          reflectedIn
                               │                │
                               ▼                ▼
                        gst:Gstr1Filing   gst:Gstr2bStatement
                               │                │
                            filedBy         generatedFor
                               │                │
                               ▼                ▼
                          gst:Supplier      gst:Recipient
   ```

   Every class named on the left (`PurchaseInvoice`, `Gstr1Invoice`,
   `Gstr2bInvoice`) is the **existing type**, unrenamed — the user's own
   instruction: *"don't make that rename part of this slice unless the
   existing queries/UI genuinely require it."* `appearsIn`/`reflectedIn`
   (canonical → per-line record) keep 105c's own names; **`reflectedIn` is
   deliberately reused a second time** (per-line `Gstr2bInvoice` →
   `Gstr2bStatement`) rather than a distinct name, because both edges mean
   the same thing at different levels — "the authority's own record of
   this" — and RDF predicates are not required to have one fixed
   subject/object type pair. `filedIn` is the one genuinely new hop on the
   GSTR-1 side, connecting a per-line declaration to the filing it came
   from. Each per-line record still lives inside its own source's named
   graph exactly as today (`GRAPH ?g { gst:g1-INV-1001 gst:invoiceKey ... }`
   is unchanged) — only the class it links up to on the GSTR-1/GSTR-2B side
   is now split into the two semantically-correct classes decision 2
   introduces.

5. **Per-line facts stay on the per-line record, not on canonical Invoice or
   Filing.** `itcAvailable`, `reverseCharge`, `invoiceType`, `placeOfSupply`,
   the tax component breakdown (`igst`/`cgst`/`sgst`/`cess`) vary per invoice
   *within* one filing — they cannot move to Filing (which is shared across
   many invoices) and Slice D (deferred) is what gives `itcAvailable`
   specifically its own claim object rather than moving it to Invoice now.

6. **Identity: business identity excludes period — corrected by the user.**
   This plan's first draft put a "period/date safeguard" *inside* the
   deterministic key. That is wrong, and GST's own GSTR-2B advisory is the
   reason: a supplier can file an invoice dated in one month into a *later*
   GSTR-1, and it then surfaces in a *later* period's GSTR-2B than its own
   invoice date — the exact carry-forward case `gst:filedDate` already
   exists in this pack to capture. **An invoice's identity does not change
   because of which period it was declared or reflected in.** Baking period
   into the identity key would create two different canonical `gst:Invoice`
   subjects for one real invoice whenever a source system associates it
   with a different period — precisely the fragmentation this whole plan
   exists to remove.

   Corrected, in the user's own words: **"Deterministic identity uses
   supplier GSTIN + normalized document number + document type, with
   invoice date/financial-year used as a collision guard and candidate
   discriminator rather than as a mandatory component of the canonical
   identity."** So:

   - **Business identity (the hard key)**: `(normalized supplierGstin,
     normalized document number, document type)`. `document type`
     future-proofs against Slice D's deferred CreditNote/DebitNote, which
     can plausibly share a numbering series with an Invoice at the same
     supplier.
   - **Collision guard / candidate discriminator (not part of identity)**:
     invoice date / financial year. Used during Slice 1's resolution step
     to judge whether two records sharing a business-identity key are
     plausibly the *same* invoice (dates close or compatible) or plausibly
     *different* invoices that happen to reuse a numbering scheme across
     years (`INV-001` every January is ordinary) — a signal that lowers
     confidence or produces an `Ambiguous` result, never something that
     silently merges or silently splits on its own.

   **The collision guard gates the exact-match path too — a second-round
   catch by the user on this plan's *previous* revision.** "Exact hard-key
   match ⇒ auto-attach" is not quite right by itself: two source records
   can share an identical `(supplierGstin, document number, document type)`
   key and still be genuinely different invoices — a supplier reusing
   `INV-001` a year apart is exactly the scenario the guard exists to name,
   and the guard must actually run on that path, not only on records that
   already failed an exact match. Corrected sequence:

   ```
   Hard key match
        │
        ▼
   Collision guard (date/FY compatibility)
        │
    ┌───┴────────┐
    ▼            ▼
   Compatible   Conflict
    │            │
    ▼            ▼
   Attach      Ambiguous
   ```

   A working definition of "compatible," proposed this session and not yet
   confirmed: **same financial year** (India's GST financial year runs
   April–March), on the reasoning that the legitimate late-filing
   carry-forward case this pack already models (`gst:filedDate`, the
   Section 16(2)(aa) rules) spans weeks to a couple of months and does not
   cross an FY boundary in any fixture this pack has, while a numbering
   collision across *years* is what the guard exists to catch. **Flag if
   this threshold is wrong** — it is this session's proposal, not something
   either review pass specified a number for, and per this project's
   standing rule every non-obvious number needs a stated reason (this is
   it) and a place to be revisited if it is not tight enough.

   **Entity resolution for ambiguous records is genuinely new platform
   work — corrected by the user after this plan's first draft treated the
   existing read-only near-miss display as sufficient. It is not.** See the
   capability section below.

## Canonical-entity lifecycle — added per the user's explicit instruction

The first draft of this plan left "when is `gst:Invoice` actually created"
implicit, which risks an implementation that creates a canonical Invoice for
every source record on ingestion and only *later* claims resolution
happened — defeating the whole point. Explicit sequence:

**Updated for the second review pass's collision-guard refinement**: the
guard now gates the exact-match path too, not only the ambiguous one.

```
SOURCE RECORDS (books / GSTR-1 / GSTR-2B, per-line, as today)
      │
      ▼
Identity evaluation (business-identity key: supplierGstin + normalized
                      document number + document type)
      │
 ┌────┴──────┐
 │           │
Hard key    No hard-key match, but scored above the resolution floor —
matches     the pack's existing ngram blocking on gst:supplierGstin/
 │          gst:invoiceNumber
 ▼           │
Collision    │
guard        │
(date/FY     │
compatible?) │
 │           │
 ├─ yes ──┐  │
 │        ▼  ▼
 │      Create/attach   Ambiguous — candidate(s) only, nothing
 │      canonical        written, nothing attached
 │      gst:Invoice       │
 │                        ▼
 └─ no ──► Ambiguous ──► Resolution finding raised through the
           (a hard-key    existing findings queue — a new,
           collision      domain-agnostic finding type
           with           (gst:InvoiceIdentityAmbiguity), not
           incompatible   gst:GstinTransposition/
           dates is a     gst:SupplierPanMismatch reused as the
           conflict,      decision itself. Existing GST findings
           not a match)   may contribute evidence; they are not
                           themselves the attachment decision.
                              │
                        ┌─────┴──────┐
                        ▼            ▼
                     Accept        Reject
                        │             │
                        ▼             ▼
                  Write a          No attachment; the two source
                  SubjectAttachment records stay under separate
                  record, attaching canonical gst:Invoice subjects.
                  the source record
                  to the chosen
                  canonical
                  gst:Invoice
```

**New acceptance criteria, added to Slice 1 below**:

- Per the user's exact wording: *"Repeated ingestion of the same source
  records is idempotent: exact deterministic matches resolve to exactly
  one canonical gst:Invoice; ambiguous candidates create no canonical
  attachment until explicitly accepted."*
- Per the user's exact wording, closing the false-merge path the first
  revision missed: *"An exact business-identity match auto-attaches only
  when collision-guard fields are compatible; a hard-key collision with
  incompatible invoice date/financial-year evidence becomes Ambiguous
  rather than being silently merged."*

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
  Epic 17's three bands: auto-attach **only on an exact business-identity
  key match that also passes the collision guard** (`New`/`Existing`
  collapse to one path here), or an `Ambiguous` result — candidates only,
  **nothing written, nothing attached** — for anything resolved only by
  score, *and* for an exact key match whose collision-guard fields
  conflict (decision 6's second correction — a hard-key collision is not
  automatically a match). This is the load-bearing property the user's
  instruction names directly: *"ambiguous matches must remain reviewable
  rather than being auto-attached solely from a fuzzy score."*
- **GST supplies an identity policy, not identity logic.** Declared in
  `pack.toml`, the same pattern `[[matching.blocking]]` already uses for
  supplier matching: which predicates form the **business-identity key**
  (`supplierGstin`, `invoiceKey`, a document-type marker — no period/date
  field, per decision 6's correction), which predicate is the
  **collision-guard/discriminator** (invoice date, checked against a
  same-financial-year compatibility rule — see decision 6), and which
  existing blocking strategy (`ngram`, already configured for GSTIN and
  invoice-number transposition) supplies ambiguous candidates when the
  business-identity key does not match exactly. No GST-specific Rust.
- **Review surface: resolved.** Reuse the findings infrastructure — queue,
  evidence display, decision endpoint, audit trail, accept/reject UI — but
  **not the existing GST finding semantics.** An ambiguous match does not
  become a `gst:GstinTransposition`/`gst:SupplierPanMismatch` finding —
  those are reconciliation findings about *tax facts*, and entity
  resolution is a different platform concern. Instead, a new,
  domain-agnostic finding shape: the resolution engine raises
  `gst:InvoiceIdentityAmbiguity` (GST's own name for the pattern; a future
  healthcare pack would raise `healthcare:PatientIdentityAmbiguity`
  against the *same* platform machinery — no new console surface per
  domain) through the existing `FindingStore`/`Finding` types, unchanged.
  Confirmed this fits without any new storage type: `Finding.subject` is
  one subject (the primary candidate), and `Evidence` entries each carry
  their **own** `subject` field — so a second (or further) candidate, its
  score, and the fields that justified it surface as evidence rows against
  their own subject, exactly the pattern `gst:GstinTransposition`'s
  `[findings.similarity]`/`resolve_by` band already uses to reach a second
  candidate today. Existing GST findings like `GstinTransposition`/
  `SupplierPanMismatch` may *contribute* evidence to a resolution finding's
  candidate list; they are never themselves the attachment decision.
  Accepting the resolution finding is what writes the `SubjectAttachment`
  record; rejecting leaves the two source records under separate canonical
  `gst:Invoice` subjects, unmerged — matching how a rejected
  `GstinTransposition` already behaves today.

This is real, multi-layer platform engineering — `graph-owl-core` (new
types), `graph-owl-storage`/`graph-owl-storage-postgres` (new persistence
for `SubjectAttachment`; `FindingStore` itself needs no new methods),
`graph-owl-resolution` (the generalized band/attach logic, extending its
existing charter), `graph-owl-api` (orchestration: constructing and
recording the `Finding`, and writing the `SubjectAttachment` on accept) —
not a pack-content change, and not something 105c Slice 1's "zero Rust
changed" precedent applies to. No new `graph-owl-server` routes: findings
already have a decision endpoint, and this reuses it.

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
      implementation) takes a caller-supplied business-identity-key policy,
      a collision-guard field, and a blocking-strategy-sourced candidate
      set, and returns exactly one of: auto-attach (exact business-identity
      key match **and** collision-guard fields compatible), or `Ambiguous`
      (candidates, evidence, confidence — nothing written) for everything
      else, including an exact key match whose collision-guard fields
      conflict. No code path writes an attachment from an `Ambiguous`
      result.
- [ ] Storage for the new attachment records, exercised by a real test
      against Postgres (this project's standing testcontainers pattern).
- [ ] A round-trip test: two GST source records with an exact
      business-identity match and compatible dates attach automatically;
      the pack's own planted transposition pair (INV-1004, `…1MZ` vs
      `…1ZM`) and PAN-mismatch pair (INV-1015) produce `Ambiguous` with the
      correct candidates and evidence, and no attachment record exists
      until a decision is made.
- [ ] **The collision-guard-on-exact-match test, per the user's exact
      wording**: *"An exact business-identity match auto-attaches only when
      collision-guard fields are compatible; a hard-key collision with
      incompatible invoice date/financial-year evidence becomes Ambiguous
      rather than being silently merged."* Concretely: two source records
      sharing `(supplierGstin, invoiceKey, documentType)` exactly, dated a
      year apart (crossing a financial-year boundary — the user's own
      `INV-001` 2025-01-10 / 2026-01-10 example), produce `Ambiguous`, not
      an auto-attach.
- [ ] **The review surface, resolved**: an ambiguous match raises a new,
      domain-agnostic `gst:InvoiceIdentityAmbiguity` finding through the
      existing `FindingStore`/`Finding` types (no new storage type — the
      primary candidate is `Finding.subject`, further candidates surface as
      `Evidence` rows carrying their own `subject`, the same pattern
      `gst:GstinTransposition`'s similarity band already uses). Accepting
      it writes the `SubjectAttachment` record; rejecting leaves the source
      records under separate canonical subjects. `gst:GstinTransposition`/
      `gst:SupplierPanMismatch` are never themselves the attachment
      decision — exercised end-to-end with a test asserting that accepting
      one of *those* findings does **not** write a `SubjectAttachment`.
- [ ] **Idempotency, per the user's exact wording**: repeated ingestion of
      the same source records is idempotent — exact deterministic matches
      resolve to exactly one canonical `gst:Invoice` (re-ingesting the same
      exact-match source record a second time does not create a second
      canonical subject or a second attachment record); ambiguous
      candidates create no canonical attachment until explicitly accepted
      (re-running resolution on an unresolved ambiguous pair produces the
      same candidates again, not a growing list and not a default
      attachment).

### Slice 2 — the B+C ontology/ingestion migration (uses Slice 1)

- [ ] `packs/gst/ontology.ttl` declares `gst:Invoice`, `gst:Gstr1Filing`,
      `gst:Gstr2bStatement`, `gst:Recipient`, and the new predicates
      (`recordedIn`, `appearsIn`, `reflectedIn` — reused at both the
      canonical→per-line-record level and the `Gstr2bInvoice`→
      `Gstr2bStatement` level — `filedIn`, `filedBy`, `generatedFor`).
      `filedDate` moves to `Gstr1Filing`'s own doc comment; `generatedDate`
      is declared on `Gstr2bStatement` and documented as optional, no
      current source confirmed to populate it.
      `PurchaseInvoice`/`Gstr1Invoice`/`Gstr2bInvoice` remain declared,
      unrenamed, documented as the per-source evidence layer rather than
      the business entity.
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
      `gstr1.ts`) emit the new shape — `gst:Invoice` (business-identity
      subject from `supplierGstin` + normalized `invoiceKey` + document
      type; **no period in the identity computation**, per decision 6), the
      unchanged per-line evidence record, and: `books.ts` emits `recordedIn`
      only (no Filing/Statement on the purchase-register side, per decision
      3); `gstr1.ts` emits a deduplicated `gst:Gstr1Filing` subject per
      (supplier, period) with `filedIn`; `gstr2b.py`/`gstr2b.ts` emit a
      deduplicated `gst:Gstr2bStatement` subject per (period) — pointing at
      the single well-known `gst:Recipient` subject, since this pack is
      single-tenant — with `reflectedIn`. No importer is left emitting the
      old shape when this plan is done — the release constraint the user
      stated directly.
- [ ] **The three-source convergence test — the user's own words, "the most
      important end-to-end test in the whole plan."** A Books record, a
      GSTR-1 record and a GSTR-2B record representing the same real invoice
      (same GSTIN, same invoice number, same document type) resolve to
      exactly one canonical `gst:Invoice`, while all three source records
      remain independently queryable in their own named graphs and retain
      their provenance (`recordedIn`/`appearsIn`/`reflectedIn` all present
      and resolvable from the one canonical subject).
- [ ] **The cross-period test — proving Filing/Statement's period-scoping
      does the temporal job it exists for. Checked against the actual
      fixtures: no existing planted invoice has an invoice-date month
      different from the 2B period it appears in** (`INV-1010`/`INV-1011`
      in `gstr2b-2026-08.ttl` are both dated within August, filed within
      August) — **this is a new fixture scenario Slice 2 adds**, not an
      existing one it preserves. An invoice dated in one month, declared by
      the supplier and reflected in a *later* month's GSTR-2B (matching
      GST's own published guidance that a late-filed document can surface
      in a later period's 2B than its invoice date — the same carry-forward
      mechanism `gst:filedDate` already exists in this pack to capture),
      queries as reflected in the *later* period — traversing `Invoice
      --reflectedIn--> Gstr2bInvoice --reflectedIn--> Gstr2bStatement
      --period--> "the later month"` — never the invoice's own, earlier
      month.
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
`bands::decide`/`score`/`rule_match` as-is, plus the collision-guard check
on both the exact and ambiguous paths) → `graph-owl-storage` +
`graph-owl-storage-postgres` (new persistence, tested against real Postgres)
→ `graph-owl-api` (orchestration: policy in, candidates out; an
`Ambiguous` result records a `gst:InvoiceIdentityAmbiguity`-style finding
through the existing `FindingStore`; accepting it writes the attachment) →
a test run against the GST pack's *own* already-shipped
transposition/PAN-mismatch fixtures, with no ontology change required to
prove it.

**Required implementation skills**: load `tdd`, `testing`,
`mutation-testing`, and `refactoring` before any code changes.

**Acceptance criteria**: this plan's Slice 1 Acceptance Criteria above.
**Architecture approved; both review passes are folded in. Present this
plan for one final human confirmation before the first file is edited, per
the user's own stated next step — not another design pass.**

**RED**: A failing test asserting that two candidate subjects sharing an
exact business-identity key **with compatible collision-guard dates**
attach automatically with a written, reversible record; that the same
exact key **with incompatible dates** (a year apart, crossing a financial
year) produces `Ambiguous`, not an auto-attach; and that two candidates
related only by an `ngram`-scored near-miss (the pack's own
INV-1004/INV-1015 shapes) produce an `Ambiguous` result with **no**
attachment record written, surfaced as a `gst:InvoiceIdentityAmbiguity`
finding rather than as `gst:GstinTransposition`/`gst:SupplierPanMismatch`
themselves.
Mutator watch: a mutant that treats a high-but-not-exact score as "close
enough to auto-attach" must be caught — the test asserting *zero*
attachment records exist after an `Ambiguous` resolution is the one that
catches it, not merely asserting the candidates list is non-empty. Equally,
a mutant that drops the collision-guard check on the exact-match path
(auto-attaching on hard-key match alone) must be caught by the
incompatible-dates test specifically — this is the false-merge path the
second review pass added because the first draft's test suite would not
have caught it.

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

### Slice 2: Canonical `gst:Invoice` + `gst:Gstr1Filing`/`gst:Gstr2bStatement`, all four ingestion surfaces, zero regression

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
**Architecture approved across two review passes, including decision 4's
wiring. Begin only once Slice 1 is shipped and green — this slice's queries
and fixtures depend on it existing, not on a stub.**

**RED**: For each of the four ingestion surfaces, a failing test asserting
the new Turtle shape (a `gst:Invoice` subject present with no period in its
computed identity, `recordedIn`/`appearsIn`/`reflectedIn` edges present, a
deduplicated `gst:Gstr1Filing` subject for `gstr1.ts` and a deduplicated
`gst:Gstr2bStatement` subject pointing at the single `gst:Recipient` for
`gstr2b.py`/`gstr2b.ts`) against a small fixture payload — extending the
existing `test_gstr2b.py`/`gstr2b.test.ts`/`books.test.ts`/`gstr1.test.ts`
files rather than replacing their existing assertions outright, since the
per-line evidence record's own shape is unchanged. Plus the two new
end-to-end fixtures/tests this slice's Acceptance Criteria add: the
three-source convergence case and the cross-period carry-forward case.
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
ambiguous — expect this to matter most on the business-identity-key
computation (no period involved, per decision 6) and the
`Gstr1Filing`/`Gstr2bStatement` deduplication-by-(supplier-or-recipient,
period) logic.

**REFACTOR**: Assess only after `scripts/verify-gst-reconciliation.sh` is
green — a rewrite this size will have real duplication across four
ingestion surfaces (the Filing/Statement-construction logic in particular)
worth a second look once behavior is proven, not before.

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
6. `cargo fmt`/`clippy` on every crate Slice 1 touches (`graph-owl-core`,
   `graph-owl-resolution`, `graph-owl-storage`, `graph-owl-storage-postgres`,
   `graph-owl-api` — no `graph-owl-server` changes expected, the review
   surface reuses the existing findings decision route). Slice 2 is not
   expected to touch any Rust — confirm nothing did.
7. `cargo mutants` scoped to Slice 1's new resolution/attachment logic —
   0 missed, per this plan's own Slice 1 acceptance criteria.
