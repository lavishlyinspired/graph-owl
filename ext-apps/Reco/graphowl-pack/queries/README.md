# These 3 files are symlinks into `packs/gst/queries/`, not copies

`missing-in-gstr2b.sparql`, `amount-mismatch.sparql`,
`tax-head-mismatch.sparql` — each `ls -la` as a relative symlink to
`../../../../packs/gst/queries/<name>.sparql`. **Corrected 16 August
2026**: these were physical copies for a few hours of this session; fixed
after review (`plans/119-architecture-audit.md` §5c wasn't the concern
here, a direct question was — see that section's own note). packs/gst is
graph-owl's own pack; the query text belongs there and nowhere else, and
a copy — even a byte-identical, documented one — is a second place to
remember to update.

**Why a symlink, and not the two other options considered**:

- *Extend the manifest schema to allow a cross-pack path*: bigger,
  invasive, and `connectors/python/graph_owl_packs/manifest.py`'s
  `..`-refusal is a real security boundary (a pack may not read outside
  its own directory) — loosening it for every pack to fix one pack's
  file layout is the wrong trade.
- *Hand-roll the finding-rule registration in Python, reading
  `packs/gst`'s files directly*: works, but reimplements
  `_register_finding_rules`'s JSON shape (a private function in
  `loader.py`) by hand instead of reusing it — the same "don't
  reimplement proven infra" mistake this session already made once with
  the pack-vocabulary duplication in §3.1.
- **A symlink**: `Manifest.load`'s path-containment check
  (`.. in candidate.parts`) operates on the *declared* string
  (`"queries/amount-mismatch.sparql"`), which stays a normal relative
  path — it's what that path points to on disk that's the pack it truly
  belongs to. `Path.is_file()`/`read_bytes()` follow it transparently, so
  `load_pack` needed zero code changes, and `git` tracks the symlink
  natively (`git add`/`git status` show it as a symlink, not a text
  diff). Zero duplicated bytes, one file, one owner.

**Why reco-now's pack still exists at all, rather than reco-now calling
`POST /packs/gst/reconcile` directly**: two independent reasons, neither
about the file location.

1. `POST /namespaces`/`POST /predicates` for the 6 fields packs/gst
   genuinely doesn't have (`hsnCode`, `imsStatus`, `noteType`,
   `voucherType`, `voucherNumber`, `originalInvoiceNumber`) have to be
   registered under *some* pack's namespace, and they're reco-now's own
   fields, not part of the published `gst:` vocabulary — registering them
   under pack id "gst" would mean graph-owl's own reference pack quietly
   grew fields no spec or statute names.
2. `GET /findings?pack=X` and `POST /packs/X/reconcile` are how the
   native engine attributes and scopes a reconciliation run. Findings
   from reco-now's real data should read as reco-now's own
   (`pack: "reco"`), not merged into `packs/gst`'s — especially since
   `packs/gst`'s own demo fixtures are deliberately never loaded into
   reco-now's deployment (`include_documents=False`, `main.py`) to avoid
   exactly that mixing.

So: **one ontology** (`packs/gst/ontology.ttl` — reco-now's own
`ontology.ttl` adds 6 properties and zero classes, reusing
`gst:PurchaseInvoice`/`gst:Gstr2bInvoice` directly, never redeclaring
them). **One copy of the query/law text** (`packs/gst`'s own, referenced
by symlink). **Two pack *registrations*** (`gst` and `reco`), because
attribution and reco-now's own 6 fields are a real, separate reason to
have two, independent of where any file lives.

**Why these 3 findings, not all 13 `packs/gst` registers — corrected
three times, each time by reading the query text instead of trusting the
previous pass.** `plans/119-architecture-audit.md` §5a/§5c is the full
record; summarized here:

1. First pass: assumed 8 findings were reachable with books+GSTR-2B
   alone, from field-level vocabulary compatibility.
2. Second pass: `MissingInBooks` moved out — its query *requires*
   `gst:Gstr1Invoice` in a mandatory `GRAPH` join, not the `OPTIONAL`
   guard `missing-in-gstr2b.sparql` uses (which is why *that* one still
   fires correctly with zero GSTR-1 data — it's the designed base case).
   Left 7 reachable, of which 2 (`GstinTransposition`,
   `SupplierPanMismatch`) need `[matching.blocking]` config this pack
   doesn't have.
3. Third pass, once actually wiring the remaining 5: 2 more turned out to
   need data or value-coding reco-now doesn't have. `ITCNotAvailable`
   reads `gst:itcAvailable` — reco-now's CSV format has no such column at
   all. `Reversed` filters `gst:reverseCharge = "R"` (the GST portal's own
   code); reco-now's `reverse_charge` values are plain "Yes"/"No" text, so
   the filter would never match even with the predicate correctly
   asserted.

**Live parity result** (`../../scripts/verify-reconcile-parity.py`,
passing): these 3 findings correctly reproduce 3 of `reconciliation.py`'s
4 categories against the fresh fixture — and, for the 1 planted amount
mismatch, `TaxHeadMismatch` fires *alongside* `AmountMismatch` for the
same invoice, a real additional distinction (the tax heads genuinely
differ by more than the rounding floor, proportionally to the taxable
value delta) `reconciliation.py` has no equivalent for at all. The 4th
category (`only_gstr2b` — an invoice in GSTR-2B with no matching books
entry) has **no native equivalent reachable with just these 3** — see
`plans/119-architecture-audit.md` §5c for why, and why that's a real gap
rather than a bug to fix here.

`../law/sections.ttl` and `../law/rule-36-4.ttl` are the same fix, same
reason — symlinks to `packs/gst/law/`, not copies. `amount-mismatch.sparql`
needs that data (the dated `gst:Provision`/`capPercent` subjects it
traverses) and it is not a demo fixture, so it's imported here even though
`packs/gst`'s own load into this deployment is vocabulary-only
(`include_documents=False`) specifically to keep the demo *invoices* out.
