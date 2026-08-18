"""Matcha backend — FastAPI server for GST/indirect-tax reconciliation."""

from __future__ import annotations

import io
import json
import math
import os
import threading
import hashlib
import uuid
from datetime import date
from pathlib import Path

import pandas as pd
from fastapi import FastAPI, File, HTTPException, UploadFile
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import Response
from graph_owl_packs.loader import LoadError, load_pack
from graph_owl_packs.reconcile import run_findings

from . import ai, db, exporters, graphowl_client, native_findings, reconciliation as rc, repo, sample_data
# Aliased: main.py already defines an `itc_position` *route handler*, which
# would shadow this import.
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
}

REQUIRED_FIELDS = {"invoice_no", "taxable"}

# keyword -> field, ordered so specific terms win
_FIELD_KEYWORDS = [
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
    if pool is None:
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

    return {"kind": kind, "confirmed": True}


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


def _ingest_scoped_to_graphowl(client_id: str, period_id: str, kind: str, dataset: dict, mapping: dict) -> dict:
    """`_ingest_to_graphowl`'s reasoning (stable source, deleted before
    every re-import) still holds — the source name itself now carries
    client_id and period_id, not just kind. The pre-B0 app served exactly
    one session at a time, so `reco-{kind}` alone never collided; two
    clients uploading concurrently now genuinely can, and a shared source
    name would mean client B's upload deletes and replaces client A's own
    books. **Known limitation, not fixed here**: the native reconcile
    engine itself (`run_findings`, called by `_run_graphowl_reconcile` and
    the client+period `reconcile_route` below) still runs unscoped over the
    *whole* graph-owl store — this was true before B0 and stays true after
    it (`_install_graphowl_pack`'s own comment already documents it). This
    fix stops one client's ingest from silently overwriting another's;
    fully isolating the reconcile step itself needs a graph-owl-side
    scoping mechanism this Python backend cannot add on its own."""
    normalized = _normalize(dataset, mapping)
    turtle = graphowl_client.rows_to_turtle(normalized, kind)
    # Source name must be 1-64 chars of [a-zA-Z0-9_-].  Two UUIDs + prefix
    # blow the limit, so we take the first 12 hex chars of a SHA-256 of the
    # (client_id, period_id, kind) tuple — unique enough for a source graph
    # name and short enough to pass graph-owl's validation.
    short_id = hashlib.sha256(f"{client_id}:{period_id}:{kind}".encode()).hexdigest()[:12]
    source = f"reco-{short_id}-{kind}"
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

    ingested = {}
    for kind, entry in datasets.items():
        try:
            ingested[kind] = _ingest_scoped_to_graphowl(client_id, period_id, kind, entry["dataset"], entry["mapping"])
        except graphowl_client.IngestError as exc:
            ingested[kind] = {"error": str(exc)}

    try:
        result = run_findings(graphowl_client.PACK_ID, GRAPH_OWL_SERVER, GRAPH_OWL_TOKEN)
        findings = graphowl_client.list_findings(GRAPH_OWL_SERVER, GRAPH_OWL_TOKEN)
    except (LoadError, graphowl_client.IngestError) as exc:
        return {"ok": False, "error": str(exc), "ingested": ingested}

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

    return {
        "ok": True,
        "ingested": ingested,
        "evaluated": result.evaluated,
        "found": result.found,
        "cases_created": created,
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
    rows = sorted((_case_row(c) for c in cases), key=lambda r: r["exposure"], reverse=True)
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

    return {
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
        if "2a" in lower or "gstr-2a" in lower or "gstr1" in lower or "gstr-1" in lower:
            # GSTR-2A/GSTR-1 — ingests as gst:Gstr1Invoice
            # (graphowl_client.CLASS_BY_KIND's own comment: packs/gst
            # deliberately has no separate Gstr2aInvoice class, 2A is "a
            # revolving view over the same supplier-declared data").
            # Backend/graph-owl support only for now — no dedicated Map/
            # Reconcile UI slot yet (plans/119-architecture-audit.md §8).
            kind, name = "gstr1", "GSTR-2A / GSTR-1"
        elif "2b" in lower or "gstr-2b" in lower or "portal" in lower or "gov" in lower:
            kind, name = "gstr2b", "Government Data"
        else:
            kind, name = "books", "Your Books"
        SESSION["datasets"][kind] = _build_dataset(payload, name, kind)
        kind_order.append(kind)
    if not SESSION["datasets"]:
        return {"ok": False, "error": "No valid files uploaded."}
    SESSION["graphowl_ingest_threads"] = []
    for kind in ("books", "gstr2b", "gstr1"):
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
        result = run_findings(graphowl_client.PACK_ID, GRAPH_OWL_SERVER, GRAPH_OWL_TOKEN)
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
            summary = (
                f"{stats['match_rate']}% of {stats['total']} invoices matched, confirming ITC of "
                f"{rc.display_tax(stats['confirmed_itc'])}. "
                f"{stats['only_books']} supplier non-filing item(s) and {stats['review']} amount "
                f"discrepancy item(s) put {rc.display_tax(stats['at_risk_itc'])} of ITC at risk, "
                f"reducing net GSTR-3B Table 4 credit to {rc.display_tax(stats['gross_itc'])}."
            )
        return summary

    job_id = _start_ai_job(1, _build)
    return {"ok": True, "job_id": job_id}


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
        f"3. Claim eligible ITC in GSTR-3B Table 4 up to {rc.display_tax(stats['gross_itc'])}.\n"
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


@app.get("/api/clients/{client_id}/periods/{period_id}/itc")
async def itc_position(client_id: str, period_id: str):
    pool = _require_db_pool()
    async with pool.acquire() as conn:
        row = await conn.fetchrow(
            """
            SELECT 
                COALESCE(SUM(COALESCE(books_amount, 0)), 0) as total_books_amount,
                COALESCE(SUM(COALESCE(portal_amount, 0)), 0) as total_portal_amount,
                COALESCE(SUM(
                    ABS(COALESCE(books_amount, 0) - COALESCE(portal_amount, COALESCE(books_amount, 0)))
                ), 0) as total_exposure,
                COUNT(*) as total_cases,
                COUNT(CASE WHEN status = 'pending' THEN 1 END) as pending_cases
            FROM case_record
            WHERE client_id = $1 AND period_id = $2
            """,
            client_id,
            period_id,
        )
    return {
        "books_amount": float(row["total_books_amount"]),
        "portal_amount": float(row["total_portal_amount"]),
        "exposure": float(row["total_exposure"]),
        "case_count": row["total_cases"],
        "pending_count": row["pending_cases"],
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
