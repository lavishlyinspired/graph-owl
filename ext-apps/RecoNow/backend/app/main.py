"""Matcha backend — FastAPI server for GST/indirect-tax reconciliation."""

from __future__ import annotations

import io
import json
import math
import os
import threading
import hashlib
import uuid
from datetime import date, timezone
from pathlib import Path

import pandas as pd
from fastapi import FastAPI, File, HTTPException, UploadFile
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import JSONResponse, Response
from graph_owl_packs.loader import LoadError, load_pack
from graph_owl_packs.reconcile import run_findings

from . import agent_runtime, agents, ai, mcp_client, capabilities, db, exporters, graphowl_client, grounding, native_findings, notice_defence, reconciliation as rc, repo, sample_data, vocabulary, working_paper
# Aliased: main.py already defines an `itc_position` *route handler*, which
# would shadow this import.
import json
import uuid
from datetime import datetime, timezone
from urllib.parse import quote
from decimal import Decimal

from .data_quality import inspect_rows
from . import case_explainer, case_graph, case_narrative, client_report, explain, follow_ups, rule_guidance, working_paper_report
from .reconcile_result import itc_position as compute_itc_position
from .reconcile_result import reconcile_buckets

app = FastAPI(title="RecoNow — Intelligence for Indirect Tax", version="1.0.0")

# graph-owl integration (plans/118-reco-now-integration.md, Slice 1).
# Best-effort throughout: graph-owl may not be running (e.g. a laptop
# with no Docker/Postgres up), and none of reco-now's existing behaviour
# may depend on that — this is additive durable storage alongside
# SESSION, not a replacement for it.
GRAPH_OWL_SERVER = os.environ.get("GRAPH_OWL_SERVER", "http://localhost:8080")
GRAPH_OWL_TOKEN = os.environ.get("GRAPH_OWL_TOKEN")
# reco-now has no pack of its own — it ingests directly into graph-owl's
# canonical packs/gst (plans/119-architecture-audit.md, 16 August 2026
# consolidation). Reaches outside ext-apps/ on purpose: this app lives
# inside the graph-owl monorepo specifically to compose with the
# platform's own reference pack, not fork it.
GST_PACK_DIR = Path(__file__).resolve().parents[4] / "packs" / "gst"

app.add_middleware(
    CORSMiddleware,
    allow_origins=[
        "http://localhost:5173", "http://127.0.0.1:5173",
        "http://localhost:5174", "http://127.0.0.1:5174",
    ],
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)


@app.middleware("http")
async def _optional_bearer_auth(request, call_next):
    """Identity, when the deployment asks for it.

    Every route trusts the `client_id` in the URL, and row-level isolation is
    tested — but isolation without identity is a courtesy, not a control.
    Setting `RECONOW_API_TOKEN` gates every `/api` route behind a bearer
    token; unset keeps the single-firm desktop default open. `/api/health`
    stays open so a load balancer can still probe the process.
    """
    token = os.environ.get("RECONOW_API_TOKEN")
    if (
        token
        and request.url.path.startswith("/api/")
        and request.url.path != "/api/health"
        and request.headers.get("authorization") != f"Bearer {token}"
    ):
        return JSONResponse(status_code=401, content={"detail": "unauthenticated"})
    return await call_next(request)


SESSION: dict = {}

# AI job registry: job_id -> {status, done, total, result, error}
AI_JOBS: dict[str, dict] = {}


FIELD_LABELS = {
    "invoice_no": "Invoice Number",
    "supplier_gstin": "Supplier GSTIN",
    "supplier_name": "Supplier Name",
    "taxable": "Taxable Amount",
    "invoice_date": "Invoice Date",
    "place_of_supply": "Place of Supply",
    "hsn": "HSN Code",
    "ims_status": "IMS Status",
    "reverse_charge": "Reverse Charge",
    "note_type": "Note Type",
    "voucher_type": "Voucher Type",
    "original_invoice_no": "Original Invoice No",
    "voucher_no": "Voucher No",
    "igst": "IGST",
    "cgst": "CGST",
    "sgst": "SGST",
    "cess": "Cess",
    "filed_date": "Filing Date",
    "period": "Return Period",
    # Optional-kind fields. Present in FIELD_LABELS so the mapping screen can
    # bind them; absent from REQUIRED_FIELDS so a file without them still
    # uploads.
    "payment_date": "Payment Date",
    "receipt_date": "Goods Receipt Date",
    "itc_available": "ITC Available",
    # GSTR-2A only — the date the snapshot was pulled from the portal. 2A is
    # dynamic, so *when* it was read is part of what it says; without this
    # every pull is indistinguishable and drift cannot be computed.
    "pulled_on": "2A Pulled On",
    # GSTR-3B Table 4 — a summary return, so these are period totals rather
    # than per-invoice values. Named for the rows a preparer sees on the
    # return: a single "ITC" figure would make the working paper's
    # gross -> reversals -> net chain untraceable.
    "itc_4a": "Table 4A — ITC Available (gross)",
    "itc_reversed_4b1": "Table 4B(1) — Reversed, permanent",
    "itc_reversed_4b2": "Table 4B(2) — Reversed, reclaimable",
    "itc_net_4c": "Table 4C — Net ITC Available",
    "itc_reclaimed_4d1": "Table 4D(1) — Reclaimed",
    "itc_unavailable_4d2": "Table 4D(2) — Unavailable by law",
}

#: Which finding rules each optional dataset switches on. A firm that cannot
#: export a payment ledger still gets every other check — but the product must
#: say what is *not* being checked rather than reporting a clean result the
#: data never earned. Silence about an unrun check reads exactly like a check
#: that found nothing.
CHECKS_BY_KIND: dict[str, tuple[str, ...]] = {
    "payments": ("gst:PaymentOverdue",),
    "grn": ("gst:GoodsReceiptTiming",),
    "gstr1": ("gst:MissingInBooks", "gst:Gstr1NotIn2b", "gst:BooksGstr1Mismatch"),
    "gstr2a": ("gst:FiledLateInGstr2a", "gst:AmendedAfterClaim"),
    # 3B does not switch on a graph rule — the 2B/3B comparison and the Rule 37
    # reversal check are computed in `app.itc_3b`, because both compare a
    # period *total* against a summary figure rather than joining invoices.
    # Named here so the product still says what is not being checked.
    "gstr3b": ("gst:ItcClaimedVsAvailable", "gst:Rule37ReversalMade"),
}

#: Why a reviewer should care that each is off, in their own terms.
CHECK_REASONS: dict[str, str] = {
    "gst:PaymentOverdue": "Rule 37 — credit must be reversed on invoices unpaid for 180 days",
    "gst:GoodsReceiptTiming": "s.16(2)(b) — no credit before the goods are received",
    "gst:MissingInBooks": "invoices the supplier declared that the books do not carry",
    "gst:Gstr1NotIn2b": "declared in GSTR-1 but not yet reached GSTR-2B",
    "gst:BooksGstr1Mismatch": "the books and the supplier's GSTR-1 disagree",
    "gst:FiledLateInGstr2a": "the supplier filed after the 2B you claimed against was frozen",
    "gst:AmendedAfterClaim": "the portal's value has changed since the 2B you claimed against",
    "gst:ItcClaimedVsAvailable": "whether Table 4A matches the 2B it is auto-populated from",
    "gst:Rule37ReversalMade": "whether the 180-day reversals actually reached Table 4B(2)",
}


def checks_disabled(uploaded_kinds: set[str]) -> dict[str, str]:
    """Rule label -> why it matters, for every check the uploaded files cannot
    support. Empty when nothing is missing."""
    disabled: dict[str, str] = {}
    for kind, labels in CHECKS_BY_KIND.items():
        if kind in uploaded_kinds:
            continue
        for label in labels:
            disabled[label] = CHECK_REASONS.get(label, "")
    return disabled

REQUIRED_FIELDS = {"invoice_no", "taxable"}

# keyword -> field, ordered so specific terms win
_FIELD_KEYWORDS = [
    # Optional-kind fields first: "payment date" and "goods receipt date" both
    # contain "date", and the generic date keywords below would otherwise claim
    # the column before the specific rule ever runs. Specific wins, which is
    # the ordering this whole table depends on.
    ("payment date", "payment_date"),
    ("paid on", "payment_date"),
    ("paid date", "payment_date"),
    ("goods receipt date", "receipt_date"),
    ("grn date", "receipt_date"),
    ("receipt date", "receipt_date"),
    ("received on", "receipt_date"),
    ("itc available", "itc_available"),
    ("itc availability", "itc_available"),
    ("itc eligible", "itc_available"),
    ("original invoice", "original_invoice_no"),
    ("orig invoice", "original_invoice_no"),
    ("invoice date", "invoice_date"),
    ("document date", "invoice_date"),
    ("invoice no", "invoice_no"),
    ("invoice number", "invoice_no"),
    ("inv no", "invoice_no"),
    ("document no", "invoice_no"),
    ("gstin", "supplier_gstin"),
    ("gst in", "supplier_gstin"),
    ("supplier name", "supplier_name"),
    ("vendor name", "supplier_name"),
    ("party name", "supplier_name"),
    ("supplier", "supplier_name"),
    ("vendor", "supplier_name"),
    ("name", "supplier_name"),
    ("taxable amount", "taxable"),
    ("taxable value", "taxable"),
    ("taxable", "taxable"),
    ("assessable value", "taxable"),
    ("place of supply", "place_of_supply"),
    ("pos", "place_of_supply"),
    ("hsn code", "hsn"),
    ("hsn", "hsn"),
    ("sac", "hsn"),
    ("ims status", "ims_status"),
    ("ims", "ims_status"),
    ("reverse charge", "reverse_charge"),
    ("rcm", "reverse_charge"),
    ("note type", "note_type"),
    ("document type", "note_type"),
    ("note", "note_type"),
    ("voucher type", "voucher_type"),
    ("voucher no", "voucher_no"),
    ("voucher", "voucher_no"),
    # A real GSTR-2B export names these "Integrated Tax"/"Central Tax"/
    # "State/UT Tax", not "IGST"/"CGST"/"SGST" — found via
    # plans/119-architecture-audit.md §5b's live parity test, which
    # silently zeroed every portal-side tax component against realistic
    # sample data (reco-now's own SAMPLE/gstr2b_*.csv use these exact
    # header names) until these were added.
    ("integrated tax", "igst"),
    ("central tax", "cgst"),
    ("state/ut tax", "sgst"),
    ("state tax", "sgst"),
    ("igst", "igst"),
    ("cgst", "cgst"),
    ("sgst", "sgst"),
    ("cess", "cess"),
    # GSTR-2A/GSTR-1 only — packs/gst's gstr1-not-in-2b.sparql reads these
    # off the gst:Gstr1Filing subject (graphowl_client.py). Harmless on a
    # books/GSTR-2B header row: nothing there matches, so these stay
    # unmapped exactly like any other absent field.
    ("filing date", "filed_date"),
    ("filed date", "filed_date"),
    # GSTR-2A pull date. A portal export names this several ways and none of
    # them is "pulled_on"; each of these was taken from a real 2A header row
    # rather than guessed.
    ("pulled on", "pulled_on"),
    ("pull date", "pulled_on"),
    ("date of download", "pulled_on"),
    ("downloaded on", "pulled_on"),
    ("as on date", "pulled_on"),
    ("as on", "pulled_on"),
    # GSTR-3B Table 4. A real 3B export labels these several ways; the row
    # numbers are the one stable part, so they are matched first.
    ("4a", "itc_4a"),
    ("itc available", "itc_4a"),
    ("4b(1)", "itc_reversed_4b1"),
    ("4b1", "itc_reversed_4b1"),
    ("4b(2)", "itc_reversed_4b2"),
    ("4b2", "itc_reversed_4b2"),
    ("4c", "itc_net_4c"),
    ("net itc", "itc_net_4c"),
    ("4d(1)", "itc_reclaimed_4d1"),
    ("4d1", "itc_reclaimed_4d1"),
    ("4d(2)", "itc_unavailable_4d2"),
    ("4d2", "itc_unavailable_4d2"),
    ("return period", "period"),
    ("period", "period"),
]


def _reset() -> None:
    SESSION.clear()
    SESSION["period"] = {"month": "March", "year": 2026}
    SESSION["tolerance"] = 1.0
    SESSION["datasets"] = {}
    SESSION["mapping"] = {}
    SESSION["results"] = None
    SESSION["normalized"] = {}
    SESSION["graphowl"] = {}
    SESSION["graphowl_reconcile"] = None
    SESSION["graphowl_ingest_threads"] = []


def _auto_map(headers: list[str]) -> dict[str, int | None]:
    mapping = {}
    used: set[int] = set()
    lower = [h.lower().strip() for h in headers]
    for keyword, field in _FIELD_KEYWORDS:
        if field in mapping:
            continue
        for idx, header in enumerate(lower):
            if idx in used:
                continue
            if keyword in header:
                mapping[field] = idx
                used.add(idx)
                break
    for field in FIELD_LABELS:
        mapping.setdefault(field, None)
    return mapping


def _parse_upload(raw: bytes, filename: str) -> list[dict]:
    name = filename.lower()
    if name.endswith(".csv"):
        return pd.read_csv(io.BytesIO(raw)).to_dict(orient="records")
    if name.endswith((".xlsx", ".xlsm")):
        return pd.read_excel(io.BytesIO(raw)).to_dict(orient="records")
    if name.endswith(".xls"):
        return pd.read_excel(io.BytesIO(raw), engine="xlrd").to_dict(orient="records")
    if name.endswith(".json"):
        data = json.loads(raw.decode("utf-8-sig"))
        if isinstance(data, dict):
            for key in ("data", "records", "invoices", "items"):
                if isinstance(data.get(key), list):
                    data = data[key]
                    break
        if isinstance(data, dict):
            data = [data]
        return data
    raise ValueError(f"Unsupported file type: {filename}")


# How many rows the Upload & map screen will render at once. A GST period's
# purchase register is routinely thousands of rows; the browser does not need
# them all to let someone check that a file landed and its columns read
# correctly, and `total_rows` still reports the real count beside the table.
_MAX_TABLE_ROWS = 500


def _blank_to_none(value: object) -> object:
    """pandas represents an empty cell as float NaN.

    That is not JSON: `json.dumps` writes a bare `NaN` token, which Postgres
    rejects outright ("invalid input syntax for type json") and strict parsers
    elsewhere reject too. It surfaced the moment uploads began being stored
    rather than held in memory, on the government purchase register's own
    empty `Note Type` and `Original Invoice No` columns.

    An empty cell means "this file says nothing here", which is None — not a
    number that happens to be un-representable.
    """
    if isinstance(value, float) and (math.isnan(value) or math.isinf(value)):
        return None
    return value


def _build_dataset(payload: list[dict], name: str, kind: str) -> dict:
    headers = list(payload[0].keys()) if payload else []
    rows = [{k: _blank_to_none(v) for k, v in row.items()} for row in payload]
    return {
        "id": kind,
        "name": name,
        "kind": kind,
        "headers": headers,
        "rows": rows,
        "total_rows": len(rows),
    }


def _normalize(dataset: dict, mapping: dict[str, int | None]) -> list[dict]:
    rows = []
    for raw in dataset["rows"]:
        record = {}
        for field, column in mapping.items():
            if column is None or column >= len(dataset["headers"]):
                record[field] = ""
                continue
            header = dataset["headers"][column]
            record[field] = raw.get(header, "")
        rows.append(record)
    return rows


@app.on_event("startup")
async def _startup() -> None:
    _reset()
    _install_graphowl_pack()
    await _connect_db()


@app.on_event("shutdown")
async def _shutdown() -> None:
    if app.state.db_pool is not None:
        await app.state.db_pool.close()


async def _connect_db() -> None:
    """B0's persistence layer, connected here rather than assumed —
    best-effort the same way `_install_graphowl_pack` is: a laptop with no
    Postgres up must still serve the pre-B0 SESSION-based screens. Routes
    that need `app.state.db_pool` (client/period, and everything B1+ layers
    on repo.py) return 503 when it is None rather than crashing the whole
    app at startup."""
    dsn = os.environ.get("DATABASE_URL")
    if not dsn:
        app.state.db_pool = None
        print("[db] DATABASE_URL not set — client/period routes will 503")
        return
    try:
        app.state.db_pool = await db.create_pool(dsn)
    except OSError as exc:
        app.state.db_pool = None
        print(f"[db] connection skipped — {exc}")


def _require_db_pool():
    pool = getattr(app.state, "db_pool", None)
    # A closed pool is not a configured one: startup's best-effort connect can
    # leave a stale handle behind (a restarted database, a test's torn-down
    # fixture), and handing it out turns every route into a 500.
    if pool is None or pool.is_closing():
        raise HTTPException(status_code=503, detail="database not configured — set DATABASE_URL")
    return pool


def _install_graphowl_pack() -> None:
    """Declare packs/gst's *vocabulary* (namespace, predicates, ontology —
    not its demo fixtures). reco-now has no pack of its own
    (plans/119-architecture-audit.md, 16 August 2026 consolidation) — it
    ingests directly under packs/gst's namespace, using the same
    predicates the pack's own finding queries read.

    **`include_documents=False` is load-bearing, not an optimisation.**
    The native reconcile engine has no per-source data isolation
    (`Catalog::reconcile_pack` runs each rule's SPARQL over the whole
    store); packs/gst's own `[[documents]]` include its planted
    INV-1001..INV-2002 demo scenarios, and loading them into reco-now's
    deployment would put graph-owl's own demo invoices into every
    reconciliation reco-now runs. Vocabulary composition is wanted here,
    not the data — `ontology.ttl` (source `gst-ontology`) and the law data
    amount-mismatch.sparql needs (`law/sections.ttl`, `law/rule-36-4.ttl`)
    are not demo fixtures and load alongside everything else in
    `packs/gst/pack.toml`'s `[[documents]]`.

    **`ontology.ttl` was missing from this list until Plan 120 Slice A** —
    `include_documents=False` skips every `[[documents]]` entry
    indiscriminately, `ontology.ttl` is one of them, and nothing imported it
    back individually the way the law data already was. A reco-now
    deployment never had the GST ontology loaded, so the Ontology Builder's
    "gst" selector read "No ontology installed yet" — not a broken pack,
    a startup step that silently skipped a file.

    Uses the one-time step `load_pack` already does for packs/gst and
    packs/hospitality, reused here rather than reimplemented. Idempotent
    (loading a pack twice is a no-op), so this runs on every startup, not
    just the first."""
    try:
        load_pack(GST_PACK_DIR, GRAPH_OWL_SERVER, GRAPH_OWL_TOKEN, include_documents=False)
        # The ontology and law data amount-mismatch.sparql needs, imported
        # directly — `include_documents=False` above excludes them along
        # with the demo fixtures, since `load_pack` has no partial-document
        # mode. Same source names packs/gst's own manifest uses, read from
        # its own directory: not a copy, the canonical file.
        for name, source in (
            ("ontology.ttl", "gst-ontology"),
            ("law/sections.ttl", "gst-law"),
            ("law/rule-36-4.ttl", "gst-law-rule-36-4"),
        ):
            text = (GST_PACK_DIR / name).read_text(encoding="utf-8")
            graphowl_client.import_document(GRAPH_OWL_SERVER, source, text, GRAPH_OWL_TOKEN)
    except LoadError as exc:
        print(f"[graphowl] pack install skipped — {exc}")
    except graphowl_client.IngestError as exc:
        print(f"[graphowl] ontology/law data import skipped — {exc}")


def _ingest_to_graphowl(kind: str, dataset: dict, mapping: dict) -> threading.Thread:
    """Land this upload's rows in graph-owl as durable graph subjects, in
    a background thread so a slow or absent graph-owl never delays the
    upload response. Records what happened in SESSION["graphowl"], which
    `overview()` deliberately never reads — this is additive, not a
    change to the existing response shape.

    Returns the thread so `/api/upload` can hand it to `/api/reconcile`,
    which joins every ingest thread before asking graph-owl to reconcile —
    the native engine can only find what has actually landed
    (plans/119-architecture-audit.md §9).

    **One stable source per kind, deleted immediately before every
    import** (plans/120-domain-agnostic-console-and-investigation-
    workspace.md, Slice D) — not a fresh random source per upload, which
    was the confirmed root cause of totals that grew across every upload a
    session ever made: `POST /graph/import/rdf` only dedupes *within* one
    source's own import graph, so a new random name every time meant a
    re-upload never replaced anything, it only added a parallel copy every
    finding query's unbound `GRAPH ?g { }` pattern then matched alongside
    the original. Deleting first, under the same stable name, makes a
    re-upload a genuine replacement."""

    def _run() -> None:
        try:
            normalized = _normalize(dataset, mapping)
            turtle = graphowl_client.rows_to_turtle(normalized, kind)
            source = f"reco-{kind}"
            if not turtle:
                SESSION["graphowl"][kind] = {"landed": 0, "skipped": 0, "rejected": []}
                return
            graphowl_client.delete_document(GRAPH_OWL_SERVER, source, GRAPH_OWL_TOKEN)
            result = graphowl_client.import_document(
                GRAPH_OWL_SERVER, source, turtle, GRAPH_OWL_TOKEN
            )
            SESSION["graphowl"][kind] = {
                "source": source,
                "landed": len(result.get("landed", [])),
                "skipped": len(result.get("skipped", [])),
                "rejected": result.get("rejected", []),
            }
        except graphowl_client.IngestError as exc:
            SESSION["graphowl"][kind] = {"error": str(exc)}

    thread = threading.Thread(target=_run, daemon=True)
    thread.start()
    return thread


@app.get("/api/health")
def health() -> dict:
    return {
        "status": "ok",
        "service": "matcha-backend",
        "ai": {"available": ai.is_available(), "model": ai.MODEL},
    }


# ---------------------------------------------------------------- Plan 122b B1
# Clients and periods — the first HTTP surface built directly on B0's
# repository layer. Everything below this point that reads workflow state
# (cases, follow-ups, approvals, ...) takes client_id, and period_id where
# the table has one, as a required path parameter — never a query-string
# option, so a route simply cannot be called without naming its scope.


@app.post("/api/clients", status_code=201)
async def create_client(payload: dict) -> dict:
    pool = _require_db_pool()
    name = payload.get("name")
    gstin = payload.get("gstin")
    state = payload.get("state")
    if not name or not gstin or not state:
        raise HTTPException(status_code=400, detail="name, gstin and state are all required")
    async with pool.acquire() as conn:
        client_id = await repo.create_client(conn, name=name, gstin=gstin, state=state)
    return {"id": client_id, "name": name, "gstin": gstin, "state": state}


@app.get("/api/clients")
async def list_clients_route() -> list[dict]:
    pool = _require_db_pool()
    async with pool.acquire() as conn:
        clients = await repo.list_clients(conn)
    return [
        {"id": str(c["id"]), "name": c["name"], "gstin": c["gstin"], "state": c["state"]} for c in clients
    ]


@app.post("/api/clients/{client_id}/periods", status_code=201)
async def create_period_route(client_id: str, payload: dict) -> dict:
    pool = _require_db_pool()
    month = payload.get("month")
    year = payload.get("year")
    if not month or not year:
        raise HTTPException(status_code=400, detail="month and year are both required")
    async with pool.acquire() as conn:
        period_id = await repo.create_period(conn, client_id=client_id, month=month, year=int(year))
    return {"id": period_id, "month": month, "year": int(year)}


@app.get("/api/clients/{client_id}/periods")
async def list_periods_route(client_id: str) -> list[dict]:
    pool = _require_db_pool()
    async with pool.acquire() as conn:
        periods = await repo.list_periods(conn, client_id=client_id)
    return [
        {"id": str(p["id"]), "month": p["month"], "year": p["year"], "status": p["status"]} for p in periods
    ]


@app.post("/api/clients/{client_id}/periods/{period_id}/cases", status_code=201)
async def create_case_route(client_id: str, period_id: str, payload: dict) -> dict:
    pool = _require_db_pool()
    invoice_no = payload.get("invoice_no")
    if not invoice_no:
        raise HTTPException(status_code=400, detail="invoice_no is required")
    async with pool.acquire() as conn:
        case_id = await repo.create_case(
            conn, client_id=client_id, period_id=period_id, invoice_no=invoice_no,
            reason_code=payload.get("reason_code"),
            supplier_name=payload.get("supplier_name"),
            supplier_gstin=payload.get("supplier_gstin"),
            books_amount=payload.get("books_amount"),
            portal_amount=payload.get("portal_amount"),
            subject=payload.get("subject"),
            summary=payload.get("summary"),
            governed_by=payload.get("governed_by"),
            evidence_count=payload.get("evidence_count"),
        )
    return {"id": case_id, "invoice_no": invoice_no, "reason_code": payload.get("reason_code")}


@app.get("/api/clients/{client_id}/periods/{period_id}/cases")
async def list_cases_route(client_id: str, period_id: str) -> list[dict]:
    pool = _require_db_pool()
    async with pool.acquire() as conn:
        cases = await repo.list_cases(conn, client_id=client_id, period_id=period_id)
    return [
        {"id": str(c["id"]), "invoice_no": c["invoice_no"], "reason_code": c["reason_code"], "status": c["status"]}
        for c in cases
    ]


@app.post("/api/clients/{client_id}/periods/{period_id}/ask")
async def ask_route(client_id: str, period_id: str, payload: dict) -> dict:
    """Plan 122b B1's own RED: grounded or refused, never an uncited
    sentence. A deterministic keyword match over this client+period's own
    `case_record` rows — not an LLM call — so "grounded" here means
    exactly what it says: every word of the answer traces to a cited row.
    """
    pool = _require_db_pool()
    question = (payload.get("question") or "").lower()
    async with pool.acquire() as conn:
        cases = await repo.list_cases(conn, client_id=client_id, period_id=period_id)

    matched = [
        c for c in cases
        if c["invoice_no"].lower() in question or (c["reason_code"] or "").lower() in question
    ]
    if not matched:
        return {"grounded": False, "answer": "Not enough evidence to answer that from what's in this period.", "citations": []}

    citations = [f"case {c['invoice_no']} · {c['reason_code'] or 'no reason code yet'}" for c in matched]
    answer = f"Found {len(matched)} matching case(s): " + ", ".join(c["invoice_no"] for c in matched) + "."
    return {"grounded": True, "answer": answer, "citations": citations}


# ---------------------------------------------------------------- Plan 122b B3
# Upload & map. `WORKSPACES` is the client+period-scoped replacement for
# SESSION["datasets"]/SESSION["mapping"] — legitimately still in-memory
# (this is in-progress mapping state, not a workflow decision), but keyed
# per (client_id, period_id) instead of being one global dict. The mapping
# *template* itself, once confirmed, persists durably via
# repo.upsert_mapping_template so a second period reuses it.
WORKSPACES: dict[str, dict] = {}


def _workspace(client_id: str, period_id: str) -> dict:
    key = f"{client_id}:{period_id}"
    return WORKSPACES.setdefault(key, {"datasets": {}})


@app.post("/api/clients/{client_id}/periods/{period_id}/datasets/{kind}/upload")
async def upload_dataset_route(client_id: str, period_id: str, kind: str, file: UploadFile = File(...)) -> dict:
    pool = _require_db_pool()
    raw = await file.read()
    try:
        payload = _parse_upload(raw, file.filename or "upload.csv")
    except Exception as exc:  # noqa: BLE001
        raise HTTPException(status_code=400, detail=f"could not parse {file.filename}: {exc}") from exc
    dataset = _build_dataset(payload, file.filename or kind, kind)

    async with pool.acquire() as conn:
        template = await repo.get_mapping_template(conn, client_id=client_id, dataset_kind=kind)
    # A template is only reused when it still describes this file's columns.
    # Reusing one across a different layout mapped invoice numbers onto GSTINs
    # — see repo.template_fits.
    if template is not None and repo.template_fits(template, dataset["headers"]):
        mapping = template["mapping"]
        from_template = True
    else:
        mapping = _auto_map(dataset["headers"])
        from_template = False

    async with pool.acquire() as conn:
        await repo.upsert_dataset_upload(
            conn, client_id=client_id, period_id=period_id, kind=kind,
            name=dataset["name"], headers=dataset["headers"], rows=dataset["rows"],
            total_rows=dataset["total_rows"], mapping=mapping, confirmed=False,
        )

    return {
        "kind": kind,
        "name": dataset["name"],
        "headers": dataset["headers"],
        "preview": dataset["rows"][:5],
        "total_rows": dataset["total_rows"],
        "mapping": mapping,
        "from_template": from_template,
        "confirmed": False,
        # What is wrong with this file, said now rather than discovered later
        # as a check that quietly did not run. Warnings only — a file with
        # problems is still the best information available.
        "issues": inspect_rows(_normalize(dataset, mapping), kind),
    }


@app.get("/api/clients/{client_id}/periods/{period_id}/datasets/{kind}")
async def get_dataset_route(client_id: str, period_id: str, kind: str) -> dict:
    """Reopen a file uploaded earlier, with the mapping as it now stands.

    Without this the mapping table could only ever show the file just picked,
    so switching screens and coming back looked like nothing had been
    uploaded.
    """
    pool = _require_db_pool()
    async with pool.acquire() as conn:
        entry = await repo.get_dataset_upload(
            conn, client_id=client_id, period_id=period_id, kind=kind
        )
    if entry is None:
        raise HTTPException(status_code=404, detail=f"no {kind} dataset uploaded for this period yet")
    return {
        "kind": entry["kind"],
        "name": entry["name"],
        "headers": entry["headers"],
        "preview": entry["rows"][:5],
        # The file's own rows, so the console can show what was actually
        # uploaded rather than only how its columns were mapped. Capped
        # because this is a screen, not an export — `total_rows` still
        # reports the true count, so a truncated view is visible as one
        # rather than looking like a short file.
        "rows": entry["rows"][:_MAX_TABLE_ROWS],
        "row_limit": _MAX_TABLE_ROWS,
        "total_rows": entry["total_rows"],
        "mapping": entry["mapping"],
        "from_template": False,
        "confirmed": entry["confirmed"],
        "issues": inspect_rows(
            _normalize({"headers": entry["headers"], "rows": entry["rows"]}, entry["mapping"]),
            kind,
        ),
    }


@app.post("/api/clients/{client_id}/periods/{period_id}/datasets/{kind}/mapping")
async def confirm_dataset_mapping_route(client_id: str, period_id: str, kind: str, payload: dict) -> dict:
    pool = _require_db_pool()
    mapping = payload.get("mapping")
    tolerance = float(payload.get("tolerance", 1.0))
    if not mapping:
        raise HTTPException(status_code=400, detail="mapping is required")

    async with pool.acquire() as conn:
        entry = await repo.get_dataset_upload(
            conn, client_id=client_id, period_id=period_id, kind=kind
        )
        if entry is None:
            raise HTTPException(status_code=404, detail=f"no {kind} dataset uploaded for this period yet")
        await repo.set_dataset_mapping(
            conn, client_id=client_id, period_id=period_id, kind=kind,
            mapping=mapping, confirmed=True,
        )
        await repo.upsert_mapping_template(
            conn,
            client_id=client_id,
            dataset_kind=kind,
            mapping=mapping,
            tolerance=tolerance,
            # Record the layout this mapping describes, so a later upload can
            # tell whether the template still applies to it.
            source_headers=entry["headers"],
        )

    # Plan 123 Slice G: the confirmed mapping also becomes a durable
    # **alignment** — "this client's `Party Code` *is* `gst:supplierGstin`" —
    # rather than a per-file binding thrown away after upload. Recorded as
    # `human`/1.0 here because the caller has just confirmed it; an automated
    # header guess is recorded separately, in the review band, so a guess can
    # never become indistinguishable from a confirmation.
    #
    # Best-effort, like every other graph-owl call in this file: an alignment
    # that does not land costs a reusable fact, not the upload.
    aligned = 0
    try:
        aligned = graphowl_client.record_alignments(
            server=GRAPH_OWL_SERVER,
            requests=vocabulary.alignment_requests(
                client_id=client_id,
                headers=entry["headers"],
                mapping=mapping,
                confirmed_by_human=True,
            ),
        )
    except Exception as exc:  # noqa: BLE001
        print(f"[graphowl] alignment recording skipped — {exc}")

    return {"kind": kind, "confirmed": True, "aligned_terms": aligned}


@app.get("/api/clients/{client_id}/periods/{period_id}/datasets")
async def list_datasets_route(client_id: str, period_id: str) -> list[dict]:
    pool = _require_db_pool()
    async with pool.acquire() as conn:
        entries = await repo.list_dataset_uploads(conn, client_id=client_id, period_id=period_id)
    return [
        {
            "kind": e["kind"],
            "name": e["name"],
            "total_rows": e["total_rows"],
            "confirmed": e["confirmed"],
        }
        for e in entries
    ]


#: Month name -> its number. `goods-receipt-timing.sparql` compares
#: `SUBSTR(?receivedAt, 1, 7)` — an ISO `YYYY-MM` — against the statement's
#: period, so the period must be in exactly that shape.
_MONTH_NUMBER = {
    m: i
    for i, m in enumerate(
        ["January", "February", "March", "April", "May", "June",
         "July", "August", "September", "October", "November", "December"],
        start=1,
    )
}


def period_label_to_yyyy_mm(month: str | None, year: int | None) -> str | None:
    """`("March", 2026)` -> `"2026-03"`, or None when the month is not one.

    None rather than a guess: a wrong period silently mis-dates every
    goods-receipt comparison, where no period leaves the rule unfired, which
    a reviewer can see.
    """
    number = _MONTH_NUMBER.get(str(month or "").strip().title())
    if number is None or year is None:
        return None
    return f"{int(year):04d}-{number:02d}"


def apply_period_fallback(rows: list[dict], period: str | None) -> list[dict]:
    """Give rows the workspace's period where the file did not state one.

    Real GSTR-2B exports frequently carry no "Return Period" column, and
    requiring one would leave s.16(2)(b) permanently dark. A period *in* the
    file is a statement of fact about that file and always wins.
    """
    if not period:
        return rows
    return [
        row if _is_present_value(row.get("period")) else {**row, "period": period}
        for row in rows
    ]


def _is_present_value(value: object) -> bool:
    return value is not None and str(value).strip() != ""


#: Graphs every reconciliation must read whatever period it is for — the
#: pack's own vocabulary and law. These are not period data: Rule 36(4)'s cap
#: lives here, and a run that could not see it would stop finding amount
#: mismatches altogether.
PACK_GRAPHS = ("gst-ontology", "gst-law", "gst-law-rule-36-4")


def period_source_name(client_id: str, period_id: str, kind: str) -> str:
    """The graph one (client, period, kind) upload lands in.

    The same construction `_ingest_scoped_to_graphowl` uses. Extracted so the
    reconciliation can name exactly the graphs it just wrote, rather than
    re-deriving the hash in two places that could drift.
    """
    short_id = hashlib.sha256(f"{client_id}:{period_id}:{kind}".encode()).hexdigest()[:12]
    return f"reco-{short_id}-{kind}"


def findings_for_period(
    findings: list[dict], known_invoices: set[tuple[str, str]]
) -> list[dict]:
    """Findings about invoices this period actually carries.

    `list_findings` returns every finding recorded for the pack, across every
    period. Scoping the *evaluation* stopped rules concluding from another
    period's facts, but findings already on record still leaked: April, whose
    GSTR-2B has no ITC-eligibility column, reported `gst:ITCNotAvailable` as
    NOT EVALUATED and 89,800 of blocked ITC in the same breath — March's.

    A finding is about an invoice, and an invoice belongs to the period whose
    files carry it.

    **Identity comes from `case_from_finding`, not from reading keys off the
    finding.** A raw finding has no `invoice_no` field — the invoice number is
    an evidence binding (`var == "number"`). An earlier version of this read
    `finding["invoice_no"]`, got None for every finding, and silently dropped
    all of them; the symptom was a period reporting zero blocked ITC while its
    own rule said the credit was blocked.

    A finding naming no supplier is matched on the invoice number alone.
    `gst:ITCNotAvailable` bound no GSTIN until recently, and dropping those
    would lose exactly the blocked-credit cases this exists to surface.
    """
    by_invoice: dict[str, set[str]] = {}
    for gstin, invoice in known_invoices:
        by_invoice.setdefault(invoice, set()).add(gstin)

    kept = []
    for finding in findings:
        identity = case_from_finding(finding)
        invoice = rc.normalize_invoice_no(identity["invoice_no"])
        gstins = by_invoice.get(invoice)
        if gstins is None:
            continue
        gstin = str(identity.get("supplier_gstin") or "").strip().upper()
        if gstin and gstin not in gstins:
            continue
        kept.append(finding)
    return kept


def reconcile_scope(client_id: str, period_id: str, kinds: list[str]) -> list[str]:
    """The graphs a reconciliation of this period may read.

    This period's uploads, plus the pack. **Not the whole store**: evaluation
    used to run unscoped, so a period with no goods-receipt file reported
    s.16(2)(b) as *passed* because another period had supplied the data — the
    exact "checked, clean" lie the three-state rule outcome exists to prevent,
    one level up.
    """
    return [period_source_name(client_id, period_id, kind) for kind in kinds] + list(PACK_GRAPHS)


def _ingest_scoped_to_graphowl(
    client_id: str, period_id: str, kind: str, dataset: dict, mapping: dict,
    period_label: str | None = None,
) -> dict:
    """`_ingest_to_graphowl`'s reasoning (stable source, deleted before
    every re-import) still holds — the source name itself now carries
    client_id and period_id, not just kind. The pre-B0 app served exactly
    one session at a time, so `reco-{kind}` alone never collided; two
    clients uploading concurrently now genuinely can, and a shared source
    name would mean client B's upload deletes and replaces client A's own
    books.

    **The limitation this docstring used to record is closed** (Plan 123
    Slice C0, 19 August 2026). It read: "the native reconcile engine itself
    still runs unscoped over the *whole* graph-owl store ... fully isolating
    the reconcile step needs a graph-owl-side scoping mechanism this Python
    backend cannot add on its own." That mechanism now exists —
    `POST /packs/{pack}/reconcile` takes a `graphs` scope, built here by
    `reconcile_scope` — and a run reads only the graphs it names."""
    normalized = _normalize(dataset, mapping)
    # The 2B statement's period, taken from the workspace when the file does
    # not carry a "Return Period" column — which real exports usually do not.
    # Without it there is no statement subject and s.16(2)(b) cannot fire.
    # `gstr2a` too: a portal 2A export carries a pull date far more often
    # than a return period, and without a period the snapshot cannot be
    # matched to the 2B it is supposed to be compared against.
    if kind in ("gstr2b", "portal", "gstr2a"):
        normalized = apply_period_fallback(normalized, period_label)
    turtle = graphowl_client.rows_to_turtle(normalized, kind)
    # Source name must be 1-64 chars of [a-zA-Z0-9_-].  Two UUIDs + prefix
    # blow the limit, so we take the first 12 hex chars of a SHA-256 of the
    # (client_id, period_id, kind) tuple — unique enough for a source graph
    # name and short enough to pass graph-owl's validation.
    source = period_source_name(client_id, period_id, kind)
    if not turtle:
        return {"source": source, "landed": 0, "skipped": 0, "rejected": []}
    graphowl_client.delete_document(GRAPH_OWL_SERVER, source, GRAPH_OWL_TOKEN)
    result = graphowl_client.import_document(GRAPH_OWL_SERVER, source, turtle, GRAPH_OWL_TOKEN)
    return {
        "source": source,
        "landed": len(result.get("landed", [])),
        "skipped": len(result.get("skipped", [])),
        "rejected": result.get("rejected", []),
    }


@app.post("/api/clients/{client_id}/periods/{period_id}/reconcile")
async def reconcile_route(client_id: str, period_id: str) -> dict:
    pool = _require_db_pool()
    async with pool.acquire() as conn:
        stored = await repo.list_dataset_uploads(conn, client_id=client_id, period_id=period_id)
    # Shape kept as {kind: {"dataset": ..., "mapping": ...}} so `_supplier_names`
    # and `_ingest_scoped_to_graphowl` read the same structure they always did.
    datasets = {
        e["kind"]: {
            "dataset": {
                "name": e["name"], "headers": e["headers"],
                "rows": e["rows"], "total_rows": e["total_rows"],
            },
            "mapping": e["mapping"],
            "confirmed": e["confirmed"],
        }
        for e in stored
    }
    if not datasets:
        raise HTTPException(status_code=409, detail="upload at least one dataset first")
    unconfirmed = [kind for kind, entry in datasets.items() if not entry["confirmed"]]
    if unconfirmed:
        raise HTTPException(
            status_code=409, detail=f"unconfirmed datasets block reconciliation: {', '.join(unconfirmed)}"
        )

    # The period this workspace is, as `YYYY-MM`, so a 2B file with no
    # "Return Period" column still gets a statement subject to compare goods
    # receipts against.
    async with pool.acquire() as conn:
        periods = await repo.list_periods(conn, client_id=client_id)
    this_period = next((p for p in periods if str(p["id"]) == period_id), None)
    workspace_period = (
        None if this_period is None
        else period_label_to_yyyy_mm(this_period["month"], this_period["year"])
    )

    ingested = {}
    for kind, entry in datasets.items():
        try:
            ingested[kind] = _ingest_scoped_to_graphowl(
                client_id, period_id, kind, entry["dataset"], entry["mapping"],
                period_label=workspace_period,
            )
        except graphowl_client.IngestError as exc:
            ingested[kind] = {"error": str(exc)}

    try:
        result = run_findings(
            graphowl_client.PACK_ID,
            GRAPH_OWL_SERVER,
            GRAPH_OWL_TOKEN,
            graphs=reconcile_scope(client_id, period_id, list(datasets)),
        )
        findings = graphowl_client.list_findings(GRAPH_OWL_SERVER, GRAPH_OWL_TOKEN)
    except (LoadError, graphowl_client.IngestError) as exc:
        return {"ok": False, "error": str(exc), "ingested": ingested}

    # `list_findings` returns every finding recorded for the pack, across every
    # period. Scoping the evaluation stopped rules *concluding* from another
    # period's facts; this stops another period's already-recorded findings
    # becoming this period's cases. See `findings_for_period`.
    known_invoices = {
        (
            str(row.get("supplier_gstin") or "").strip().upper(),
            rc.normalize_invoice_no(row.get("invoice_no")),
        )
        for entry in datasets.values()
        for row in _normalize(entry["dataset"], entry["mapping"])
        if _is_present_value(row.get("invoice_no"))
    }
    findings = findings_for_period(findings, known_invoices)

    # What the engine said about each rule, stored as it said it. Reco Now
    # used to *infer* "this check is off" from which files had been uploaded —
    # a Python guess sitting beside graph-owl's own execution record, free to
    # disagree with it without anything noticing.
    async with pool.acquire() as conn:
        await repo.replace_rule_outcomes(
            conn, client_id=client_id, period_id=period_id, outcomes=result.rules
        )

    created = 0
    async with pool.acquire() as conn:
        # A re-run must not duplicate cases for the same invoice — the
        # register (B4) reads case_record directly, and reconcile is meant
        # to be safely re-runnable (a re-upload, a corrected mapping).
        existing = {
            (c["invoice_no"], c["reason_code"])
            for c in await repo.list_cases(conn, client_id=client_id, period_id=period_id)
        }
        # The rules bind a supplier GSTIN but not a trading name — the name
        # lives in the uploaded file, so resolve it from there rather than
        # leaving every case showing a bare GSTIN.
        names_by_gstin = _supplier_names(datasets)
        for finding in findings:
            case = case_from_finding(finding)
            if case["dedup_key"] in existing:
                continue
            await repo.create_case(
                conn, client_id=client_id, period_id=period_id,
                invoice_no=case["invoice_no"],
                reason_code=case["reason_code"],
                subject=case["subject"],
                summary=case["summary"],
                governed_by=case["governed_by"],
                evidence_count=case["evidence_count"],
                supplier_gstin=case["supplier_gstin"],
                supplier_name=names_by_gstin.get(case["supplier_gstin"] or ""),
                books_amount=case["books_amount"],
                portal_amount=case["portal_amount"],
            )
            existing.add(case["dedup_key"])
            created += 1

    # Plan 123 §5: the reconciliation finishing is an **event**, not just a
    # return value. Every agent subscribed to it now actually runs — reads the
    # period's cases, decides something, and leaves a trace of both. Nothing
    # subscribed before, which is why every agent in this product had to be
    # clicked.
    runs_before = len(AGENT_RUNS)
    woken = wake_agents(
        "reconciliation.finished",
        cases=await _cases_for(client_id, period_id),
        client_id=client_id,
        period_id=period_id,
        findings=result.found,
    )

    # The trace is the audit trail; it used to live in a capped module global
    # and evaporate on restart. Persisted beside the cases it was produced
    # from — best-effort like every other graph-owl/db integration point: a
    # run that was recorded in memory must not fail the request that made it.
    try:
        async with pool.acquire() as conn:
            for record in AGENT_RUNS[runs_before:]:
                await repo.insert_agent_run(conn, record=record)
    except Exception as exc:  # noqa: BLE001
        print(f"[db] agent-run persistence skipped — {exc}")

    return {
        "ok": True,
        "ingested": ingested,
        "evaluated": result.evaluated,
        "found": result.found,
        "cases_created": created,
        "agents_woken": woken,
    }


# How a rule's own evidence bindings name the two sides of a comparison.
#
# These are not tuning constants — they are the variable names the pack's
# SPARQL rules bind (`packs/gst/queries/*.sparql`), read off real finding
# output rather than assumed. Keying on the bindings instead of on the rule
# label means a new rule that binds `claimed`/`filed` works without a change
# here, and one that binds something else reports no amount rather than a
# wrong one.
_AMOUNT_PAIRS: tuple[tuple[str, str], ...] = (
    ("claimed", "filed"),          # gst:AmountMismatch — books vs portal taxable value
    ("bookedIgst", "filedIgst"),   # gst:TaxHeadMismatch — same invoice, tax split differs
)

# Bindings that describe a one-sided exposure: the books recorded tax the
# portal has no counterpart for (gst:SupplierNotFiled, gst:PotentialMismatch).
# There is no portal figure to compare against, so the portal side stays None
# — which `_case_exposure` reads as "all of it is at risk", not "zero".
_SINGLE_SIDED: tuple[str, ...] = ("taxAmount",)


def _to_float(value: object) -> float | None:
    try:
        return float(str(value))
    except (TypeError, ValueError):
        return None


def evidence_for_subject(findings: list, subject: str, reason_code: str | None) -> list[dict]:
    """The facts one finding rests on, as graph-owl recorded them.

    A case stored only `evidence_count`, so the console could say "4 fact(s)
    cited" and show none of them. The facts are what make a case defensible —
    including `gst:citation`, the provision whose cap Rule 36(4) read from the
    graph — so a reviewer can see the number's basis rather than trust it.

    `reason_code` matters: one subject can carry two findings (INV-MAR-011 is
    both an AmountMismatch and a TaxHeadMismatch) and returning the wrong
    one's facts would explain a case with another case's evidence.
    """
    for finding in findings:
        if finding.get("subject") != subject:
            continue
        if reason_code is not None and finding.get("label") != reason_code:
            continue
        return [
            {
                "predicate": e.get("predicate"),
                "value": e.get("value"),
                "var": e.get("var"),
            }
            for e in finding.get("evidence") or []
        ]
    return []


def _supplier_names(datasets: dict) -> dict[str, str]:
    """GSTIN → trading name, read from whatever the user uploaded.

    The pack's rules bind `gstin` because that is the identifier the statute
    keys on, but a register showing only GSTINs is unreadable. Every uploaded
    dataset is scanned, not just books, so a name present in only one file
    still resolves.
    """
    names: dict[str, str] = {}
    for entry in datasets.values():
        dataset, mapping = entry["dataset"], entry["mapping"]
        gstin_col, name_col = mapping.get("supplier_gstin"), mapping.get("supplier_name")
        if gstin_col is None or name_col is None:
            continue
        headers = dataset["headers"]
        if not (0 <= gstin_col < len(headers) and 0 <= name_col < len(headers)):
            continue
        for row in dataset["rows"]:
            gstin = str(row.get(headers[gstin_col], "")).strip()
            name = str(row.get(headers[name_col], "")).strip()
            if gstin and name and gstin not in names:
                names[gstin] = name
    return names


def _decimals_to_floats(value):
    """`Decimal` is exact and `json` cannot serialize it. Converting at the
    HTTP boundary keeps every computation above this line exact — money
    arithmetic in float is how a working paper comes to disagree with itself
    by a rupee."""
    if isinstance(value, Decimal):
        return float(value)
    if isinstance(value, dict):
        return {k: _decimals_to_floats(v) for k, v in value.items()}
    if isinstance(value, list):
        return [_decimals_to_floats(v) for v in value]
    return value


def case_from_finding(finding: dict) -> dict:
    """Everything a Reco Now case takes from one graph-owl finding.

    Kept a pure function of the finding so it can be tested against real
    captured rule output without a database or a running graph-owl — the
    reconcile loop below is then just persistence.

    Note `governedBy`: the wire shape is camelCase (graph-owl serialises
    `Finding` that way). Reading `governed_by` here silently recorded no rule
    reference on every case, which is the same defect as the earlier
    `finding["rule"]`/`label` one and equally invisible, since a missing rule
    reference looks exactly like a rule that has none.
    """
    evidence = finding.get("evidence") or []
    ev = {e["var"]: e["value"] for e in evidence if "var" in e and "value" in e}
    subject_uri = str(finding.get("subject") or finding.get("id") or "unknown")

    books_amount: float | None = None
    portal_amount: float | None = None
    for books_var, portal_var in _AMOUNT_PAIRS:
        if books_var in ev or portal_var in ev:
            books_amount = _to_float(ev.get(books_var))
            portal_amount = _to_float(ev.get(portal_var))
            break
    else:
        for single in _SINGLE_SIDED:
            if single in ev:
                books_amount = _to_float(ev.get(single))
                break

    reason_code = finding.get("label")
    invoice_no = ev.get("number") or subject_uri
    return {
        "invoice_no": invoice_no,
        "reason_code": reason_code,
        "subject": subject_uri,
        "summary": finding.get("summary"),
        # camelCase on the wire — see the docstring.
        "governed_by": finding.get("governedBy"),
        "evidence_count": len(evidence),
        "supplier_gstin": ev.get("gstin"),
        "books_amount": books_amount,
        "portal_amount": portal_amount,
        # One invoice can genuinely have two different problems — INV-MAR-011
        # has both a tax-head mismatch and an amount mismatch. Keying dedup on
        # the invoice alone dropped whichever arrived second, which happened to
        # be the one carrying the money.
        "dedup_key": (invoice_no, reason_code),
    }


def _amount(value: object) -> float | None:
    """Postgres NUMERIC arrives as Decimal, which FastAPI serialises as a JSON
    *string* — `"17100"`, and `"1.8E+5"` for 180000. Every amount therefore
    crosses the wire as a float, or the console renders scientific notation to
    someone reading a tax position."""
    return None if value is None else float(value)


def _case_exposure(case: dict) -> float:
    books = float(case["books_amount"]) if case["books_amount"] is not None else 0.0
    if case["portal_amount"] is None:
        return books
    return abs(books - float(case["portal_amount"]))


def _dataset_total(entry: dict | None, field: str, only: set[str] | None = None) -> float | None:
    """Sum one mapped field across an uploaded file's rows.

    `only` restricts to a set of invoice numbers, which is how the dashboard
    separates the book value carrying a case from the rest. Returns None when
    the file or the mapping is absent — "no books uploaded" is a different
    statement from "₹0 of books", and a dashboard that renders them the same
    way is lying about one of them.
    """
    if entry is None:
        return None
    mapping, headers = entry["mapping"], entry["headers"]
    column = mapping.get(field)
    if column is None or not (0 <= column < len(headers)):
        return None
    invoice_col = mapping.get("invoice_no")
    invoice_header = (
        headers[invoice_col] if invoice_col is not None and 0 <= invoice_col < len(headers) else None
    )
    header = headers[column]
    total = 0.0
    for row in entry["rows"]:
        if only is not None:
            if invoice_header is None:
                return None
            if str(row.get(invoice_header, "")).strip() not in only:
                continue
        try:
            total += float(row.get(header) or 0)
        except (TypeError, ValueError):
            continue
    return total


def group_by_exposure(cases: list[dict], field: str, absent: str | None = None) -> list[dict]:
    """Group cases by one field, totalling with the shared exposure rule.

    Every "cases grouped by X, with a money column" screen goes through here.
    They were separate SQL aggregations, each free to define exposure its own
    way, and they did: `COALESCE(portal_amount, books_amount)` turned a case
    with no portal side into ABS(x - x) = 0, so the screens disagreed with the
    register about the same cases.
    """
    groups: dict[str, list[dict]] = {}
    for case in cases:
        value = case.get(field)
        if value is None:
            if absent is None:
                continue
            value = absent
        groups.setdefault(value, []).append(case)
    return sorted(
        (
            {"key": key, "case_count": len(members), "exposure": period_exposure(members)}
            for key, members in groups.items()
        ),
        key=lambda r: r["exposure"],
        reverse=True,
    )


def period_exposure(cases: list[dict]) -> float:
    """Money at risk across a set of cases, counting each invoice once.

    Two rules can flag the same invoice: `gst:SupplierNotFiled` and
    `gst:PotentialMismatch` both fired on INV-MAR-013 for the same ₹17,100.
    Summing per case reported ₹52,070 at risk for March 2026 where ₹26,240 is
    — the same rupees counted twice because two rules noticed them.

    Where rules disagree about how much of one invoice is at risk, the largest
    is taken. That states the strongest claim the rules actually make about
    that invoice; adding them would invent a total no rule computed.
    """
    worst: dict[str, float] = {}
    for case in cases:
        invoice = case["invoice_no"]
        worst[invoice] = max(worst.get(invoice, 0.0), _case_exposure(case))
    return sum(worst.values())


def _case_row(case: dict) -> dict:
    return {
        "id": str(case["id"]),
        "invoice_no": case["invoice_no"],
        "reason_code": case["reason_code"],
        "status": case["status"],
        "supplier_name": case["supplier_name"],
        "supplier_gstin": case["supplier_gstin"],
        "books_amount": _amount(case["books_amount"]),
        "portal_amount": _amount(case["portal_amount"]),
        "exposure": _case_exposure(case),
    }


@app.get("/api/clients/{client_id}/periods/{period_id}/register")
async def register_route(client_id: str, period_id: str, reason_code: str | None = None) -> dict:
    """Plan 122b B4's own RED: the exposure total equals the sum of the
    *filtered* rows — computed here from the same list the response
    returns, in one pass, so a filter and its total cannot silently
    disagree the way two independently-computed values could."""
    pool = _require_db_pool()
    async with pool.acquire() as conn:
        cases = await repo.list_cases(conn, client_id=client_id, period_id=period_id)
    if reason_code is not None:
        cases = [c for c in cases if c["reason_code"] == reason_code]
    # Human title, meaning and next action from the pack, plus what is wrong
    # with *this* invoice computed from its own figures — so the list is
    # readable without opening anything, and `gst:AmountMismatch` never
    # reaches a business reader as a label.
    rows = rule_guidance.decorate(
        sorted((_case_row(c) for c in cases), key=lambda r: r["exposure"], reverse=True),
        _pack_guidance(),
    )
    rows = [{**row, "narrative": case_narrative.narrate(row)} for row in rows]
    # Not `sum(r["exposure"] ...)`: two rules flagging one invoice would count
    # the same rupees twice. See `period_exposure`.
    return {"rows": rows, "total_exposure": period_exposure(cases)}


@app.get("/api/clients/{client_id}/periods/{period_id}/exceptions")
async def exceptions_route(client_id: str, period_id: str) -> list[dict]:
    pool = _require_db_pool()
    async with pool.acquire() as conn:
        cases = await repo.list_cases(conn, client_id=client_id, period_id=period_id)
    return [
        {"reason_code": g["key"], "count": g["case_count"], "total_exposure": g["exposure"]}
        for g in group_by_exposure(cases, "reason_code", absent="unclassified")
    ]


@app.get("/api/clients/{client_id}/periods/{period_id}/register/{case_id}")
async def case_detail_route(client_id: str, period_id: str, case_id: str) -> dict:
    pool = _require_db_pool()
    async with pool.acquire() as conn:
        case = await repo.get_case(conn, client_id=client_id, case_id=case_id)
        if case is None or str(case["period_id"]) != period_id:
            raise HTTPException(status_code=404, detail="case not found for this client and period")
        siblings = await repo.list_cases(conn, client_id=client_id, period_id=period_id)
        ims_decisions = await repo.list_ims_decisions(conn, client_id=client_id, period_id=period_id)

    group = [c for c in siblings if c["reason_code"] == case["reason_code"]]
    group.sort(key=lambda c: c["created_at"])
    index = next(i for i, c in enumerate(group) if str(c["id"]) == case_id)
    prev_id = str(group[index - 1]["id"]) if index > 0 else None
    next_id = str(group[index + 1]["id"]) if index < len(group) - 1 else None

    evidence: list[dict] = []
    graph_reachable = False
    if case["subject"]:
        try:
            findings = graphowl_client.list_findings(GRAPH_OWL_SERVER, GRAPH_OWL_TOKEN)
            graph_reachable = True
            evidence = evidence_for_subject(findings, case["subject"], case["reason_code"])
        except graphowl_client.IngestError:
            graph_reachable = False

    row = _case_row(case)
    row.update(
        {
            "subject": case["subject"],
            "summary": case["summary"],
            "governed_by": case["governed_by"],
            "evidence_count": case["evidence_count"],
            # The facts themselves, read live from graph-owl. Stored counts
            # let the console say "4 fact(s) cited" and show none of them; a
            # case that cannot show its evidence is an assertion. Read live
            # rather than copied at reconcile time so it reflects the graph as
            # it stands, and degrades to an empty list — with `evidence_count`
            # still present — when graph-owl is unreachable.
            "evidence": evidence,
            "graph_reachable": graph_reachable,
            "group_reason_code": case["reason_code"],
            "graphowl_url": GRAPH_OWL_SERVER,
            "prev_id": prev_id,
            "next_id": next_id,
            "ims_decisions": [
                {"decision": d["decision"], "decided_at": d["decided_at"].isoformat()}
                for d in ims_decisions
                if str(d["case_id"]) == case_id
            ],
        }
    )
    return row


@app.post("/api/clients/{client_id}/periods/{period_id}/register/{case_id}/ims", status_code=201)
async def case_ims_decision_route(client_id: str, period_id: str, case_id: str, payload: dict) -> dict:
    pool = _require_db_pool()
    decision = payload.get("decision")
    if decision not in ("accept", "reject", "pending"):
        raise HTTPException(status_code=400, detail='decision must be "accept", "reject" or "pending"')
    async with pool.acquire() as conn:
        case = await repo.get_case(conn, client_id=client_id, case_id=case_id)
        if case is None or str(case["period_id"]) != period_id:
            raise HTTPException(status_code=404, detail="case not found for this client and period")
        decision_id = await repo.create_ims_decision(
            conn, client_id=client_id, period_id=period_id, case_id=case_id, decision=decision
        )
    return {"id": decision_id, "decision": decision}


#: Every model output this process refused, and why — Plan 123 Slice F.
#:
#: **For an agentic product the record of what an agent tried and was refused
#: is worth more than the record of what it produced.** A refusal nobody
#: counts is a safety property nobody can audit, and the first question after
#: an incident is "has this happened before".
#:
#: In-process and bounded: this is an observability surface, not an audit log,
#: and a genuine audit trail belongs in graph-owl's own agent activity rather
#: than in a list that dies with the worker.
AGENT_REFUSALS: list[dict] = []

#: Above this the oldest are dropped. A refusal ledger that grows without
#: bound is a memory leak wearing the clothes of a safety feature.
MAX_REFUSALS_HELD = 200


#: The live trigger bus. In-process, like `AGENT_REFUSALS` and for the same
#: reason: this is the *shape* of the runtime, and a durable subscription
#: store belongs in graph-owl's own `graph-owl-events` rather than in a second
#: implementation here.
AGENT_REGISTRY = agent_runtime.default_registry()

#: What every agent is told before anything else. Stated once so two agents
#: cannot be given different instructions about the same rule.
AGENT_SYSTEM_PROMPT = (
    "You are a precise Indian indirect-tax assistant working inside a GST "
    "reconciliation tool. You never invent, compute, round or introduce a "
    "figure. Every number you use must appear verbatim in what you are given."
)

#: Agents start with the grants their work needs. Deny-by-default still holds —
#: these are granted explicitly here, and revoking one from the UI takes effect
#: on the very next write because every write re-checks.
for _agent in ("triage", "vendor", "explainer", "risk"):
    AGENT_REGISTRY.grant(_agent, "propose")

#: Runs this process has performed, newest last.
AGENT_RUNS: list[dict] = []


async def _cases_for(client_id: str, period_id: str) -> list[dict]:
    """The period's cases, for an agent to work on.

    Its own read rather than a parameter threaded through every caller: an
    agent's inputs should not depend on which route happened to fire the event,
    or two callers of one event would wake the same agent with different data.
    """
    pool = _require_db_pool()
    async with pool.acquire() as conn:
        return await repo.list_cases(conn, client_id=client_id, period_id=period_id)


def wake_agents(event: str, cases: list[dict] | None = None, **context) -> list[str]:
    """Fire an event and **actually run** every agent subscribed to it.

    **This used to record that an agent would have run.** It created an
    `AgentRun`, took its empty summary, and appended it — no agent read
    anything, decided anything or produced anything, and the activity screen
    faithfully reported that nothing had happened.

    Each run now keeps its full trace: what it read, what it decided and why,
    every model call with whether the answer was grounded, and every refused
    write. That trace is the deliverable — an agent whose reasoning cannot be
    inspected afterwards is one nobody can be accountable for.

    **An event nobody subscribes to is not an error.** Most events have no
    listener, and treating that as a failure fills the log with noise.
    """
    woken = AGENT_REGISTRY.woken_by(event)
    model = (lambda prompt: ai.chat(AGENT_SYSTEM_PROMPT, prompt)) if ai.is_available() else None

    for agent in woken:
        implementation = agents.AGENTS.get(agent)
        if implementation is None:
            # Subscribed but not yet built. Recorded as skipped rather than
            # silently dropped: a subscription with no implementation is a gap
            # somebody should see, not a no-op.
            AGENT_RUNS.append(
                {
                    "id": uuid.uuid4().hex[:12],
                    "agent": agent,
                    "event": event,
                    "status": "skipped",
                    "error": "no implementation for this agent yet",
                    "spans": [],
                    "context": context,
                    "started_at": datetime.now(timezone.utc).isoformat(),
                }
            )
            continue
        try:
            kwargs: dict = {
                "cases": cases or [],
                "registry": AGENT_REGISTRY,
                "model": model,
                "context": context,
            }
            # Only the graph-backed agent takes an MCP caller; passing one to
            # every agent would suggest they all use it, and two of them do
            # not.
            if agent == "risk":
                kwargs["mcp"] = lambda tool, args: mcp_client.call(GRAPH_OWL_SERVER, tool, args)
            run = implementation(**kwargs)
            record = {**run.summary(), "spans": run.spans, "writes": run.writes,
                      "refusals": run.refusals, "context": context}
        except Exception as exc:  # noqa: BLE001
            # An agent that throws must not take the request with it, and the
            # failure has to be visible — a run that vanished is worse than one
            # that failed.
            record = {
                "id": uuid.uuid4().hex[:12], "agent": agent, "event": event,
                "status": "failed", "error": str(exc), "spans": [], "context": context,
            }
        record["started_at"] = datetime.now(timezone.utc).isoformat()
        AGENT_RUNS.append(record)

    if len(AGENT_RUNS) > MAX_REFUSALS_HELD:
        del AGENT_RUNS[: len(AGENT_RUNS) - MAX_REFUSALS_HELD]
    return woken


@app.get("/api/agents/runs")
async def agent_runs_route(agent: str | None = None, status: str | None = None) -> dict:
    """Every agent run this deployment has performed, newest first.

    Read from the durable store when one is configured — the runs survive a
    restart there — and from process memory otherwise, which is all a
    database-less desktop session has.
    """
    pool = getattr(app.state, "db_pool", None)
    if pool is not None:
        async with pool.acquire() as conn:
            runs = await repo.list_agent_runs(conn)
    else:
        runs = list(reversed(AGENT_RUNS))
    if agent:
        runs = [r for r in runs if r.get("agent") == agent]
    if status:
        runs = [r for r in runs if r.get("status") == status]

    return {
        "runs": [{k: v for k, v in r.items() if k != "spans"} for r in runs],
        "running": [r["id"] for r in runs if r.get("status") == "running"],
        "counts": {
            s: sum(1 for r in AGENT_RUNS if r.get("status") == s)
            for s in ("completed", "failed", "running", "skipped")
        },
        "scope": "this process only — a durable record belongs in graph-owl agent activity",
    }


@app.get("/api/agents/runs/{run_id}")
async def agent_run_detail_route(run_id: str) -> dict:
    """One run's full trace: every step, in order, with what went in and out.

    The hierarchy current agent-observability practice asks for — tool calls
    and decisions, not only the model calls. A trace of model calls alone
    cannot answer the question anyone actually asks after a bad outcome, which
    is *what did it look at before it decided that*.
    """
    run = next((r for r in AGENT_RUNS if r.get("id") == run_id), None)
    if run is None:
        pool = getattr(app.state, "db_pool", None)
        if pool is not None:
            async with pool.acquire() as conn:
                run = await repo.get_agent_run(conn, run_id=run_id)
    if run is None:
        raise HTTPException(status_code=404, detail="no such run")
    return run


@app.get("/api/agents/runs/{run_id}/report")
async def agent_run_report_route(run_id: str) -> dict:
    """What this agent did, as prose a person can read or send.

    **Assembled from the trace, never from the model.** A report about what an
    agent did that was itself written by a model is two claims stacked on each
    other, and the one underneath is the one you needed. Every line here is
    rendered from a recorded span.
    """
    run = next((r for r in AGENT_RUNS if r.get("id") == run_id), None)
    if run is None:
        raise HTTPException(status_code=404, detail="no such run")

    lines = [
        f"Agent: {run.get('agent')}",
        f"Woken by: {run.get('event')}",
        f"Outcome: {run.get('status')}"
        + (f" — {run['error']}" if run.get("error") else ""),
        f"Took: {run.get('ms')} ms" if run.get("ms") is not None else "Took: not recorded",
        "",
        "What it did, step by step:",
    ]
    for index, span in enumerate(run.get("spans") or [], start=1):
        detail = span.get("because") or span.get("error") or ""
        lines.append(
            f"  {index}. [{span.get('kind')}] {span.get('name')} — {span.get('status')}"
            f" ({span.get('ms')} ms){' · ' + detail if detail else ''}"
        )
    if not run.get("spans"):
        lines.append("  (no steps recorded)")

    writes = run.get("writes") or []
    lines += ["", f"Proposed: {len(writes)} write(s)."]
    for write in writes:
        payload = write.get("payload") or {}
        summary = ", ".join(f"{k}: {len(v) if isinstance(v, list) else v}" for k, v in payload.items())
        lines.append(f"  - {write.get('capability')} — {summary}")

    refusals = run.get("refusals") or []
    if refusals:
        lines += ["", f"Refused: {len(refusals)}."]
        for refusal in refusals:
            lines.append(f"  - {refusal.get('capability')}: {refusal.get('reason')}")

    tokens, cost = run.get("tokens"), run.get("cost")
    lines += [
        "",
        # `None`, never 0 — a cost of zero claims the run was free.
        f"Model usage: {tokens if tokens is not None else 'not measured'} tokens, "
        f"{cost if cost is not None else 'not measured'} cost.",
    ]

    return {"run_id": run_id, "report": "\n".join(lines), "generated_from": "the run's own trace"}


@app.get("/api/agents/subscriptions")
async def agent_subscriptions_route() -> dict:
    """Which agent wakes on which event, and what each may do.

    An agent fleet whose wiring is not inspectable is one nobody can reason
    about after an incident.

    `implemented` names which subscriptions have code behind them: seven of
    the ten below are declared but not yet built, and a screen that lists
    them indistinguishably from the real ones advertises a fleet that does
    not exist.
    """
    return {
        "subscriptions": [
            {"agent": s.agent, "event": s.event, "implemented": s.agent in agents.AGENTS}
            for s in sorted(agent_runtime.DEFAULT_SUBSCRIPTIONS, key=lambda s: (s.event, s.agent))
        ],
        "grants": sorted(
            ({"agent": a, "capability": c} for a, c in AGENT_REGISTRY._grants),
            # Dicts have no ordering; sort on the pair. Grants exist from
            # module import, so this route 500'd on every call until now.
            key=lambda g: (g["agent"], g["capability"]),
        )
        if AGENT_REGISTRY._grants
        else [],
    }


@app.post("/api/agents/{agent}/grant/{capability}")
async def grant_route(agent: str, capability: str) -> dict:
    AGENT_REGISTRY.grant(agent, capability)
    return {"agent": agent, "capability": capability, "granted": True}


@app.delete("/api/agents/{agent}/grant/{capability}")
async def revoke_route(agent: str, capability: str) -> dict:
    """Revoke a grant, effective immediately — including mid-run.

    Every write re-checks, which is what makes this meaningful: a grant
    checked only at dispatch cannot be revoked at all, because the run is
    already past the check by the time anyone clicks.
    """
    AGENT_REGISTRY.revoke(agent, capability)
    return {"agent": agent, "capability": capability, "granted": False}


@app.get("/api/agents/activity")
async def agent_activity_route() -> dict:
    """What the model was refused, and why — Plan 123 Slice F.

    Reports refusals rather than successes on purpose. A list of things that
    worked tells a reviewer nothing they cannot see on the screens themselves;
    a list of figures an agent tried to state and could not support is the
    only place the fact-citation rule becomes visible as something that is
    actually running.
    """
    if len(AGENT_REFUSALS) > MAX_REFUSALS_HELD:
        del AGENT_REFUSALS[: len(AGENT_REFUSALS) - MAX_REFUSALS_HELD]
    return {
        "refusals": list(reversed(AGENT_REFUSALS)),
        "runs": list(reversed(AGENT_RUNS)),
        "held": len(AGENT_REFUSALS),
        "cap": MAX_REFUSALS_HELD,
        # Stated so a reader does not mistake this for an audit trail.
        "scope": "this process only — a durable record belongs in graph-owl agent activity",
    }


@app.get("/api/clients/{client_id}/periods/{period_id}/cases/{case_id}/graph")
async def case_graph_route(client_id: str, period_id: str, case_id: str) -> dict:
    """The invoice's own neighbourhood — the same visual pattern the GraphOWL
    console's Explore screen uses, seeded on this case's subject.

    Reuses graph-owl's real `/graph/context`, the same call Explore itself
    makes — this is not a second graph rendering pipeline, it is the one that
    already exists, reshaped for an SVG panel instead of a canvas one.
    """
    pool = _require_db_pool()
    async with pool.acquire() as conn:
        case = await repo.get_case(conn, client_id=client_id, case_id=case_id)
    if case is None:
        raise HTTPException(status_code=404, detail="no such case")

    seed = case.get("subject")
    if not seed:
        # No recorded subject — nothing to seed the walk from. An empty
        # picture, not an error: the case explanation above this panel still
        # works without it.
        return {"seed": None, "nodes": [], "edges": []}

    context = graphowl_client.graph_context(GRAPH_OWL_SERVER, seed)
    nodes = context.get("nodes") or []
    # graph-owl's own node ids are the Sid's local id, not the full IRI we
    # seeded with — the node the walk started from is found by matching the
    # `iri` it also returns, rather than reproducing Sid encoding here.
    seed_local = next((n["id"] for n in nodes if n.get("iri") == seed), seed)

    # The class per node, for the badge. `/graph/context`'s own `sources`
    # field names import graphs, not RDF classes — resolved separately, keyed
    # by IRI, then re-keyed by the local id `build_picture` uses.
    by_iri = graphowl_client.node_classes(
        GRAPH_OWL_SERVER, [n["iri"] for n in nodes if n.get("iri")]
    )
    classes = {
        n["id"]: by_iri[n["iri"]]
        for n in nodes
        if n.get("iri") in by_iri
    }

    return case_graph.build_picture(
        seed=seed_local, nodes=nodes, edges=context.get("edges") or [], classes=classes
    )


@app.get("/api/clients/{client_id}/periods/{period_id}/cases/{case_id}/explain")
async def explain_case_route(client_id: str, period_id: str, case_id: str) -> dict:
    """Why this rule fired for **this** invoice, from its real data.

    The pack's guidance says what the rule means; `case_narrative` states the
    two headline figures. Neither reads the rest of the row. This gathers both
    sides in full — tax heads, dates, HSN, place of supply — and asks a model
    to say what is notable, which is the kind of work current practice on AI in
    tax says models are actually good at.

    **Every figure is supplied, and nothing outside them survives.** A model
    handed a case with no numbers can only invent them; handed every number in
    the row it can be specific without inventing anything. Anything it states
    beyond the supplied set is refused by `grounding` and the computed sentence
    shown instead — so the worst case is the answer we had before, never a
    confident wrong one.
    """
    pool = _require_db_pool()
    async with pool.acquire() as conn:
        case = await repo.get_case(conn, client_id=client_id, case_id=case_id)
        if case is None:
            raise HTTPException(status_code=404, detail="no such case")
        stored = await repo.list_dataset_uploads(conn, client_id=client_id, period_id=period_id)

    by_kind = {e["kind"]: e for e in stored}

    def row_for(kind: str) -> dict | None:
        entry = by_kind.get(kind)
        if entry is None:
            return None
        dataset = {"headers": entry["headers"], "rows": entry["rows"]}
        rows = graphowl_client.net_credit_notes(
            graphowl_client.aggregate_invoice_lines(_normalize(dataset, entry["mapping"]))
        )
        wanted = graphowl_client.normalize_invoice_no(case["invoice_no"])
        return next(
            (
                r
                for r in rows
                if graphowl_client.normalize_invoice_no(r.get("invoice_no")) == wanted
            ),
            None,
        )

    guidance = _pack_guidance().get(str(case.get("reason_code")), {})
    facts = case_explainer.gather_facts(
        case=case,
        books_row=row_for("books"),
        portal_row=row_for("gstr2b"),
        guidance=guidance,
    )

    computed = case_narrative.narrate(case)
    if not ai.is_available():
        return {
            "text": computed,
            "source": "computed",
            "facts": facts,
            # Said out loud rather than left to look like the model's answer.
            "note": "No inference model is reachable, so this is the computed "
            "explanation. Every figure in it is read from your data.",
        }

    drafted = ai.chat(
        "You are a precise Indian indirect-tax assistant. You never invent figures.",
        case_explainer.build_prompt(facts),
    )
    if not drafted:
        return {"text": computed, "source": "computed", "facts": facts, "note": None}

    checked = grounding.ground_draft(
        draft=drafted,
        supplied=case_explainer.numeric_facts(facts),
        log=AGENT_REFUSALS,
    )
    if not checked["grounded"]:
        return {
            "text": computed,
            "source": "computed",
            "facts": facts,
            "note": "The model's explanation stated a figure your data does not "
            "carry, so it was refused. This is the computed explanation.",
            "refusal": checked["reason"],
        }
    return {"text": drafted.strip(), "source": "model", "facts": facts, "note": None}


@app.get("/api/clients/{client_id}/cases/{case_id}/defence")
async def case_defence_route(client_id: str, case_id: str) -> dict:
    """Everything an officer would ask about one case — Plan 123 Slice E.

    figure → finding → provision → derivation → source. The derivation comes
    from graph-owl's `/reasoning/explain`, which has existed since Epic 6 and
    which nothing asked for until now: Reco Now had the finding and the
    citation and stopped there, so "why was this credit treated this way"
    could be answered only with "the system flagged it".

    **A graph-owl that is unreachable degrades the chain rather than failing
    it.** A defence pack missing its derivation step, and saying so, is more
    use than a 502 — the other four links are still the best available answer.
    """
    pool = _require_db_pool()
    async with pool.acquire() as conn:
        case = await repo.get_case(conn, client_id=client_id, case_id=case_id)
        if case is None:
            raise HTTPException(status_code=404, detail="no such case")
        uploads = await repo.list_dataset_uploads(
            conn, client_id=client_id, period_id=case["period_id"]
        )

    explanation: dict = {}
    if case.get("subject"):
        try:
            query = notice_defence.explain_query(
                case, predicate="gst:taxAmount", value=str(case.get("books_amount") or "")
            )
            explanation = graphowl_client._request(
                f"{GRAPH_OWL_SERVER.rstrip('/')}/reasoning/explain"
                f"?s={quote(query['s'], safe='')}&p={quote(query['p'], safe='')}"
                f"&o={quote(query['o'], safe='')}",
                method="GET",
            ) or {}
        except Exception as exc:  # noqa: BLE001
            print(f"[graphowl] explain unavailable — {exc}")

    books = next((u for u in uploads if u["kind"] == "books"), None)
    upload = (
        {
            "filename": books.get("filename"),
            "uploaded_at": str(books.get("uploaded_at") or ""),
            "row": None,
        }
        if books
        else None
    )

    return notice_defence.defence_chain(case=case, explanation=explanation, upload=upload)


@app.get("/api/clients/{client_id}/suppliers/{gstin}/memory")
async def supplier_memory_route(client_id: str, gstin: str) -> dict:
    """What this supplier reliably does, across every period held — Plan 123
    Slice E.

    **The capability Reco Now was missing entirely.** A supplier flagged in
    March, April and May was flagged three separate times with nothing
    connecting them: next period the same finding arrives looking like a first
    occurrence. This reads every period at once and says whether there is a
    habit — and only where there is one, because a supplier late once is an
    incident and recording it as a characteristic would prejudice every future
    period against them on a single data point.
    """
    pool = _require_db_pool()
    async with pool.acquire() as conn:
        observations = await repo.supplier_observations(conn, client_id=client_id, gstin=gstin)

    pattern = capabilities.supplier_pattern(observations)
    if pattern is None:
        return {
            "gstin": gstin,
            "pattern": None,
            # Named rather than left as an empty response: "nothing recurring"
            # and "not enough history to say" are different answers, and only
            # one of them is reassuring.
            "periods_seen": len({o["period"] for o in observations if o.get("period")}),
            "threshold": capabilities.MIN_PERIODS_FOR_A_PATTERN,
        }

    name = next((o.get("supplier_name") for o in observations if o.get("supplier_name")), gstin)
    return {
        "gstin": gstin,
        "pattern": pattern,
        "memory": capabilities.memory_for_supplier(gstin=gstin, name=name, pattern=pattern),
        "threshold": capabilities.MIN_PERIODS_FOR_A_PATTERN,
    }


@app.post("/api/clients/{client_id}/suppliers/{gstin}/memory")
async def record_supplier_memory_route(client_id: str, gstin: str) -> dict:
    """Write the pattern to graph-owl as a memory, where it survives periods.

    Idempotent in the way that matters: graph-owl supersedes rather than
    edits, so writing the same pattern twice leaves a correctable history
    rather than a duplicate — and a memory that later turns out to be wrong
    can be withdrawn without erasing the fact that it was once believed.
    """
    body = await supplier_memory_route(client_id, gstin)
    memory = body.get("memory")
    if memory is None:
        raise HTTPException(
            status_code=409,
            detail=(
                f"no pattern to record — this supplier has been seen in "
                f"{body['periods_seen']} period(s), and a pattern needs "
                f"{body['threshold']}"
            ),
        )

    try:
        graphowl_client._request(
            f"{GRAPH_OWL_SERVER.rstrip('/')}/memories",
            method="POST",
            body=json.dumps(memory).encode(),
        )
    except Exception as exc:  # noqa: BLE001
        raise HTTPException(status_code=502, detail=f"graph-owl refused the memory: {exc}") from exc

    return {"recorded": True, "memory": memory}


@app.post("/api/clients/{client_id}/cases/{case_id}/waive")
async def waive_case_route(client_id: str, case_id: str, payload: dict) -> dict:
    """Accept an exception, with a reason and an expiry — Plan 123 Slice E.

    **Replaces Reco Now's own `approval` table, which required neither.** A
    waiver that does not expire is a rule change nobody voted for: the check
    stops running and nobody remembers deciding that. graph-owl requires both,
    and keys the waiver on the finding's *identity* rather than its row id,
    because results are replaced wholesale each pass and a waiver keyed on a
    row would point at nothing after the next run.
    """
    pool = _require_db_pool()
    async with pool.acquire() as conn:
        case = await repo.get_case(conn, client_id=client_id, case_id=case_id)
    if case is None:
        raise HTTPException(status_code=404, detail="no such case")

    expires_raw = payload.get("expires_at") or payload.get("expiresAt")
    if not expires_raw:
        raise HTTPException(status_code=400, detail="expires_at is required — a waiver must expire")
    try:
        expires_at = datetime.fromisoformat(str(expires_raw).replace("Z", "+00:00"))
        request = capabilities.waiver_request(
            shape=case["reason_code"],
            focus_node=case.get("subject") or f"urn:invoice:{case['invoice_no']}",
            constraint=case.get("governed_by") or case["reason_code"],
            reason=str(payload.get("reason") or ""),
            expires_at=expires_at,
        )
    except ValueError as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc

    try:
        graphowl_client._request(
            f"{GRAPH_OWL_SERVER.rstrip('/')}/validation/waivers",
            method="POST",
            body=json.dumps(request).encode(),
        )
    except Exception as exc:  # noqa: BLE001
        raise HTTPException(status_code=502, detail=f"graph-owl refused the waiver: {exc}") from exc

    return {"waived": True, "waiver": request}


@app.post("/api/clients/{client_id}/periods/{period_id}/follow-ups/drafts")
async def follow_up_drafts_route(client_id: str, period_id: str) -> dict:
    """Draft a chase message per supplier who has not filed.

    **Runs the vendor agent rather than duplicating it.** The agent already
    knows which findings mean "the supplier has not filed", already dedupes an
    invoice flagged by two rules, and already puts every draft through the
    grounding check. A second implementation here would be a second set of
    those decisions to keep in step.

    Grouped per supplier, because that is who receives it: a supplier with
    three unfiled invoices gets one message, not three.
    """
    cases = await _cases_for(client_id, period_id)
    model = (lambda prompt: ai.chat(AGENT_SYSTEM_PROMPT, prompt)) if ai.is_available() else None

    run = agents.run_vendor(
        cases=[dict(c) for c in cases],
        registry=AGENT_REGISTRY,
        model=model,
        context={"client_id": client_id, "period_id": period_id},
    )
    AGENT_RUNS.append({**run.summary(), "spans": run.spans, "writes": run.writes,
                       "refusals": run.refusals,
                       "started_at": datetime.now(timezone.utc).isoformat()})

    drafts = next((w["payload"]["drafts"] for w in run.writes if "drafts" in w["payload"]), [])
    return {
        "groups": follow_ups.group_drafts(drafts=drafts, cases=[dict(c) for c in cases]),
        # The run is linked so a reader can see what the agent actually did —
        # every model call, and anything it was refused.
        "run_id": run.id,
    }


@app.get("/api/clients/{client_id}/periods/{period_id}/working-paper/report")
async def working_paper_report_route(client_id: str, period_id: str) -> dict:
    """The working paper written up as a document, with a filename to save it under.

    A table of five figures answers "what is the number". A working paper has
    to answer *how did you get there, and what did you leave out* — the
    question a partner or an officer actually asks.

    Grounded like every other generated text here. A working paper is filed
    evidence: a figure invented in one is worse than having no working paper.
    """
    paper = await working_paper_route(client_id, period_id)
    pool = _require_db_pool()
    async with pool.acquire() as conn:
        period = await repo.get_period(conn, client_id=client_id, period_id=period_id)

    label = f"{(period or {}).get('month', '')} {(period or {}).get('year', '')}".strip() or "this period"
    filename, computed = working_paper_report.downloadable(paper, period=label)

    if not ai.is_available():
        return {"report": computed, "filename": filename, "source": "computed",
                "note": "No inference model is reachable, so this is the computed write-up."}

    drafted = ai.chat(AGENT_SYSTEM_PROMPT, working_paper_report.build_prompt(computed))
    if not drafted:
        return {"report": computed, "filename": filename, "source": "computed", "note": None}

    # Every figure the computed document prints, so a faithful rewrite cannot
    # be refused — the same contract the client report uses.
    supplied = {f"line_{i}": line for i, line in enumerate(computed.splitlines()) if line.strip()}
    checked = grounding.ground_draft(draft=drafted, supplied=supplied, log=AGENT_REFUSALS)
    if not checked["grounded"]:
        return {"report": computed, "filename": filename, "source": "computed",
                "note": "The model's write-up stated a figure this paper does not carry, so it "
                "was refused. This is the computed version.",
                "refusal": checked["reason"]}
    return {"report": drafted.strip(), "filename": filename, "source": "model", "note": None}


@app.get("/api/clients/{client_id}/periods/{period_id}/client-report")
async def client_report_route(client_id: str, period_id: str) -> dict:
    """The monthly report a CA sends a client.

    **The skeleton is ours; only the prose is the model's.** A model asked for
    "a report" produces a different shape every time, and a document a client
    receives monthly must not be reorganised at random.

    Grounded like every other generated text here: a report stating a figure
    the reconciliation does not carry is refused and the computed version
    returned. This is the worst place in the product for an invented number,
    because it leaves the building.
    """
    recon = await reconciliation_route(client_id, period_id)
    pool = _require_db_pool()
    async with pool.acquire() as conn:
        period = await repo.get_period(conn, client_id=client_id, period_id=period_id)

    facts = client_report.build_facts(
        period=f"{(period or {}).get('month', '')} {(period or {}).get('year', '')}".strip()
        or "this period",
        counts=recon["counts"],
        itc=recon["itc"],
        match_rate=recon["match_rate"],
        outcomes=recon["rule_outcomes"],
    )
    computed = client_report.computed_report(facts)

    if not ai.is_available():
        return {"report": computed, "source": "computed", "facts": facts,
                "note": "No inference model is reachable, so this is the computed report."}

    drafted = ai.chat(AGENT_SYSTEM_PROMPT, client_report.build_prompt(facts, computed))
    if not drafted:
        return {"report": computed, "source": "computed", "facts": facts, "note": None}

    checked = grounding.ground_draft(
        draft=drafted, supplied=client_report.groundable(facts), log=AGENT_REFUSALS
    )
    if not checked["grounded"]:
        return {
            "report": computed, "source": "computed", "facts": facts,
            "note": "The model's report stated a figure your reconciliation does not "
            "carry, so it was refused. This is the computed report.",
            "refusal": checked["reason"],
        }
    return {"report": drafted.strip(), "source": "model", "facts": facts, "note": None}


@app.get("/api/clients/{client_id}/periods/{period_id}/working-paper")
async def working_paper_route(client_id: str, period_id: str) -> dict:
    """The GSTR-3B working paper — gross → deductions → net, every figure
    traced, and the filed return beside it.

    Plan 123 §4 calls this *the deliverable*. It is the document a partner
    reviews and an officer asks about, so each line names its own source and
    every statutory deduction cites the provision that required it.

    Amounts come from the **cases** rather than from the raw findings, because
    a case already carries the tax at stake (`case_from_finding` resolves it
    from the finding's own evidence bindings) and is what the rest of the
    product counts. Deriving it twice, differently, is how the summary and the
    detail come to disagree.
    """
    pool = _require_db_pool()
    async with pool.acquire() as conn:
        stored = await repo.list_dataset_uploads(conn, client_id=client_id, period_id=period_id)
        cases = await repo.list_cases(conn, client_id=client_id, period_id=period_id)

    by_kind = {entry["kind"]: entry for entry in stored}

    def rows_for(kind: str) -> list[dict]:
        entry = by_kind.get(kind)
        if entry is None:
            return []
        dataset = {"headers": entry["headers"], "rows": entry["rows"]}
        return graphowl_client.net_credit_notes(
            graphowl_client.aggregate_invoice_lines(_normalize(dataset, entry["mapping"]))
        )

    def tax_of(row: dict) -> Decimal:
        return Decimal(str(graphowl_client._combined_tax_amount(row)))

    portal = [{"tax_amount": tax_of(row)} for row in rows_for("gstr2b")]

    # A case's own booked tax is the amount at stake. **`None` is passed
    # through, never coerced to zero** — `build_working_paper` counts an
    # unquantified deduction separately, and flattening it here would report a
    # complete chain over a case whose amount nobody established. That is
    # exactly what this route did until a live run showed a flagged
    # `gst:PaymentOverdue` deducting zero.
    findings = [
        {
            "label": case["reason_code"],
            "tax_amount": None
            if case["books_amount"] is None
            else Decimal(str(case["books_amount"])),
        }
        for case in cases
    ]

    # The 3B, if one was uploaded. Its figures are period totals, so the first
    # row is the whole return — unlike every line-level kind here.
    filed_rows = rows_for("gstr3b")
    gstr3b = filed_rows[0] if filed_rows else None

    paper = working_paper.build_working_paper(
        gstr2b=portal, findings=findings, gstr3b=gstr3b
    )
    paper["explain"] = explain.WORKING_PAPER
    # Says out loud why this total differs from the ITC position screen's.
    paper["compare_note"] = explain.ITC_VS_WORKING_PAPER
    return _decimals_to_floats(paper)


@app.get("/api/clients/{client_id}/periods/{period_id}/reconciliation")
async def reconciliation_route(client_id: str, period_id: str) -> dict:
    """The reconciliation *result* — four buckets, a match rate, an ITC
    position — not just its exceptions.

    A finding is raised only when something is wrong, so a matched invoice
    produces none. Every screen built on findings alone could report what
    needs attention and never what was done, which is the first thing a
    reviewer asks. Computed from the uploaded rows and the findings together,
    in one pass, so the summary and the list cannot disagree.
    """
    pool = _require_db_pool()
    async with pool.acquire() as conn:
        stored = await repo.list_dataset_uploads(conn, client_id=client_id, period_id=period_id)
        cases = await repo.list_cases(conn, client_id=client_id, period_id=period_id)
        outcomes = await repo.list_rule_outcomes(conn, client_id=client_id, period_id=period_id)

    by_kind = {e["kind"]: e for e in stored}

    def rows_for(kind: str) -> list[dict]:
        entry = by_kind.get(kind)
        if entry is None:
            return []
        dataset = {"headers": entry["headers"], "rows": entry["rows"]}
        # Same aggregation the graph ingestion applies, so the buckets count
        # invoices the way the rules compare them.
        return graphowl_client.net_credit_notes(
            graphowl_client.aggregate_invoice_lines(_normalize(dataset, entry["mapping"]))
        )

    findings = [
        {
            "invoice_no": c["invoice_no"],
            "reason_code": c["reason_code"],
            "supplier_gstin": c["supplier_gstin"],
        }
        for c in cases
    ]

    result = reconcile_buckets(rows_for("books"), rows_for("gstr2b"), findings)
    position = compute_itc_position(result)

    # One case id per invoice, so a row's drawer can ask for its explanation.
    # An invoice with two findings has two cases; the first is enough to
    # explain the invoice, and the row already lists every label.
    case_by_invoice: dict[str, str] = {}
    for case in cases:
        key = graphowl_client.normalize_invoice_no(case["invoice_no"])
        case_by_invoice.setdefault(key, str(case["id"]))
    for row in result.rows:
        row["case_id"] = case_by_invoice.get(
            graphowl_client.normalize_invoice_no(row.get("invoice_no"))
        )

    return {
        # Every figure on this screen, with how it was derived and what to do
        # about it — Plan 123, the "show your working" pass. Sent with the
        # data rather than hardcoded in the component, so a figure and its
        # stated derivation are one edit.
        "explain": explain.BUCKETS,
        # Every rule label carries its authored title, meaning and next action
        # — `gst:AmountMismatch` means nothing to a business reader. The
        # guidance lives in the pack, never here: a healthcare or banking pack
        # names entirely different findings.
        "guidance": _pack_guidance(),
        "explain_itc": explain.ITC_POSITION,
        # What each rule concluded, as **graph-owl reported it** — passed,
        # flagged, or not evaluated because a declared requirement had no
        # instances. Not inferred here from which files exist: that was a
        # second opinion competing with the engine's own execution record.
        #
        # `checks_disabled` is kept as the pre-reconciliation view: before a
        # run there are no outcomes, and a reviewer still needs to know which
        # checks the uploaded files can support.
        "rule_outcomes": rule_guidance.decorate(outcomes, _pack_guidance()),
        # Per-case: what is wrong with *this* invoice, computed from its own
        # figures. The pack's guidance says what the rule means; this says what
        # happened here. Computed rather than generated — a sentence stating an
        # amount must never be able to state a wrong one.
        "case_detail": [
            {
                **row,
                "narrative": case_narrative.narrate(row),
            }
            for row in rule_guidance.decorate(
                [dict(c) for c in cases], _pack_guidance()
            )
        ],
        "checks_disabled": checks_disabled(set(by_kind)) if not outcomes else {
            o["label"]: CHECK_REASONS.get(o["label"], "")
            for o in outcomes if o["status"] == "notEvaluated"
        },
        "total": result.total,
        "match_rate": result.match_rate,
        "counts": result.counts,
        "itc": {k: float(v) for k, v in position.items()},
        "have_books": "books" in by_kind,
        "have_portal": "gstr2b" in by_kind,
        "rows": [
            {
                "invoice_no": r["invoice_no"],
                "supplier_gstin": r["supplier_gstin"],
                "supplier_name": r["supplier_name"],
                "bucket": r["bucket"],
                "books_taxable": float(r["books_taxable"]),
                "portal_taxable": float(r["portal_taxable"]),
                "books_tax": float(r["books_tax"]),
                "portal_tax": float(r["portal_tax"]),
                "difference": float(r["books_taxable"] - r["portal_taxable"]),
                "labels": r["labels"],
                "blocked": r["blocked"],
                # What the row's drawer asks to have explained. `None` where
                # nothing was flagged — an invoice with no finding has no case,
                # and the drawer says so rather than offering an explanation of
                # nothing.
                "case_id": r.get("case_id"),
            }
            for r in result.rows
        ],
    }


@app.get("/api/clients/{client_id}/analytics")
async def analytics_route(client_id: str) -> dict:
    """Exposure per period, for the periods this client actually has.

    The Analytics screen drew a five-month Apr–Aug series and an insight
    reading "Match rate improved 6 points since April" for a client whose
    only reconciled period was March 2026. `has_trend` is the honest answer
    to "can this be plotted over time": one period is a number, not a trend,
    and the screen says so rather than inventing four more.
    """
    pool = _require_db_pool()
    async with pool.acquire() as conn:
        periods = await repo.list_periods(conn, client_id=client_id)
        rows = []
        for period in periods:
            pid = str(period["id"])
            cases = await repo.list_cases(conn, client_id=client_id, period_id=pid)
            rows.append(
                {
                    "period_id": pid,
                    "label": f"{period['month']} {period['year']}",
                    "year": period["year"],
                    "month": period["month"],
                    "status": period["status"],
                    "case_count": len(cases),
                    "exposure": period_exposure(cases),
                }
            )

    rows.sort(key=lambda r: (r["year"], _MONTH_ORDER.get(r["month"], 0)))
    return {"periods": rows, "has_trend": len(rows) > 1}


_MONTH_ORDER = {
    m: i
    for i, m in enumerate(
        ["January", "February", "March", "April", "May", "June",
         "July", "August", "September", "October", "November", "December"],
        start=1,
    )
}


@app.get("/api/clients/{client_id}/periods/{period_id}/dashboard")
async def dashboard_route(client_id: str, period_id: str) -> dict:
    """Plan 122b B2, scoped honestly: real totals computed directly from
    case_record/approval, not the mockup's full 6-panel layout. A case's
    exposure is books_amount minus portal_amount when both are known;
    "not yet in 2B at all" (portal_amount is null) counts the full books
    amount at risk, not zero — the same reasoning the mockup's own "only
    in books" bucket uses. Every total here is a direct aggregate of the
    same rows `needs_decision` lists, so the two can never silently
    disagree — the plan's own stated RED for this screen."""
    pool = _require_db_pool()
    async with pool.acquire() as conn:
        cases = await repo.list_cases(conn, client_id=client_id, period_id=period_id)
        approvals = await repo.list_approvals(conn, client_id=client_id, period_id=period_id, status="pending")
        datasets = await repo.list_dataset_uploads(conn, client_id=client_id, period_id=period_id)
        periods = await repo.list_periods(conn, client_id=client_id)

    period = next((p for p in periods if str(p["id"]) == period_id), None)

    scored = sorted(
        (
            {
                "invoice_no": c["invoice_no"],
                "reason_code": c["reason_code"],
                "supplier_name": c["supplier_name"],
                "exposure": _case_exposure(c),
                "status": c["status"],
            }
            for c in cases
        ),
        key=lambda row: row["exposure"],
        reverse=True,
    )

    # Book value of everything uploaded, and how much of it the reconciliation
    # left unflagged. Both are derived from the same files the reconciliation
    # read, so "reconciled" here means "in books and not carrying a case" —
    # stated that way in the response rather than implied by a label.
    books = next((d for d in datasets if d["kind"] == "books"), None)
    books_total = _dataset_total(books, "taxable") if books else None
    flagged_invoices = {c["invoice_no"] for c in cases}
    books_flagged = _dataset_total(books, "taxable", only=flagged_invoices) if books else None
    clean_total = (
        None if books_total is None or books_flagged is None else books_total - books_flagged
    )

    return {
        "period_label": None if period is None else f"{period['month']} {period['year']}",
        "case_count": len(cases),
        # Must equal the register's total to the rupee — same helper, so they
        # cannot drift apart.
        "total_exposure": period_exposure(cases),
        "needs_decision": scored,
        "pending_approvals": len(approvals),
        "supplier_count": len({c["supplier_gstin"] for c in cases if c["supplier_gstin"]}),
        "invoice_count": len(flagged_invoices),
        # None rather than 0 where no file has been uploaded: "₹0 of books"
        # and "no books uploaded" are different statements.
        "books_total": books_total,
        "clean_total": clean_total,
        "datasets": [
            {
                "kind": d["kind"],
                "name": d["name"],
                "total_rows": d["total_rows"],
                "confirmed": d["confirmed"],
            }
            for d in datasets
        ],
        "reconciled": len(cases) > 0 or bool(datasets),
    }


@app.post("/api/clients/{client_id}/periods/{period_id}/approvals", status_code=201)
async def create_approval_route(client_id: str, period_id: str, payload: dict) -> dict:
    pool = _require_db_pool()
    decision_type = payload.get("decision_type")
    if not decision_type:
        raise HTTPException(status_code=400, detail="decision_type is required")
    async with pool.acquire() as conn:
        approval_id = await repo.create_approval(
            conn, client_id=client_id, period_id=period_id, decision_type=decision_type,
            amount=payload.get("amount"), requested_by=None,
        )
    return {"id": approval_id, "decision_type": decision_type, "amount": payload.get("amount"), "status": "pending"}


@app.get("/api/clients/{client_id}/periods/{period_id}/approvals")
async def list_approvals_route(client_id: str, period_id: str, status: str | None = None) -> list[dict]:
    pool = _require_db_pool()
    async with pool.acquire() as conn:
        approvals = await repo.list_approvals(conn, client_id=client_id, period_id=period_id, status=status)
    return [
        {"id": str(a["id"]), "decision_type": a["decision_type"], "amount": a["amount"], "status": a["status"]}
        for a in approvals
    ]


@app.post("/api/clients/{client_id}/periods/{period_id}/approvals/{approval_id}/decide")
async def decide_approval_route(client_id: str, period_id: str, approval_id: str, payload: dict) -> dict:
    pool = _require_db_pool()
    status = payload.get("status")
    if status not in ("approved", "rejected"):
        raise HTTPException(status_code=400, detail='status must be "approved" or "rejected"')
    async with pool.acquire() as conn:
        decided = await repo.decide_approval(conn, client_id=client_id, approval_id=approval_id, status=status)
    if decided is None:
        raise HTTPException(status_code=404, detail="approval not found for this client")
    return {"id": str(decided["id"]), "status": decided["status"]}


def _start_ai_job(total: int, runner) -> str:
    """Register a background AI job and return its id. runner(job) does the work."""
    job_id = uuid.uuid4().hex
    AI_JOBS[job_id] = {"status": "running", "done": 0, "total": total, "result": None, "error": None}
    threading.Thread(target=_safe_run_ai_job, args=(job_id, runner), daemon=True).start()
    return job_id


def _safe_run_ai_job(job_id: str, runner) -> None:
    try:
        result = runner(AI_JOBS[job_id])
        AI_JOBS[job_id].update({"status": "done", "result": result})
    except Exception as exc:  # noqa: BLE001
        AI_JOBS[job_id].update({"status": "error", "error": str(exc)})


@app.get("/api/ai/jobs/{job_id}")
def ai_job_status(job_id: str) -> dict:
    job = AI_JOBS.get(job_id)
    if not job:
        return {"ok": False, "error": "Unknown job"}
    return {"ok": True, **job}


@app.post("/api/reset")
def reset() -> dict:
    _reset()
    return {"ok": True}


@app.post("/api/sample")
def load_sample() -> dict:
    _reset()
    SESSION["datasets"]["books"] = _build_dataset(sample_data.books_rows(), "Your Books", "books")
    SESSION["datasets"]["gstr2b"] = _build_dataset(sample_data.gstr2b_rows(), "Government Data", "gstr2b")
    SESSION["mapping"] = {
        "books": _auto_map(SESSION["datasets"]["books"]["headers"]),
        "gstr2b": _auto_map(SESSION["datasets"]["gstr2b"]["headers"]),
    }
    _ai_map_in_background()
    return overview()


def _ai_map_in_background() -> None:
    """Ask Ollama to refine column mapping without blocking the response."""

    def _refine(kind: str, headers: list[str], current: dict[str, int | None]) -> None:
        try:
            refined = ai.map_columns(headers, FIELD_LABELS)
            if refined:
                merged = dict(current)
                merged.update(refined)
                SESSION["mapping"][kind] = merged
        except Exception:  # noqa: BLE001
            pass

    for kind, dataset in SESSION.get("datasets", {}).items():
        headers = dataset["headers"]
        current = dict(SESSION.get("mapping", {}).get(kind, {}))
        if not headers:
            continue
        threading.Thread(target=_refine, args=(kind, headers, current), daemon=True).start()


@app.post("/api/upload")
async def upload(files: list[UploadFile] = File(...)) -> dict:
    _reset()
    kind_order = []
    for file in files:
        raw = await file.read()
        try:
            payload = _parse_upload(raw, file.filename or "upload.csv")
        except Exception as exc:  # noqa: BLE001
            return {"ok": False, "error": f"Could not parse {file.filename}: {exc}"}
        lower = file.filename.lower() if file.filename else ""
        # 2A is checked **before** GSTR-1 and before 2B, because the three
        # filename patterns overlap ("gstr-2a" contains neither "gstr1" nor
        # "2b", but a file named "gstr2a_and_2b_export" contains both). 2A is
        # the more specific claim, so it wins — Plan 123 Slice C.
        # 3B first: it is the only summary return, and "gstr3b" contains no
        # substring the others match on.
        if "3b" in lower or "gstr-3b" in lower:
            kind, name = "gstr3b", "GSTR-3B (summary return)"
        elif "2a" in lower or "gstr-2a" in lower:
            kind, name = "gstr2a", "GSTR-2A (portal, dynamic)"
        elif "gstr1" in lower or "gstr-1" in lower or "iff" in lower:
            kind, name = "gstr1", "GSTR-1 / IFF (supplier declared)"
        elif "2b" in lower or "gstr-2b" in lower or "portal" in lower or "gov" in lower:
            kind, name = "gstr2b", "Government Data"
        else:
            kind, name = "books", "Your Books"
        SESSION["datasets"][kind] = _build_dataset(payload, name, kind)
        kind_order.append(kind)
    if not SESSION["datasets"]:
        return {"ok": False, "error": "No valid files uploaded."}
    SESSION["graphowl_ingest_threads"] = []
    for kind in ("books", "gstr2b", "gstr1", "gstr2a", "gstr3b"):
        if kind in SESSION["datasets"]:
            SESSION["mapping"][kind] = _auto_map(SESSION["datasets"][kind]["headers"])
            thread = _ingest_to_graphowl(kind, SESSION["datasets"][kind], SESSION["mapping"][kind])
            SESSION["graphowl_ingest_threads"].append(thread)
    return overview()


@app.get("/api/graphowl/status")
def graphowl_status() -> dict:
    """What Slice 1's background ingestion did for the current upload —
    not part of `overview()`, so existing consumers of that response are
    unaffected by this integration existing at all.

    Also reports the installed pack and whether graph-owl is reachable at
    all. The console's header displayed "GST PACK 1.4.2" as a literal, so it
    claimed a pack version regardless of what was installed — and claimed one
    even when graph-owl was unreachable, which is precisely when a user needs
    to know it is not.
    """
    pack: dict | None = None
    reachable = False
    try:
        installed = graphowl_client.list_packs(GRAPH_OWL_SERVER, GRAPH_OWL_TOKEN)
        reachable = True
        gst = next((p for p in installed if p.get("packId") == graphowl_client.PACK_ID), None)
        if gst is not None:
            pack = {"id": gst.get("packId"), "version": gst.get("version"), "terms": gst.get("termCount")}
    except graphowl_client.IngestError:
        reachable = False

    return {
        "ok": True,
        "server": GRAPH_OWL_SERVER,
        "reachable": reachable,
        "pack": pack,
        "datasets": SESSION.get("graphowl", {}),
    }


@app.post("/api/mapping")
def save_mapping(payload: dict) -> dict:
    SESSION["mapping"] = payload.get("mapping", {})
    SESSION["tolerance"] = float(payload.get("tolerance", 1.0))
    if payload.get("period"):
        SESSION["period"] = payload["period"]
    return {"ok": True}


def _select_results(
    books: list[dict],
    portal: list[dict],
    gstr1: list[dict],
    graphowl_reconcile: dict,
    tolerance: float,
) -> list[dict]:
    """The one decision point between the two reconciliation sources —
    plans/119-architecture-audit.md §9, the cutover. Native findings are
    primary now that parity is demonstrated (scripts/verify-reconcile-
    parity.py); `reconciliation.py`'s own tolerance/matching math is kept
    only as the same best-effort fallback every other graph-owl
    integration point in this file already has — an unreachable or
    not-yet-installed native engine must not break the app, matching
    `_install_graphowl_pack`'s and `_ingest_to_graphowl`'s own reasoning."""
    if graphowl_reconcile.get("error"):
        return rc.reconcile(books, portal, tolerance=tolerance)
    return native_findings.reconcile(books, portal, gstr1, graphowl_reconcile.get("findings", []))


@app.post("/api/reconcile")
def run_reconcile() -> dict:
    # The native engine can only find what has actually landed — join
    # every ingest thread /api/upload started before asking it to run.
    # Ingestion is small CSV rows over localhost, so this is a bounded,
    # short wait in the normal case; the timeout keeps a genuinely stuck
    # ingest from hanging this endpoint forever.
    for thread in SESSION.get("graphowl_ingest_threads", []):
        thread.join(timeout=15)

    books = _normalize(SESSION["datasets"]["books"], SESSION["mapping"].get("books", {}))
    portal = _normalize(SESSION["datasets"]["gstr2b"], SESSION["mapping"].get("gstr2b", {}))
    gstr1 = (
        _normalize(SESSION["datasets"]["gstr1"], SESSION["mapping"].get("gstr1", {}))
        if "gstr1" in SESSION["datasets"]
        else []
    )
    SESSION["normalized"] = {"books": books, "gstr2b": portal, "gstr1": gstr1}

    _run_graphowl_reconcile()
    SESSION["results"] = _select_results(
        books, portal, gstr1, SESSION["graphowl_reconcile"], SESSION["tolerance"]
    )
    return overview()


def _run_graphowl_reconcile() -> None:
    """Runs graph-owl's native rule engine and records what it found in
    SESSION["graphowl_reconcile"] — read by `_select_results` above (the
    primary path, since the 16 August 2026 cutover) and by
    `/api/graphowl/reconcile` (a diagnostic view of the same data).

    Synchronous, not backgrounded — `/api/reconcile`'s response now
    depends on this having finished. Still best-effort: graph-owl may not
    be running at all, and that must degrade `_select_results` to the
    Python fallback rather than fail the endpoint."""
    try:
        # Scoped to what this session actually uploaded, plus the pack's own
        # vocabulary and law graphs — the same discipline `reconcile_route`
        # applies via `reconcile_scope`. Unscoped, the rules read the whole
        # store, and another client's data could satisfy or trigger a rule
        # about this session's files.
        scope = [f"reco-{kind}" for kind in SESSION.get("datasets", {})] + list(PACK_GRAPHS)
        result = run_findings(
            graphowl_client.PACK_ID, GRAPH_OWL_SERVER, GRAPH_OWL_TOKEN, graphs=scope
        )
        findings = graphowl_client.list_findings(GRAPH_OWL_SERVER, GRAPH_OWL_TOKEN)
        SESSION["graphowl_reconcile"] = {
            "evaluated": result.evaluated,
            "found": result.found,
            "opened": result.opened,
            "alreadyOpen": result.already_open,
            "findings": findings,
        }
    except (LoadError, graphowl_client.IngestError) as exc:
        SESSION["graphowl_reconcile"] = {"error": str(exc)}


@app.get("/api/graphowl/reconcile")
def graphowl_reconcile_status() -> dict:
    """What the native graph-owl reconcile engine found, run alongside
    reconciliation.py per Slice 2 — a parallel view for comparison, not
    yet the source of truth `overview()` reads from."""
    return {"ok": True, "reconcile": SESSION.get("graphowl_reconcile")}


@app.get("/api/overview")
def overview() -> dict:
    datasets = {}
    for kind, dataset in SESSION.get("datasets", {}).items():
        datasets[kind] = {
            "id": dataset["id"],
            "name": dataset["name"],
            "kind": dataset["kind"],
            "headers": dataset["headers"],
            "preview": dataset["rows"][:5],
            "total_rows": dataset["total_rows"],
            "mapping": SESSION.get("mapping", {}).get(kind, {}),
        }
    results = SESSION.get("results")
    stats = rc.match_stats(results) if results else None
    classifications = rc.classify_mismatches(results) if results else []
    return {
        "ok": True,
        "period": SESSION.get("period", {"month": "March", "year": 2026}),
        "tolerance": SESSION.get("tolerance", 1.0),
        "datasets": datasets,
        "stats": stats,
        "classifications": classifications,
        "supplier_health": rc.supplier_health(results) if results else [],
        "ims_actions": rc.ims_actions(results) if results else [],
        "results": results,
        # The single source of truth for graph-owl's own base URL — already
        # configured for the backend's own use (graphowl_client.py). Since
        # graph-owl-server embeds and serves its own console, this is
        # always also the console's base URL, not a coincidence of the
        # local dev setup. Lets the frontend build an "Open in GraphOWL"
        # link without a second, possibly-drifting env var of its own.
        "graphowl_url": GRAPH_OWL_SERVER,
    }


@app.get("/api/export/csv")
def export_csv() -> Response:
    results = SESSION.get("results") or []
    return Response(
        exporters.export_working_paper_csv(results),
        media_type="text/csv",
        headers={"Content-Disposition": 'attachment; filename="working-paper.csv"'},
    )


@app.get("/api/export/working-paper.xlsx")
def export_working_paper() -> Response:
    results = SESSION.get("results") or []
    return Response(
        exporters.export_working_paper_xlsx(results),
        media_type="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        headers={"Content-Disposition": 'attachment; filename="working-paper.xlsx"'},
    )


@app.get("/api/export/itc-register.xlsx")
def export_itc_register() -> Response:
    results = SESSION.get("results") or []
    return Response(
        exporters.export_itc_register(results),
        media_type="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        headers={"Content-Disposition": 'attachment; filename="itc-register.xlsx"'},
    )


@app.get("/api/export/report.md")
def export_report_md() -> Response:
    """Download the last generated client report as a Markdown file."""
    report = SESSION.get("last_report", "")
    if not report:
        report = "# No report generated yet\n\nGenerate a client report first using the Act page."
    return Response(
        report,
        media_type="text/markdown",
        headers={"Content-Disposition": 'attachment; filename="reconow-report.md"'},
    )


@app.post("/api/act/follow-ups")
def generate_follow_ups() -> dict:
    results = SESSION.get("results") or []
    period = SESSION.get("period", {"month": "March", "year": 2026})
    target = [row for row in results if row["status"] == rc.STATUS_ONLY_BOOKS]

    def _build(job: dict) -> list[dict]:
        messages = []
        for i, row in enumerate(target):
            book = row["book"]
            drafted = ai.draft_follow_up(
                supplier=book["supplier"],
                gstin=book["gstin"],
                invoice_no=book["invoice_no"],
                itc=row["itc"],
                period=period,
            )
            # **Plan 123 Slice F — the control the prompt only asks for.**
            # `draft_follow_up`'s system prompt says "no invented figures";
            # that is a request. This checks, against the figures the prompt
            # was actually given, and drops a draft that states any other —
            # this document goes to a third party, and a confident wrong
            # amount in it is worse than no draft at all. The deterministic
            # fallback below is what the reader gets instead.
            if drafted:
                checked = grounding.ground_draft(
                    draft=drafted,
                    supplied={
                        "itc": row["itc"],
                        "invoice_no": book["invoice_no"],
                        "gstin": book["gstin"],
                        "year": period.get("year"),
                    },
                    log=AGENT_REFUSALS,
                )
                if not checked["grounded"]:
                    drafted = None
            messages.append(
                {
                    "supplier": book["supplier"],
                    "gstin": book["gstin"],
                    "invoice_no": book["invoice_no"],
                    "itc": row["itc"],
                    "message": drafted
                    or (
                        f"Subject: GSTR-1 filing for invoice {book['invoice_no']} — please file\n\n"
                        f"Dear {book['supplier']},\n\n"
                        f"We note that invoice {book['invoice_no']} (ITC {rc.display_tax(row['itc'])}) "
                        f"does not appear in our GSTR-2B for the current period.\n\n"
                        f"As per Section 16(2)(aa) of the CGST Act, ITC is available only when the invoice "
                        f"is reported in your GSTR-1 and appears in our GSTR-2B. Kindly file your GSTR-1 at "
                        f"the earliest so we can claim the credit.\n\n"
                        f"Thanks,\nAccounts Team"
                    ),
                }
            )
            job["done"] = i + 1
        return messages

    job_id = _start_ai_job(max(1, len(target)), _build)
    return {"ok": True, "job_id": job_id, "total": len(target), "messages": []}


@app.post("/api/act/report")
def generate_report() -> dict:
    stats = rc.match_stats(SESSION.get("results") or [])
    classifications = rc.classify_mismatches(SESSION.get("results") or [])
    period = SESSION.get("period", {"month": "March", "year": 2026})
    ims = rc.ims_actions(SESSION.get("results") or [])

    def _build(job: dict) -> str:
        report = ai.generate_client_report(period, stats, classifications, ims)
        if not report:
            report = _template_report(stats, classifications, period)
        SESSION["last_report"] = report
        return report

    job_id = _start_ai_job(1, _build)
    return {"ok": True, "job_id": job_id}


@app.get("/api/act/summary")
def ai_act_summary() -> dict:
    results = SESSION.get("results") or []
    if not results:
        return {"ok": True, "summary": "Load and reconcile a data set first to see the AI summary."}
    stats = rc.match_stats(results)
    classifications = rc.classify_mismatches(results)

    def _build(job: dict) -> str:
        summary = ai.ai_summary(stats, classifications)
        if not summary:
            summary = _fallback_summary(stats, classifications)
        return summary

    job_id = _start_ai_job(1, _build)
    return {"ok": True, "job_id": job_id}


def _fallback_summary(stats: dict, classifications: list[dict]) -> str:
    """The deterministic summary when no model is available.

    `gross_itc` is matched + review + portal-only tax — never the net, and
    never the GSTR-3B Table 4 figure, both of which this used to call it."""
    return (
        f"{stats['match_rate']}% of {stats['total']} invoices matched, confirming ITC of "
        f"{rc.display_tax(stats['confirmed_itc'])}. "
        f"{stats['only_books']} supplier non-filing item(s) and {stats['review']} amount "
        f"discrepancy item(s) put {rc.display_tax(stats['at_risk_itc'])} of ITC at risk. "
        f"Gross ITC across matched, review and portal-only invoices is "
        f"{rc.display_tax(stats['gross_itc'])}."
    )


def _template_report(stats: dict, classifications: list[dict], period: dict) -> str:
    report = (
        f"# GSTR-2B Reconciliation Report — {period['month']} {period['year']}\n\n"
        f"## Executive Summary\n\n"
        f"This report summarises the reconciliation of the purchase register against GSTR-2B for "
        f"{period['month']} {period['year']}.\n\n"
        f"- **Total invoices:** {stats['total']}\n"
        f"- **Matched:** {stats['matched']} ({stats['match_rate']}%) — ITC confirmed at "
        f"{rc.display_tax(stats['confirmed_itc'])}\n"
        f"- **Amount discrepancies:** {stats['review']}\n"
        f"- **In books, not in GSTR-2B:** {stats['only_books']} (supplier non-filing risk)\n"
        f"- **In GSTR-2B, not in books:** {stats['only_gstr2b']}\n\n"
        f"## Risk Assessment\n\n"
        f"ITC at risk is {rc.display_tax(stats['at_risk_itc'])}. Items requiring attention:\n\n"
    )
    for item in classifications:
        if item["count"]:
            report += f"- **{item['title']}** ({item['count']}): {rc.display_tax(item['itc'])} — {item['action']} ({item['reference']})\n"
    report += (
        f"\n## Recommended Actions\n\n"
        f"1. Follow up with suppliers whose invoices are missing from GSTR-2B so they file GSTR-1.\n"
        f"2. Verify amount discrepancies with suppliers before filing GSTR-3B.\n"
        f"3. Claim the confirmed ITC of {rc.display_tax(stats['confirmed_itc'])} in GSTR-3B Table 4; "
        f"the {rc.display_tax(stats['at_risk_itc'])} at risk follows only once the items above are resolved.\n"
    )
    return report


@app.get("/api/clients/{client_id}/periods/{period_id}/suppliers")
async def list_suppliers(client_id: str, period_id: str):
    # Grouped in Python over `period_exposure` rather than in SQL. The SQL
    # here read `COALESCE(portal_amount, books_amount)`, so a case with no
    # portal side became ABS(x - x) = 0 — and a supplier who filed nothing at
    # all was reported as the one costing nothing, which is exactly backwards.
    # Sharing the helper is what stops two screens defining "exposure"
    # differently; see `period_exposure`.
    pool = _require_db_pool()
    async with pool.acquire() as conn:
        cases = await repo.list_cases(conn, client_id=client_id, period_id=period_id)

    groups: dict[tuple[str, str | None], list[dict]] = {}
    for case in cases:
        if case["supplier_gstin"] is None:
            continue
        groups.setdefault((case["supplier_gstin"], case["supplier_name"]), []).append(case)

    rows = sorted(
        (
            {
                "supplier_gstin": gstin,
                "supplier_name": name,
                "case_count": len(members),
                "total_exposure": period_exposure(members),
                "pending_count": sum(1 for m in members if m["status"] == "pending"),
            }
            for (gstin, name), members in groups.items()
        ),
        key=lambda r: r["total_exposure"],
        reverse=True,
    )
    return [
        {
            "gstin": r["supplier_gstin"],
            "name": r["supplier_name"],
            "case_count": r["case_count"],
            "total_exposure": float(r["total_exposure"]),
            "pending_count": r["pending_count"],
        }
        for r in rows
    ]


#: The pack's per-finding guidance, fetched once per process.
#:
#: Cached because every screen wants it and it changes only when a pack is
#: reinstalled — a fetch per request would put a graph-owl round trip in front
#: of every render, for data that is effectively static.
_GUIDANCE_CACHE: dict[str, dict] = {}


def _pack_guidance(pack: str = "gst") -> dict:
    if pack not in _GUIDANCE_CACHE:
        _GUIDANCE_CACHE[pack] = graphowl_client.console_guidance(GRAPH_OWL_SERVER, pack)
    return _GUIDANCE_CACHE[pack]


@app.get("/api/clients/{client_id}/periods/{period_id}/itc")
async def itc_position(client_id: str, period_id: str):
    """Where the period's input tax credit stands — **the same five classes the
    reconcile screen reports**, from the same computation.

    **This route used to disagree with that screen, and the disagreement was
    the product lying.** It summed `case_record`: only the *flagged* invoices,
    double-counting any invoice carrying two findings, excluding every clean
    one — then labelled the result `books_amount`, `portal_amount` and
    `exposure` as though they were period totals. On real data it reported an
    "exposure" of ₹14,750 that could not be derived from anything else on any
    screen.

    Those keys are **removed rather than silently redefined**: a caller still
    reading `exposure` should break loudly rather than quietly get a different
    number.

    Every figure carries its own derivation, what it means in a business
    reader's terms, and what to do about it — because "blocked" and "pending"
    are the same size of number and opposite situations, and a figure without
    its remedy is half an answer.
    """
    recon = await reconciliation_route(client_id, period_id)
    position = recon["itc"]

    return {
        "position": position,
        "counts": recon["counts"],
        "explain": explain.ITC_POSITION,
        # Two correct numbers measuring different populations look like a bug
        # unless each says what it counted. This is the most confusing pair of
        # screens in the product.
        "compare_note": explain.ITC_VS_WORKING_PAPER,
    }


@app.get("/api/clients/{client_id}/periods/{period_id}/atrisk")
async def at_risk(client_id: str, period_id: str):
    pool = _require_db_pool()
    async with pool.acquire() as conn:
        rows = await conn.fetch(
            """
            SELECT 
                supplier_gstin,
                supplier_name,
                COALESCE(SUM(
                    ABS(COALESCE(books_amount, 0) - COALESCE(portal_amount, COALESCE(books_amount, 0)))
                ), 0) as at_risk_amount,
                COUNT(*) as case_count
            FROM case_record
            WHERE client_id = $1 AND period_id = $2
              AND supplier_gstin IS NOT NULL
            GROUP BY supplier_gstin, supplier_name
            ORDER BY at_risk_amount DESC
            """,
            client_id,
            period_id,
        )
    return [
        {
            "gstin": r["supplier_gstin"],
            "name": r["supplier_name"],
            "at_risk_amount": float(r["at_risk_amount"]),
            "case_count": r["case_count"],
        }
        for r in rows
    ]


@app.get("/api/clients/{client_id}/periods/{period_id}/followups")
async def list_followups(client_id: str, period_id: str):
    pool = _require_db_pool()
    async with pool.acquire() as conn:
        rows = await conn.fetch(
            """
            SELECT 
                id,
                invoice_no,
                supplier_name,
                reason_code,
                ABS(COALESCE(books_amount, 0) - COALESCE(portal_amount, COALESCE(books_amount, 0))) as exposure,
                status,
                subject,
                summary
            FROM case_record
            WHERE client_id = $1 AND period_id = $2
              AND status IN ('pending', 'open')
            ORDER BY exposure DESC
            """,
            client_id,
            period_id,
        )
    return [
        {
            "case_id": str(r["id"]),
            "invoice_no": r["invoice_no"],
            "supplier_name": r["supplier_name"],
            "reason_code": r["reason_code"],
            "exposure": float(r["exposure"]),
            "status": r["status"],
            "subject": r["subject"],
            "summary": r["summary"],
        }
        for r in rows
    ]


@app.get("/api/clients/{client_id}/periods/{period_id}/authority")
async def authority(client_id: str, period_id: str):
    """Cases grouped by the provision that governs them — `gst:Rule36-4`,
    `gst:Section16-2-aa` — as graph-owl's own rules report it."""
    pool = _require_db_pool()
    async with pool.acquire() as conn:
        cases = await repo.list_cases(conn, client_id=client_id, period_id=period_id)
    return [
        {"authority": g["key"], "case_count": g["case_count"], "exposure": g["exposure"]}
        for g in group_by_exposure(cases, "governed_by")
    ]


@app.get("/api/clients/{client_id}/periods/{period_id}/obligations")
async def obligations(client_id: str, period_id: str):
    pool = _require_db_pool()
    async with pool.acquire() as conn:
        cases = await repo.list_cases(conn, client_id=client_id, period_id=period_id)
    return [
        {"obligation": g["key"], "case_count": g["case_count"], "exposure": g["exposure"]}
        for g in group_by_exposure(cases, "reason_code", absent="Unspecified")
    ]


@app.get("/api/clients/{client_id}/periods/{period_id}/risk")
async def supplier_risk(client_id: str, period_id: str):
    pool = _require_db_pool()
    async with pool.acquire() as conn:
        rows = await conn.fetch(
            """
            SELECT 
                supplier_gstin,
                supplier_name,
                COUNT(*) as case_count,
                COALESCE(SUM(
                    ABS(COALESCE(books_amount, 0) - COALESCE(portal_amount, COALESCE(books_amount, 0)))
                ), 0) as total_exposure,
                COALESCE(MAX(
                    ABS(COALESCE(books_amount, 0) - COALESCE(portal_amount, COALESCE(books_amount, 0)))
                ), 0) as max_exposure,
                COUNT(CASE WHEN status = 'pending' THEN 1 END) as pending_count
            FROM case_record
            WHERE client_id = $1 AND period_id = $2
              AND supplier_gstin IS NOT NULL
            GROUP BY supplier_gstin, supplier_name
            ORDER BY total_exposure DESC
            """,
            client_id,
            period_id,
        )
    return [
        {
            "gstin": r["supplier_gstin"],
            "name": r["supplier_name"],
            "case_count": r["case_count"],
            "total_exposure": float(r["total_exposure"]),
            "max_exposure": float(r["max_exposure"]),
            "pending_count": r["pending_count"],
        }
        for r in rows
    ]


@app.get("/api/reset/status")
async def reset_status():
    pool = _require_db_pool()
    async with pool.acquire() as conn:
        clients = await conn.fetchval("SELECT COUNT(*) FROM client")
        periods = await conn.fetchval("SELECT COUNT(*) FROM period")
        cases = await conn.fetchval("SELECT COUNT(*) FROM case_record")
        approvals = await conn.fetchval("SELECT COUNT(*) FROM approval")
        users = await conn.fetchval("SELECT COUNT(*) FROM app_user")
    return {
        "clients": clients,
        "periods": periods,
        "cases": cases,
        "approvals": approvals,
        "users": users,
    }


@app.get("/api/clients/{client_id}/periods/{period_id}/deliverables")
async def list_deliverables(client_id: str, period_id: str):
    pool = _require_db_pool()
    async with pool.acquire() as conn:
        rows = await conn.fetch(
            """
            SELECT id, kind, status, generated_at
            FROM deliverable
            WHERE client_id = $1 AND period_id = $2
            ORDER BY generated_at DESC
            """,
            client_id,
            period_id,
        )
    return [
        {
            "id": str(r["id"]),
            "kind": r["kind"],
            "status": r["status"],
            "generated_at": r["generated_at"].isoformat(),
        }
        for r in rows
    ]


@app.get("/api/clients/{client_id}/periods/{period_id}/mappings")
async def list_mappings(client_id: str, period_id: str):
    pool = _require_db_pool()
    async with pool.acquire() as conn:
        rows = await conn.fetch(
            """
            SELECT id, dataset_kind, mapping, tolerance, updated_at
            FROM mapping_template
            WHERE client_id = $1
            ORDER BY updated_at DESC
            """,
            client_id,
        )
    return [
        {
            "id": str(r["id"]),
            "dataset_kind": r["dataset_kind"],
            "mapping": r["mapping"],
            "tolerance": r["tolerance"],
            "updated_at": r["updated_at"].isoformat(),
        }
        for r in rows
    ]


@app.get("/api/rules")
async def list_rules():
    pool = _require_db_pool()
    async with pool.acquire() as conn:
        rows = await conn.fetch(
            """
            SELECT DISTINCT reason_code, COUNT(*) as count
            FROM case_record
            WHERE reason_code IS NOT NULL
            GROUP BY reason_code
            ORDER BY count DESC
            """
        )
    rules = [
        {
            "id": f"rule-{i+1}",
            "code": r["reason_code"],
            "name": r["reason_code"].replace("_", " ").title(),
            "severity": "high" if i < 3 else "medium",
            "enabled": True,
            "case_count": r["count"],
        }
        for i, r in enumerate(rows)
    ]
    if not rules:
        rules = [
            {"id": "rule-1", "code": "TAX_HEAD_MISMATCH", "name": "Tax Head Mismatch", "severity": "high", "enabled": True, "case_count": 0},
            {"id": "rule-2", "code": "SUPPLIER_NOT_FILED", "name": "Supplier Not Filed", "severity": "high", "enabled": True, "case_count": 0},
            {"id": "rule-3", "code": "DUPLICATE_INVOICE", "name": "Duplicate Invoice", "severity": "medium", "enabled": True, "case_count": 0},
            {"id": "rule-4", "code": "MISSING_IN_PORTAL", "name": "Missing in Portal", "severity": "medium", "enabled": True, "case_count": 0},
            {"id": "rule-5", "code": "AMOUNT_MISMATCH", "name": "Amount Mismatch", "severity": "low", "enabled": True, "case_count": 0},
        ]
    return rules


@app.get("/api/users")
async def list_users():
    pool = _require_db_pool()
    async with pool.acquire() as conn:
        rows = await conn.fetch(
            """
            SELECT u.id, u.name, u.email, u.role,
                   COUNT(cr.id) as assigned_cases
            FROM app_user u
            LEFT JOIN case_record cr ON cr.assigned_to = u.id
            GROUP BY u.id, u.name, u.email, u.role
            ORDER BY u.name
            """
        )
    return [
        {
            "id": str(r["id"]),
            "name": r["name"],
            "email": r["email"],
            "role": r["role"],
            "assigned_cases": r["assigned_cases"],
        }
        for r in rows
    ]


@app.get("/api/clients/{client_id}/periods/{period_id}/imports")
async def list_imports(client_id: str, period_id: str):
    pool = _require_db_pool()
    async with pool.acquire() as conn:
        rows = await conn.fetch(
            """
            SELECT id, dataset_kind, mapping, tolerance, updated_at
            FROM mapping_template
            WHERE client_id = $1
            ORDER BY updated_at DESC
            """,
            client_id,
        )
    return [
        {
            "id": str(r["id"]),
            "kind": r["dataset_kind"],
            "columns_mapped": sum(1 for v in json.loads(r["mapping"]).values() if v is not None),
            "tolerance": r["tolerance"],
            "imported_at": r["updated_at"].isoformat(),
        }
        for r in rows
    ]


@app.get("/api/clients/{client_id}/periods/{period_id}/crossperiod")
async def cross_period(client_id: str, period_id: str):
    pool = _require_db_pool()
    async with pool.acquire() as conn:
        periods = await conn.fetch(
            "SELECT id, month, year FROM period WHERE client_id = $1 ORDER BY year, month",
            client_id,
        )
        if len(periods) < 2:
            return []
        results = []
        for p in periods:
            rows = await conn.fetch(
                """
                SELECT supplier_gstin, supplier_name,
                       COUNT(*) as case_count,
                       COALESCE(SUM(
                           ABS(COALESCE(books_amount, 0) - COALESCE(portal_amount, COALESCE(books_amount, 0)))
                       ), 0) as exposure
                FROM case_record
                WHERE client_id = $1 AND period_id = $2
                  AND supplier_gstin IS NOT NULL
                GROUP BY supplier_gstin, supplier_name
                """,
                client_id,
                str(p["id"]),
            )
            for r in rows:
                results.append({
                    "period": f"{p['month']} {p['year']}",
                    "period_id": str(p["id"]),
                    "gstin": r["supplier_gstin"],
                    "name": r["supplier_name"],
                    "case_count": r["case_count"],
                    "exposure": float(r["exposure"]),
                })
        return results


@app.get("/api/clients/{client_id}/periods/{period_id}/eligibility")
async def eligibility(client_id: str, period_id: str):
    pool = _require_db_pool()
    async with pool.acquire() as conn:
        rows = await conn.fetch(
            """
            SELECT 
                supplier_gstin,
                supplier_name,
                invoice_no,
                books_amount,
                portal_amount,
                CASE 
                    WHEN portal_amount IS NULL THEN 'missing_portal'
                    WHEN books_amount IS NULL THEN 'missing_books'
                    WHEN ABS(COALESCE(books_amount, 0) - COALESCE(portal_amount, 0)) > 1 THEN 'amount_mismatch'
                    ELSE 'eligible'
                END as eligibility,
                COALESCE(books_amount, 0) as books,
                COALESCE(portal_amount, 0) as portal
            FROM case_record
            WHERE client_id = $1 AND period_id = $2
            ORDER BY 
                CASE 
                    WHEN portal_amount IS NULL THEN 0
                    WHEN books_amount IS NULL THEN 1
                    WHEN ABS(COALESCE(books_amount, 0) - COALESCE(portal_amount, 0)) > 1 THEN 2
                    ELSE 3
                END,
                supplier_gstin
            """,
            client_id,
            period_id,
        )
    return [
        {
            "gstin": r["supplier_gstin"],
            "name": r["supplier_name"],
            "invoice_no": r["invoice_no"],
            "books_amount": float(r["books"]),
            "portal_amount": float(r["portal"]),
            "eligibility": r["eligibility"],
        }
        for r in rows
    ]
