Exactly. **This changes the UI design significantly.**

If GraphOWL is the engine behind Reco Now, then Reco Now should **not merely call GraphOWL in the background to get reconciliation results**.

The UI should be designed around the capabilities GraphOWL gives it.

The principle I'd use is:

> **Reco Now is the workflow lens over GraphOWL.**
>
> The user sees GST concepts and actions; underneath, every important screen is powered by GraphOWL's graph, evidence, provenance, reasoning, temporal and entity-resolution capabilities.

So I would revise the UI I proposed earlier into the following architecture.

---

# 1. The fundamental architecture

Think of it as three layers:

```text
                         RECO NOW UI
                    GST user's experience
                              │
                              │
                  ┌───────────▼───────────┐
                  │   Reco Application    │
                  │                       │
                  │ Workflow / Actions    │
                  │ GST-specific views    │
                  │ Review / Follow-up    │
                  └───────────┬───────────┘
                              │
                         GraphOWL API
                              │
                  ┌───────────▼───────────┐
                  │       GRAPHOWL        │
                  │                       │
                  │ Graph                 │
                  │ Traversal             │
                  │ Evidence              │
                  │ Provenance            │
                  │ Reasoning             │
                  │ Entity Resolution     │
                  │ Temporal              │
                  │ Findings              │
                  │ Ontology               │
                  └───────────┬───────────┘
                              │
                  ┌───────────▼───────────┐
                  │ GST Knowledge Pack    │
                  │                       │
                  │ Books                 │
                  │ GSTR-1               │
                  │ GSTR-2B              │
                  │ IMS                  │
                  │ GST rules             │
                  │ Filing periods        │
                  │ Goods receipt         │
                  └───────────────────────┘
```

The important thing:

**Reco does not reproduce GraphOWL functionality.**

It **composes GraphOWL functionality into GST workflows.**

---

# 2. Think of every Reco screen as a "GraphOWL-powered view"

For every feature, ask:

> What GraphOWL capability makes this screen intelligent?

For example:

| Reco screen      | GraphOWL capability underneath         |
| ---------------- | -------------------------------------- |
| Reconciliation   | Graph findings + matching              |
| Exception        | Finding + evidence graph               |
| Invoice          | Entity + relationships                 |
| Supplier         | Entity resolution + graph neighborhood |
| Cross-period     | Temporal graph                         |
| Why mismatch?    | Reasoning + provenance                 |
| ITC exposure     | Graph aggregation                      |
| Supplier risk    | Graph analytics                        |
| IMS              | Event/state relationships              |
| Period close     | Temporal graph + findings              |
| Duplicate        | Entity resolution                      |
| Follow-up        | Graph-derived case context             |
| AI investigation | Graph traversal + evidence             |
| Graph view       | Direct GraphOWL deep link              |

This is the key design philosophy.

---

# 3. Reco homepage should already expose GraphOWL intelligence

Instead of:

```text
Matched: 93%
Mismatch: 7%
```

make the dashboard:

```text
                    AUGUST 2026
                 GST Reconciliation

        ┌────────────┬────────────┬────────────┐
        │ Books      │ GSTR-1     │ GSTR-2B    │
        │ 12,482     │ 12,306     │ 12,197     │
        └────────────┴────────────┴────────────┘

                    RECONCILIATION

              11,634       93.2%
               matched

     ┌────────────────────────────────────────┐
     │ EXCEPTIONS                              │
     │                                        │
     │ 263  Only in Books                     │
     │ 125  Only in 2B                        │
     │ 181  Amount mismatch                   │
     │  47  Cross-period                      │
     │  18  Possible duplicate                │
     │  31  Eligibility review                │
     └────────────────────────────────────────┘

                    GRAPH INSIGHTS

     47 invoices appear to be cross-period
     31 may have GSTIN identity variations
     18 possible duplicate entities
     9 suppliers account for 71% of exposure

                    ACTION REQUIRED

     ₹4.2L ITC currently requires review

     [Investigate] [Review exceptions]
```

Those insights are not dashboard calculations invented by Reco.

They're **GraphOWL-derived interpretations of the graph**.

---

# 4. The most important change: exceptions become graph-backed cases

This is probably the biggest UI change I'd make.

Don't have a row like:

```text
INV-1025 | XYZ Ltd | ₹38,000 | Mismatch
```

Instead:

```text
INV-1025

XYZ Pvt Ltd
₹38,000

⚠ Amount mismatch

Why?
────────────────────────

GSTIN             ✓
Invoice number    ✓
Invoice date      ✓
Taxable value     ✓
IGST              ✕

Books             ₹38,000
GSTR-1            ₹37,500
GSTR-2B           ₹37,500

[Investigate]
```

And then:

```text
GraphOWL evidence available

3 source records
7 supporting relationships
2 relevant findings

[View evidence]
[Open in GraphOWL]
```

Now the UI is **advertising the GraphOWL capability** without turning Reco into GraphOWL.

---

# 5. Every exception should have a "Why?"

This should become a standard Reco interaction.

```text
WHY?
```

Click it.

The drawer:

```text
WHY WAS THIS FLAGGED?

Finding
AmountMismatch

─────────────────────────────

MATCHING
✓ Supplier GSTIN
✓ Invoice number
✓ Invoice date
✓ Taxable value

DIFFERENCE
✕ IGST

Books       ₹38,000
GSTR-2B     ₹37,500

─────────────────────────────

EVIDENCE

Books
 └── Invoice INV-1025

GSTR-1
 └── Supplier declaration

GSTR-2B
 └── Generated record

─────────────────────────────

GOVERNANCE

Rule
AmountMismatch

[View rule]
[View evidence graph]
```

That `View evidence graph` button is where GraphOWL comes alive.

---

# 6. The evidence drawer should actually be a mini GraphOWL experience

Don't just show:

```text
Source: GSTR-2B
```

Show:

```text
EVIDENCE

                     INV-1025
                         │
             ┌───────────┼────────────┐
             │           │            │
           Books       GSTR-1       GSTR-2B
             │           │            │
          ₹38,000     ₹37,500      ₹37,500
```

Then:

```text
Sources
──────────────
Purchase Register
GSTR-1
GSTR-2B

Provenance
──────────────
Imported 16 Aug
Source document: ...
```

The user gets a **small semantic graph**, but doesn't need to understand GraphOWL.

---

# 7. "Open in GraphOWL" should be the escape hatch

This is crucial.

Reco should have:

```text
[View evidence]
```

for normal users.

And:

```text
[Open full investigation in GraphOWL ↗]
```

for advanced users.

Then GraphOWL opens:

```text
Invoice
  │
  ├── Supplier
  ├── Books
  ├── GSTR-1
  ├── GSTR-2B
  ├── FilingPeriod
  ├── Evidence
  └── Findings
```

So:

```text
Reco
 ↓
business investigation
 ↓
GraphOWL
 ↓
semantic investigation
```

That's the boundary I would preserve.

---

# 8. Supplier page should be graph-powered

The supplier page shouldn't just be:

```text
Supplier
GSTIN
Invoices
Amount
```

It should contain a **Supplier Intelligence** section.

```text
XYZ Pvt Ltd
GSTIN: 29XXXXXX

────────────────────────────────

RECONCILIATION

382 invoices
344 matched
18 mismatched
12 missing in 2B
8 cross-period

ITC exposure
₹4.82L

────────────────────────────────

SUPPLIER BEHAVIOR

Late filing             High
Amount discrepancies    Medium
Cross-period            High
GSTIN variations        Low

────────────────────────────────

GRAPH RELATIONSHIPS

Invoices              382
GSTR-1 filings          9
GSTR-2B records       371
Periods affected        4

[Explore supplier graph]
```

GraphOWL makes those relationships natural.

---

# 9. Supplier graph should not be a separate "graph feature"

This is subtle.

Don't make:

```text
Supplier
Overview | Graph
```

and hide everything interesting behind Graph.

Instead:

```text
Supplier

Overview

Performance
────────────────────
Late filing: 31%
Mismatch rate: 7%

Graph-derived patterns
────────────────────
8 invoices carried forward
3 GSTIN variations
2 repeated amount discrepancies

Evidence
────────────────────
...

[Explore full graph]
```

The graph **feeds the normal UI**.

That's what "GraphOWL is the engine" should mean.

---

# 10. Cross-period should be powered by temporal GraphOWL

The current GraphOWL GST model has explicitly moved toward `FilingPeriod` and period-aware reasoning.

So don't build cross-period as a custom Reco rule.

UI:

```text
INV-1025

JULY
Books
₹38,000

      ↓

AUGUST
GSTR-1
₹38,000

      ↓

AUGUST
GSTR-2B
₹38,000
```

Then:

```text
Graph interpretation

Invoice date: July 28
Supplier filing: August
2B appearance: August

Classification:
CROSS-PERIOD

[Why?]
```

The UI is effectively exposing GraphOWL's temporal reasoning.

---

# 11. The ITC page should be graph-derived

Instead of simply aggregating database rows:

```text
ITC at risk = sum(mismatches)
```

GraphOWL can understand why the exposure exists.

Example:

```text
ITC AT RISK
₹4.2L

WHY?

Supplier non-filing              ₹1.1L
Cross-period                     ₹0.9L
Amount discrepancy               ₹0.8L
GSTIN identity issue             ₹0.6L
Goods receipt timing             ₹0.5L
Other                             ₹0.3L
```

Click:

```text
Goods receipt timing
```

and GraphOWL provides:

```text
Invoice
  ↓
2B period
  ↓
GoodsReceipt event
  ↓
Receipt date
  ↓
Eligibility finding
```

This is exactly where your graph engine gives Reco something a conventional relational reconciliation UI struggles to express cleanly.

---

# 12. The "Why?" experience should be universal

I'd make this a design system component:

```text
<WhyButton />
```

It can appear everywhere.

### Dashboard

```text
₹4.2L at risk
[Why?]
```

### Supplier

```text
High cross-period risk
[Why?]
```

### Invoice

```text
Mismatch
[Why?]
```

### Period

```text
Period cannot be closed
[Why?]
```

### ITC

```text
₹900K unavailable
[Why?]
```

And every time:

```text
Why?
 ↓
GraphOWL evidence / finding / reasoning
```

That could become one of Reco's signature UX patterns.

---

# 13. "Explain" should be different from "Why?"

I'd use:

### Why?

Deterministic evidence.

```text
Why was this flagged?
```

### Explain

LLM interpretation.

```text
Explain this to me.
```

Example:

```text
WHY?

AmountMismatch
Books ₹38K
2B ₹37.5K

EXPLAIN

"The invoice itself appears to be the same across
Books and GSTR-2B. The discrepancy is isolated to
the IGST amount. The supplier's GSTR-1 and the
generated 2B both report ₹37,500, suggesting the
difference originates in the supplier declaration
rather than invoice identity."
```

That's a beautiful separation:

```text
GraphOWL → Why
LLM      → Explain
```

---

# 14. "Investigate" is different again

Then:

```text
[Investigate]
```

means:

> Agent, go beyond the current case and find out what is happening.

For example:

```text
Investigate INV-1025
```

Agent might search:

```text
Current period
Previous period
Next period
Supplier history
GSTIN variations
Invoice variations
Related documents
Evidence graph
```

Result:

```text
INV-1025

Initial finding:
AmountMismatch

Additional investigation:
Supplier has 7 similar mismatches this quarter.

5 of them were corrected in subsequent filings.

Likely pattern:
Supplier reporting inconsistency.

[View supplier pattern]
[Create follow-up]
```

So the UX becomes:

```text
WHY       → GraphOWL
EXPLAIN   → LLM
INVESTIGATE → Agent
```

That is an excellent product model.

---

# 15. "Resolve" should remain deterministic / human-controlled

Then:

```text
[Resolve]
```

should NOT be an LLM button.

It opens:

```text
Resolution

Classification
[Supplier correction expected ▾]

Comment
...

Evidence reviewed
✓

[Resolve case]
```

The agent can **recommend**:

```text
Suggested:
Supplier correction expected

Reason:
8 similar historical cases resolved this way.
```

But human confirms.

---

# 16. Your UI therefore gets a four-level interaction model

This is what I would standardize across Reco:

```text
┌─────────────────────────────────────────────────┐
│                                                 │
│  WHY?       EXPLAIN       INVESTIGATE       ACT │
│   │            │               │              │ │
│   ▼            ▼               ▼              ▼ │
│ GraphOWL      LLM            Agent          Human│
│ Evidence    Explanation    Investigation    Action│
│                                                 │
└─────────────────────────────────────────────────┘
```

This is much cleaner than trying to make the agent do everything.

---

# 17. The reconciliation table should surface GraphOWL intelligence

Current-style table:

```text
Invoice | Supplier | Books | 2B | Status
```

Better:

```text
Invoice    Supplier       ITC       Status       Intelligence
─────────────────────────────────────────────────────────────
1025       XYZ Ltd        ₹38K      Mismatch     Why available
1026       ABC Ltd        ₹92K      Missing      Cross-period likely
1027       PQR Ltd        ₹1.2L     Missing      Supplier filed
1028       LMN Ltd        ₹48K      Review       GSTIN variation
```

The last column might show:

```text
🧠 Cross-period
🔎 Evidence
⚠ Identity
```

Clicking it opens the corresponding GraphOWL-backed insight.

---

# 18. GraphOWL findings should become Reco "reason codes"

The GST pack already has first-class findings such as `PotentialMismatch`, `AmountMismatch`, `ITCNotAvailable`, `Reversed`, `GstinTransposition`, and `PaymentOverdue`, with queries, evidence and governing citations.

Reco should **not recreate those rules**.

Instead:

```text
GraphOWL Finding
       ↓
Reco reason code
       ↓
Reco UX
```

For example:

```text
gst:AmountMismatch
        ↓
"Amount mismatch"
        ↓
Exception UI
```

```text
gst:GstinTransposition
        ↓
"Possible GSTIN transposition"
        ↓
Identity warning UI
```

```text
gst:PaymentOverdue
        ↓
"Payment timing issue"
        ↓
Follow-up UI
```

That is the right architecture.

---

# 19. GraphOWL's ontology should also inform the UI

Reco shouldn't hard-code every GST object forever.

The GST pack already has domain vocabulary.

The UI can understand:

```text
Invoice
Supplier
PurchaseEvent
PaymentEvent
GoodsReceipt
Gstr1Invoice
Gstr2bInvoice
FilingPeriod
Finding
Evidence
```

Then Reco can render:

```text
Invoice
 ├── issuedBy → Supplier
 ├── belongsTo → FilingPeriod
 ├── reportedIn → GSTR-1
 ├── appearsIn → GSTR-2B
 ├── receivedVia → GoodsReceipt
 └── paidVia → PaymentEvent
```

That's an extremely strong long-term architecture.

---

# 20. This means Reco becomes thinner over time

Initially:

```text
Reco frontend
    │
    ├── matching logic
    ├── period logic
    ├── supplier logic
    ├── evidence logic
    └── GST logic
```

You don't want this.

Eventually:

```text
Reco frontend
    │
    ├── GST workflow
    ├── GST presentation
    ├── actions
    └── UX state

GraphOWL
    │
    ├── graph
    ├── findings
    ├── evidence
    ├── reasoning
    ├── identity
    ├── temporal
    └── provenance
```

That is much cleaner.

---

# 21. The Reco UI should have a "semantic context bar"

This would be a nice GraphOWL-powered component.

At the top of a case:

```text
┌──────────────────────────────────────────────────────────┐
│ INV-1025                                                  │
│ XYZ Pvt Ltd                                               │
│                                                          │
│ Graph context                                            │
│ 3 sources · 8 relationships · 2 findings · 1 period     │
│                                                          │
│ [Why?] [Explain] [Investigate] [Open GraphOWL ↗]         │
└──────────────────────────────────────────────────────────┘
```

This tells the user:

> There is much more semantic information behind this case.

Without overwhelming them.

---

# 22. Agent should get the same GraphOWL context

When the agent investigates:

```text
Reco Case
    ↓
GraphOWL subject
    ↓
Agent
```

The agent should receive:

```text
Case ID
Invoice ID
Supplier ID
Filing period
Findings
Evidence references
Graph context
```

Not raw database dumps.

Then it can ask GraphOWL:

```text
find_path(invoice, supplier)
get_history(invoice)
find_cross_period(invoice)
get_evidence(invoice)
find_related_findings(invoice)
```

This makes the agent dramatically more reliable.

---

# 23. GraphOWL should provide "semantic APIs", not just database APIs

This is probably the most important engineering recommendation.

Don't make Reco call:

```text
GET /nodes
GET /edges
```

everywhere.

Expose higher-level capabilities:

```text
GET /reconciliation/{case}/explanation
GET /reconciliation/{case}/evidence
GET /reconciliation/{case}/context

GET /invoice/{id}/graph
GET /invoice/{id}/history
GET /invoice/{id}/related-findings

GET /supplier/{id}/risk-context
GET /supplier/{id}/reconciliation-history

GET /period/{id}/cross-period
GET /period/{id}/itc-context
```

Underneath, GraphOWL uses:

```text
Traversal
SPARQL
Ontology
Reasoning
Named graphs
Evidence
Temporal queries
Entity resolution
```

Reco doesn't need to know how.

---

# 24. This is also where MCP becomes interesting

Eventually:

```text
Reco Agent
    │
    ▼
GraphOWL MCP
    │
    ├── search_entity
    ├── get_evidence
    ├── find_path
    ├── get_history
    ├── get_findings
    ├── explain_relationship
    └── query_graph
```

Then your agent architecture becomes:

```text
             RECO AGENT
                  │
          ┌───────┴────────┐
          │                │
       Reco MCP        GraphOWL MCP
          │                │
    workflow tools     semantic tools
```

That is a very clean separation.

---

# 25. The final Reco UI becomes this

```text
                         RECO NOW
                   GST Reconciliation
                           │
       ┌───────────────────┼────────────────────┐
       │                   │                    │
   RECONCILE           INVESTIGATE            ACT
       │                   │                    │
   Dashboard             Why?                 Resolve
   Register              Explain              Follow-up
   Exceptions            Evidence             IMS
   ITC                   Graph                Close
   Periods               History
       │                   │
       └───────────────────┼────────────────────┘
                           │
                     GRAPHOWL ENGINE
                           │
      ┌────────────┬───────┼────────┬────────────┐
      │            │       │        │            │
    Graph       Findings Evidence Temporal    Reasoning
      │            │       │        │            │
      └────────────┴───────┼────────┴────────────┘
                           │
                      GST PACK
```

---

# 26. And the user experience becomes beautifully simple

The accountant sees:

```text
INV-1025
⚠ Amount mismatch

₹38,000 vs ₹37,500

[Why?] [Explain] [Investigate] [Resolve]
```

Then:

### Why?

```text
GraphOWL
→ exact facts
→ findings
→ evidence
```

### Explain?

```text
LLM
→ human-readable explanation
```

### Investigate?

```text
Agent
→ searches GraphOWL
→ checks related periods
→ checks supplier history
→ produces investigation
```

### Resolve?

```text
Human
→ accepts / rejects / follows up
```

That is the architecture I would build around.

---

# 27. Most importantly: don't make "Graph" a Reco navigation item

I would **not** have:

```text
Reco Now

Dashboard
Reconciliation
Exceptions
Suppliers
Graph
Settings
```

Instead:

```text
Reco Now

Dashboard
Reconciliation
Exceptions
Suppliers
ITC
IMS
Periods
Follow-ups
```

And inside almost every important object:

```text
[View evidence]
[Explain]
[Investigate]
[Open in GraphOWL]
```

Because **GraphOWL is not a Reco feature**.

It is the **semantic substrate powering Reco**.

That distinction will keep the two products clean as they evolve.

---

## The architecture in one sentence

**Reco Now should own the GST user's workflow; GraphOWL should own everything that makes that workflow semantically intelligent; the LLM should explain it; the agent should investigate it; and the human should remain in control of consequential actions.**

That is the direction I'd take rather than trying to make the current Reco UI simply "look better."
