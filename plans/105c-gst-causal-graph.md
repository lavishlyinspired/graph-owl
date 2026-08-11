# Plan: GST as a causal graph, not a two-table join (scoping only — no code)

**Status**: **Scoping document, 11 August 2026.** Written in parallel with `plans/105b-native-reconcile-engine.md`'s implementation, at the user's explicit direction to run both tracks side by side. **Nothing in this document is implemented.** It exists to be reviewed, not merged into code, until a human picks it up.

**Origin**: a design critique from the user, arguing that GST reconciliation as currently built uses graph-owl's storage but not its graph — that the six `[[findings]]` rules are structurally row-matching (`Invoice A → rules → Invoice B → MATCH`), buildable with Python + SQL, and that the real GraphOwl value would be multi-hop causal traversal, temporal reasoning across filing periods, an evidence chain, and an agent that answers "why is my ITC lower this month" — not a mismatch report.

## The finding that grounds this document

The critique is correct, and there is a single concrete fact that proves it rather than merely illustrating it:

**`gst:Supplier` is declared as a class in `packs/gst/ontology.ttl` and is never instantiated anywhere.** Every invoice fixture (`packs/gst/fixtures/purchase-register.ttl`, `gstr2b.ttl`) carries `gst:supplierGstin "27AABCU9603R1ZM"` as a **literal string property**, not an edge to a `gst:Supplier` subject. The class exists in the vocabulary; no graph node of that type has ever been created. The same is true of `Company`/`Buyer` — neither is declared at all.

So today's graph has exactly one real edge per invoice (`onInvoice`, from a `PurchaseEvent`/`PaymentEvent` back to the invoice it happened to) and one join key (`supplierGstin` + `invoiceNumber` + `period`, compared as strings across two directly-related fact sets). Everything else — supplier identity, the filing pipeline, temporal state — is absent as *structure*, even though it is expressed in SPARQL rather than pandas. **The critique's core claim is not "this isn't graph-shaped enough," it is "this graph has one edge type."**

## What the richer model looks like

```turtle
gst:Company         a gst:Class .   # the taxpayer running this deployment
gst:Supplier        a gst:Class .   # already declared — now actually instantiated
gst:Invoice         a gst:Class .   # supersedes the PurchaseInvoice/Gstr2bInvoice split below
gst:Filing          a gst:Class .   # one GSTR-1/GSTR-3B/GSTR-2B submission, for one supplier, one period
gst:CreditNote      a gst:Class .
gst:DebitNote       a gst:Class .
gst:Amendment       a gst:Class .
gst:Itc             a gst:Class .   # the claim itself, distinct from the invoice supporting it

gst:issuedBy        a gst:Property .  # Invoice -> Supplier
gst:purchasedFrom   a gst:Property .  # Company -> Supplier
gst:appearsIn       a gst:Property .  # Invoice -> Filing (a GSTR-1 filing naming this invoice)
gst:reflectedIn     a gst:Property .  # Invoice -> Filing (the GSTR-2B side)
gst:claims          a gst:Property .  # Company -> Itc
gst:supports        a gst:Property .  # Invoice -> Itc
gst:adjusts         a gst:Property .  # CreditNote/DebitNote -> Invoice
gst:supersedes      a gst:Property .  # Amendment -> Invoice
```

**Why `PurchaseInvoice`/`Gstr2bInvoice` collapse into one `Invoice` class with two `Filing` edges, rather than staying separate.** The current split *is* the one-hop-join structure the critique targets: two invoice types compared directly. Modeling one invoice with two `Filing` relationships (`appearsIn` the GSTR-1 filing, `reflectedIn` the GSTR-2B filing) is what turns "compare these two rows" into "walk from the invoice to each filing and compare what each filing says" — the same underlying facts, a materially different query shape, and the one that supports the January→February example below.

**This is pack content, not platform engineering.** Every one of these classes and edges is authored in `packs/gst/ontology.ttl`, `mapping.yaml`, and the connector normalizers (`gstr2b.py`, its TypeScript port) — no new Rust. `plans/105-domain-neutrality.md`'s own boundary already states this: "domain entities are graph subjects... described by the pack's own ontology," never a platform change. The engines this needs — multi-hop traversal, SPARQL joins across three hops, OWL reasoning — are already generic and already shipped (`graph-owl-traversal`, `graph-owl-query`). **Nothing here requires a new Rust primitive**, with two explicit exceptions named below.

## The two hops re-expressed

**"Why is INV-1002 mismatched?"** Today: one `OPTIONAL` join in `amount-mismatch.sparql`, comparing two `taxableValue` literals. Under the richer model, the same SPARQL query is still a single request, but it traverses:

```
Invoice INV-1002 → issuedBy → Supplier ABC → appearsIn → Filing(GSTR-1, July)
                                            → reflectedIn → Filing(GSTR-2B, July) → taxAmount
```

— and the *evidence* attached to the resulting finding is the filing each figure came from, not just the two numbers. This is still one SPARQL query (SPARQL already expresses arbitrary-hop graph patterns); what changes is that the query has hops to walk because the graph now has edges to walk them on.

**The January→February example is the one genuinely new capability**, and it is not solved by richer ontology alone. `Filing` nodes are period-scoped (a GSTR-2B filing for July is a different node than one for August), so "was this invoice reflected in *any* filing across periods" is a traversal question the current per-period-import model cannot answer today regardless of ontology richness — each upload lands in its own source graph with no link between periods. This needs the **evidence-chain walk** (platform doc P7: Entity → Evidence → Source → Provenance → Assertion → Finding, with missing-hop determination) to exist as a real capability, not just richer data.

## What is genuinely new engineering, and what already exists

Cross-checked against `plans/25-graphowl-intelligence-platform.md`'s own build order — none of this is a new invention, all of it is already sequenced, unbuilt:

| Capability the critique wants | Status | Where |
|---|---|---|
| Multi-hop graph traversal | **Exists** — `graph-owl-traversal`, `graph-owl-query` | No new work |
| SPARQL queries expressing multi-hop patterns | **Exists** — already how the 6 current rules are written | Pack content only (richer ontology + queries) |
| Dated/temporal rule resolution (Rule 36(4)'s cap) | **Exists** — already resolved by traversal over `law/rule-36-4.ttl` | Extend the pattern to filing periods |
| Evidence chain (Entity→Evidence→Source→Provenance→Finding, missing-hop aware) | **Missing** — platform doc P7 | Rust, `graph-owl-traversal`/`graph-owl-engine`, per the three-way crate rule |
| "Why is my ITC lower" — natural-language causal investigation | **Missing entirely** — needs the tool surface + agent | Platform doc P10 (8 MCP intelligence tools, 0 exist today) + P11 (LangGraph agent, not started) |
| Cross-period linkage (Jan miss → Feb appearance) | **Missing** — no capability today links two periods' imports | Part of P7's evidence walk; needs a `Filing`-per-period model first |

**The honest scope statement**: the ontology richness is a pack-content task (TTL + connector normalizer changes, hours not weeks, and it is exactly the kind of change `plans/105b`'s Slice A–E work is agnostic to — a richer multi-hop SPARQL query runs through the identical rule evaluator being built there). The evidence chain and the agent are P7/P10/P11-sized platform work, already scoped in the intelligence platform doc, not invented by this document — this plan's contribution is showing *why GST specifically* is the right first pack to prove them against (per the platform doc's own words, "GST pack first — it exercises almost everything").

## Proposed sequencing (for review, not started)

1. **Ontology + fixture expansion** (pack content only): add `Company`/`Filing`/`CreditNote`/`DebitNote`/`Amendment`/`Itc` classes and the `issuedBy`/`appearsIn`/`reflectedIn`/`purchasedFrom`/`claims`/`supports`/`adjusts`/`supersedes` predicates to `packs/gst/ontology.ttl`; update `gstr2b.py`/its TS port to emit `Supplier` as a real subject with `issuedBy` edges instead of a literal GSTIN; rewrite the 6 existing queries to traverse through `Supplier`/`Filing` rather than joining invoice-to-invoice directly. **No Rust change.** This can start any time, independent of P5b.
2. **Evidence-chain walk (P7)**: the Rust traversal that answers "what supports this finding, and what's missing." This is the platform doc's own next-unbuilt primitive after findings (P5, done) and fusion (P6, not started but not blocking GST specifically).
3. **MCP intelligence tool surface (P10)** + **the agent (P11)**: only once P7 exists to give the agent something to traverse. This is where "why is my ITC lower this month" becomes answerable.

Steps 2–3 are large enough that they should go through `story-splitting`/`grill-me` before any implementation plan is written, exactly as `plans/105b`'s own AskUserQuestion flagged. This document stops at scoping; it does not commit to slices.

## What this document deliberately does not do

- It does not touch `plans/105b-native-reconcile-engine.md`'s in-flight Slices B–E. The native rule evaluator being built there is the substrate either model runs through — a 1-hop join and a 3-hop traversal are both just SPARQL to that evaluator.
- It does not propose new Rust crates. Ontology richness is pack data; the evidence chain and agent land in already-named crates/layers per the platform doc's existing crate placement rules.
- It does not commit to a timeline. The honest sizing is: ontology expansion is small and can happen any time; the evidence chain and agent are multi-week platform work that should be scoped properly, not estimated here.
