# Worked example: how the GST pack exercises domain neutrality, end to end

**Status**: Explanation document, 11 August 2026 — **no code, no plan to implement.** Written to answer "how does domain neutrality actually work for the GST use case?" with the *real* pack (`packs/gst/`) rather than a hypothetical. Companion to `plans/105-domain-neutrality.md`; read that for the design, this for the data walk.

**The claim being demonstrated.** A domain is added to graph-owl as **configuration and data** — no Rust, no TypeScript. The GST pack is the proof-of-concept the platform was designed against; `packs/hospitality/` is the proof it generalizes. Every mechanism below is exercised by both packs identically, over vocabulary they share nothing in common.

## The real pack, file by file

```
packs/gst/
  pack.toml                       # namespace + predicates + matching + findings + console queue
  ontology.ttl                    # gst: classes and properties (the vocabulary)
  law/sections.ttl                # statutory sections as subjects
  law/rule-36-4.ttl               # dated provisions (Rule 36(4) caps, in force between two dates)
  fixtures/events.ttl             # purchase/payment events (for the 180-day rule)
  fixtures/purchase-register.ttl  # what the taxpayer claims  (source graph "gst-purchase-register")
  fixtures/gstr2b.ttl             # what suppliers filed      (source graph "gst-gstr2b")
  queries/*.sparql                # six registered reconciliation rules
```

Namespace: `https://graph-owl.dev/packs/gst#` (prefix `gst`). Every term is a *runtime* term — nothing here required a line of Rust (`ontology.ttl:1-8`).

## Step 1 — install: the namespace and predicates register at runtime (DN-1)

The loader declares the namespace and each predicate through the same endpoints the ontology pack epic built (`POST /namespaces`, `POST /predicates`). Two runtime tables, no code:

- `namespaces` (V14): `code 1024` ← `https://graph-owl.dev/packs/gst#`, `declared_by 'pack:gst'`. Codes below `namespace::RUNTIME_START` (1024) are refused — a deployment can never redefine `dsc:` or `rdf:` (`V14__namespace_registry.sql:16-21`).
- `predicates` (V3): `(1024, 'supplierGstin', value_type=str, core=FALSE)`, `(1024, 'issuedBy', value_type=ref)`, … `core = FALSE` means the organisation extended the vocabulary without a release (`V3__predicate_registry.sql:3-8`).

**Why this was the blocker.** Before DN-1, `Sid::from_iri` resolved a fixed compile-time array and `namespace_iri` was a `match` returning `&'static str`. An IRI in the pack's namespace resolved to `None` → `RdfError::UnrecognisedIri` on import, so **zero packs could load their ontology**. The last domain that needed a namespace got it by editing core Rust (`namespace::CUI`/`SNOMED_CT`/`RXNORM`). Now a resolver built from the `namespaces` table resolves `https://graph-owl.dev/packs/gst#supplier-27AABCU9603R1ZM` → `Sid::new(1024, "supplier-27AABCU9603R1ZM")` — while the shipped `rdf:`/`dsc:` still resolve on the same allocation-free path (`namespaces.rs:1-27`).

## Step 2 — import: the facts land as flakes, in named graphs

`POST /graph/import/rdf` lands each document in its own source graph (`graph:import:gst-purchase-register`, `graph:import:gst-gstr2b`), as flakes `{s, p, o, cx, t, op}` (`flake.rs:410-427`). Two documents, two graphs, one vocabulary.

From `purchase-register.ttl` — the taxpayer claims invoice INV-1002 under supplier `27AABCU9603R1ZM`:

```turtle
gst:pr-INV-1002 rdf:type gst:PurchaseInvoice ;
    gst:issuedBy      gst:supplier-27AABCU9603R1ZM ;
    gst:invoiceNumber "INV-1002" ;
    gst:invoiceDate   "2026-07-09" ;
    gst:taxableValue  "100000.00" ;
    gst:taxAmount     "18000.00" ;
    gst:period        "2026-07" .
```

From `gstr2b.ttl` — the authority reports the same invoice, and the supplier is the **same graph subject**:

```turtle
gst:2b-INV-1002 rdf:type gst:Gstr2bInvoice ;
    gst:issuedBy      gst:supplier-27AABCU9603R1ZM ;
    gst:invoiceNumber "INV-1002" ;
    gst:taxableValue  "95000.00" ;   # ← disagrees with the register
    gst:taxAmount     "17100.00" ;
    gst:itcAvailable  "Y" ;
    gst:reverseCharge "N" ;
    gst:period        "2026-07" .
```

Two details worth noticing, both neutral-by-construction:

- **`issuedBy` is an edge, not a literal.** `gst:Supplier` is a real subject both sides point at (`packs/gst/ontology.ttl:49-52`, Epic 105c). A transposition is therefore a *second* Supplier node (`gst:supplier-27AABCU9603R1MZ`), which is what makes the n-gram rule below meaningful — it suspects two nodes are one entity.
- **Amounts are strings, not floats.** A monetary value parsed to IEEE double at the graph boundary would lose the exactness a tax figure needs; the queries compare them as decimals (`pack.toml:51-54`).

## Step 3 — matching: blocking strategies are configured, never written (DN-2)

The pack's `matching.yaml`-equivalent in `pack.toml` composes **generic algorithms** over **named fields**. No `GstinKey` variant exists — `plans/105-domain-neutrality.md` is explicit that `gstin_key` would be the per-domain hardcoding this epic exists to remove:

```toml
[[matching.blocking]]
strategy = "normalized"                 # case-insensitive join key
fields = ["gst:supplierGstin", "gst:invoiceNumber"]

[[matching.blocking]]
strategy = "composite"                  # invoice + date window, because two
                                        # systems rarely record the same day
[[matching.blocking.of]]
strategy = "normalized"
fields = ["gst:supplierGstin"]
[[matching.blocking.of]]
strategy = "date_window"
fields = ["gst:invoiceDate"]
days = 7

[[matching.blocking]]
strategy = "ngram"                      # sees through a transposition
fields = ["gst:supplierGstin"]          # that exact/normalized cannot
n = 3
```

The doc comment states it plainly: *"The identical strategies the hospitality pack uses, over different fields. `normalized` there matched a phone number; here it matches an invoice number."* (`pack.toml:47-50`). One implementation, two domains.

## Step 4 — findings: six rules, and how two of them work

A finding rule is **a query plus a binding and nothing else** (`pack.toml:167-177`): `query` names a SPARQL entry, `subject` names the variable holding the graph subject the finding is about, and `evidence` maps each remaining binding to the predicate it came from — written out because `?claimed` is not `gst:taxAmount`, and evidence a reviewer cannot follow back into the graph is worse than none.

The six findings, as declared in `pack.toml`:

| Label | What fires it | Governed by |
|---|---|---|
| `gst:PotentialMismatch` | Claimed in register, never filed | `gst:Section16-2-aa` |
| `gst:AmountMismatch` | Both filed, values differ beyond the cap then in force | `gst:Rule36-4` |
| `gst:ITCNotAvailable` | Matched, authority says no credit | `gst:Section17-5` |
| `gst:Reversed` | Matched, flagged reverse-charge | `gst:Section16-2-aa` |
| `gst:GstinTransposition` | Near-identical GSTINs, same invoice number | `gst:MatchingPolicy` |
| `gst:PaymentOverdue` | Unpaid past 180 days | `gst:Section16-2-d` |

**AmountMismatch is the flagship of the law-as-data design.** The cap is *not* a constant in the query — it is read from the graph by traversal: the query finds the `gst:Provision` in force on the invoice date (latest `effectiveFrom` ≤ date, via `OPTIONAL` + `!BOUND`), compares the delta against that provision's own `capPercent`, and projects the `citation` into the evidence so a reviewer sees which notification the judgement was made under (`queries/amount-mismatch.sparql:4-19,48-72`). That is why INV-2001 (2020, 5% delta, 10% cap then in force) is **clean** while INV-1002 (2026, 5% delta, nil cap) is a finding — same rule, same query, different answer, because the law lives in `law/rule-36-4.ttl` and amending it is adding a subject, not editing a threshold (`fixtures/purchase-register.ttl:14-21`).

**GstinTransposition is the division of labour between SPARQL and a strategy.** The query deliberately over-fetches — every candidate pair with the same invoice number and period under *different* GSTINs — because SPARQL has no notion of string similarity. The `[findings.similarity]` band in the manifest keeps only the near-identical ones: `ngram, n=3`, `at_least 0.40`, `at_most 0.999` (`pack.toml:252-271`). The thresholds are measured against the fixture, not chosen by feel: the planted transposition `27AABCU9603R1ZM` vs `27AABCU9603R1MZ` scores **0.619**, the nearest genuinely different supplier scores **0.065**, so 0.40 sits in the widest available gap. The `at_most` half is load-bearing — without it every *correctly* matched invoice scores 1.0 and is reported as a suspected typo, which would make a reviewer stop trusting the queue on day one.

Both rules traverse `issuedBy` rather than reading a literal off the invoice (`queries/gstin-transposition.sparql:18-22`) — a query shape only the graph makes possible.

## Step 5 — the findings render through the generic review queue

```toml
[console.queues]
id = "gst-reconciliation"
label = "GST reconciliation"
source = "findings"
labels = [ "gst:PotentialMismatch", "gst:AmountMismatch", "gst:ITCNotAvailable",
           "gst:Reversed", "gst:GstinTransposition", "gst:PaymentOverdue" ]
actions = ["accept", "reject-with-reason", "defer"]
evidence = "side-by-side"
```

`ReviewQueue.tsx` never names a queue; every difference lives in this config (`ui/src/features/review/queues.ts:1-24`). Adjudicating a GST amount mismatch and a hospitality guest duplicate is the same interaction — which is the whole point (`plans/105-domain-neutrality.md:36`).

## What was neutral at each step

| Step | Neutral mechanism | What a per-domain design would have done |
|---|---|---|
| Namespace | `namespaces` table, `1024+` range, `NamespaceResolver` | `namespace::GST` constant in `graph-owl-core` |
| Predicates | `predicates` table, `core = FALSE` | new Rust predicate variant |
| Matching | `Normalized`/`Composite`/`DateWindow`/`NGram` over named fields | `GstinKey` strategy |
| Law | `gst:Provision` subjects + traversal | cap constant in the query |
| Entity shape | `gst:Supplier` is a graph subject | a new catalog `AssetKind` |
| Console | `queues.ts` config | a GST-specific review component |

## Boundaries honored (DN-3/DN-4)

- An invoice is **not** a catalog asset. `AssetKind` stays a fixed enum; `gst:Supplier`/`gst:PurchaseInvoice` are flakes in the pack's named graph under the pack's own namespace. The two layers meet only where a *column* is said to *mean* a domain concept (Epic 24's glossary link), never by widening `AssetKind` (`plans/105-domain-neutrality.md:54-62`).
- The console covers graph-shaped work only. Maps/time-series/BI are excluded for every domain equally — a positioning boundary, not a neutrality failure.
- The regression guard is real: `scripts/check-namespace-neutrality.py` (wired into `scripts/gate.sh`) fails the build if a domain namespace constant is added to `graph-owl-core`. CUI/SNOMED_CT/RXNORM are allowlisted as grandfathered, not endorsed (`plans/105-domain-neutrality.md:112`).

## The honest caveat

GST is one of the seven Indian financial-compliance domains the platform was **designed against** — it shares a vocabulary shape, identifier-based matching, a legal spine and deadline arithmetic with the family. Seven samples from one family prove nothing about neutrality, which is exactly why the acceptance test is **hospitality** — no statute, no government identifier, no reconciliation of two authorities' records — and why the claim is that the hospitality pack loads with zero `.rs` and zero `.tsx` changes. GST working end to end is necessary but not sufficient evidence; the neutrality claim stands or falls on the pack that shares nothing with it.

## Reading order

1. `plans/105-domain-neutrality.md` — the epic: the four chokepoints, the acceptance test.
2. `packs/gst/pack.toml` — namespace, predicates, matching, all six findings, console queue.
3. `packs/gst/ontology.ttl` + `fixtures/*.ttl` — the vocabulary and the planted scenarios.
4. `packs/gst/queries/*.sparql` — the rules, each with its design rationale in the header.
5. `packs/hospitality/pack.toml` — the same loader, strategies and queue over a foreign domain.
