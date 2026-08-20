# End-to-end GST use case: tracing a purchase-invoice ITC mismatch through GraphOWL

Every step below was run live (`http://localhost:5180`, local `gst` pack) before being written down — the term names, button labels, query results and row counts are real, not illustrative. Follow it in order; each step uses the real output of the one before it.

**Scenario.** A supplier's purchase invoice was recorded in the buyer's purchase register, but the supplier never declared it in their own GSTR-2B return — the exact case the glossary already defines as `PotentialMismatch`. This walkthrough traces that concept from vocabulary, through the ontology, to the real reconciliation data, into the graph explorer, and out through an export and a natural-language question — using every capability Studio, Explore and the console's own search bar currently have.

## 1. Build — find the term the scenario is built on

1. Go to **Studio → Build**, glossary `gst`.
2. Click **PotentialMismatch** in the term list.
3. You'll see: status `approved`, definition *"An invoice claimed in the purchase register that the supplier never filed."* That sentence is the scenario — everything below answers it with real data.

### Create a new term (the authoring path)

4. In the **Term name** box, type `ItcReversalCandidate` → **New term**. It's created as `draft`.
5. Go to **Studio → Glossary**. It now appears under *Candidates — draft & in review*.
6. Click **Submit for review** — status moves to `in review`.
7. In the **reviewer user id** box, type `system` (the only seeded user in a fresh local instance) → **Assign reviewer**.
   - If you type an unknown id here, the UI correctly rejects it: *"Could not assign that reviewer — is it a known user id?"* — reviewers are a real foreign key, not free text.
8. Click **Promote to approved**. The term now appears under *Approved*, both in the Glossary tab and back in Build.

## 2. Business view — the plain-language check

Go to **Studio → Business view**. Find `PotentialMismatch` in the list — same definition, no relations, no technical fields. This is the read-only view a non-technical reviewer would use to sanity-check wording.

## 3. Proposals — the governance queue

Go to **Studio → Proposals**. Note: this queue is **not pack-scoped** — in a fresh instance it shows generic seeded proposals (*Master Vendor Record*, *Exposure Threshold*, *Ghost Entity*) rather than anything GST-specific. That's real, current behavior, not a gap in this walkthrough — there's simply no GST-specific proposal seeded yet. Use **Approve / Reject / Request info** here when a real one exists.

## 4. Graph — connect the new term to the scenario

Go to **Studio → Graph** (the glossary's own term-relationship graph, not the ontology).

1. **From**: `ItcReversalCandidate`
2. **Relation**: `related`
3. **To**: `PotentialMismatch`
4. Click **Connect**.

Verify it: go back to **Build → ItcReversalCandidate** — under **RELATIONS** you'll see `related <PotentialMismatch's id>`. This is a real write, not a preview.

## 5. Ontology — four views over the real GST model

Go to **Studio → Ontology**, pack `gst`. You'll see **18 classes · 11 relationships · 33 properties**, all read from the live `/sparql` endpoint (no separate ontology backend — it's the same pack import every other tab reads). Four view toggles sit top-right: **Graph · Table · Editor · Alignments**.

### Graph

Pan/zoom/drag to explore. `PurchaseInvoice` sits at the center of most edges — `issued by → Supplier`, `belongs to period → Filing period`, `appears in (GSTR-1/IFF)`, etc. Nodes and labels are drawn heavier than the instance-graph default specifically because an 18-class diagram needs more visual weight to survive `fitView`'s zoom-out — if this ever looks faded again, that's a real regression, not a styling nitpick.

### Table

Switch to it, type `invoice` in the filter — you'll see every class, relationship and property whose name contains it, including `PurchaseInvoice`'s own `issuedBy`/`onInvoice` relationships. The properties panel (33 entries — `taxAmount`, `itcAvailable`, `claimDeadline`, `hsnCode`, etc.) is intentionally **not** attributed to a class: a plain `gst:Property` triple has no declared domain, so which class carries it is only derivable from real instance data — stated in the banner above the graph, not hidden.

### Editor

**A separate, author-owned scratch ontology document — never `gst`'s own shipped declarations.** Saving here writes to a fixed graph (`graph:import:ontology-editor`), independent of any installed pack; it's for declaring your *own* new classes/properties, validated through the exact same shapes-and-reasoning gate every RDF import goes through.

1. Paste into the textarea:
   ```turtle
   @prefix gst: <https://graph-owl.dev/packs/gst#> .
   @prefix owl: <http://www.w3.org/2002/07/owl#> .

   gst:CreditNoteTest a owl:Class .
   ```
2. **Preview** — parses only, no write. Shows the one declared subject.
3. **Check** — runs shapes + reasoning. Shows `accepted`/`rejected`/new-inference count.
4. **Save** — `"Saved: 1 subject landed."` Verified live via a direct `/sparql` query afterward: the triple genuinely exists in `graph:import:ontology-editor`.

**A real constraint worth knowing if Save ever rejects something Check accepted**: predicates need to be pre-registered (a `predicate registry`, separate from the namespace registry) before a value can be asserted under them — adding `rdfs:label "..."` to the snippet above gets rejected at Save (`"predicate 257:label is not defined"`) even though Check accepts it, because Check validates shapes/reasoning, not the predicate registry. `rdf:type` is always fine; a fresh custom predicate on a pack's own namespace may not be.

### Alignments

**Cross-vocabulary class equivalence** — is SNOMED's concept the same *class* as ICD-10's code, confidence-gated, human-confirmable. In a fresh instance this is correctly empty (*"Nothing to review"*): `gst`/`hospitality` are single-vocabulary business packs, so there's nothing in the 0.5–0.8 review band to show. To see it working, seed one real test entry (there's no "add" button in the UI on purpose — this queue only ever shows what a *computed* matcher proposed):

```
curl -X POST http://localhost:8080/alignments -H "content-type: application/json" -d '{
  "kind": "match", "left": "1024:PurchaseInvoice", "right": "1024:Supplier",
  "predicate": "closeMatch", "source": {"kind": "computed", "detail": "demo"},
  "confidence": 0.6, "lossyReverse": false
}'
```

Reload the Alignments view — the entry appears with its confidence, Left/Right, and source. Click **Confirm**: writes a real `skos:closeMatch` triple (verified live via SPARQL), attributed to your own resolved name from `GET /me`. Click **Reject** (or re-run the same `curl` at `"confidence": 0`) to clear it again without ever having written anything.

## 6. Explore — the same invoices, as a graph, with real connectivity

Go to **Explore** (left nav, under UNDERSTAND). Paste one of the two real mismatch invoice IRIs found in step 8 below — `https://graph-owl.dev/packs/gst#books-11AABCZ9999A1Z1-INV-APR-013` — into the entity picker, or navigate to it from a search result.

Click the `PurchaseInvoice` node in the canvas. The right-hand panel shows its own findings (`gst:ITCNotAvailable` or similar, with the bound evidence values), and below that, **"How connected is this?"** — real degree centrality over this exact bounded neighbourhood (`graph-owl-analytics`'s `petgraph`-backed degree/orphan computation, reachable from the console for the first time as of this session): a table of every node in the walk, incoming/outgoing edge counts, and which ones are orphaned within it. This is genuinely computed, not estimated — the same numbers a direct SPARQL count over the same neighbourhood would produce.

## 7. Validate — run the glossary/ontology consistency check

Go to **Studio → Validate → Run validation**. Note: findings shown may span the whole glossary, not just terms touched in this walkthrough (a fresh instance shows generic seeded findings like `Vendor: missing definition` alongside anything from this scenario) — read it as glossary-wide, not scenario-scoped.

## 8. SPARQL — find the real mismatches

Go to **Studio → SPARQL**. Run these in order.

**a. See what data actually exists** (the default box only queries the *unnamed* default graph, which is empty — everything here lives in named graphs):

```sparql
SELECT DISTINCT ?g WHERE { GRAPH ?g { ?s ?p ?o } }
```

You'll see 10 graphs, including a matched pair for one supplier's period: `graph:import:reco-302f19ab8be4-books` (the purchase register) and `graph:import:reco-12f30df6d73d-gstr2b` (that supplier's GSTR-2B).

**b. The real reconciliation query** — every purchase-register invoice whose `gst:invoiceKey` (the ontology's own *"normalized invoice matching key"*) has no counterpart in GSTR-2B:

```sparql
PREFIX gst: <https://graph-owl.dev/packs/gst#>
SELECT ?booksInvoice ?key WHERE {
  GRAPH <https://graph-owl.dev/ns/catalog#graph:import:reco-302f19ab8be4-books> {
    ?booksInvoice a gst:PurchaseInvoice ;
                  gst:invoiceKey ?key .
  }
  FILTER NOT EXISTS {
    GRAPH <https://graph-owl.dev/ns/catalog#graph:import:reco-12f30df6d73d-gstr2b> {
      ?gstr2bInvoice gst:invoiceKey ?key .
    }
  }
}
```

**Result, verified live: exactly 2 rows out of 15 purchase invoices for this supplier/period** —

| booksInvoice | key |
|---|---|
| `gst#books-11AABCZ9999A1Z1-INV-APR-013` | `INVAPR013` |
| `gst#books-22AABCX8888B1ZQ-INV-APR-014` | `INVAPR014` |

These two are real `PotentialMismatch` cases — claimed in the books, absent from GSTR-2B.

> **A pitfall worth knowing, found while verifying this**: a version of this query built with `BIND(IRI(REPLACE(STR(?booksInvoice), "books-", "gstr2b-")) AS ?candidate)` + `FILTER NOT EXISTS` on that derived IRI returns **15 of 15 as "unmatched" — wrong**, even though several of those invoice keys (e.g. `INVAPR003`) genuinely exist in the GSTR-2B graph. The join-on-shared-literal form above is the one that gives the correct, verified answer. Prefer matching on the domain key (`gst:invoiceKey`) over reconstructing an IRI by string substitution.

**c. Pull one mismatch's full detail** (swap in the other invoice's IRI to see its own facts):

```sparql
SELECT ?p ?o WHERE {
  GRAPH <https://graph-owl.dev/ns/catalog#graph:import:reco-302f19ab8be4-books> {
    <https://graph-owl.dev/packs/gst#books-11AABCZ9999A1Z1-INV-APR-013> ?p ?o
  }
}
```

This returns all 16 of its facts — `hsnCode "8471"`, `igst "17100"`, `cgst "0.0"`, etc. — the line-item detail behind the mismatch.

## 9. Ask GraphOWL — the same answers, from the search bar

The header's **"Search or ask GraphOWL…"** box (any page) does real catalog search *and*, now, real grounded question-answering. This needs one extra local process running — it's a deliberately separate, out-of-process service (`examples/gst-reconcile/ask_server.py`), not part of the Rust binary, matching graph-owl's own rule that agent/LLM orchestration stays outside the console's own process:

```
LLM_API_BASE_URL=http://localhost:11434/v1 LLM_MODEL=<your ollama model> \
  python3 examples/gst-reconcile/ask_server.py
```

(`LLM_API_BASE_URL`/`LLM_MODEL` are optional — omit them for structured-only answers with no narration. Any OpenAI-compatible endpoint works, Ollama included, at zero code changes.)

Two real, verified questions:

1. **Type**: `Which invoices are unpaid past 180 days?` → click **Ask GraphOWL: "..."**. This matches fixed evaluation question 5 (word-overlap routing against `packs/gst/eval/questions.md`'s own 15 questions) and returns a real, narrated table of two overdue invoices with real dates, day-counts and the citation `Section 16-2-d`.
2. **Type**: `how many invoices are there for patel chemicals and co` → **Ask GraphOWL**. This doesn't match any of the 15 fixed questions — it's routed by a second, independent tier that recognises "invoices ... for/from/by `<party>`", fuzzy-matches the name against all 14 real suppliers in the graph (`gst:supplierName`, punctuation-normalized so "and" matches "&"), and returns: *"2 invoices for Patel Chemicals & Co: books-19AABCP8087C1ZV-INV-APR-006, books-19AABCP8087C1ZV-INV-MAR-006."* Cross-checked against a direct SPARQL count beforehand.

**The honest scope, stated in the dropdown itself**: this is not general natural-language Q&A. A question matching neither the 15 fixed questions nor the "invoices for `<party>`" pattern gets a plain *"None of the fixed reconciliation questions this can answer look like that"* — not a guess. For genuinely open-ended questions, `integrations/langchain/examples/gst_investigation_agent.py`'s real MCP tool-calling agent is the answer (also Ollama-compatible, also verified live) — it just costs tens of seconds per run, which is why it isn't wired into a search-bar-shaped endpoint.

## 10. Export — hand off the finding

Go to **Studio → Export → CSV → Export as CSV**. A new row appears in **Export history** with status `RUNNING`, then `COMPLETE` once done, with a **Download** link. Other formats available: JSON-LD, RDF (Turtle), SKOS, Excel.

---

## What this exercised

| Surface | Real action taken |
|---|---|
| Build | Read `PotentialMismatch`; created, inspected `ItcReversalCandidate` |
| Glossary | draft → in review → approved, including the real reviewer-assignment constraint |
| Business view | Plain-language definition check |
| Proposals | Reviewed the governance queue (noted: not pack-scoped) |
| Graph | Authored a real `related` edge between two terms |
| Ontology → Graph/Table | Explored the real 18-class / 11-relationship / 33-property GST model |
| Ontology → Editor | Authored, checked and saved a real new class through the shapes/reasoning gate; hit and documented the real predicate-registry constraint |
| Ontology → Alignments | Seeded, confirmed and cleanly retracted a real cross-vocabulary alignment, including the real `skos:closeMatch` triple it writes |
| Explore | Opened a real mismatch invoice's neighbourhood and read genuine `petgraph`-computed connectivity |
| Validate | Ran glossary/ontology consistency validation |
| SPARQL | Ran a real cross-graph reconciliation query against live instance data, found 2 genuine mismatches |
| Ask GraphOWL | Got a real narrated answer to a fixed evaluation question, and a real supplier-invoice count for a question none of the 15 covers |
| Export | Queued and confirmed a real CSV export |

If you want to reproduce this from scratch: `ItcReversalCandidate` and the demo `CreditNoteTest`/alignment entries were all deleted or retracted after verification, so the glossary and ontology-editor scratch document are back to their starting state — every numbered step above will work exactly as written.
