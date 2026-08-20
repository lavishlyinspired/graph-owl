# Plan: "Ask GraphOWL" — wiring the search bar's dead half to a real, local-LLM-backed answer

**Status**: Active

## Goal

The console's header search bar has always said **"Search or ask GraphOWL…"**, but only the *search* half was ever real (`SearchBox.tsx` → `GET /search`, genuine catalog lookup). This wires the *ask* half to a real, grounded, live-verified answer — narrated by any OpenAI-compatible model, Ollama included, with zero code changes needed to swap providers.

## Two deliverables, both requested

1. **Document the external pattern.** graph-owl's own architecture (`plans/00j-language-boundaries.md`) keeps agent/LLM orchestration *out* of the Rust binary and console, on purpose — see that doc's "third category" section. The already-working, already-tested way to point any of this at Ollama:
   ```
   LLM_API_BASE_URL=http://localhost:11434/v1 LLM_MODEL=<your model> \
     python3 examples/gst-reconcile/reconcile_agent.py --all --narrate
   ```
   No API key, no code change — Ollama's `/v1` endpoint is OpenAI-compatible and both reference agents (`reconcile_agent.py`, and `integrations/langchain/examples/gst_investigation_agent.py` for genuinely open-ended, MCP tool-calling questions) already speak it via `LLM_API_BASE_URL`/`LLM_MODEL`/`LLM_API_KEY`.

2. **Wire the console's own search bar to it anyway** — a real, scoped feature, not just documentation. Built as `examples/gst-reconcile/ask_server.py`, a small stdlib-only HTTP wrapper around `reconcile_agent.py`'s existing `answer()`/`narrate()`, plus a new `best_match()` router (word-overlap, prefix-fuzzy, negation-aware — 6 passing tests) that maps free text to one of the 15 fixed evaluation questions from `packs/gst/eval/questions.md`.

## Why this stays a separate process, not a graph-owl-server route

Same reasoning `00j` already states for connectors and embeddings: an LLM call is slow (narration alone ran 10–30s against local Ollama in testing), talks to something outside graph-owl's own release cadence, and must not be able to stall a request path other console features depend on. `ask_server.py` runs on its own port (`8090` by default), proxied only in dev (`vite.config.ts`'s `/ask` entry) — in production this would be its own deployable, the same shape a real Snowflake or dbt connector already takes.

## What this is not

**Not general natural-language Q&A.** `best_match()` routes to one of 15 fixed, curated questions — the same ones `reconcile_agent.py` can score against a hand-written answer key. A question with no good match returns an honest "none of these look like that", not a best-effort guess. The console says this plainly, in the dropdown itself (`searchAskScopeNote`), not just in this doc. For a genuinely open-ended question, `gst_investigation_agent.py`'s real MCP tool-calling loop is the answer — it works (verified live, see below) but costs tens of seconds per run and was deliberately not wired into a search-bar-shaped endpoint for that reason.

## Acceptance criteria

- [x] `ask_server.py` — stdlib only, matching `reconcile_agent.py`'s own purity constraint (`scripts/check-examples-purity.py`-class file).
- [x] `best_match()`: 6 pytest cases — real question-1 wording, real question-5 wording, a candidate-scoped question by its own invoice number, refuses unrelated text, refuses blank input, picks the better of two plausible candidates. All pass; the full `examples/gst-reconcile/` suite (53 tests) still passes.
- [x] `POST /ask {"question": "..."}` → `{kind: "noMatch"}` / `{kind: "error"}` / `{kind: "answered", questionNumber, answer, narration?, narrationError?}`.
- [x] Narration is optional and additive — no `LLM_API_BASE_URL`/`LLM_MODEL` set means a structured-only answer, not a failure.
- [x] Console: the search bar's dropdown gets an "Ask GraphOWL: "..."" action alongside real catalog results (not replacing them), with the scope note shown before any answer arrives.
- [x] `tsc`/`vitest`/`eslint` clean on every touched file.
- [x] **Fully verified live, through the actual running console, both branches:**
  - A question mapping to a label with zero current findings (`gst:Reversed` — confirmed via `GET /findings` that this pack's live data has none right now) correctly answered *"I'm sorry, but I don't have any invoices, citations, or evidence..."* — the grounding design refusing to fabricate, not a bug.
  - A question mapping to a label with real findings (`gst:PaymentOverdue`) correctly returned two real invoices, real dates, real day-counts, real citation (`Section 16-2-d`), narrated by the local Ollama model (`gpt-oss:20b-cloud`) with no invented invoices.

## Slice 2: supplier invoice counts — found live, by a real user, same day

**"How many invoices are there for patel chemicals and co" got an honest `noMatch`** — the 15 fixed questions have no slot for "count invoices by party", even though the graph genuinely has the answer (`gst:supplierName`, `gst:issuedBy`). Tracing why the obvious SPARQL queries first returned nothing was itself informative: `gst:supplierName`/`gst:supplierGstin` triples exist only inside named import graphs (`GRAPH ?g { ... }`), never the default graph — the exact same lesson this session already learned once and had to re-apply here.

Added a second, independent routing tier in `ask_server.py`, checked *before* `best_match()`: `extract_supplier_query()` (regex: "invoice(s) ... for/from/by NAME", requires the word "invoice" so it can't misfire on an unrelated "... for tomorrow") → `best_supplier_match()` (fuzzy match against all 14 real suppliers, punctuation-normalized so "and" matches "&") → two real SPARQL queries (which suppliers exist, which invoices they issued) → a real count and invoice list.

- [x] 7 new pytest cases (extraction from "for"/"from" phrasing, refuses a question naming no party, refuses a non-invoice question, matches a real supplier by partial name, matches despite punctuation, refuses an unrelated name) — 13/13 in this file, 60/60 across the whole `examples/gst-reconcile/` suite.
- [x] **Verified live, through the actual running console**: "how many invoices are there for patel chemicals and co" → *"2 invoices for Patel Chemicals & Co: books-19AABCP8087C1ZV-INV-APR-006, books-19AABCP8087C1ZV-INV-MAR-006"* — both real invoice ids, cross-checked against a direct SPARQL count beforehand.

## Running it

```
LLM_API_BASE_URL=http://localhost:11434/v1 LLM_MODEL=gpt-oss:20b-cloud \
GRAPH_OWL_SERVER=http://localhost:8080 \
python3 examples/gst-reconcile/ask_server.py
```

Must be running for `graphowl-app`'s dev proxy (`/ask` → `:8090`) to have anything to reach — same manual-process model this dev environment already uses for `graph-owl-server` and `vite` themselves. Not started automatically by either.

---
*Delete this file when the plan is complete.*
