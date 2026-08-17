Yes — and this is where Reco Now + GraphOWL can become substantially more powerful than a conventional GST reconciliation product.

But I would make one architectural rule:

LLMs and agents should operate on top of the reconciliation + GraphOWL semantic layer. They should not become the source of truth for GST reconciliation.

The deterministic reconciliation engine decides facts. GraphOWL provides relationships, evidence and reasoning. The LLM/agent interprets, investigates and orchestrates.

1. The architecture I would use
                         RECO NOW
                            │
                    Accountant / CA
                            │
                            ▼
                    ┌───────────────┐
                    │ Agent / LLM   │
                    │ Orchestration │
                    └───────┬───────┘
                            │
             ┌──────────────┼──────────────┐
             ▼              ▼              ▼
        Reco Tools      GraphOWL Tools   GST Tools
             │              │              │
       reconciliation    traversal       rules
       exceptions        evidence        calculations
       suppliers         reasoning       validations
       IMS               provenance      compliance
       periods           ontology
             │              │
             └──────────────┼──────────────┘
                            ▼
                    Deterministic Engine
                            │
                            ▼
                 Books / GSTR-1 / 2B
                 IMS / GST sources

The key is that the LLM sits above these systems.

2. There should be three different AI capabilities

Don't call everything an "agent."

I'd separate:

A. AI Assistant

Answers questions.

B. Investigation Agent

Investigates a reconciliation problem.

C. Action Agent

Performs controlled workflow actions.

And eventually:

D. Monitoring Agent

Watches for things that change.

3. AI Assistant

Put a small:

Ask Reco

box at the top.

A user could type:

Why is my ITC lower this month?

The LLM should not simply answer from the dashboard.

It should call tools:

get_period_summary(August)


get_itc_exposure(August)


get_top_exceptions(August)


get_cross_period_cases(August)


get_supplier_issues(August)

Then produce:

Your August ITC is ₹18.7L lower than
your books indicate.


The main reasons are:


₹7.2L — invoices not appearing in 2B
₹4.1L — cross-period invoices
₹3.8L — amount mismatches
₹2.4L — eligibility/review cases
₹1.2L — other exceptions


The largest contributor is XYZ Pvt Ltd.


Would you like me to investigate those cases?

That's useful.

4. Natural-language reconciliation queries

This could be extremely powerful.

User:

Show me invoices above ₹1 lakh that are missing from 2B.

Agent translates that into deterministic tool calls:

search_reconciliation(
    period="2026-08",
    status="only_in_books",
    min_itc=100000
)

Then the UI produces an actual table:

Invoice       Supplier       ITC       Status
INV-1025      ABC Ltd        ₹1.8L     Missing 2B
INV-1077      XYZ Ltd        ₹1.4L     Missing 2B
...

The LLM is merely the interface.

5. Investigation Agent

This is where GraphOWL becomes extremely valuable.

Suppose the user clicks:

Why is INV-1025 missing from 2B?

Instead of displaying a static explanation, the agent can investigate.

Agent plan
1. Get reconciliation case
2. Inspect Books record
3. Find supplier entity
4. Search GSTR-1
5. Search GSTR-2B
6. Search adjacent periods
7. Check GSTIN variants
8. Check invoice number variants
9. Inspect supplier filing history
10. Build evidence chain
11. Determine likely explanation

Tools:

get_invoice()
get_supplier()
find_matching_documents()
find_paths()
get_evidence_graph()
get_supplier_history()
search_cross_period()
get_filing_period()

Then:

INV-1025 is not currently in August GSTR-2B.


However, I found the invoice in the supplier's
August GSTR-1 filing.


The supplier filed the invoice on August 11,
after the relevant July reporting cycle.


I also found a matching record in September 2B.


Conclusion:
This appears to be a cross-period carry-forward,
not a supplier non-filing.


Confidence: High


[View evidence]
[Open GraphOWL]

That is a real agentic investigation.

6. The agent should use GraphOWL as its investigation substrate

This is the really important part.

Don't give the agent:

Neo4j query access

and tell it to figure things out.

Give it semantic tools.

For example:

graph.search_entity
graph.get_entity
graph.get_neighbors
graph.find_path
graph.get_evidence
graph.get_provenance
graph.get_history
graph.explain_relationship
graph.get_contradictions

Then:

Reco Agent
     │
     ├── reconciliation.get_case
     │
     ├── reconciliation.search
     │
     ├── graph.get_entity
     │
     ├── graph.find_path
     │
     ├── graph.get_evidence
     │
     └── graph.get_history

This keeps the agent domain-aware without putting GraphOWL semantics into the agent itself.

7. The LLM should never invent reconciliation results

This distinction is critical.

Bad:

LLM:
"I think invoice INV-1025 probably matches."

Good:

Reconciliation engine:
MATCH_STATUS = PROBABLE
MATCH_SCORE = 0.94

Then the LLM says:

The reconciliation engine classified this as a probable match with a 0.94 match score because GSTIN, invoice number and date match, while the tax amount differs by ₹500.

The LLM explains the result.

It does not create the result.

8. Use agents for exception investigation

This could become the core AI feature.

Instead of:

327 exceptions

give the user:

[Investigate exceptions]

Agent:

327 exceptions
       │
       ├── 181 amount differences
       ├── 72 invoice/date differences
       ├── 47 cross-period
       ├── 18 duplicates
       └── 9 other

Then investigate groups.

For example:

"Investigate all cross-period exceptions."

The agent can process the 47 cases and classify:

41 genuine carry-forward
4 invoice number variations
2 unresolved

The important thing:

Agent does not silently change reconciliation state.

It creates:

Suggested explanation
Suggested classification
Evidence
Confidence

Then the human approves.

9. Supplier Investigation Agent

This is potentially one of the best features.

User:

Why is XYZ Pvt Ltd causing so many problems?

Agent investigates:

Supplier:
XYZ Pvt Ltd


Invoices analyzed:
382


Problems:
18 amount mismatches
12 missing in 2B
8 cross-period
3 GSTIN variations


Historical pattern:
────────────────────
July       4 issues
August    12 issues
September  8 issues


Total ITC exposure:
₹4.82L

Then:

Most of the current exposure comes from delayed supplier filings. Eight invoices appeared in a subsequent GSTR-2B period. Three additional cases contain GSTIN inconsistencies.

And then:

[Show evidence]
[View supplier graph]
[Create supplier follow-up]
10. Agent-generated supplier follow-ups

Once an issue is understood:

Agent:
Draft a message to XYZ about the 12 missing invoices.

It can generate:

Subject:
GST reconciliation – invoices missing from GSTR-2B


Dear XYZ Team,


During our August 2026 GST reconciliation,
we identified 12 invoices recorded in our books
that are not currently reflected in GSTR-2B.


Total taxable value: ₹...
Total ITC: ₹...


Please verify the corresponding GSTR-1 filings
and confirm whether these invoices are expected
to appear in a subsequent GSTR-2B period.


Regards,
...

But:

Agent drafts
       ↓
Human reviews
       ↓
Send

not autonomous sending by default.

11. Period-close Agent

This could be a fantastic feature.

User:

Can I close August?

Agent runs:

check_period_status()
check_open_exceptions()
check_ims_pending()
check_itc_exposure()
check_cross_period()
check_supplier_followups()

Then:

AUGUST 2026 PERIOD REVIEW


✓ Books imported
✓ GSTR-1 available
✓ GSTR-2B available
✓ Reconciliation completed


⚠ 38 exceptions remain
⚠ ₹4.2L ITC exposure
⚠ 6 IMS records pending
⚠ 9 cross-period cases


Recommendation:
Do not close yet.


Primary unresolved exposure:
₹2.7L across 7 suppliers.

That is far better than an AI chatbot.

12. “What should I work on first?”

This is where an agent can prioritize.

User:

What should I review first?

Agent evaluates:

ITC exposure
+
confidence
+
supplier importance
+
deadline
+
historical behavior
+
cross-period likelihood
+
resolution effort

Then:

PRIORITY 1


XYZ Pvt Ltd
₹3.2L exposure
18 invoices
High probability of supplier correction


PRIORITY 2


ABC Industries
₹1.8L exposure
3 invoices
Potential duplicate


PRIORITY 3


PQR Ltd
₹1.1L exposure
12 invoices
Likely cross-period

This is an excellent use of LLM + deterministic scoring.

13. Agent should produce an investigation report

Imagine clicking:

Generate investigation

The agent produces:

INV-1025 INVESTIGATION


Issue
Amount mismatch


Books
₹38,000


GSTR-1
₹37,500


GSTR-2B
₹37,500


Finding
AmountMismatch


Evidence
──────────────
Books record
GSTR-1 filing
GSTR-2B record


Supplier history
──────────────
3 previous amount mismatches


Likely cause
Supplier reported a different tax amount.


Recommended action
Verify supplier invoice and request correction
if the supplier filing is incorrect.


Confidence
High

Every statement should have an evidence link.

14. This is where GraphOWL's provenance is essential

The agent response should have citations internally tied to graph facts.

For example:

Supplier filed GSTR-1 on 11 Aug 2026
[Evidence E-103]


Invoice appears in September GSTR-2B
[Evidence E-221]


Supplier has 4 similar carry-forward cases
[Evidence E-301]

Click:

[E-221]

→ Reco evidence drawer

or:

[Open in GraphOWL]

→ GraphOWL investigation.

That gives you auditable AI.

15. LLM should explain GST rules, but rule execution stays deterministic

Example:

User:

Why does this invoice need review?

LLM:

The invoice appears in GSTR-2B, but the available evidence indicates the goods receipt occurred after the relevant period. That creates a timing issue that needs review against the applicable ITC eligibility requirements.

Behind that:

GST rule engine
      +
GoodsReceipt event
      +
FilingPeriod
      +
GraphOWL evidence

The LLM explains.

It does not decide the law.

16. Legal research agent

Eventually I'd have:

GST Research Agent

But constrain it.

User:

Explain why this exception may affect ITC.

Agent retrieves:

GST rule
      ↓
relevant statutory section
      ↓
GraphOWL governed_by relationship
      ↓
invoice facts
      ↓
evidence

Response:

This case is associated with Section 16(2)(b).


Relevant facts:
• Invoice appears in GSTR-2B
• Goods receipt recorded on Aug 18
• Filing period: August


The system therefore flags this for review.


[View rule]
[View evidence]

Not:

"You definitely cannot claim this ITC."

That distinction is important.

17. Multi-agent architecture

I wouldn't create 20 agents.

Start with perhaps:

                    Reco Supervisor
                          │
        ┌─────────────────┼─────────────────┐
        │                 │                 │
   Reconciliation     Investigation      Period Agent
      Agent              Agent
        │                 │
        │          ┌──────┼───────┐
        │          │      │       │
        │       Graph   Evidence Supplier
        │       Agent    Agent    Agent
        │
        └───────────────┬─────────────────
                        │
                   Action Agent

But even this may be more than you need initially.

18. I would start with ONE agent

This is important.

Don't build:

GST Agent
Supplier Agent
Evidence Agent
IMS Agent
Legal Agent
ITC Agent

from day one.

Build:

Reco Investigation Agent

Give it maybe 15–20 tools.

For example:

RECO


get_period_summary
search_invoices
get_reconciliation_case
get_exception
get_supplier_summary
search_cross_period
get_ims_status
get_itc_exposure


GRAPHOWL


search_entity
get_entity
get_neighbors
find_path
get_evidence
get_provenance
get_history
get_reasoning
get_contradictions


ACTIONS


create_followup
add_note
assign_case
draft_message

That is enough to build something genuinely useful.

19. Agent UI

I wouldn't make it a ChatGPT-like full-screen chat.

Make it a copilot panel.

┌──────────────────────────────────────────────┐
│ RECO INVESTIGATOR                            │
├──────────────────────────────────────────────┤
│                                              │
│ Why is INV-1025 missing from 2B?              │
│                                              │
│ Investigating...                             │
│                                              │
│ ✓ Checked Books                             │
│ ✓ Found supplier                             │
│ ✓ Checked GSTR-1                            │
│ ✓ Checked GSTR-2B                           │
│ ✓ Checked next period                       │
│                                              │
│ Finding                                      │
│                                              │
│ The invoice appears in the supplier's       │
│ GSTR-1 but not August 2B. A matching        │
│ record appears in September 2B.             │
│                                              │
│ Likely cross-period carry-forward.           │
│                                              │
│ Confidence: High                             │
│                                              │
│ [View evidence] [Open GraphOWL]              │
│                                              │
└──────────────────────────────────────────────┘

This is much better than a giant chatbot.

20. Show the agent's work

For trust, give an expandable:

Investigation steps
✓ Reconciliation case retrieved
✓ Supplier identified
✓ GSTR-1 searched
✓ GSTR-2B searched
✓ September 2B searched
✓ GSTIN variation checked
✓ Evidence graph traversed

But don't expose chain-of-thought.

Show tool/action summaries, not hidden reasoning.

21. Agent confidence should be separate from reconciliation confidence

This is important.

You might have:

Reconciliation match confidence
94%

and:

Agent explanation confidence
High

These are different.

The first is a deterministic matching result.

The second is confidence in the agent's interpretation.

Don't merge them.

22. Human approval gates

Anything that changes accounting/tax state should require approval.

Safe for automatic agent execution
Search
Read
Compare
Summarize
Investigate
Draft
Prioritize
Generate report
Human approval
Change reconciliation status
Accept exception
Reject exception
IMS action
Modify mapping
Close period
Send supplier communication
Change GST rule

That gives you a safe agentic architecture.

23. The best feature: “Investigate all”

Imagine:

327 exceptions


[Investigate all]

Agent processes them.

Result:

327 exceptions


Likely explanations
────────────────────────────


Cross-period                  81
Supplier filing delay         72
Amount mismatch               64
GSTIN variation               31
Invoice number variation      28
Possible duplicate            19
Unresolved                    32

Then:

AI reviewed 295 / 327


High confidence
221


Medium confidence
74


Requires human investigation
32

Now the accountant doesn't have to manually inspect 327 records.

24. Agent learns from resolution history—but carefully

Suppose humans repeatedly classify:

GSTIN transposition

Then the system can learn patterns.

But don't let the LLM silently modify GST logic.

Instead:

Observed pattern


12 previous cases were resolved as
GSTIN transposition when:


• GSTIN edit distance ≤ 1
• invoice number matched
• supplier matched
• amount matched


[Create proposed matching strategy]

Then an administrator approves the rule.

That turns agent observations into governed deterministic logic.

25. Reco Now could eventually have “Explain this month's reconciliation”

One click:

Explain August reconciliation

Output:

AUGUST 2026 RECONCILIATION


12,482 book invoices were compared with
12,197 GSTR-2B records.


93.2% matched directly.


The remaining exceptions are primarily caused by:


1. Supplier filing delays — ₹7.2L
2. Amount differences — ₹3.8L
3. Cross-period documents — ₹4.1L
4. Missing book records — ₹2.4L


The largest unresolved supplier exposure is
XYZ Pvt Ltd at ₹3.2L.


Nine invoices appear likely to be
cross-period carry-forwards.


Six IMS actions remain pending.


Recommended priority:
1. XYZ Pvt Ltd
2. 6 pending IMS decisions
3. ₹3.8L amount mismatches

That's the kind of AI summary a finance person would actually value.

26. The ultimate architecture

I'd make the separation extremely clean:

                    ┌───────────────────────┐
                    │       RECO NOW        │
                    │                       │
                    │ GST Workflow / UI     │
                    └───────────┬───────────┘
                                │
                         Agent Interface
                                │
                    ┌───────────▼───────────┐
                    │   RECO INVESTIGATOR   │
                    │                       │
                    │ Plan → Tools → Verify │
                    └───────────┬───────────┘
                                │
           ┌────────────────────┼────────────────────┐
           │                    │                    │
           ▼                    ▼                    ▼
      Reco Tools          GraphOWL Tools        GST Rules
           │                    │                    │
       Matching             Traversal            Deterministic
       Exceptions            Evidence             findings
       Suppliers             Provenance            validation
       Periods               Reasoning             calculations
       IMS                   History
           │                    │                    │
           └────────────────────┼────────────────────┘
                                │
                                ▼
                         GRAPHOWL CORE
                                │
                                ▼
                    Books / GSTR-1 / 2B / IMS

And then:

LLM
  ↓
understands user intent
  ↓
chooses tools
  ↓
reads deterministic results
  ↓
asks GraphOWL for evidence
  ↓
synthesizes explanation
  ↓
proposes action
  ↓
human approves
27. The key principle

I would actually write this into the Reco architecture documentation:

The LLM may interpret GraphOWL; it may not redefine GraphOWL.

And:

The agent may orchestrate reconciliation capabilities; it may not replace deterministic reconciliation.

And:

Every material AI conclusion must be traceable to reconciliation results, graph evidence, or an explicitly identified external source.

That gives you a very strong product architecture.

So the division becomes
                    RECO NOW
                       │
              "What should I do?"
                       │
                       ▼
                    AGENT
                       │
              "Investigate this."
                       │
                       ▼
                   GRAPHOWL
                       │
             "What is actually true?"
                       │
                       ▼
              DETERMINISTIC GST
                       │
             "What did the data say?"

That, in my view, is the real opportunity with your architecture.

You're not building another GST reconciliation SaaS with an LLM bolted onto the side.

You're building:

A GST reconciliation workbench where deterministic reconciliation finds the exception, GraphOWL explains the exception, and an agent investigates and operationalizes the exception.

That is a much stronger product position