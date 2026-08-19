# Real GST APIs for Reco Now — research and integration plan

Status: research complete, nothing implemented yet. Plan-only document — no
code changes are in this commit.

## TL;DR

Reco Now today has **zero live API integration**. Every dataset (books,
GSTR-2A, GSTR-2B, payments, GRN) is a manually uploaded CSV/XLSX/JSON file
(`POST /api/clients/{client}/periods/{period}/datasets/{kind}/upload`,
`ext-apps/RecoNow/backend/app/main.py:631`). `ext-apps/Reco/SAMPLE/*.csv` is
that same shape, hand-authored — not pulled from anywhere live.

There is **no self-serve, no-strings-attached way to pull real GSTR-2A/2B/1/3B
data from a script**. GSTN does not expose its return-filing APIs to
individual developers at all — only to licensed **GSPs** (GST Suvidha
Providers), and even a GSP can only fetch a taxpayer's data after that
taxpayer explicitly authorises API access from inside their own GST portal
login (Profile → "Manage API access", a session that expires in 6h–30 days).
This is true no matter which vendor sits in front of it — ClearTax, Sandbox.co.in,
MasterGST, Cygnet, all of them require this same taxpayer-authorised session for
real return data. **This is the single fact that shapes every recommendation
below** — it is not a signup-friction problem, it's by design, because this is
live tax-filing data.

What genuinely works self-serve, today, with no live business or GSTN
approval needed:

| Tier | What | Real data? | Needs a live GSTIN's portal session? | Fits Reco Now's data model |
|---|---|---|---|---|
| **A** | GSTIN verification / enrichment | Yes, real GSTN registry | No | Enrich `Supplier`, not a reconciliation dataset |
| **B** | e-Invoice IRN sandbox (NIC) | Sandbox data, NIC's own published test GSTINs | No (uses NIC's test credentials) | Adjacent — IRN, not GSTR-2A/2B |
| **C** | GSTR-1 / 2A / 2B / 3B fetch via a GSP | Yes, real, exactly the shape Reco Now needs | **Yes** | Direct replacement for the CSV upload — but only for a real pilot client |
| **D** | e-Way Bill API | Real | Yes, plus a manual GSTN helpdesk approval | Not worth building yet |

Recommendation: build **A** first (it's real, free, and ships this week),
keep **B** in reserve for a future e-invoice feature, treat **C** as the real
integration but gate it behind "do we have a pilot client's portal
authorisation," and don't bother with **D** yet.

---

## 1. What Reco Now has today (verified against the code, not assumed)

- `ext-apps/Reco/SAMPLE/*.csv` — 11 files: `purchase_register_{mar,apr,aug}2026.csv`
  (the books side), `gstr2b_{mar,apr,aug}2026.csv` + one `_with_itc` variant
  (the portal side), `gstr2a_aug2026.csv`, `payments_{mar,apr}2026.csv`,
  `grn_mar2026.csv`. All hand-authored fixtures matching the column headers
  the app's auto-mapper expects — not exports from any live system.
- Upload path: `POST /api/clients/{client_id}/periods/{period_id}/datasets/{kind}/upload`
  accepts `.csv`, `.xlsx`, `.xlsm`, `.xls`, or `.json` (`main.py:288`,
  `_parse_upload`), where `kind` is one of `books`, `gstr2b`, `gstr2a`,
  `gstr1`, `gstr3b`, `payments`, `grn` (`CHECKS_BY_KIND`, `main.py:116`).
- Column mapping is keyword-based, not positional (`_auto_map` +
  `_FIELD_KEYWORDS`, `main.py:156`) — a header containing "taxable value" or
  "taxable amount" both resolve to the same internal `taxable` field. This
  matters below: **a JSON payload from a real API, reshaped to use headers
  the keyword table already recognises, uploads through the exact same path
  a CSV does — no new ingestion code required for a first cut.**
- `gstr3b` is handled differently from the others: it's a **period total**
  (Table 4A/4B(1)/4B(2)/4C/4D(1)/4D(2)), not an invoice list — see
  `itc_3b.py` and the `itc_4a`/`itc_reversed_4b1`/… fields in `FIELD_LABELS`
  (`main.py:99-108`). A GSTR-3B API integration returns one row per period,
  not one row per invoice.
- No API client code exists anywhere in the tree today: `grep -rl "api.gst.gov.in\|GSP\|GSTN\|einvoice" backend/app` returns nothing.

## 2. The access model — read this before picking an API

GSTN's own return-filing system (GSTR-1/2A/2B/3B, IMS) is **not open**. The
practical chain is always:

```
Your app  →  a GSP's API (ClearTax / Sandbox.co.in / MasterGST / Cygnet / …)
          →  GSTN's actual System APIs
```

To pull a taxpayer's real GSTR-2B, the GSP needs a session token that **only
that taxpayer can create**, by logging into `services.gst.gov.in` themselves,
going to My Profile → Manage API Access, and granting a GSP session for a
window between 6 hours and 30 days
([ClearTax: GST API Access](https://cleartax.in/s/gst-api-access)). No GSP,
however friendly its developer console, can skip this — it is GSTN's own
authorisation gate, not a vendor limitation. This is also why GSTN's own
"[GST Developer Portal](https://developer.gst.gov.in/apiportal/)" — which
hosts specs, sample code and public keys — still requires GSP/ASP
registration to get working sandbox credentials; it is a spec repository, not
a self-serve key generator.

Two consequences for Reco Now specifically:

- **Reco Now's current sample GSTINs are fictional** (`27AABCS1429B1Z8` etc.)
  and cannot authorise anything on the real GST portal. Tier C literally
  cannot be demoed against today's demo data — it needs an actual registered
  business willing to grant API access.
- Tier A and B don't have this problem: GSTIN verification reads public
  registry data with no taxpayer-side authorisation step, and NIC's
  e-Invoice sandbox ships its **own** published test GSTINs specifically so
  you don't need a real business to try it.

## 3. The candidates, tier by tier

### Tier A — GSTIN verification / enrichment (build this first)

**What it is**: given a GSTIN, returns the real registered legal name, trade
name, registration status (active/cancelled), constitution of business,
jurisdiction, and filing frequency — pulled from the actual GST registry.
Not return data, not reconciliation-shaped, but real and immediately useful.

**Providers with a genuinely self-serve free tier** (signup = email, no
GSTIN/business needed, no portal-authorisation step):

- **[Sandbox.co.in](https://sandbox.co.in/gst)** — part of the same
  `developer.sandbox.co.in` platform that also documents GSTR-2A/2B/e-Invoice
  endpoints (see Tier C). "Create account instantly," stated free tier to
  start.
- **[gstincheck.co.in](https://gstincheck.co.in/)** — free API key by email,
  20 free requests to test, real (not checksum-only) registry data.
- Several smaller providers with similar free-request quotas surfaced in
  search (Microvista, AppyFlow, KnowYourGST) — worth a five-minute bake-off
  before committing, since this tier is commoditised and terms change often.

**Why it's worth building even though it's small**: Reco Now already has a
`Supplier` concept (`gst:Supplier` nodes, per `case_graph.py`'s
`BADGE_FOR_CLASS`). Right now a supplier's name/GSTIN comes only from
whatever a books/portal file happens to say — nothing confirms the GSTIN is
even real, still registered, or that the legal name matches. A verification
call at upload time (or on demand from the supplier's own detail view) is a
real, live, "the AI/data isn't just from your file" feature the user has
asked for repeatedly this session, and it's the one tier that ships without
waiting on a pilot client.

**Integration point**: new module `app/gstin_verify.py` (same shape as
`app/graphowl_client.py` — a thin best-effort HTTP wrapper with a typed
result and a graceful `None`/refusal on failure, not a bare `except
Exception`, per this project's own documented lesson about swallowed
`NameError`s). Call it from wherever a GSTIN is first seen during upload
mapping, and surface it as a badge on the supplier detail drawer next to the
existing `GeneratedBadge` pattern — "verified against GSTN registry, as of
&lt;date&gt;" vs "not yet verified."

### Tier B — e-Invoice IRN sandbox (NIC) — reserve for a future e-invoice feature

**What it is**: [einv-apisandbox.nic.in](https://einv-apisandbox.nic.in/) —
NIC's own public developer portal for the Invoice Registration Portal (IRP).
Generate/cancel an IRN, fetch invoice details by IRN, generate an e-way bill
from an IRN, GSTIN sync. This is genuinely try-it-today: the portal
[documents its own test GSTINs](https://einv-apisandbox.nic.in/) (e.g.
`33GSPTN1882G1Z3`, `27GSPMH1881G1ZH`) specifically so a developer doesn't
need a real registered business to exercise the flow.

Note there are now **six government-empanelled IRPs**, not just NIC — Cygnet,
ClearTax, IRIS and others also run IRP infrastructure with their own sandbox
front ends (e.g. `einvoice6.gst.gov.in` for IRIS's IRP) — worth checking if
one has a friendlier onboarding flow than NIC's own when this tier gets
built.

**Why it's tier B, not tier A**: it doesn't touch reconciliation. Reco Now's
core loop is books-vs-portal comparison; IRN generation/validation is a
different capability (was this invoice ever registered as an e-invoice? does
its IRN match what's in GSTR-2B's `Note Type`/IRN columns?). Real, useful,
but a separate feature from what's being asked for right now — flagged here
so it isn't lost, not because it's not worth doing.

### Tier C — GSTR-1 / 2A / 2B / 3B fetch (the actual CSV-upload replacement)

**What it is**: the API-shaped version of exactly what Reco Now ingests
today. [Sandbox.co.in's public API reference](https://developer.sandbox.co.in/reference/gstr-2a-b2b-api)
documents a GSTR-2A B2B endpoint; the same platform's
[GST overview](https://developer.sandbox.co.in/api-reference/gst/overview)
lists GSTR-1 filing, GSTR-2B fetch/reconciliation, GSTR-3B filing, GSTR-9,
ITC reconciliation, and bulk GSTIN validation as available operations. Other
GSPs (ClearTax, MasterGST, Cygnet, WhiteBooks) offer materially the same
surface, wrapped differently.

**Why this can't be demoed today**: as explained in §2, every one of these
still needs the taxpayer to grant portal-authorised API access to that GSP for
that specific GSTIN. There is no synthetic-data escape hatch here the way
there is for Tier B — GSTR-2A/2B is inherently "this specific registered
business's real filing data," and no vendor sandbox fakes that away.

**When to build this**: the moment there's a real pilot client (a real CA
firm or business with a real GSTIN willing to click "Manage API access" on
their own GST portal login) rather than the synthetic Demo Corp data Reco
Now runs on now. At that point:

1. Pick one GSP (Sandbox.co.in is the strongest self-serve candidate found —
   instant account creation, public API docs, a stated free tier — but
   re-verify current pricing/limits at signup time, since commercial terms on
   these platforms move fast).
2. The pilot client authorises that GSP on `services.gst.gov.in`.
3. Backend fetches GSTR-2A/2B/3B for the period, reshapes the response to the
   header names `_FIELD_KEYWORDS` already recognises (or extends that table —
   cheaper than inventing a parallel ingestion path), and either (a) feeds it
   through the existing upload endpoint as a synthesized JSON payload, or (b)
   a new `POST /datasets/{kind}/fetch` endpoint that skips the file round-trip
   and calls `_build_dataset` directly. (b) is the better long-term shape but
   (a) is a legitimate first slice — it proves the mapping is right using
   infrastructure that already has test coverage.
4. GSTR-3B's period-total shape (§1) means its fetch is a single small
   response per period, not a page-through — simpler to build than 2A/2B, and
   worth doing first within this tier for that reason.

### Tier D — e-Way Bill API — not worth building yet

**What it is**: NIC's e-way bill generation/tracking API,
[docs.ewaybillgst.gov.in](https://docs.ewaybillgst.gov.in/apidocs/index.html).

**Why it's last**: unlike the e-Invoice sandbox, e-way bill pre-production
access is explicitly **not self-serve** — the onboarding doc says a
GSP/taxpayer/transporter must be *shortlisted by GSTN* and email a helpdesk
address to request pre-production credentials, then complete IP whitelisting
before going further
([on-boarding process](https://docs.ewaybillgst.gov.in/apidocs/on-boarding-process.html)).
That's a multi-week manual approval cycle for a capability Reco Now doesn't
have a current use case for (it reconciles tax credit, not goods movement).
Revisit only if a future feature actually needs e-way bill data.

## 4. Recommended build order

1. **Tier A, GSTIN verification** — one new module, one new endpoint, one
   badge on the supplier drawer. Real external data, ships this week, no
   dependency on anyone else's cooperation. TDD as usual: RED on a fake
   GSTIN returning "not found," RED on a real-shaped response populating the
   badge, GREEN, then wire into the upload-mapping flow.
2. **Decide on a pilot client** (a real GSTIN, someone willing to click
   "Manage API access") — this is a business decision, not an engineering
   one, and it's the actual blocker for Tier C. Nothing else in this plan
   should wait on it.
3. **Tier B, e-Invoice sandbox**, opportunistically — good next demo of "real
   external system, real API, nothing hand-waved," and doesn't need the
   pilot-client decision either. Lower priority than A only because it's
   adjacent to reconciliation rather than inside it.
4. **Tier C, GSTR-2A/2B/3B fetch**, once a pilot client exists — start with
   GSTR-3B (single period-total response) before GSTR-2A/2B (invoice-level,
   larger, dynamic over time per the existing `2A Pulled On` field's own
   comment about 2A being a moving target).
5. **Tier D** — park it.

## 5. Costs, secrets, and compliance — before any of this is wired up

- **API keys are real secrets.** A GSP key can pull a real business's filing
  data; treat it with the same care as a database credential — never
  committed, never logged, environment-variable or secrets-manager only.
- **Tier C data is live taxpayer data**, not synthetic fixtures. Once a real
  pilot client's data flows through Reco Now, this stops being a demo and
  becomes a system holding real financial/compliance information — retention,
  access control and the product's own `plans/00i-licensing.md`-style
  discipline (if RecoNow has an equivalent data-handling doc; if not, this is
  worth writing before Tier C, not after) all become load-bearing, not
  theoretical.
- **Re-verify pricing and free-tier limits at signup time for every vendor
  named above.** These are commercial products with terms that change
  independently of this document; nothing here should be read as a locked-in
  quote.
- **Domain-neutrality**: this is entirely GST-specific integration work. Per
  this project's standing rule, none of it belongs in graph-owl's Rust
  crates — it lives in `ext-apps/RecoNow/backend/app/` (or `packs/gst/`) the
  same way every other GST-specific concept in this codebase already does.

## Sources

- [ClearTax — All You Need to Know About GST API Access](https://cleartax.in/s/gst-api-access)
- [GST Developer Portal (official)](https://developer.gst.gov.in/apiportal/)
- [Sandbox.co.in — GST API product page](https://sandbox.co.in/gst)
- [Sandbox.co.in — GST API overview (developer docs)](https://developer.sandbox.co.in/api-reference/gst/overview)
- [Sandbox.co.in — GSTR-2A B2B API reference](https://developer.sandbox.co.in/reference/gstr-2a-b2b-api)
- [GST-NIC — e-Invoice API Sandbox](https://einv-apisandbox.nic.in/)
- [e-Way Bill API Developer's Portal](https://docs.ewaybillgst.gov.in/apidocs/index.html)
- [e-Way Bill API on-boarding process](https://docs.ewaybillgst.gov.in/apidocs/on-boarding-process.html)
- [IRIS IRP — Access to Sandbox](https://einvoice6.gst.gov.in/content/kb/access-to-sandbox/)
- [gstincheck.co.in — free GSTIN validator](https://gstincheck.co.in/)
