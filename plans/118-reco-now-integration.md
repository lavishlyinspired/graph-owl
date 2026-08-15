# Plan 118 — reco-now as a pack-owning integrator

**Branch**: main (committed directly, per `CLAUDE.md`)
**Status**: Slice 1 in progress, 15 August 2026.
**Depends on**: Epic 105 (`plans/105-domain-neutrality.md`) — the pack format, the
generic loader, and the DN-3 acceptance bar this plan reuses rather than
re-designs.
**Strategy note**: `ext-apps/Reco/graph-owl-reco-now.html` (published artifact:
<https://claude.ai/code/artifact/fe24193b-0de8-4aa7-a5da-b3e5e690df44>) —
the market-pattern research and architecture options this plan implements
the first slice of. Saved as a static copy alongside the app since a
published artifact URL is not guaranteed to outlive this session.

## Why this plan is short

Investigated before writing a line of code, because the obvious framing —
"design a configurable-pack mechanism so external apps aren't stuck with a
hardcoded GST ontology" — turned out to already be built and already proven:

- **Packs are 100% data.** `pack.toml` (TOML manifest) plus `.ttl`/`.sparql`
  files. `connectors/python/graph_owl_packs/loader.py`'s own words: *"the
  loader knows nothing about any domain... hospitality, tax and automotive
  differ only in the bytes it reads."*
- **The loader already takes an arbitrary directory.**
  `load_pack(directory: Path, server: str, ...)` — a pack can live anywhere
  a filesystem path can point, including inside a consuming app's own repo.
  Nothing needs to be built to make packs "configurable"; that property is
  already load-bearing enough that Epic 105 shipped a *second*, independently
  owned pack (`packs/hospitality/`) specifically to prove it.
- **There is a CI-enforced guard against exactly the mistake this task warned
  against.** `scripts/check-namespace-neutrality.py` (wired into
  `scripts/gate.sh`) fails the build if a domain namespace constant is added
  to `graph-owl-core`. Verified clean at 18 allowlisted constants today.
- **The acceptance bar already exists and this plan reuses it verbatim**
  (`105-domain-neutrality.md` line 74): *a pack loads its own namespace,
  resolves its records, and is queryable — with ZERO changes to any `.rs`
  file and ZERO changes to any `.tsx` file.* Slice 1 below is checked against
  that same bar, the same way `scripts/verify-pack-load.sh` checks it for
  `packs/gst` and `packs/hospitality`.

So this plan is not "build configurability." It is: **give reco-now its own
pack, owned inside `ext-apps/Reco/`, never inside graph-owl's `packs/` — proving
per-integrator ownership concretely rather than reusing the bundled example
— and wire reco-now's real upload path to it.**

## What reco-now actually is (verified against `ext-apps/Reco/backend`)

- FastAPI app, `app.main`, title *"RecoNow — Intelligence for Indirect Tax"*.
  `SESSION: dict = {}` is the **entire** persistence layer — reset by
  `_reset()` at the top of every `/api/upload` call (`main.py:32,104,261`).
- `FIELD_LABELS` (`main.py:38-56`) names 17 fields reco-now already
  extracts from an uploaded Purchase Register or GSTR-2B file: invoice
  number, supplier GSTIN/name, taxable value, invoice date, place of
  supply, HSN code, IMS status, reverse-charge flag, note type, voucher
  type/number, original invoice number, and the four tax components
  (IGST/CGST/SGST/Cess).
- `reconciliation.py` matches by an exact-then-fuzzy `(gstin, invoice_no)`
  key, then a tolerance compare on amounts, into four statuses: matched,
  review (amount mismatch), only-books, only-portal.
- **This field set is real, not identical to `packs/gst/ontology.ttl`.**
  `packs/gst` has no `hsnCode`, `imsStatus`, `noteType`, `voucherType`,
  `originalInvoiceNumber` or `voucherNumber` — reco-now needs its own
  ontology, not the bundled example, which is the concrete reason this
  isn't "point reco-now at `packs/gst`."

## Explicitly out of scope for this plan

- **Replacing `reconciliation.py`'s matching with graph-owl's native
  `POST /packs/{id}/reconcile` engine.** Real and valuable, but the native
  engine returns findings+evidence, a different shape from the
  books/portal paired-row view reco-now's frontend already renders — it
  needs its own slice with its own acceptance criteria, named as a
  follow-on below, not folded into ingestion.
- **Grounding `ai.py`'s email drafts via `graph-owl-mcp`.** Same reason —
  a separate observable behaviour, a separate slice.
- **Auth.** reco-now has none today; graph-owl runs in built-in open mode
  for this plan. Real auth on both sides is a prerequisite for real client
  data, not for this integration proof — noted, not solved here.

## Slice 1 — an uploaded file lands as durable graph subjects under reco-now's own pack

**Value**: an accountant using reco-now uploads a Purchase Register or
GSTR-2B file; instead of vanishing from an in-process dict on the next
upload, every row becomes a queryable, durable graph subject in graph-owl —
proving the engine integration end to end, without disturbing the existing
reconciliation view.

**Path**: `POST /api/upload` (real endpoint, real multipart file) → existing
`_parse_upload`/`_auto_map`/`_normalize` (unchanged) → **new**:
`graphowl_client.rows_to_turtle(rows, kind, source)` → **new**:
`graphowl_client.import_document(server, source, turtle_text)` → graph-owl's
already-shipped `POST /graph/import/rdf` → Postgres. Verified by querying
`GET /namespaces` and `POST /sparql` against the live server, and by the
response returning what landed/skipped/rejected — the same three-way outcome
`graph_owl_packs.loader.DocumentResult` already reports for a static pack
document.

**Setup this slice needs (horizontal, justified inline per the `planning`
skill's exception — named, verifiable, smaller than doing it ad hoc, no
speculative abstraction beyond what's used):**

1. `ext-apps/Reco/graphowl-pack/pack.toml` — id `reco`, namespace
   `https://reconow.dev/pack#`, prefix `reco`. **No `[[documents]]`,
   `[[findings]]`, `[[queries]]` or `[matching.blocking]`** — this slice is
   ingestion only, so the manifest carries only `[pack]` and
   `[[predicates]]`. Matching/reconciliation config is Slice 2's, not
   invented early.
2. `ext-apps/Reco/graphowl-pack/ontology.ttl` — two classes
   (`reco:BooksInvoice`, `reco:PortalInvoice`, matching reco-now's own
   `kind` values) and 17 properties, one per `FIELD_LABELS` entry,
   authored from reco-now's own field list — not copied from
   `packs/gst/ontology.ttl`. `[[predicates]]` in `pack.toml` registers the
   same 17, `value_type = 1` (string) throughout, for the identical reason
   `packs/gst` keeps amounts as strings: a monetary value parsed to a float
   at the graph boundary loses the exactness a tax figure needs.
3. `ext-apps/Reco/backend/requirements.txt` gains one editable, path-local
   dependency: `graph_owl_packs` (`connectors/python`, monorepo-relative
   now that `ext-apps/Reco` lives inside graph-owl) — for the one proven,
   already-tested primitive this slice reuses: `load_pack` to declare the
   namespace and register the 17 predicates once, at backend startup.
   Nothing else from that package is used here.
4. **New module** `ext-apps/Reco/backend/app/graphowl_client.py` — stdlib
   `urllib` only, matching every other graph-owl Python client in this
   repo's own stated convention ("a loader is not a place to acquire an
   HTTP dependency"). Two pure/thin pieces:
   - `rows_to_turtle(rows: list[dict], kind: str, source: str) -> str` —
     pure, no I/O. One subject per row,
     `<https://reconow.dev/data/{kind}/{gstin}-{invoice_no}>`
     (percent-encoded), typed `reco:BooksInvoice`/`reco:PortalInvoice`.
     **A field with no value is omitted, never written as an empty
     literal** — the same "absent vs. recorded blank" distinction
     `graph_owl_packs/erpnext.py` already draws, for the same reason: a
     reconciliation asking "was this recorded" needs the two states to
     stay distinguishable.
   - `import_document(server: str, source: str, turtle: str, token=None) ->
     dict` — one `POST /graph/import/rdf?source=...&format=turtle`,
     returning the server's landed/skipped/rejected counts unmodified.

**Acceptance criteria:**

- [ ] `rows_to_turtle` on an empty row list returns an empty string (no
      prefix declaration emitted for zero triples).
- [ ] `rows_to_turtle` on one books row with every field populated emits one
      subject, typed `reco:BooksInvoice`, with all 17 predicates present.
- [ ] `rows_to_turtle` on a row with some fields empty/`None`/NaN omits
      exactly those predicates — asserted as a **negative**: the emitted
      text does not contain the omitted predicate's local name at all, not
      merely that the test doesn't check for it.
- [ ] A value containing a Turtle-significant character (`"`, `\`, a
      newline) round-trips escaped correctly — this is the concrete failure
      mode a real supplier name or invoice note can hit (`O"Brien Textiles`,
      a multi-line note), and an unescaped quote would corrupt every triple
      after it, not just its own.
- [ ] Two rows with the same `invoice_no` under different `supplier_gstin`
      produce two distinct subject IRIs (the negative the gst pack's own
      design note calls out: an exact-string subject key silently merges
      unrelated invoices).
- [ ] `import_document` against a real graph-owl-server (open mode, no
      token) lands the expected subject count, verified by a follow-up
      `POST /sparql` query against `graph:import:{source}` — not just a 200
      response, which the DN-1 write-up's own history shows can hide a
      predicate-registration-ordering bug that silently rejects every
      subject.
- [ ] `/api/upload`, exercised end-to-end with a **fresh** sample
      Purchase-Register/GSTR-2B pair (new fixture, not
      `ext-apps/Reco/SAMPLE/README2.md`'s existing 17 rows), leaves the
      existing `overview()` response byte-for-byte unchanged — this slice
      is additive, not a replacement of the working UI.
- [ ] `scripts/check-namespace-neutrality.py` still passes (nothing added
      to `graph-owl-core`), and `git diff` touches zero `.rs` and zero
      `.tsx` files anywhere in the repo — the DN-3 bar, applied here.

**RED**: pytest, `ext-apps/Reco/backend/tests/test_graphowl_client.py` — every
bullet above as a separate test, written first, run to confirm they fail for
the right reason (module doesn't exist yet / function returns nothing).

**GREEN**: minimum `graphowl_client.py` to pass. No retry logic, no
connection pooling, no batching beyond one `POST` per upload — none of that
is asked for by a criterion above.

**MUTATE**: no `.rs` changes in this slice, so `cargo mutants` does not
apply. Python has no mutation tool established in this project's
conventions; the discipline substitutes explicit negative-case tests for
every positive one (empty-omission, distinct-subject, escaping) — the same
gap class this project's own mutation history says is the one that
actually survives.

**REFACTOR**: assess after green; skip if it adds nothing (`refactoring`
skill).

**Done when**: all acceptance criteria pass against a live local
graph-owl-server (Postgres + `cargo build --release -p graph-owl-server`,
open mode, port 8080 — the same shape `scripts/demo.sh` already uses,
without its banking-estate seed or agent service, neither of which this
slice touches), `pytest ext-apps/Reco/backend/tests` is green, and the
neutrality checks above pass.

## Follow-on slices (named, not implemented here)

### Slice 2 — reconciliation runs through graph-owl's native engine

**Value**: the four-way match (matched / review / only-books / only-portal)
comes from `POST /packs/reco/reconcile` — the same native engine
`packs/gst` already exercises — instead of `reconciliation.py`'s in-process
join. Needs: `[[findings]]`/`[[queries]]`/`[matching.blocking]` added to
`ext-apps/Reco/graphowl-pack/pack.toml` (reco-now's own matching thresholds,
not gst's), and a translation from the engine's finding+evidence shape into
the `{status, reason, book, portal}` rows the frontend already renders —
its own acceptance criteria, not folded into Slice 1.

### Slice 3 — grounded AI drafting via MCP

**Value**: `ai.py`'s `draft_follow_up` calls graph-owl-mcp's
`get_asset_context`/`explain_lineage` for the invoice in question before
drafting, instead of the four scalar arguments it receives today. Blocked
on Slice 2 existing (there's no graph-owl-side finding to explain until
reconciliation runs there).

## Pre-PR quality gate (Slice 1)

1. `pytest ext-apps/Reco/backend/tests` green.
2. `scripts/check-namespace-neutrality.py` green.
3. `git status` shows changes only under `ext-apps/Reco/` — no `crates/`, no
   `ui/`, no `packs/` (reco-now's pack lives in its own app directory, not
   graph-owl's).
4. Manual end-to-end: fresh sample data through the real `/api/upload`,
   confirmed landed via `/sparql` against a locally running
   graph-owl-server.
