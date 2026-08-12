# Running the console + Reconciliation Agent — what was fixed, how to start it, and what everything is for

**Date**: 12 August 2026

## 1. What was actually broken, and what was fixed

| Symptom you saw | Real cause | Fix |
|---|---|---|
| Agent chat gave no results | `agent_service` (the Python backend behind the chat) was never started — only `graph-owl-server` was running | Started it: `python3 -m uvicorn agent_service.server:app --port 8899` |
| "OIDC shouldn't be disabled" | An earlier fix had disabled it to work around a token problem | Restored real OIDC — `authentication="oidc"` confirmed in the server log, real Auth0 sign-in wall confirmed in the browser |
| Packs / Vocabulary / Governance / Agent chat all looked empty after signing in | **`scripts/demo.sh` unconditionally deletes and recreates the Postgres container on every run** (`docker rm -f` — confirmed by reading the script, not assumed). Your own `./scripts/demo.sh --gst` run (pasted in your message) recreated it fresh, then failed to auto-catalogue or load the GST pack for lack of a token — leaving an empty, OIDC-authenticated server | Re-ran the seed pipeline once (`OIDC_ISSUER= OIDC_AUDIENCE= ./scripts/demo.sh --gst`, the workaround the script itself prints), then restarted the **server binary directly** — not `demo.sh` — with no OIDC override, so it picked up real `OIDC_ISSUER`/`OIDC_AUDIENCE` from `.env` while pointed at the now-seeded database. Verified via a direct `psql` query (bypassing HTTP auth) that 124 assets, the GST namespace, and 9 findings survived the switch |
| Chat history vanished when you clicked to another tab and back | A real bug: `AgentChat` was **unmounted**, not hidden, on every section switch (`App.tsx`'s routing rendered it inside a conditional that destroys the whole component). That also silently killed any in-flight investigation's stream, not just the transcript | `AgentChat` now renders in a permanently-mounted sibling, toggled with CSS instead of conditional rendering. Verified live: asked a question, navigated to Governance, navigated back — thread and its state were still there |
| "No domain pack installed" | Same data wipe as above — the GST pack's namespace genuinely didn't exist | Fixed by the re-seed above. Confirmed live: `Admin → Packs` now shows GST, `gst · 1024 · https://graph-owl.dev/packs/gst#` |
| Vocabulary / Classifications / Domains / Ontology packs empty | Same data wipe | Fixed by the re-seed. `Vocabulary → Glossary` now shows `GstReconciliation`; `Ontology packs` shows it was imported from `packs/gst/glossary.ttl` |
| Governance: nothing happens when you look at it | Two separate things: (a) same data wipe meant there was nothing to validate/reason over, and (b) **the page genuinely requires a click** — "Run validation" and "Run reasoning" are real buttons, not auto-triggered | Clicked both live: `Run validation` → "0 shape(s) ran, nothing violated" (correct — this pack registers no SHACL shapes); `Run reasoning` → "Full run — 0 derived, 0 replaced, maintained to t=184" (correct — no OWL entailment rules in this pack). Both work; they just needed data and a click |
| `Admin → Agent activity` still empty | **Not a bug — a real architectural fact worth knowing.** That page lists agents with their own granted MCP identity (`PUT /agents/{id}/grant`) and what they did under it. The Reconciliation Agent deliberately has no such identity: every MCP call it makes runs under **your own signed-in token**, forwarded per request, specifically so the chat agent can never see or do more than you can. That's why this ledger has nothing from chat — there's no separate "agent" principal to show | Left as-is; changing it would mean giving the chat agent its own standing identity, which is a real security trade-off, not a one-line fix. Flagging it rather than silently working around it |
| A question still fails right now | `Error code: 429 … FreeUsageLimitError: Rate limit exceeded` from the LLM provider (`opencode.ai/zen`, model `deepseek-v4-flash-free`) | **External, not a code bug.** The configured fallback model (`laguna-s-2.1-free`) shares the same account/key, so it hits the same limit — retrying won't help until the free-tier window resets, or you configure a different provider/key |

## 2. How to start everything going forward

**Two processes, in two terminals.** Do this once per work session; don't repeat step 1 casually — see the warning below.

### Terminal 1 — the console + graph-owl-server

If you're starting completely fresh (empty database is fine, or you don't mind losing current data):
```bash
./scripts/demo.sh --gst
```
This seeds Postgres, catalogues the demo estate, loads the GST pack, and starts `graph-owl-server` with real OIDC (reads `.env`). It will print the same "Cannot load the GST pack: OIDC is active but no token was obtained" error you saw — that's expected the first time; **the workaround it prints is correct**:
```bash
OIDC_ISSUER= OIDC_AUDIENCE= ./scripts/demo.sh --gst
```
This runs the seed/catalogue/pack-load steps with OIDC off just long enough to write the data, using a real server process the whole time. Once that completes, if you want the *serving* server to actually require sign-in (which you do), restart the binary without the override:
```bash
lsof -ti:8080 | xargs kill 2>/dev/null
DATABASE_URL="postgres://postgres:postgres@localhost:55432/graphowl" \
  ./target/release/graph-owl-server &
```
The data you just seeded is in Postgres, not in the process, so this restart keeps it and now requires real Auth0 sign-in.

**⚠️ `scripts/demo.sh` is not idempotent — it deletes and recreates Postgres every time it runs (`docker rm -f`).** Once you have real data you want to keep, never run `demo.sh` again in that session. To restart the server (e.g. after a UI rebuild), always restart the **binary directly** as shown above, pointed at the same `DATABASE_URL`. This is exactly what bit you this time.

### Terminal 2 — the Reconciliation Agent's backend
```bash
cd integrations/langchain
source .venv/bin/activate
set -a; source ../../.env; set +a
export GRAPH_OWL_SERVER=http://localhost:8080
python3 -m uvicorn agent_service.server:app --port 8899
```
With real OIDC on, you do **not** need to set `GRAPH_OWL_TOKEN` — the console forwards your own signed-in token with every question.

### Then
Open `http://localhost:8080`, sign in with your identity provider, and go to the **Agent** tab in the left nav.

## 3. What data to give the Reconciliation Agent

Attach two files via the paperclip button in the chat composer:

1. **A GSTR-2B export**, JSON, one object per invoice with fields like: `invoiceNumber`, `supplierGstin`, `taxableValue`, `cgst`/`sgst`/`igst`, `totalTax`, `itcAvailability`, `filingStatus`.
2. **Your purchase register**, JSON, the same invoices as booked internally: `invoiceNumber`, `vendorGstin`, `taxableValue`, `cgst`/`sgst`/`igst`, `totalTax`.

Ready-made samples already exist and are safe to reuse or use as a template for your own data:
- `integrations/langchain/agent_service/sample_data/gstr2b_sample.json`
- `integrations/langchain/agent_service/sample_data/purchase_register_sample.json`

(11 invoices, one planted in each of 8 outcome categories — matched, amount mismatch, missing from one side or the other, GSTIN transposition, ITC-not-available, cancelled/reversed, and genuine supplier mismatch. Full answer key and two live-verified Q&A transcripts are in `plans/agent-file-upload-results.md` from the session that built this.)

Click a staged or attached file chip any time to preview exactly what was uploaded, in a popup.

## 4. Questions worth asking, moderate to complex

**Straightforward** (already live-verified against a hand-derived answer key — see `plans/agent-file-upload-results.md`):
- "Reconcile these two attached files and tell me every mismatch, grouped by category."
- "Which invoices are missing from the purchase register?"

**Moderate** — needs the model to combine categories, not just list them:
- "Which mismatches are simple data-entry errors I can fix myself, versus ones I need to chase the supplier for?"
- "Group the findings by counterparty GSTIN — which suppliers account for the most mismatches?"
- "For every amount mismatch, tell me whether the register overstates or understates versus GSTR-2B, and by how much in total."

**Complex** — needs real domain reasoning, already proven live:
- "Which invoices carry real ITC risk that should be escalated first, and why?" (live-verified — correctly separated inadmissible/must-reverse credit from merely-correctable typos from a legitimate-but-unclaimed opportunity)
- "If I can only chase three things before the filing deadline, which three matter most and what's the financial exposure of ignoring the rest?"
- "Build me an audit-ready action list: for each mismatch, who owns the fix (finance vs. supplier vs. nobody), and what's the severity?"

**One real limitation to know**: each question starts a brand-new, independent investigation with no memory of earlier ones. A follow-up like "based on the reconciliation you just ran…" in a fresh thread won't work — re-attach the files (or restate the question fully) every time.

## 5. What the other empty-looking sections are actually for

**Obligations** — a calendar of compliance deadlines derived from the graph: GST return filing dates, and (per the roadmap) other law-graph-traversed obligations tied to specific provisions (e.g. Rule 36(4)'s ITC reversal window). It's populated by data a pack contributes — the GST pack is the first one wired up to it. The point is to turn "the law says X by date Y" into something the graph can surface proactively rather than something buried in a PDF.

**Vocabulary → Glossary / Classifications / Domains / Ontology packs** — four different governance concerns that happen to share one page:
- **Glossary**: business terms (e.g. "GstReconciliation") that get attached to assets so a column named `amt_2` can carry the meaning "Reconciled Taxable Value."
- **Classifications**: sensitivity/PII-style tagging taxonomies.
- **Domains**: groupings of assets by business area (e.g. "Finance," "Retail Ops") rather than by system.
- **Ontology packs**: the actual RDF/OWL/SKOS source a pack ships (`packs/gst/glossary.ttl` for GST) — read-only here, since it's imported, not hand-authored per Governance's own convention ("extend it with an override rather than editing a term directly").

**Governance (`Run validation` / `Run reasoning`)** — two independent checks over the same graph, both **report-only, never blocking**:
- **Validation** runs SHACL shapes against the data and lists what's structurally wrong (e.g. "every table must have an owner").
- **Reasoning** runs the OWL/RDFS entailment engine and lists what got newly derived or retracted from existing facts (e.g. inferred subclass relationships).

Both being "0" right now is correct, not broken — the GST pack doesn't currently ship shapes or entailment rules; it's a reconciliation pack, not an ontology-heavy one. The buttons exist so any future pack that *does* ship shapes/rules has somewhere to run them.

## 6. Still open

- The LLM free-tier rate limit (429) will keep blocking real answers until it resets or you configure a different `LLM_MODEL`/provider/key in `.env`.
- "Select and install a domain pack from the UI" doesn't exist yet — packs are currently installed via the `graph-owl-load-pack` CLI (what `demo.sh` calls for you). `Admin → Packs` is read-only discovery of what's already installed, not a browse-and-install catalog. That would be new scope, not a bug fix, if you want it.
