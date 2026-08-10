# GSTR-2B: what the provider actually does — measured, 11 August 2026

**Purpose: establish real provider behaviour before designing around it.** Every
line here is from a live call against a provisioned GSP sandbox, not from
documentation. Where documentation and observation disagree, the observation is
recorded and the documentation is marked wrong.

No credentials appear in this file. They live outside the repository.

---

## 1. The documented endpoint does not exist

The credentials note supplied with the sandbox describes:

```
POST /api/v1/gstr2b/fetch     — Fetch GSTR-2B (ITC statement)
Auth: OAuth 2.0 bearer token (client_credentials grant)
```

**Neither is real.** The provider's own Postman collection — the authoritative
list, extracted from the supplied archive — has no `/api/v1/` prefix, no
`gstr2b/fetch`, and no client-credentials grant anywhere. The GSTR-2B surface is
three calls:

```
GET  /gstr2b/all?gstin=&rtnprd=&filenum=&email=
PUT  /gstr2b/gen2b?email=
GET  /gstr2b/get2b?gstin=&int_tran_id=&email=
```

That `gen2b` is a `PUT` and `get2b` takes an `int_tran_id` says the shape: GSTR-2B
is **generated asynchronously and then collected**, not fetched in one call. Any
design assuming a single synchronous fetch is wrong before it starts.

**Take from this**: the "Quick spec" in a vendor's marketing copy is not an API
reference. The Postman collection and a live call are.

## 2. Authentication is taxpayer-OTP, not client credentials

Three steps, all `GET`, all on the base URL directly (no `/gst` path segment):

```
GET /authentication/otprequest?email=…     → txn; OTP sent BY GSTN to the taxpayer
GET /authentication/authtoken?email=&otp=  → auth token
GET /authentication/refreshtoken?email=…   → extends, every 6 hours
```

Credentials travel as **headers**, not as a bearer token: `client_id`,
`client_secret`, `gst_username`, `state_cd`, `ip_address`.

**The OTP goes to the taxpayer's registered mobile and email at the GST portal**,
not to the calling application. This is the single most important fact for
anything scheduled: no unattended process can complete a first authentication.

GSTN's own FAQ describes the escape hatch — a **"Longer Session"**, which the
*taxpayer* opts into at gst.gov.in → My Profile. With it, one OTP authentication
is followed by up to **30 days** of `refreshtoken` calls with no further OTP.
Without it, every session needs a human with the taxpayer's phone.

So a nightly reconciliation is only possible for taxpayers who have opted into a
longer session, and it stops working when that window lapses. That is a product
constraint, not an implementation detail.

## 3. The provider abstracts GSTN's payload encryption

GSTN's raw API returns a session encryption key (`sek`) alongside the auth token
and expects payloads encrypted against it — the supplied archive contains a
public key and an encrypt/decrypt sample for exactly this.

**The provider does not pass that through.** Its `authtoken` call takes only
`email` and `otp` — no `app_key` — and `/public/search` returned plain,
unencrypted JSON. So the encryption layer is the GSP's problem, and the
connector's assumption of plain JSON in and out is correct.

This was worth confirming rather than assuming: had it been passed through, the
connector would need AES/RSA handling and the "GSP is replaceable behind a base
URL" claim would have been false.

## 4. What was verified working

`GET /public/search` — no OTP required — returned the real taxpayer record for
the sandbox GSTIN: legal name, jurisdiction (Tamil Nadu, ADYAR), registration
date 01/07/2017, `Regular` taxpayer, status `Active`.

That single call establishes, without any taxpayer consent:

- the sandbox base URL is right;
- the client id and secret are valid;
- `gst_username`, `state_cd` and the registered email are accepted;
- the network path and TLS work;
- responses are plain JSON.

**Everything except taxpayer authorization is therefore proven.** Worth knowing
because it splits a vague "the integration does not work" into a precise "one
portal setting is missing".

`GET /public/rettrack` returned `RET13510 No Record found for the provided
Inputs` — a clean structured error, and a reminder that this sandbox taxpayer has
no filed returns.

## 5. What is blocked, and by what exactly

```
GET /authentication/otprequest
→ {"error":{"errorCode":"AUTH4037",
    "errorMessage":"API access is not available or user expiry Duration is
     less than or equal to auth token expiry duration"},"status_cd":"0"}
```

`AUTH4037` has two readings and both are **settings on the GST portal, owned by
the taxpayer**:

1. API access has not been enabled for the GST username, or
2. the session duration the taxpayer selected is not longer than the auth
   token's own duration.

Neither the GSP nor this project can set either. The remedy is: sign in to the
GST portal as the sandbox taxpayer, **My Profile → Manage API Access → enable**,
and choose a session duration (up to 30 days).

**This is the confirmation of a risk that was previously only predicted.** The
fetch was never the hard part; the authorization lifecycle is.

## 6. Acceptance test: where it stands

| # | Step | Status |
|---|---|---|
| 1 | Obtain the sandbox access token | **Blocked** — `AUTH4037`, taxpayer has not enabled API access |
| 2 | Fetch GSTR-2B for a test GSTIN | Blocked by 1 |
| 3 | Capture the raw JSON | Blocked by 1 |
| 4 | Confirm invoice-level B2B data is returned | Blocked by 1 — **the question that still has no answer** |
| 5 | Feed it through `normalize()` | Not exercised on live data; proven on an API-shaped fixture |
| 6 | Confirm the rules produce the expected findings | Same |
| 7 | Invalid/expired authorization fails loudly | **Passed** — see below |

### Item 7, against real bodies

The connector was run **unmodified** against five responses, four of them
captured verbatim from the live sandbox this session:

| Response | Outcome |
|---|---|
| `AUTH4037` — API access not enabled | refused |
| `RET13510` — no record found | refused |
| unregistered-email rejection | refused |
| empty body | refused |
| success envelope with no `docdata` | refused |
| a genuine response (positive control) | 7 invoices |

**No error body was read as "no invoices".** That is the property that matters:
a failed fetch reported as a clean reconciliation would tell a taxpayer every
claimed invoice is unmatched, or that nothing is wrong, depending on which side
failed. Both are worse than an outright error.

## 6a. A different product on the same account **does** authenticate

The GSP issued a second credential set — 36 state-wise username/password pairs
— for **e-Invoice and e-Way Bill**, which are separate products from GST
returns. Tested, and the result reframes the blocker:

```
GET /ewaybillapi/v1.03/authenticate?email=&username=&password=
  → {"irp":"NIC1","status_cd":"1","status_desc":"If authentication succeeds"}

GET /ewaybillapi/v1.03/ewayapi/getgstindetails?GSTIN=…
  → legal name, state code, taxpayer type REG, status ACT   ("EWAYBILL request succeeds")

GET /ewaybillapi/v1.03/ewayapi/gethsndetailsbyhsncode?hsncode=1001
  → "WHEAT AND MESLIN - Durum wheat"
```

**Authentication here is username + password, with no taxpayer OTP at all.**
That is a fundamentally cheaper integration than GST returns, and it means the
credential chain, the network path and the account are all sound — what fails
on GST returns fails for a reason specific to GST returns.

Three corrections this produced, each of which would have cost an afternoon:

- The base URL is `apisandbox.whitebooks.in`, **not** `…/eway` — the latter
  answers `No API configured for :/eway/ewaybillapi/…`. The credentials note's
  per-product base URLs are wrong for this product too.
- The e-Invoice/e-Way Bill GSTINs are **different subjects** from the GST-returns
  ones (`33AAGCB1286Q003` against `33AAGCB1286Q1ZB`). Using one set for the
  other product returns the same `AUTH4037`, tested both ways, which is easy to
  misread as "the credentials are broken".
- **The API requires the password as a URL query parameter.** That is the
  provider's design, not a choice available to a caller, and it is worth
  recording: query strings land in access logs, proxy logs and browser history.
  A production deployment should treat any host that sees these URLs as holding
  the credential.

**What this does not do is unblock GSTR-2B.** e-Way Bill movement documents are
not input-tax-credit evidence, and the reconciliation this pack performs needs
the return. But it does mean a live GSP integration can be demonstrated today,
and it narrows `AUTH4037` to the GST-returns product specifically — consistent
with that product's card reading `Subscription: Not Started` while its sandbox
credentials read `Enabled`.

**e-Invoice is the one worth testing next**, because IRN data *is*
invoice-level and could serve as a third evidence source beside the purchase
register and GSTR-2B.

## 6b. A second GSP, tested — and the pattern is now clear

The obvious response to one provider blocking is to try another. Done, with a
second GSP's free-trial credentials:

```
POST https://api.sandbox.co.in/authenticate   (x-api-key, x-api-secret)
  → 200, a JWT access token, immediately
```

**No OTP, no taxpayer consent, no provisioning wait.** Where the first provider
needs a taxpayer to approve an OTP before anything, this one issues an
application token from an API key in one call. That difference alone is worth
knowing when choosing a provider.

But GSTR-2B is blocked here too, differently, and the two failures together are
the actual finding:

| Key | Host | GSTR-2B result |
|---|---|---|
| live | `api.sandbox.co.in` | `403 Insufficient privilege` |
| test | `api.sandbox.co.in` | `403 Use production API key for production environment` |
| test | `test-api.sandbox.co.in` | **`200`** — but see below |

The test key against the test host is the only combination that returns `200`,
and the two 403s are worth reading carefully: they are *different* messages, and
the second one names its own fix. A test key is scoped to a test host; pointing
it at production is a configuration error the API states plainly.

**The `200` is a canned mock, not a return.** The body is GSTR-**3B**-shaped —
`tx_pmt`, `sup_details`, `itc_elg`, `intr_ltfee` — carries a GSTIN and period
nobody asked for (`24ABKCS2033B1ZV`, `112024`), and is **byte-identical at 3433
bytes across every period and every `file_number`** tested. There is no
`docdata`, no `b2b`, no `ctin`. The path was right; the environment simply
serves a fixed sample.

That last check is the one that mattered. A `200` with plausible tax figures is
exactly the kind of response a connector would happily normalize and a
reconciliation would happily run against, producing findings about a taxpayer
who does not exist. **Requesting two different periods and comparing byte counts
took one command and turned an apparent success into a known mock.**

### What both providers agree on

Two independent GSPs, two different failure modes, one conclusion: **GSTR-2B
return data is a provisioned, paid capability everywhere.** Free tiers give
authentication, public registry lookups and mock responses. They do not give
another taxpayer's return, which — given the return *is* that taxpayer's private
data — is the correct behaviour rather than an obstacle to route around.

So "try a different GSP" is answered: it changes the auth model, and it does not
change the provisioning requirement.

## 6c. How this is actually done in practice — and it is not the API

Researched rather than assumed, because two providers blocking on the same
capability suggested the premise was worth checking. The established
reconciliation products support **two** acquisition paths, and the API is the
*second* one:

1. **The taxpayer downloads the GSTR-2B JSON from gst.gov.in and uploads it.**
   This is the path every mainstream tool leads with — JSON and Excel import,
   bulk multi-month consolidation, alongside the purchase register.
2. **GSP API fetch**, which still requires the taxpayer to enable API access on
   the portal for that application. It removes the manual step; it does not
   remove the consent.

**This reframes the whole blocker.** Everything measured above is about
path 2 — the *optimisation*. Path 1 needs no GSP, no OTP, no subscription and
no provisioning, because the taxpayer has already authenticated to the portal
as themselves and exported their own data. It is not a workaround or a
degraded mode; it is what the market actually runs on.

And graph-owl already implements it. `graph-owl-gstr2b --from-file` takes a
saved response, and `normalize` finds `docdata` wherever the wrapper puts it —
verified against all three real nestings, which produce **identical Turtle**:

| Shape | Source |
|---|---|
| `data.data.docdata` | GSP API response |
| `data.docdata` | portal JSON download |
| `docdata` | bare |

So the acquisition tiers, honestly ordered by what is available today:

| Tier | Needs | Status |
|---|---|---|
| **Portal JSON upload** | the taxpayer, once a month | **works now, no dependency** |
| GSP API fetch | portal API access + a provisioned subscription | blocked, tickets open |
| e-Invoice / e-Way Bill | api credentials only, no OTP | authenticates, invoice-level data available |

**The product is not blocked.** A CA with a downloaded GSTR-2B JSON and a
purchase register can run the full six-rule reconciliation today. What the API
would add is removing one manual download a month — real convenience, and worth
having, but not on the critical path to a working reconciliation.

That is worth stating plainly because the last several days of investigation
could easily read as "the integration does not work". The integration that
matters works; the one that is blocked is a scheduling convenience.

## 7. What is still unknown

**Whether the sandbox returns realistic invoice-level `docdata.b2b` at all.**
This is the binary question the whole exercise exists to answer and it remains
open. A stateful sandbox with a dummy ledger may return a rich return, an empty
one, or a fixed sample — and the difference decides whether the development loop
can run against live-shaped data or must stay on fixtures.

Two further unknowns behind it:

- the `gen2b` → `get2b` asynchronous cycle: how long generation takes, and what
  `get2b` returns while it is still running;
- whether `/gstr2b/all`'s `filenum` is required, and what it means.

**Nothing was changed in the connector, and no `GSTINConnection` state machine
was built** — deliberately. The states depend on expiry semantics only a real
authorized session can reveal, and designing them from a diagram would guess at
exactly what section 5 shows is worth measuring.

## 8. The next action, and it is not a code change

Enable API access for the sandbox taxpayer on the GST portal, then re-run:

```
GET /authentication/otprequest   → collect the OTP from the taxpayer's phone
GET /authentication/authtoken    → auth token
GET /gstr2b/all                  → the answer to item 4
```

Only after item 4 has a real answer is there anything to design.
