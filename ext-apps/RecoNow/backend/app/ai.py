"""Ollama-backed AI helpers for Matcha.

Uses the local Ollama server. Falls back to deterministic templates when the
model is unavailable so the app never hard-fails on AI.
"""

from __future__ import annotations

import json
import os
import urllib.error
import urllib.request

OLLAMA_URL = os.environ.get("OLLAMA_URL", "http://127.0.0.1:11434")
MODEL = os.environ.get("MATCHA_OLLAMA_MODEL", "gpt-oss:20b-cloud")

_available: bool | None = None


def is_available() -> bool:
    global _available
    if _available is not None:
        return _available
    try:
        with urllib.request.urlopen(f"{OLLAMA_URL}/api/tags", timeout=3) as res:
            payload = json.loads(res.read())
        names = [m.get("name", "") for m in payload.get("models", [])]
        _available = any(name.startswith(MODEL.split(":")[0]) for name in names)
    except Exception:  # noqa: BLE001
        _available = False
    return _available


def _post(path: str, body: dict, timeout: float = 180) -> dict:
    request = urllib.request.Request(
        f"{OLLAMA_URL}{path}",
        data=json.dumps(body).encode("utf-8"),
        headers={"Content-Type": "application/json"},
    )
    with urllib.request.urlopen(request, timeout=timeout) as res:
        return json.loads(res.read())


def chat(system: str, user: str, timeout: float = 180) -> str | None:
    try:
        payload = _post(
            "/api/chat",
            {
                "model": MODEL,
                "stream": False,
                "messages": [
                    {"role": "system", "content": system},
                    {"role": "user", "content": user},
                ],
                "options": {"temperature": 0.3, "num_ctx": 8192},
            },
            timeout=timeout,
        )
        return payload.get("message", {}).get("content", "").strip() or None
    except Exception:  # noqa: BLE001
        return None


def map_columns(headers: list[str], field_labels: dict[str, str]) -> dict[str, int | None] | None:
    """Ask the model to map source headers to known fields. Returns None on failure."""
    fields = "\n".join(f"- {key}: {label}" for key, label in field_labels.items())
    prompt = (
        "Map each of these source columns to the most appropriate field from the list below.\n"
        f"Source columns:\n{json.dumps(headers, indent=0)}\n\n"
        f"Available fields (key: label):\n{fields}\n\n"
        "Return a JSON object only, with keys being field keys and values being "
        "the 0-based index of the source column, or null when unmapped. "
        "Each column may be used at most once. Prefer exact semantic matches."
    )
    system = (
        "You are a data-mapping assistant for Indian GST reconciliation software. "
        "You map spreadsheet columns to canonical fields. Reply only with valid JSON, no prose."
    )
    try:
        raw = chat(system, prompt, timeout=60)
        if not raw:
            return None
        start = raw.find("{")
        end = raw.rfind("}") + 1
        if start == -1 or end <= start:
            return None
        parsed = json.loads(raw[start:end])
        result = {}
        for key, label in field_labels.items():
            value = parsed.get(key)
            if isinstance(value, bool):
                result[key] = int(value) if value else None
            elif isinstance(value, (int, float)):
                result[key] = int(value)
            elif isinstance(value, str) and value.strip().isdigit():
                result[key] = int(value.strip())
            else:
                result[key] = None
        return result
    except Exception:  # noqa: BLE001
        return None


def draft_follow_up(supplier: str, gstin: str, invoice_no: str, itc: float, period: dict) -> str | None:
    system = (
        "You are a professional tax consultant in India writing a courteous, "
        "professional email to a supplier about a missing GSTR-1 filing. "
        "Reference the CGST Act correctly (Section 16(2)(aa)). Keep it under 150 words. "
        "Plain text, no markdown."
    )
    user = (
        f"Supplier: {supplier}\nGSTIN: {gstin}\nInvoice number: {invoice_no}\n"
        f"ITC involved: INR {itc:,.0f}\nPeriod: {period.get('month')} {period.get('year')}\n\n"
        "Draft a follow-up email requesting that the supplier file their GSTR-1 "
        "so the invoice appears in our GSTR-2B and we can claim the credit."
    )
    return chat(system, user)


def generate_client_report(period: dict, stats: dict, classifications: list[dict], ims: list[dict]) -> str | None:
    risk_summary = "\n".join(
        f"- {c['title']}: {c['count']} item(s), INR {c['itc']:,.0f} ({c['action']})"
        for c in classifications
        if c["count"]
    ) or "- None flagged"
    ims_summary = "\n".join(
        f"- {a['title']}: {a['count']} invoice(s), INR {a['itc']:,.0f}"
        for a in ims
        if a["count"]
    ) or "- None"
    system = (
        "You are an indirect-tax specialist at a chartered accountancy firm in India. "
        "Write a professional, plain-language client report in Markdown summarizing a "
        "GSTR-2B reconciliation. Be precise, cite the correct sections of the CGST Act "
        "(Section 16(2)(aa), Section 34, Section 16(4)) and recommend clear next steps. "
        "Use INR formatting with commas (e.g. INR 1,17,000). No invented figures."
    )
    user = (
        f"Period: {period.get('month')} {period.get('year')}\n"
        f"Total invoices: {stats.get('total', 0)}\n"
        f"Matched: {stats.get('matched', 0)} ({stats.get('match_rate', 0)}%)\n"
        f"Amount discrepancies: {stats.get('review', 0)}\n"
        f"In books, not in GSTR-2B: {stats.get('only_books', 0)}\n"
        f"In GSTR-2B, not in books: {stats.get('only_gstr2b', 0)}\n"
        f"ITC confirmed on matched invoices: INR {stats.get('confirmed_itc', 0):,.0f}\n"
        f"ITC at risk: INR {stats.get('at_risk_itc', 0):,.0f}\n"
        f"Gross ITC across matched, review and portal-only invoices (not the filed Table 4 figure): "
        f"INR {stats.get('gross_itc', 0):,.0f}\n\n"
        f"Risk classifications:\n{risk_summary}\n\n"
        f"Recommended IMS actions:\n{ims_summary}\n\n"
        "Write the report with sections: Executive Summary, Findings, Risk Assessment, "
        "Recommended Actions."
    )
    return chat(system, user)


def ai_summary(stats: dict, classifications: list[dict]) -> str | None:
    system = (
        "You are a tax analyst. Summarize a GSTR-2B reconciliation outcome in 2-3 concise "
        "sentences for an accountant. No markdown, no invented figures."
    )
    risk = ", ".join(
        f"{c['title']} ({c['count']} item(s), INR {c['itc']:,.0f})"
        for c in classifications
        if c["count"]
    )
    user = (
        f"Match rate: {stats.get('match_rate', 0)}%. "
        f"Matched: {stats.get('matched', 0)} invoices with ITC of INR {stats.get('confirmed_itc', 0):,.0f}. "
        f"At-risk items: {risk}. "
        f"Gross ITC across matched, review and portal-only invoices is "
        f"INR {stats.get('gross_itc', 0):,.0f} — do not call this the net or the GSTR-3B Table 4 figure. "
        "Write the summary."
    )
    return chat(system, user)
