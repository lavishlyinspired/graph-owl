review this plan first and understand and  then implement this GraphOWL is much further along than the architecture I described.
  
  Your repo already contains several of the exact primitives I recommended.
  
  The most important conclusion
  
  You should not redesign GraphOWL into the architecture I described.
  
  Instead, you should finish and strengthen the GST-specific layer on top of the architecture you already have.
  
  Your current direction is actually very close to the right architecture:
  
                      GraphOWL
                         │
               ┌─────────┴─────────┐
               │                   │
          Platform Core        GST Pack
               │                   │
        ┌──────┼───────┐      ┌────┼────────┐
        │      │       │      │    │        │
      Graph  Query  Traversal Ontology Rules Connectors
        │      │       │      │    │        │
        └──────┴───────┘      └────┼────────┘
                                    │
                              Reconciliation
                                    │
                              Findings/Evidence
                                    │
                                    ▼
                                 Agent
  
  Your repo already has the beginning of this.
  
  1. What GraphOWL already has
  
  Your current repo includes:
  
  connectors/python/graph_owl_packs/
      gstr2b.py
      erpnext.py
      reconcile.py
      loader.py
      manifest.py
  
  and a substantial Rust platform under crates/.
  
  That is important because your architecture already separates:
  
  platform capability from pack-specific GST logic.
  
  That is exactly what you need for GST → healthcare → finance later.
  
  2. Your GSTR-2B ingestion is already designed correctly
  
  Your gstr2b.py is actually much better architected than a conventional GST application.
  
  It explicitly treats the GSTN/GSP payload as evidence, normalizes it into your GST ontology, and keeps the normalizer pure so a future GSP/fetcher can be swapped without changing downstream logic.
  
  This is exactly the architectural separation I was recommending:
  
  GSP/API
     ↓
  fetch
     ↓
  normalize
     ↓
  GraphOWL vocabulary
  
  rather than:
  
  GST API
     ↓
  GST-specific application logic
  
  Your connector even protects against a particularly dangerous failure mode: treating an API failure as an empty GSTR-2B and thereby generating false "missing" findings.
  
  That's excellent.
  
  3. You already have deterministic reconciliation
  
  This is another correction to my previous answer.
  
  Your reconcile.py no longer performs reconciliation itself. It delegates evaluation to the native GraphOWL server:
  
  POST /packs/{pack}/reconcile
  
  The Python connector is now orchestration only.
  
  That is exactly what you want.
  
  Your repo's own design explicitly says the deterministic graph intelligence belongs in Rust, with Python orchestrating.
  
  So your architecture is already:
  
  Python
     = connector / orchestration
  
  
  Rust
     = graph intelligence / reconciliation
  
  That is stronger than the simplistic architecture I initially described.
  
  4. And your GST reconciliation has already moved beyond two-table matching
  
  This is the most important discovery.
  
  Your 108-books-gstr1-gstr2b-reconciliation.md shows that you already implemented a three-source reconciliation model:
  
  Books
  GSTR-1
  GSTR-2B
  
  rather than merely:
  
  Books ↔ GSTR-2B
  
  The plan says slices 1–5, 7 and 8 have shipped, and the Reconciliation route already supports three uploads plus a reconciliation statement and grouped exceptions.
  
  That is much closer to the architecture I recommended.
  
  5. You already have the right "findings" model
  
  Your repo has moved toward:
  
  [[findings]]
  
  where a finding has:
  
  label
  summary
  governed_by
  query
  evidence
  
  and the loader is generic.
  
  That means GST findings aren't hard-coded into Rust or Python.
  
  This is a huge architectural advantage.
  
  Your own planning notes say that the six existing finding rules are pack declarations, with citations and first-class SPARQL queries.
  
  So the model is already:
  
  GST rule
     ↓
  SPARQL
     ↓
  Finding
     ↓
  Evidence
  
  rather than:
  
  if gst_case_x:
      ...
  
  That's the correct direction for GraphOWL.
  
  6. The evidence graph is also already there
  
  This is where my previous answer significantly overlapped with things you've already implemented.
  
  Your repo already has:
  
  GET /findings/{id}/evidence-graph
  
  and the traversal engine walks outward from the finding subject.
  
  You also have:
  
  Catalog::node_sources
  Catalog::near_miss_node
  
  for identifying source provenance and near-match candidates.
  
  So you already have the beginnings of:
  
  Finding
     ↓
  Invoice
     ↓
  Supplier
     ↓
  Source
     ↓
  Evidence
  
  This means the real problem is not "build an evidence graph."
  
  The real problem is:
  
  Make the evidence graph rich enough and agent-accessible enough to perform actual investigations.
  
  7. You also already made Supplier a real graph entity
  
  Your 105c-gst-causal-graph.md captures an important architectural change.
  
  Originally:
  
  Invoice
    └── supplierGstin = "27..."
  
  You moved toward:
  
  Invoice
      │
      └── issuedBy
              ↓
           Supplier
  
  That is exactly what you need.
  
  The plan explicitly records that this was shipped and that the GSTIN is now represented through a real gst:Supplier node and gst:issuedBy relationship.
  
  This is the point at which GST starts becoming a real graph rather than a pair of RDF-shaped tables.
  
  8. Where your architecture is still incomplete
  
  This is where I would focus.
  
  Your own 105c document actually identifies the biggest gap:
  
  GST has become graph-shaped, but the agent and evidence-chain investigation capability are still missing.
  
  The plan explicitly marks:
  
  Multi-hop traversal        EXISTS
  SPARQL multi-hop            EXISTS
  Temporal rule resolution    EXISTS
  Evidence-chain walk         MISSING
  Agentic investigation       MISSING
  Cross-period linkage        MISSING
  
  That is exactly the next phase.
  
  9. So the GST architecture for YOUR repo should actually be
  
  Not my earlier generic architecture.
  
  This:
  
                      ┌─────────────────────────┐
                      │      GraphOWL Core       │
                      │                         │
                      │ Graph / RDF / SPARQL    │
                      │ Traversal                │
                      │ Rules                    │
                      │ Provenance               │
                      │ Policy                   │
                      │ MCP                      │
                      └────────────┬────────────┘
                                   │
                   ┌───────────────┴──────────────┐
                   │                              │
           ┌───────▼────────┐            ┌────────▼────────┐
           │    GST Pack    │            │ Future Packs    │
           │                │            │ Healthcare etc.  │
           │ ontology.ttl   │            │ ontology         │
           │ mapping.yaml   │            │ rules            │
           │ findings       │            │ connectors       │
           │ connectors     │            │                  │
           └───────┬────────┘            └──────────────────┘
                   │
        ┌──────────┼─────────────┐
        │          │             │
      Books      GSTR-1       GSTR-2B
        │          │             │
        └──────────┼─────────────┘
                   ▼
          ┌─────────────────────┐
          │ Reconciliation      │
          │ Engine              │
          │                     │
          │ SPARQL + matching   │
          │ + native Rust       │
          └──────────┬──────────┘
                     ▼
                Findings
                     │
              ┌──────┴──────┐
              ▼             ▼
         Evidence       Governance/
          Graph          Rules
              │             │
              └──────┬──────┘
                     ▼
                MCP tools
                     │
                     ▼
               GST Agent
                     │
                     ▼
                   LLM
  
  That maps almost directly onto what your repo already contains.
  
  10. The agent should NOT perform reconciliation
  
  This becomes even clearer after seeing your actual repo.
  
  The correct model for GraphOWL is:
  
  Agent
    │
    ├── ask graph
    ├── inspect evidence
    ├── explain finding
    ├── traverse
    ├── run reconciliation
    └── retrieve rules
  
  not:
  
  Agent
    └── calculate whether invoice matches
  
  Your MCP implementation already reflects this philosophy.
  
  Your ContextSource exposes things such as:
  
  search
  lineage
  impact
  governance
  query_graph
  traverse
  find_evidence
  explain
  reconcile
  
  That is a very good foundation for the GST agent.
  
  11. Your MCP surface is actually the bridge to the agent
  
  This is where I would now concentrate.
  
  The repo already has the tools:
  
  traverse()
  find_evidence()
  explain()
  reconcile()
  
  and the broader MCP surface contains:
  
  search()
  query_graph()
  governance()
  lineage()
  impact()
  
  So you don't need a special:
  
  gst_reconciliation_agent.py
  
  Instead you need a generic GraphOWL investigation agent capable of using these tools.
  
  Then GST becomes the first pack it investigates.
  
  12. One significant problem I see: domain-subject access
  
  Your own MCP code contains an important warning.
  
  traverse() is currently restricted to catalog assets, not arbitrary pack-domain subjects like:
  
  gst:Invoice
  gst:Supplier
  gst:Gstr2bInvoice
  
  The comment explicitly says that pack-domain subjects don't yet have the same policy model and therefore traversal is deliberately blocked there.
  
  This is probably one of the most important remaining platform gaps.
  
  Because your ideal agent must eventually be able to do:
  
  Invoice INV-1042
         ↓
  Supplier
         ↓
  GSTR-1
         ↓
  Filing
         ↓
  GSTR-2B
         ↓
  GoodsReceipt
         ↓
  ITC finding
  
  without creating GST-specific traversal code.
  
  So I would solve this at the GraphOWL platform level, not GST.
  
  13. Your temporal model is also moving in the right direction
  
  Your plan already identifies FilingPeriod and the July → August carry-forward problem.
  
  You also caught a real implementation issue:
  
  GSTR-2B period ≠ invoice date
  
  Your connector currently needs to derive the graph period from the return period, not the invoice date.
  
  That is exactly the type of issue that makes your graph much better than a simple reconciliation database.
  
  You want:
  
  Invoice
    invoiceDate = July 7
  
  
  GSTR2B Filing
    returnPeriod = August
  
  
  Invoice
    appearsIn
      → August GSTR-2B
  
  rather than:
  
  Invoice.period = July
  14. Your strongest GST concept is actually causal investigation
  
  Your 105c title says it perfectly:
  
  GST as a causal graph, not a two-table join.
  
  That's the product direction I would keep.
  
  For example:
  
  Why is my July ITC lower?
  
  Agent:
  
  July ITC
   ↓
  ₹350 missing
   ↓
  Invoice INV-1002
   ↓
  Supplier ABC
   ↓
  Supplier filed GSTR-1 late
   ↓
  not present in July 2B
   ↓
  appeared in August 2B
  
  This requires the graph, evidence, temporal model and agent.
  
  A traditional reconciliation application cannot do this naturally.
  
  15. What I would change in the roadmap
  
  Based on the actual repo, I would now prioritize:
  
  P0 — Finish GST evidence model
  
  You already started this.
  
  Complete:
  
  Company
  Supplier
  Invoice
  Filing
  ReturnPeriod
  GSTR1
  GSTR2B
  ITC
  GoodsReceipt
  Payment
  
  and:
  
  issuedBy
  appearsIn
  reflectedIn
  claims
  supports
  adjusts
  supersedes
  
  Your 105c already proposes most of this.
  
  P1 — Evidence-chain traversal
  
  This is the most important missing platform primitive.
  
  Something conceptually like:
  
  investigate(finding)
  
  returning:
  
  Finding
   ├── subject
   ├── evidence
   ├── source
   ├── provenance
   ├── missing-hop
   ├── temporal context
   └── governing rule
  P2 — Make pack-domain graph traversal agent-safe
  
  Generalize your policy system so:
  
  traverse(gst:Invoice)
  
  is supported without GST-specific Rust.
  
  P3 — Build generic GraphOWL investigation agent
  
  Not:
  
  GSTAgent
  
  but:
  
  GraphOWL Investigator
  
  whose capabilities are:
  
  search
  query
  traverse
  find_evidence
  explain
  reconcile
  governance
  P4 — GST-specific agent skills
  
  Then configure GST prompts/skills around:
  
  reconciliation
  ITC investigation
  supplier investigation
  period investigation
  mismatch explanation
  GST-rule explanation
  
  The agent itself remains domain-neutral.
  
  16. There is one architectural distinction I would change from your current thinking
  
  You currently have:
  
  Finding
     ↓
  Evidence Graph
  
  which is good.
  
  But eventually I think you want:
  
  Finding
     ↓
  Investigation
     ↓
  Evidence Graph
     ↓
  Reasoning Trace
     ↓
  Conclusion
  
  Because an agent may have to perform multiple steps:
  
  Step 1
  find invoice
  
  
  Step 2
  find supplier
  
  
  Step 3
  find supplier filing
  
  
  Step 4
  find relevant 2B
  
  
  Step 5
  compare periods
  
  
  Step 6
  check goods receipt
  
  
  Step 7
  retrieve rule
  
  
  Step 8
  form conclusion
  
  The investigation itself becomes a first-class auditable object.
  
  That would fit GraphOWL exceptionally well.
  
  17. Comparison: my generic proposal vs your actual GraphOWL
  Capability	Generic architecture I proposed	Your GraphOWL
  File/API ingestion	Needed	Already exists
  GSTR-2B normalizer	Needed	Already exists
  Canonical GST vocabulary	Needed	Already exists
  GST ontology	Needed	Already exists and evolving
  Named source graphs	Needed	Already exists
  Deterministic reconciliation	Needed	Already native Rust
  Findings	Needed	Already pack-configured
  SPARQL rules	Needed	Already exists
  Governance citations	Needed	Already exists
  Supplier graph	Needed	Already implemented
  Evidence graph	Needed	Already exists
  Multi-hop traversal	Needed	Already exists
  MCP	Needed	Already exists
  Agent	Needed	Main remaining gap
  Rich domain-subject agent traversal	Needed	Platform gap
  Cross-period investigation	Needed	Partially modeled / gap remains
  Investigation trace	Recommended	Worth adding
  
  So I would not rebuild the ingestion/reconciliation architecture.
  
  18. The real GraphOWL GST target
  
  I now see the product more clearly from the repository:
  
                       GRAPHOWL
                           │
                  "Investigate Data"
                           │
           ┌───────────────┴────────────────┐
           │                                │
        Platform                         Domain Pack
           │                                │
           │                              GST
           │                                │
           │                ┌───────────────┼───────────────┐
           │                │               │               │
           │              Books           GSTR1           GSTR2B
           │                │               │               │
           │                └───────────────┼───────────────┘
           │                                │
           │                         reconciliation
           │                                │
           │                             Finding
           │                                │
           │                          Evidence Graph
           │                                │
           └───────────────────┬────────────┘
                               │
                          Investigation
                               │
                        Generic Agent
                               │
                               ▼
                             USER
  
  That's much more compelling than simply "GST reconciliation software."
  
  My recommendation
  
  Do not spend your next effort rebuilding the GST ingestion or reconciliation engine.
  
  Your next major investment should be:
  
  P7 evidence-chain investigation → domain-subject traversal/policy → P10 MCP intelligence completion → P11 generic GraphOWL agent.
  
  Then GST becomes the showcase domain that proves the platform can answer questions such as:
  
  "Why is my July ITC ₹2.3 lakh lower?"
  
  "Which suppliers are responsible?"
  
  "Which invoices moved from July to August 2B?"
  
  "Show me the evidence for the ₹350 exception."
  
  "Is this missing ITC a supplier filing problem, a GSTIN problem, a period problem, or a goods-receipt problem?"
  
  And crucially, the agent answers those questions by investigating the graph, rather than pretending to be the reconciliation engine.
  
  Your repository is already architected much closer to that end state than my first answer assumed.
  
  but i dont think that the ontology is correct in the pack and also the DOCUMENT EXTRACTION    │ and   CANONICAL GST MODEL  is correctly done
  
  Yes — I think your concern is correct, and after looking at the repo plus the actual GST source structures, I would change my previous assessment.
  
  The problem is deeper than "add a few more GST classes."
  
  The fundamental issue
  
  Your current GraphOWL GST implementation is doing roughly this:
  
  SOURCE DOCUMENT
        ↓
  source-specific parser
        ↓
  GST RDF vocabulary
        ↓
  reconciliation rules
        ↓
  finding
  
  What you actually need is:
  
  SOURCE
    ↓
  DOCUMENT UNDERSTANDING
    ↓
  EXTRACTED FACTS + PROVENANCE
    ↓
  SOURCE DATA MODEL
    ↓
  ONTOLOGY ALIGNMENT
    ↓
  CANONICAL GST MODEL
    ↓
  ENTITY RESOLUTION
    ↓
  GRAPH
    ↓
  RECONCILIATION / REASONING
    ↓
  EVIDENCE
    ↓
  AGENT
  
  And those middle layers are currently collapsed together in GraphOWL.
  
  That is the architectural issue I would fix before investing heavily in the agent.
  
  1. Your gstr2b.py is a connector, not a document-extraction architecture
  
  Your current GSTR-2B connector is actually well engineered as a structured API normalizer.
  
  It takes a known GSTR-2B JSON structure and converts it directly into GraphOWL's GST vocabulary.
  
  But that is not the same thing as a general ingestion architecture.
  
  You need to distinguish:
  
  Structured source
  GSTR-2B JSON
  GSTR-1 JSON
  ERP API
  
  from:
  
  Semi/unstructured source
  Purchase Register.xlsx
  Purchase Register.csv
  Invoice PDF
  Scanned Invoice
  Email attachment
  E-invoice JSON
  E-way bill
  GST portal Excel
  
  A GST reconciliation product must be able to ingest all of these.
  
  And the extraction layer should not know that the destination is GST reconciliation.
  
  2. The current Books importer is a warning sign
  
  Your repo now has:
  
  ui/src/features/packs/books.ts
  
  for CSV/TSV purchase-register ingestion.
  
  And GSTR-1 has:
  
  ui/src/features/packs/gstr1.ts
  
  This is convenient for the current demo, but architecturally it is backwards.
  
  The UI should not fundamentally be responsible for understanding:
  
  "This spreadsheet is a GST purchase register."
  
  Instead:
  
  Upload
    ↓
  Document profiling
    ↓
  Document type detection
    ↓
  Schema/header detection
    ↓
  Extraction
    ↓
  Canonicalization
  
  should happen before the GST pack gets involved.
  
  3. You need a proper DOCUMENT layer
  
  I would introduce this conceptual layer:
  
  Document
  ├── DocumentIdentity
  ├── DocumentType
  ├── Source
  ├── Format
  ├── ExtractionRun
  ├── ExtractedField
  ├── ExtractedTable
  ├── ExtractedRow
  ├── ExtractedCell
  └── Provenance
  
  For example:
  
  purchase_register_july.xlsx
          │
          ├── format = XLSX
          ├── documentType = PurchaseRegister
          ├── source = ERP export
          │
          └── ExtractionRun
                │
                ├── sheet = Purchases
                ├── header row = 4
                ├── rows = 12,450
                └── extraction confidence = 0.99
  
  Then:
  
  ExtractedRow #1821
   ├── "Vendor GSTIN" → 27ABCDE...
   ├── "Invoice No" → INV/2026/102
   ├── "Invoice Date" → 2026-07-07
   ├── "Taxable Value" → ₹10,000
   └── "IGST" → ₹1,800
  
  Only after that should GST semantics be applied.
  
  4. More importantly: extraction must retain provenance
  
  This is extremely important for your agent.
  
  Don't turn:
  
  Excel cell D1821
  
  directly into:
  
  gst:invoiceNumber "INV-102"
  
  and throw away the source.
  
  Instead:
  
  ExtractedField
      │
      ├── value = "INV/2026/102"
      ├── sourceDocument = purchase-register.xlsx
      ├── sheet = Purchases
      ├── row = 1821
      ├── column = Invoice No
      └── extractionConfidence = 0.997
  
  Then alignment says:
  
  ExtractedField
         │
         │ mappedTo
         ▼
  gst:Invoice.invoiceNumber
  
  Now the agent can answer:
  
  "Where did you get this invoice number?"
  
  with:
  
  Purchase Register → Purchases sheet → row 1821 → Invoice No column.
  
  That's the kind of evidence-native graph GraphOWL should be building.
  
  5. The current GST ontology is also too source-oriented
  
  This is the bigger issue with packs/gst/ontology.ttl.
  
  The current model has things like:
  
  PurchaseInvoice
  Gstr2bInvoice
  
  That is a smell.
  
  Why?
  
  Because:
  
  an invoice is not inherently a "purchase invoice" or a "GSTR-2B invoice".
  
  Those describe the role/context in which a source reports an invoice.
  
  The underlying business object is:
  
  Invoice
  
  Then:
  
  Invoice
     │
     ├── reportedBy → Supplier
     │
     ├── appearsIn → GSTR1Filing
     │
     ├── reflectedIn → GSTR2BFiling
     │
     └── recordedIn → PurchaseRegister
  
  This is exactly the direction your own 105c plan started moving toward, but then stopped short of completing.
  
  I agree with your instinct: the ontology should be redesigned before it becomes entrenched.
  
  6. Don't make GSTR-2B an Invoice class
  
  This is particularly important.
  
  GSTR-2B is a statement/document, not the invoice itself.
  
  Official GST guidance describes GSTR-2B as an auto-drafted ITC statement generated from supplier/ECO filings and other sources, and importantly it is static for a particular period.
  
  So conceptually:
  
  Invoice
     ↑
     │ contains/reflects
     │
  GSTR2BStatement
     │
     └── returnPeriod = 2026-07
  
  not:
  
  Gstr2bInvoice
  
  Similarly:
  
  GSTR1Return
     └── reports → Invoice
  
  and:
  
  PurchaseRegister
     └── records → Invoice
  
  The same business entity can therefore have multiple representations/evidence assertions.
  
  7. The canonical model should sit between sources and ontology
  
  This is where I think GraphOWL needs the biggest conceptual change.
  
  You currently have:
  
  GSTR2B
      ↓
  gst:Gstr2bInvoice
  
  Instead:
  
  GSTR2B
      ↓
  Extracted representation
      ↓
  Alignment
      ↓
  Canonical Invoice
  
  Likewise:
  
  Purchase Register
      ↓
  Extracted row
      ↓
  Alignment
      ↓
  Canonical Invoice
  
  and:
  
  GSTR-1
      ↓
  Extracted record
      ↓
  Alignment
      ↓
  Canonical Invoice
  
  Then GraphOWL gets:
  
                   Canonical Invoice
                    /       |       \
                   /        |        \
                  ↓         ↓         ↓
            Purchase     GSTR-1    GSTR-2B
             Register     Filing     Statement
  
  That is much more powerful.
  
  8. Canonical GST model should NOT be the same as ontology
  
  This distinction is crucial.
  
  You need three separate things:
  
  A. Source schema
  
  What the source actually contains.
  
  Example:
  
  GSTR-2B:
  ctin
  inum
  dt
  txval
  igst
  cgst
  sgst
  cess
  itcavl
  rev
  ...
  B. Canonical GST data model
  
  What GraphOWL needs operationally:
  
  Invoice
  Supplier
  Recipient
  Filing
  ReturnPeriod
  TaxComponent
  Supply
  CreditNote
  DebitNote
  ITC
  GoodsReceipt
  Payment
  C. GST ontology
  
  What those concepts mean and how they relate:
  
  Invoice
    subclassOf Document
  
  
  Supplier
    subclassOf TaxablePerson
  
  
  TaxComponent
    hasTaxType
    hasAmount
  
  
  Invoice
    issuedBy Supplier
    issuedTo Recipient
    hasTaxComponent TaxComponent
  
  The canonical model is optimized for data integration and computation.
  
  The ontology is optimized for meaning, relationships, constraints and reasoning.
  
  They shouldn't be conflated.
  
  9. And the canonical model needs a much richer invoice
  
  The current GSTR-2B normalizer captures things such as GSTIN, invoice number/date, taxable value and tax components.
  
  That's useful, but the canonical Invoice should be closer to:
  
  Invoice
  │
  ├── identity
  │   ├── documentNumber
  │   ├── documentType
  │   ├── invoiceDate
  │   └── IRN
  │
  ├── parties
  │   ├── supplier
  │   ├── recipient
  │   ├── dispatchFrom
  │   └── shipTo
  │
  ├── supply
  │   ├── supplyType
  │   ├── placeOfSupply
  │   ├── reverseCharge
  │   └── eCommerceOperator
  │
  ├── amounts
  │   ├── taxableValue
  │   ├── invoiceValue
  │   ├── discount
  │   └── rounding
  │
  ├── taxes
  │   ├── IGST
  │   ├── CGST
  │   ├── SGST
  │   ├── Cess
  │   └── StateCess
  │
  ├── lines
  │   ├── HSN/SAC
  │   ├── description
  │   ├── quantity
  │   ├── unit
  │   ├── rate
  │   └── lineTax
  │
  ├── lifecycle
  │   ├── issued
  │   ├── filed
  │   ├── amended
  │   ├── cancelled
  │   └── received
  │
  └── evidence
      ├── sourceDocument
      ├── sourceRecord
      └── extraction
  
  The official e-invoice schema demonstrates why the canonical model needs to be broader: it includes supplier/recipient identities, document type/number/date, POS, line items, HSN, quantity, taxable value, tax rates, tax components, invoice totals, references, payment information, transport information, and more.
  
  You don't need to put all ~130 fields into the ontology.
  
  But your canonical model must be capable of representing them.
  
  10. You also need Document, Filing and Assertion as first-class concepts
  
  I would make this distinction:
  
                       BUSINESS WORLD
                            │
                         Invoice
                            │
            ┌───────────────┼────────────────┐
            │               │                │
         ERP says        Supplier says     GST says
            │               │                │
            ▼               ▼                ▼
   Purchase Register    GSTR-1          GSTR-2B
            │               │                │
            └───────────────┼────────────────┘
                            │
                      Assertions
                            │
                            ▼
                    Canonical Graph
  
  This is much better than making each source representation a different invoice class.
  
  11. GSTR-1 itself is not "the invoice"
  
  GSTR-1 is a return/filing containing outward-supply declarations.
  
  The GST offline utility itself has multiple tables including B2B, B2BA, B2CL, etc.
  
  And e-invoice information can be auto-populated into different GSTR-1 tables, including B2B and credit/debit note tables.
  
  So your canonical model needs to preserve:
  
  Invoice
     │
     ├── declaredIn → GSTR1Filing
     └── reflectedIn → GSTR2BFiling
  
  rather than treating:
  
  Gstr1Invoice
  Gstr2bInvoice
  
  as two independent business entities.
  
  12. And you are currently missing important GST document types
  
  This is another reason I wouldn't freeze the current ontology.
  
  A serious GST reconciliation model needs at least:
  
  Invoice
  CreditNote
  DebitNote
  InvoiceAmendment
  CreditNoteAmendment
  DebitNoteAmendment
  
  The GST system explicitly handles these categories; the current IMS advisory, for example, distinguishes invoices, invoice amendments, debit notes, debit-note amendments, credit notes and credit-note amendments.
  
  And e-invoice itself covers invoices, credit notes and debit notes.
  
  So:
  
  Invoice
  
  shouldn't be the end of the model.
  
  13. There is another major problem: ITC is not an Invoice property
  
  This is extremely important for the agent.
  
  Don't model:
  
  Invoice
     └── itcAvailable = true
  
  as the ultimate business meaning.
  
  Instead:
  
  Recipient
     │
     └── claims
            ↓
           ITC
            │
            ├── supportedBy → Invoice
            ├── reflectedIn → GSTR2B
            ├── relatesTo → ReturnPeriod
            └── eligibilityAssessment → Assessment
  
  Because:
  
  invoice existence ≠ ITC eligibility ≠ ITC claim.
  
  Your own plan already recognized this when it separated GSTR-2B presence from the goods-receipt requirement under Section 16(2)(b).
  
  That should become a fundamental canonical-model principle.
  
  14. The GSTR-2B itcavl field is evidence, not truth
  
  This is another subtle but important point.
  
  The connector currently turns:
  
  itcavl
  
  into the graph.
  
  That's fine.
  
  But semantically this should mean:
  
  GSTR2BStatement
     └── reports
           └── ITCAvailabilityAssessment
  
  not:
  
  Invoice
     └── ITCEligible = true
  
  Because GraphOWL's job should be able to reason:
  
  GSTR-2B says available
  +
  goods received?
  +
  180-day payment?
  +
  blocked credit?
  +
  reverse charge?
  +
  other applicable rule?
  
  and then arrive at an eligibility assessment.
  
  This is precisely where GraphOWL becomes valuable.
  
  15. Document extraction should be domain-agnostic
  
  This is especially important given your broader GraphOWL goal.
  
  You don't want:
  
  GST PDF extractor
  GST Excel extractor
  GST JSON extractor
  
  You want:
  
                   GraphOWL Ingestion
                         │
         ┌───────────────┼────────────────┐
         ↓               ↓                ↓
      PDF/Scan        Excel/CSV         JSON/API
         │               │                │
         └───────────────┼────────────────┘
                         ↓
                 Document Understanding
                         ↓
                  Extracted Structure
                         ↓
                  Semantic Alignment
                         ↓
                Domain Canonical Model
  
  Then healthcare can do:
  
  Hospital PDF
   → extracted clinical facts
   → healthcare ontology
  
  and GST:
  
  Purchase register
   → extracted financial facts
   → GST ontology
  
  Same extraction platform. Different domain alignment.
  
  That's the GraphOWL architecture you have been trying to build.
  
  16. I would therefore change your GST architecture to this
  Layer 1 — Raw evidence
  RawDocument
  RawAPIResponse
  RawSpreadsheet
  RawPDF
  RawJSON
  
  Immutable.
  
  Layer 2 — Document understanding
  Document
  DocumentType
  Table
  Row
  Cell
  Field
  Page
  Section
  
  With provenance.
  
  Layer 3 — Source schema
  GSTR2BSchema
  GSTR1Schema
  PurchaseRegisterSchema
  EInvoiceSchema
  
  This describes what the source calls things.
  
  Layer 4 — Semantic alignment
  source field
        ↓
  candidate concept
        ↓
  canonical GST concept
        ↓
  confidence
        ↓
  mapping provenance
  
  For example:
  
  "Supplier GSTIN"
          ↓
  gst:Supplier.gstin
  
  
  "Vendor GST No"
          ↓
  gst:Supplier.gstin
  
  
  "CTIN"
          ↓
  gst:Supplier.gstin
  
  
  "SellerDtls.Gstin"
          ↓
  gst:Supplier.gstin
  
  This is where your ontology alignment capability becomes useful.
  
  Layer 5 — Canonical GST model
  Supplier
  Recipient
  Invoice
  InvoiceLine
  Tax
  TaxComponent
  Filing
  ReturnPeriod
  ITC
  CreditNote
  DebitNote
  GoodsReceipt
  Payment
  Layer 6 — GST ontology
  
  Defines:
  
  meaning
  relationships
  constraints
  hierarchies
  rules
  legal semantics
  Layer 7 — Graph
  
  Now instantiate canonical entities and their evidence.
  
  Layer 8 — Reconciliation
  entity resolution
  +
  matching
  +
  SPARQL
  +
  temporal reasoning
  +
  GST rules
  Layer 9 — Evidence
  Finding
   ↓
  Assertion
   ↓
  Source
   ↓
  Document
   ↓
  Page / sheet / row / cell
  Layer 10 — Agent
  
  Only now:
  
  User question
   ↓
  Agent
   ↓
  investigation plan
   ↓
  GraphOWL tools
   ↓
  graph
   ↓
  evidence
   ↓
  reasoning
   ↓
  answer
  17. This also changes how your agent should answer
  
  Suppose the user asks:
  
  Why didn't I get ₹350 of ITC in July?
  
  The agent shouldn't simply query:
  
  findings where amount = 350
  
  It should investigate:
  
  Question
   ↓
  Identify ITC shortfall
   ↓
  Find affected return period
   ↓
  Find candidate invoices
   ↓
  Resolve invoice identity
   ↓
  Inspect supplier
   ↓
  Inspect supplier declaration
   ↓
  Inspect GSTR-2B filing
   ↓
  Inspect filing period
   ↓
  Inspect goods receipt
   ↓
  Inspect applicable GST provisions
   ↓
  Build causal chain
   ↓
  Answer
  
  And return something like:
  
  ₹350 relates to invoice INV-123 issued by Supplier ABC on 7 July. The supplier's GSTR-1 declaration was filed after the July GSTR-2B cutoff, so the document did not appear in July's 2B. It subsequently appeared in August's 2B. The purchase register confirms the invoice, while the July 2B does not. Therefore this is a period-timing exception, not a supplier non-filing exception.
  
  Every sentence should be traceable to graph evidence.
  
  18. So I would actually pause the agent work
  
  This is my revised recommendation after looking at both your repo and the GST source structures.
  
  Don't build the GST agent yet.
  
  First fix this:
  
                      ❌ CURRENT
  Source → GST classes → graph
  
  into:
  
                      ✅ TARGET
  
  
  Source
    ↓
  Document extraction
    ↓
  Source schema
    ↓
  Semantic alignment
    ↓
  Canonical GST model
    ↓
  GST ontology
    ↓
  Entity resolution
    ↓
  Graph
    ↓
  Reconciliation
    ↓
  Evidence
    ↓
  Agent
  
  Your current reconciliation engine, SPARQL infrastructure, finding model, MCP surface and traversal capabilities are valuable and should stay. But the semantic ingestion foundation underneath them needs to be corrected first.
  
  Your own repo already has the ontology file, GST glossary, pack configuration, GSTR-1/GSTR-2B connectors and reconciliation plans, so this is a refactoring/re-modeling of the GST pack, not a reason to throw away GraphOWL.
  
  The key architectural principle I'd adopt
  
  Sources don't create ontology entities directly. Sources produce evidence. Evidence is extracted into source facts. Source facts are aligned to a canonical domain model. The ontology gives those canonical entities meaning.
  
  That one change will make GraphOWL much more genuinely domain-agnostic and will prevent the GST pack from becoming a collection of hard-coded source schemas disguised as an ontology.
  
  And importantly, official GST material confirms that the source world is much richer than the current pack: GSTR-2B is a period-specific static ITC statement, GSTR-1 contains multiple document/supply tables, and e-invoice has a substantially richer invoice structure including parties, lines, taxes, references, payment and logistics information.
  
  I would therefore redesign the GST pack's ontology + canonical model + ingestion/extraction layers before adding more GST rules or the agent.	

But don't put the GST extractor in Rust

This is the important refinement.

I would not interpret Option 1 as:

Rust
 └── PDF parser
 └── Excel parser
 └── OCR
 └── GST extractor

Instead:

                 GraphOWL Platform
                       │
              Provenance Model
                       │
       ┌───────────────┼───────────────┐
       │               │               │
    Document       ExtractionRun    SourceRef
    Field/Cell     Provenance       Evidence
                       │
                       ▼
              generic extraction
                  interfaces
                       │
          ┌────────────┼────────────┐
          ▼            ▼            ▼
       PDF/Doc       Excel        JSON/API
       adapter       adapter       adapter
          │            │            │
          └────────────┼────────────┘
                       ▼
                Semantic Mapping
                       │
                ┌──────┴──────┐
                ▼             ▼
               GST        Healthcare

The platform owns the model and contracts.

The pack/source adapters produce the extracted facts.

What should actually live at platform level?

I would define generic concepts like:

Document
DocumentPart
ExtractionRun
ExtractedField
ExtractedTable
ExtractedRow
ExtractedCell
SourceLocation
Provenance

For example:

Document
  id
  uri
  mimeType
  hash
  source
  createdAt
ExtractionRun
  id
  document
  extractor
  extractorVersion
  startedAt
  completedAt
  status
ExtractedCell
  document
  sheet
  row
  column
  rawValue
  normalizedValue
  confidence

And critically:

ExtractedFact
      │
      ├── derivedFrom → ExtractedCell
      ├── extractionRun → ExtractionRun
      └── sourceDocument → Document

These are not GST concepts.

Then GST should consume them

Suppose the Excel has:

Vendor GSTIN | Invoice No | Date | Taxable Value | IGST
27ABC...     | INV-102    | 7/7  | 10,000        | 1,800

GraphOWL should conceptually produce:

ExtractedCell
   │
   ├── value = "INV-102"
   ├── row = 1821
   ├── column = "Invoice No"
   └── document = PurchaseRegister.xlsx
          │
          │ semantic mapping
          ▼
gst:Invoice
   └── invoiceNumber = "INV-102"

And retain:

gst:Invoice
   └── invoiceNumber
          └── derivedFrom
                └── ExtractedCell
                     └── PurchaseRegister.xlsx
                          └── row 1821

Now your agent can eventually answer:

"Where did INV-102 come from?"

with an actual provenance path.

And this fixes your current ontology problem

This is the part I would emphasize to whoever is asking you this design question.

Don't do:

GSTR2B JSON
    ↓
gst:Gstr2bInvoice

Instead:

GSTR2B JSON
    ↓
Document / Source Record
    ↓
Extracted Facts
    ↓
Semantic Mapping
    ↓
Canonical GST Entity

Likewise:

Purchase Register
    ↓
Document
    ↓
Extracted Rows
    ↓
Semantic Mapping
    ↓
Canonical Invoice

and:

GSTR-1
    ↓
Document
    ↓
Extracted Records
    ↓
Semantic Mapping
    ↓
Canonical Invoice

Then the three sources converge on the same semantic entity.

This is the architecture I recommend
                    ┌───────────────────────┐
                    │    GRAPHOWL CORE      │
                    │                       │
                    │ Document              │
                    │ ExtractionRun         │
                    │ Field / Cell          │
                    │ Provenance            │
                    │ Source                 │
                    └───────────┬───────────┘
                                │
                         extracted facts
                                │
                    ┌───────────▼───────────┐
                    │ SEMANTIC ALIGNMENT    │
                    │                       │
                    │ Source schema         │
                    │ → canonical concept   │
                    └───────────┬───────────┘
                                │
                ┌───────────────┼────────────────┐
                ▼               ▼                ▼
              GST          Healthcare         Finance
                │               │                │
        Canonical GST    Canonical Clinical   Canonical
            Model              Model           Finance
                │
                ▼
          GST Ontology
                │
                ▼
             Graph
                │
       ┌────────┼─────────┐
       ▼        ▼         ▼
 Reconciliation Rules  Evidence
       │        │         │
       └────────┼─────────┘
                ▼
             Agent

That is the architecture I think you should lock in.

One thing I would not do yet

Don't immediately create a new Rust crate just because the option says:

"A new crate ..."

Your repo has an explicit crate-architecture gate.

So the decision should be:

Choose platform-level provenance conceptually, then determine whether it belongs inside an existing generic crate or genuinely requires a new crate.

You don't want:

graph-owl-provenance

just for the sake of creating a crate.

If the existing architecture has a natural home, put the primitives there.

And don't make Document/Field/Cell the GST ontology

This is the distinction I'd put in the implementation plan:

GraphOWL core namespace
owl:Document
owl:ExtractionRun
owl:ExtractedField
owl:ExtractedCell
owl:Source
owl:Provenance
owl:derivedFrom
owl:locatedAt
owl:extractedBy
GST namespace
gst:Invoice
gst:Supplier
gst:Recipient
gst:Filing
gst:GSTR1
gst:GSTR2B
gst:ITC
gst:TaxComponent
gst:CreditNote
gst:DebitNote

Then:

gst:Invoice
     │
     └── core:derivedFrom
             ↓
       core:ExtractedRecord
             │
             └── core:derivedFrom
                     ↓
                 core:Document

That's the clean separation.

So if you're sitting on that screen right now

Select:

✅ Platform-level (new Rust primitive, domain-agnostic)

But before implementation, I would tell the coding agent:

Do not create GST-specific provenance classes. Define the provenance model as GraphOWL core infrastructure, reusable by every pack. Keep document extraction adapters outside GST semantics. Do not create a new Rust crate until 00e-crate-architecture.md confirms an existing crate cannot own the primitives. The GST pack should consume the generic provenance API and only define GST-specific semantic mappings and canonical entities.

And I would add one more requirement:

Do not freeze the current GST ontology yet. First redesign the pipeline as Document → Extraction → Source Schema → Semantic Alignment → Canonical GST Model → GST Ontology → Graph.

That is the decision I would make. It prevents you from fixing the GST pack today and then having to tear it apart again when you add healthcare.


Parent
**Actor**: a CA/data engineer using the GST pack's reconciliation, and eventually an investigation agent built on it.
**Need**: the GST pack models one real-world invoice as one graph entity with multiple source-attestations (not three unrelated classes), and every fact the pack asserts carries provenance precise enough to answer "where did this come from" — down to a spreadsheet cell, a JSON path, or a filed return.
**Outcome**: reconciliation and the planned evidence-chain-walk/agent work (`105e`, `105s`) sit on a foundation that doesn't need tearing up again for ITC eligibility, credit notes, or a second pack.
**Current constraint**: `packs/gst/ontology.ttl` has `PurchaseInvoice`/`Gstr1Invoice`/`Gstr2bInvoice` as three classes instead of one canonical `Invoice`; `itcAvailable` is an Invoice property instead of its own claim; Epic 21's real, shipped `Claim`/`Provenance` model can't express tabular/JSON evidence, only prose byte-offsets.

## Recommended First Slice
Widen Epic 21's `Provenance.evidence` from `TextSpan`-only to a location enum covering prose span / tabular cell / JSON path, with zero behavior change to the existing markdown/runbook use case.

Why first: everything else depends on it existing, it has the clearest regression bar (existing Epic 21 tests stay green), and it proves "extend, don't replace" before the ontology work commits to relying on it.

## Split Candidates

| Slice | Value | Includes | Defers | Acceptance | Release constraint |
|---|---|---|---|---|---|
| **A — Widen `Provenance` evidence location** | Every future fact can cite exactly where it came from | New `Evidence` enum in `graph_owl_core::extraction` (`Text`/`Cell{sheet,row,column}`/`JsonPath`); `Provenance.evidence` retyped | GST actually using it; UI changes; catalog-vs-domain-subject validation | Existing Epic 21 tests pass unmodified except the retype; new test round-trips a `Claim` with `Evidence::Cell` through JSON | Shippable alone |
| **B — Canonical `gst:Invoice` replaces the 3 source classes** | "Which invoices did Supplier X issue" is one traversal, not three | `gst:Invoice`; `issuedBy` (kept) + new `declaredIn`/`reflectedIn`/`recordedIn`; same ~10 fields, once per invoice; `gstr2b.py`'s `to_turtle()` rewritten, citing `Evidence::JsonPath` | Rich ~30-field Invoice; CreditNote/DebitNote/amendments; GSTR-1/books rewrite (that's C) | Existing missing-in-gstr2b/late-filing/reverse-charge SPARQL queries return identical findings against the new shape on the same fixtures — the regression bar | **Not releasable alone** — breaks reconciliation until C also lands |
| **C — Books & GSTR-1 ingestion emit the canonical shape** | Plan 108's shipped three-source reconciliation keeps working | Whatever emits Turtle for books/GSTR-1 today rewritten to match B | New document types; ITC separation (D) | Same books/GSTR-1 fixtures plan 108 already tests against produce the same reconciliation statement | Ships together with B |
| **D — ITC becomes its own claim object** | ITC eligibility becomes a real, separately-evidenced question instead of one copied boolean | `gst:ITC`; `claims`/`supportedBy`/`reflectedIn` edges; `itcAvailable` moves off Invoice | ITC eligibility *reasoning* (goods-receipt + 180-day + reverse-charge combined) — this only relocates the fact | A query for "ITC claims not yet supported by a goods receipt" becomes expressible | Ships after B+C are stable |

## Parking Lot
- Rich canonical Invoice, CreditNote/DebitNote/amendments — deferred per your answer.
- MCP `traverse()`'s catalog-only restriction (original proposal's point #12) — real gap, but a separate authz/policy concern, not ontology shape.
- **Open question I couldn't resolve from the repo**: is books/GSTR-1 ingestion Python (parallel to `gstr2b.py`) or TypeScript (`ui/src/features/packs/{books,gstr1}.ts`)? `connectors/python/graph_owl_packs/` only has `gstr2b.py`, `erpnext.py`, `reconcile.py`, `loader.py`, `manifest.py` — no `gstr1.py`/`books.py`. If it's TS-side, slice C is a UI change, not a Python one — needs checking before C is planned in detail.

## Warnings
- B is deliberately not shippable alone; if you want a smaller releasable unit, B+C need to land as one slice, not sequential releases.
- Every slice touches `packs/gst/fixtures/*.ttl` and `packs/gst/queries/*.sparql` — this project has already been burned by a `replace_all` silently corrupting shared fixtures. Exact-block edits, not blanket find/replace.
- Per `00i-licensing.md` rule 2: when D's follow-ons (rich Invoice, CreditNote/DebitNote) get planned later, re-derive field names from the published GST statute/return-format spec, not from either pasted proposal's text.


hecked the actual main branch, and the open question is now resolved:

Books and GSTR-1 ingestion are currently TypeScript/UI-side, not Python.

Evidence from the repo

ui/src/features/packs/books.ts contains the full purchase-register parser and normalizer. It explicitly parses CSV/TSV in the browser, detects column aliases, validates required columns, normalizes dates/money, and produces BooksInvoice records.

ui/src/features/packs/gstr1.ts similarly contains the full GSTR-1/GSTR-2A JSON normalizer and Turtle generation, including supplier filing date/period handling and multi-rate tax aggregation.

Meanwhile, connectors/python/graph_owl_packs/ really does contain gstr2b.py, erpnext.py, reconcile.py, loader.py, and related files, but no books.py or gstr1.py.

So your Claude agent's conclusion is correct

The current architecture is effectively:

GSTR-2B
   ↓
Python connector
   ↓
GraphOWL pack


Books CSV/TSV
   ↓
TypeScript browser importer
   ↓
GraphOWL pack


GSTR-1 / GSTR-2A JSON
   ↓
TypeScript browser importer
   ↓
GraphOWL pack

And the three TypeScript importers share normalization helpers through gstText.ts; both books.ts and gstr1.ts explicitly mention that shared normalization.

What this means for Slice C

Do not plan Slice C as a Python ingestion change.

For the current implementation, it should be treated as a UI/TypeScript ingestion path.

But there is a deeper architectural issue I would flag before implementing it:

               Current
                  │
       ┌──────────┼──────────┐
       ↓          ↓          ↓
     Python       TS         TS
     GSTR2B     Books       GSTR1
       │          │          │
       └──────────┼──────────┘
                  ▼
             GST Turtle

That is exactly the inconsistency we were discussing.

You now have two different ingestion runtimes producing the same semantic domain.

I would therefore tell Claude:

Books and GSTR-1 are currently browser-side TypeScript importers (ui/src/features/packs/books.ts, ui/src/features/packs/gstr1.ts); GSTR-2B is Python (connectors/python/graph_owl_packs/gstr2b.py). Do not invent books.py/gstr1.py for Slice C. First preserve current runtime ownership while redesigning the ingestion contract so all source adapters emit the same generic extracted/source-fact representation and provenance model. Do not duplicate the existing TS normalizers into Python merely for architectural symmetry.

That's important.

I would not "fix" this by moving Books/GSTR-1 into Python immediately

That would solve symmetry but potentially make the architecture worse:

TS → Python → Rust

instead of:

TS/Python adapters
       ↓
same generic ingestion contract
       ↓
canonical model

The real goal is runtime-independent ingestion contracts, not "everything must be Python."

So: the open question is resolved: Slice C is currently TypeScript/UI-side. The next design question should be how to introduce the generic extraction/provenance boundary without needlessly rewriting those working importers.

Where does books.ts / gstr1.ts create the provenance?

Currently: nowhere.

They perform extraction/normalization entirely in the browser and then generate Turtle directly in TypeScript.

For Books:

CSV/TSV
  ↓
books.ts
  ├── parseDelimited()
  ├── column detection
  ├── normalize()
  └── toTurtle()
           ↓
       Turtle string

The toTurtle() function directly creates gst:Supplier and gst:PurchaseInvoice triples.

GSTR-1 does the same:

JSON
 ↓
gstr1.ts
 ├── returnBody()
 ├── slabTotals()
 ├── normalize()
 └── toTurtle()
          ↓
       Turtle string

Its toTurtle() similarly directly produces gst:Gstr1Invoice triples.

Then importFile.ts takes that Turtle and sends it to:

api.importRdf(source, turtle)

So the actual boundary is:

             BROWSER
                │
   ┌────────────┼────────────┐
   │            │            │
books.ts     gstr1.ts     gstr2b?
   │            │
   ▼            ▼
Turtle        Turtle
   │            │
   └──────┬─────┘
          ▼
     api.importRdf()
          │
          ▼
        SERVER

importThroughSurface() explicitly says convert first, always, and then sends the generated Turtle through api.importRdf.

Therefore

Your proposed runtime-independent contract doesn't currently exist at the Books/GSTR-1 boundary.

That's the gap.

2. And this changes how I would design Slice A

There is an important correction to the Claude agent's wording:

Slice A is not yet a TS/Python shared contract.

The current Rust extraction contract was designed around an out-of-process worker and is already explicitly JSON-serializable. The core file says these types are intended as a wire contract, with Serialize + Deserialize, precisely so external workers can exchange JSON with Rust.

And Provenance currently looks conceptually like:

Provenance
├── sourceId
├── extractor
├── extractorVersion
├── extractedAt
└── evidence: TextSpan

where TextSpan is:

{
  start: number,
  end: number
}

So the Rust side already has a JSON-compatible wire contract, but the evidence representation is currently fundamentally text-oriented.

3. And that's the real problem

The existing model assumes:

Document
   ↓
text
   ↓
claim
   ↓
TextSpan(start,end)

That's perfectly reasonable for:

PDF
OCR
Markdown
LLM extraction

But it does not adequately represent:

Excel
CSV
JSON
API response

For GST Books:

PurchaseRegister.xlsx
   Sheet: Purchases
      Row: 1821
         Column: Invoice No

There is no meaningful:

TextSpan(????, ????)

that gives the user a stable citation.

And for GSTR-1 JSON:

payload
  → b2b[14]
      → inv[3]
          → inum

again, a text span isn't the natural evidence locator.

So Slice A's direction is right, but its current evidence abstraction needs to be widened before Slice C uses it.

4. I would NOT make Evidence an enum

This is the part where I disagree slightly with the framing in the question.

Don't create:

enum Evidence {
    TextSpan(...),
    Cell(...),
    JsonPath(...),
}

as your long-term contract.

Because six months later you'll have:

PDF page
PDF bounding box
OCR region
Excel cell
CSV row
JSON path
API response
database row
email attachment
web page
DOM node

and your supposedly domain-neutral core becomes:

enum Evidence {
    ...
}

that grows forever.

That violates the exact extensibility principle the current extraction code is trying to preserve.

5. Instead: make evidence a discriminated, open-ended location object

Something like:

{
  "sourceId": "purchase-register-july.xlsx",
  "extractor": "graphowl-books-csv",
  "extractorVersion": "1.0",
  "extractedAt": "2026-08-15T...",
  "evidence": {
    "kind": "tabular",
    "location": {
      "sheet": "Purchases",
      "row": 1821,
      "column": "Invoice No"
    }
  }
}

For GSTR-1:

{
  "evidence": {
    "kind": "json",
    "location": {
      "path": "$.b2b[14].inv[3].inum"
    }
  }
}

For PDF:

{
  "evidence": {
    "kind": "text",
    "location": {
      "start": 1204,
      "end": 1248
    }
  }
}

And later:

{
  "evidence": {
    "kind": "pdfRegion",
    "location": {
      "page": 7,
      "x": 124,
      "y": 318,
      "width": 240,
      "height": 24
    }
  }
}

The contract is generic, while the locations are source-format-specific.

6. Even better: separate source identity from evidence location

I'd make it:

Provenance
│
├── source
│   ├── sourceId
│   ├── uri
│   ├── contentHash
│   └── mediaType
│
├── extraction
│   ├── extractor
│   ├── version
│   └── extractedAt
│
└── evidence
    ├── kind
    └── location

That gives you:

source identity
       +
where inside source
       +
how it was extracted

This is much cleaner.

7. Now look at the really important consequence for Books

Today:

books.ts
   ↓
BooksInvoice
   ↓
toTurtle()

Instead, eventually:

books.ts
   ↓
ExtractedRecord
   │
   ├── supplierGstin
   │      └── provenance → cell B1821
   │
   ├── invoiceNumber
   │      └── provenance → cell C1821
   │
   ├── invoiceDate
   │      └── provenance → cell D1821
   │
   └── taxableValue
          └── provenance → cell E1821
              ↓
       Semantic mapping
              ↓
       Canonical Invoice
              ↓
            Turtle

That is the architecture we were talking about earlier.

8. And GSTR-1 becomes much better

Currently GSTR-1 normalizes supplier filing data and invoice slabs, then writes a canonical-ish Gstr1Invoice Turtle representation.

Instead:

GSTR-1 JSON
    │
    ▼
ExtractedRecord
    │
    ├── ctin
    │    └── JSONPath $.b2b[14].ctin
    │
    ├── inum
    │    └── JSONPath $.b2b[14].inv[3].inum
    │
    ├── txval
    │    └── JSONPath $.b2b[14].inv[3].itms[0].itm_det.txval
    │
    └── iamt
         └── JSONPath ...

Then semantic mapping.

This gives you field-level evidence, not just document-level provenance.

9. But there is a practical problem: TS needs to construct this

You're absolutely right about this:

If Slice A's type is the wire contract, TS must be able to produce it.

And because books.ts is currently TypeScript, it can't instantiate a Rust struct.

Therefore the clean solution is:

One language-neutral schema

For example:

schemas/
    extraction/
        provenance.schema.json
        extracted-record.schema.json
        extraction-result.schema.json

Then:

                 JSON Schema
                      │
             ┌────────┴────────┐
             ▼                 ▼
           Rust               TypeScript
             │                 │
       serde types        TS types/validator
             │                 │
             └────────┬────────┘
                      ▼
                same wire shape

That is much better than making the Rust struct itself the cross-runtime contract.

10. But don't duplicate the schema manually

This is important.

Don't do:

Rust Provenance struct
+
TypeScript Provenance interface
+
JSON Schema

and manually maintain three things.

Pick one canonical schema source.

For GraphOWL, I would strongly consider:

JSON Schema
      ↓
Rust generated/validated types
      ↓
TypeScript generated types

or, given your Rust-first core architecture:

Rust serde model
       ↓
generated JSON Schema
       ↓
TS types

Either can work.

The important thing is:

There must be exactly one wire contract.

11. One more thing: don't make the browser import directly into final ontology

This is now the bigger architectural issue I see from the repo.

Currently:

books.ts
   ↓
gst:PurchaseInvoice Turtle
   ↓
importRdf()

and:

gstr1.ts
   ↓
gst:Gstr1Invoice Turtle
   ↓
importRdf()

This means the importer is simultaneously doing:

parsing
+
normalization
+
semantic modeling
+
ontology projection

That's exactly the coupling you were worried about earlier.

I would eventually change the boundary to:

books.ts
   ↓
generic extracted records
   ↓
provenance
   ↓
semantic alignment
   ↓
canonical GST model
   ↓
RDF projection
12. So my recommendation for Slice A/B/C changes

I would restructure the sequence slightly.

Slice A — Provenance contract

Don't merely widen TextSpan.

Define the runtime-neutral provenance wire model:

Source
ExtractionRun
EvidenceLocation
ExtractedField / Claim

with evidence locations supporting at least:

text
tabular
json

No GST types.

Slice B — Rust ingestion boundary

Make Rust accept/validate the same JSON contract.

The existing extraction API already explicitly treats ExtractionResult as a JSON process boundary.

Extend that principle to structured sources.

Slice C — TS Books + GSTR-1

Do not rewrite them in Python.

Change:

normalize()
   ↓
toTurtle()

toward:

normalize()
   ↓
ExtractedRecord[]
   +
Provenance
   ↓
semantic mapping
   ↓
canonical GST

The TS code remains the runtime adapter.

13. And GSTR-2B Python follows the same contract

Then your architecture becomes beautiful:

                       GraphOWL Extraction Contract
                                  │
                  ┌───────────────┴───────────────┐
                  │                               │
             TypeScript                         Python
                  │                               │
             books.ts                         gstr2b.py
             gstr1.ts                         ERP adapter
                  │                               │
                  └───────────────┬───────────────┘
                                  ▼
                         Extracted Facts
                                  │
                         Provenance attached
                                  │
                                  ▼
                       Semantic Alignment
                                  │
                                  ▼
                       Canonical GST Model
                                  │
                                  ▼
                                RDF

That is the runtime-independent architecture you were looking for.

14. One correction to your current wording

You said:

"Slice A widens Provenance.evidence in graph_owl_core::extraction to cover tabular/JSON locations."

I would change that requirement.

Don't think:

evidence: TextSpan → evidence: bigger enum

Think:

Provenance.evidence becomes a format-neutral evidence-location contract.

That distinction matters enormously for the future.

My final recommendation

Q1 — Where is provenance currently constructed?

It isn't. Books and GSTR-1 currently parse and normalize in TypeScript, directly generate Turtle client-side, and send that Turtle through api.importRdf().

Q2 — Should the Rust Evidence/TextSpan type be the contract?

The wire contract should be shared, but I would not make the Rust enum itself the contract. Define a language-neutral JSON Schema for provenance/evidence locations, then have Rust and TypeScript consume/produce that same shape.

And I would make the immediate target:

             SOURCE
               │
               ▼
        TS / Python adapter
               │
               ▼
       ExtractedRecord[]
               │
        + provenance
               │
               ▼
       Semantic Alignment
               │
               ▼
       Canonical GST Model
               │
               ▼
             RDF

That is the change that will solve your current GST ontology/extraction problem without making GraphOWL GST-specific.