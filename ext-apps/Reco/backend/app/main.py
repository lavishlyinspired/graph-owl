"""Matcha backend — FastAPI server for GST/indirect-tax reconciliation."""

from __future__ import annotations

import io
import json
import os
import threading
import uuid
from datetime import date
from pathlib import Path

import pandas as pd
from fastapi import FastAPI, File, UploadFile
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import Response
from graph_owl_packs.loader import LoadError, load_pack
from graph_owl_packs.reconcile import run_findings

from . import ai, exporters, graphowl_client, reconciliation as rc, sample_data

app = FastAPI(title="RecoNow — Intelligence for Indirect Tax", version="1.0.0")

# graph-owl integration (plans/118-reco-now-integration.md, Slice 1).
# Best-effort throughout: graph-owl may not be running (e.g. a laptop
# with no Docker/Postgres up), and none of reco-now's existing behaviour
# may depend on that — this is additive durable storage alongside
# SESSION, not a replacement for it.
GRAPH_OWL_SERVER = os.environ.get("GRAPH_OWL_SERVER", "http://localhost:8080")
GRAPH_OWL_TOKEN = os.environ.get("GRAPH_OWL_TOKEN")
GRAPH_OWL_PACK_DIR = Path(__file__).resolve().parents[2] / "graphowl-pack"
# reco-now's pack extends this one (plans/119-architecture-audit.md §3.1) —
# it asserts gst: predicates directly rather than duplicating them, so
# packs/gst's predicates must be registered before reco-now's own pack
# installs, and before any upload is ingested. Reaches outside ext-apps/
# on purpose: this app lives inside the graph-owl monorepo specifically to
# compose with the platform's own reference pack, not fork it.
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


def _build_dataset(payload: list[dict], name: str, kind: str) -> dict:
    headers = list(payload[0].keys()) if payload else []
    rows = payload
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
def _startup() -> None:
    _reset()
    _install_graphowl_pack()


def _install_graphowl_pack() -> None:
    """Declare packs/gst's *vocabulary* (namespace, predicates, ontology —
    not its demo fixtures), then reco-now's own — in that order, because
    `graphowl_client.rows_to_turtle` asserts gst: predicates directly
    (plans/119-architecture-audit.md §3.1) and `POST /graph/import/rdf`
    refuses any flake whose predicate is not yet registered.

    **`include_documents=False` on the gst load is load-bearing, not an
    optimisation.** The native reconcile engine has no per-source data
    isolation (`Catalog::reconcile_pack` runs each rule's SPARQL over the
    whole store); packs/gst's own `[[documents]]` are its planted
    INV-1001..INV-2002 demo scenarios, and loading them into reco-now's
    deployment would put graph-owl's own demo invoices into every
    reconciliation reco-now runs. Vocabulary composition is wanted here,
    not the data.

    Both loads use the one-time step `load_pack` already does for
    packs/gst and packs/hospitality, reused here rather than
    reimplemented. Idempotent (loading a pack twice is a no-op), so this
    runs on every startup, not just the first."""
    try:
        load_pack(GST_PACK_DIR, GRAPH_OWL_SERVER, GRAPH_OWL_TOKEN, include_documents=False)
        load_pack(GRAPH_OWL_PACK_DIR, GRAPH_OWL_SERVER, GRAPH_OWL_TOKEN)
    except LoadError as exc:
        print(f"[graphowl] pack install skipped — {exc}")


def _ingest_to_graphowl(kind: str, dataset: dict, mapping: dict) -> None:
    """Land this upload's rows in graph-owl as durable graph subjects, in
    a background thread so a slow or absent graph-owl never delays the
    upload response. Records what happened in SESSION["graphowl"], which
    `overview()` deliberately never reads — this is additive, not a
    change to the existing response shape."""

    def _run() -> None:
        try:
            normalized = _normalize(dataset, mapping)
            turtle = graphowl_client.rows_to_turtle(normalized, kind)
            if not turtle:
                SESSION["graphowl"][kind] = {"landed": 0, "skipped": 0, "rejected": []}
                return
            source = f"reco-{kind}-{uuid.uuid4().hex[:8]}"
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

    threading.Thread(target=_run, daemon=True).start()


@app.get("/api/health")
def health() -> dict:
    return {
        "status": "ok",
        "service": "matcha-backend",
        "ai": {"available": ai.is_available(), "model": ai.MODEL},
    }


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
        if "2b" in lower or "gstr-2b" in lower or "portal" in lower or "gov" in lower:
            kind, name = "gstr2b", "Government Data"
        else:
            kind, name = "books", "Your Books"
        SESSION["datasets"][kind] = _build_dataset(payload, name, kind)
        kind_order.append(kind)
    if not SESSION["datasets"]:
        return {"ok": False, "error": "No valid files uploaded."}
    for kind in ("books", "gstr2b"):
        if kind in SESSION["datasets"]:
            SESSION["mapping"][kind] = _auto_map(SESSION["datasets"][kind]["headers"])
            _ingest_to_graphowl(kind, SESSION["datasets"][kind], SESSION["mapping"][kind])
    return overview()


@app.get("/api/graphowl/status")
def graphowl_status() -> dict:
    """What Slice 1's background ingestion did for the current upload —
    not part of `overview()`, so existing consumers of that response are
    unaffected by this integration existing at all."""
    return {"ok": True, "server": GRAPH_OWL_SERVER, "datasets": SESSION.get("graphowl", {})}


@app.post("/api/mapping")
def save_mapping(payload: dict) -> dict:
    SESSION["mapping"] = payload.get("mapping", {})
    SESSION["tolerance"] = float(payload.get("tolerance", 1.0))
    if payload.get("period"):
        SESSION["period"] = payload["period"]
    return {"ok": True}


@app.post("/api/reconcile")
def run_reconcile() -> dict:
    books = _normalize(SESSION["datasets"]["books"], SESSION["mapping"].get("books", {}))
    portal = _normalize(SESSION["datasets"]["gstr2b"], SESSION["mapping"].get("gstr2b", {}))
    SESSION["normalized"] = {"books": books, "gstr2b": portal}
    SESSION["results"] = rc.reconcile(books, portal, tolerance=SESSION["tolerance"])
    threading.Thread(target=_run_graphowl_reconcile, daemon=True).start()
    return overview()


def _run_graphowl_reconcile() -> None:
    """Dual-path, per plans/119-architecture-audit.md §5b step 4: the
    native engine runs ALONGSIDE reconciliation.py here, not instead of
    it — cutover (steps 6-7) only happens once parity is actually
    demonstrated, not assumed. Best-effort and backgrounded, same as
    ingestion: graph-owl may not have finished landing this upload yet
    (ingestion is its own background thread, started at /api/upload) or
    may not be running at all, and neither should block or fail the
    Python-side reconciliation this endpoint already returns."""
    try:
        result = run_findings("reco", GRAPH_OWL_SERVER, GRAPH_OWL_TOKEN)
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
