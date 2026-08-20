"""Wires the console's "Search or ask GraphOWL…" bar to a real answer —
Plan `graphowl-app/plans/ask-graphowl.md`.

**Deliberately narrow, and says so.** This routes free text to one of
`reconcile_agent.py`'s 15 fixed evaluation questions by word overlap, not
a general natural-language interface — the deterministic routing table
that makes those answers scoreable against `packs/gst/eval/questions.md`
is exactly what a free-text match must not bypass. For an open-ended
question none of the 15 covers, `integrations/langchain/examples/
gst_investigation_agent.py`'s real MCP tool-calling loop is the answer —
run directly, not wired into this server, because a single ReAct
investigation run costs tens of seconds and this endpoint is a search bar,
not a batch job.

Stdlib only, matching `reconcile_agent.py`'s own purity constraint.

    python examples/gst-reconcile/ask_server.py
    # then POST {"question": "..."} to http://localhost:8090/ask
"""

from __future__ import annotations

import json
import os
import re
import sys
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from reconcile_agent import (  # noqa: E402
    QUESTIONS,
    AgentError,
    answer,
    findings,
    narrate,
)

_WORD = re.compile(r"[a-z0-9]+")

#: Common words carry no matching signal and dilute the real overlap —
#: "the invoices are" matches almost every question a little, which is
#: worse than matching none of them a lot. **Negation words stay out of
#: this list on purpose** — "never filed" and "not a problem" mean the
#: opposite of "filed" and "a problem", and questions 6-10 exist
#: specifically to test whether that distinction is kept.
_STOPWORDS = frozenset(
    "a an and are by do for from has have in is it "
    "of on or that the this to was were which who will with".split()
)


def _words(text: str) -> set[str]:
    return {w for w in _WORD.findall(text.lower()) if w not in _STOPWORDS}


def _same_word(a: str, b: str) -> bool:
    """Treats "file"/"filed" and "invoice"/"invoices" as the same word via
    a shared 4-character prefix, rather than a hand-rolled stemmer —
    "filed" stripped of "ed" is "fil", "file" is not, and getting a
    stemmer's edge cases symmetric is not worth it for a 15-question
    routing table. A 4-character prefix is long enough that unrelated
    short words ("was"/"wash") do not collide."""
    if a == b:
        return True
    return len(a) >= 4 and len(b) >= 4 and a[:4] == b[:4]


def _overlap(input_words: set[str], spec_words: set[str]) -> set[str]:
    return {w for w in input_words if any(_same_word(w, s) for s in spec_words)}


def best_match(question_text: str, threshold: float = 0.2) -> int | None:
    """The `QUESTIONS` key whose own wording overlaps `question_text` the
    most, by Jaccard-style similarity over non-stopword tokens — or
    `None` when nothing clears `threshold`. A forced wrong match is worse
    than an honest refusal: the caller shows "none of the questions this
    can answer look like that", not a confident answer to the wrong
    thing.
    """
    input_words = _words(question_text)
    if not input_words:
        return None
    best_key: int | None = None
    best_score = 0.0
    for key, spec in QUESTIONS.items():
        spec_words = _words(spec.text)
        overlap = _overlap(input_words, spec_words)
        if not overlap:
            continue
        score = len(overlap) / len(input_words | spec_words)
        if score > best_score:
            best_score = score
            best_key = key
    return best_key if best_score >= threshold else None


# ---- Supplier invoice count — a second, independent routing tier.
#
# **Found live, not designed up front**: a real question ("how many
# invoices are there for patel chemicals and co") had no match among the
# 15 fixed questions and got an honest, unhelpful `noMatch` — even though
# the graph genuinely has the answer. `gst:supplierName` and `gst:issuedBy`
# make "invoices for/from/by <party>" a real, answerable query, distinct
# from `best_match`'s fixed table: this asks *which entity*, not *which
# finding kind*, so it stays a separate function rather than a 16th
# `QuestionSpec` the table was never shaped for. ----

_INVOICE_FOR_PATTERN = re.compile(
    r"invoices?\b.*?\b(?:for|from|by)\s+(?P<name>.+?)\s*[\?\.]*$",
    re.IGNORECASE,
)


def extract_supplier_query(question_text: str) -> str | None:
    """The supplier name a free-text question names, if it looks like
    "how many/which invoices ... for/from/by <supplier>" — or `None`.
    Requires the word "invoice(s)" so an unrelated "... for tomorrow"
    question is never mistaken for one about a party."""
    match = _INVOICE_FOR_PATTERN.search(question_text)
    if not match:
        return None
    name = match.group("name").strip()
    return name or None


def _normalize_name(name: str) -> set[str]:
    """"Patel Chemicals & Co" and "patel chemicals and co" must compare
    equal — punctuation carries no signal, and "&"/"and" are the same
    conjunction spelled two ways in real supplier data."""
    folded = name.lower().replace("&", " and ")
    return {w for w in re.findall(r"[a-z0-9]+", folded) if w not in _STOPWORDS}


def best_supplier_match(
    name: str, suppliers: list[tuple[str, str]], threshold: float = 0.5
) -> tuple[str, str] | None:
    """The `(subject, name)` pair whose own name overlaps `name` the most
    — or `None` when nothing clears `threshold`. Same refusal discipline
    as `best_match`: a search bar guessing the wrong company is worse
    than admitting it does not recognise the name."""
    input_words = _normalize_name(name)
    if not input_words:
        return None
    best: tuple[str, str] | None = None
    best_score = 0.0
    for subject, supplier_name in suppliers:
        candidate_words = _normalize_name(supplier_name)
        overlap = input_words & candidate_words
        if not overlap:
            continue
        score = len(overlap) / len(input_words | candidate_words)
        if score > best_score:
            best_score = score
            best = (subject, supplier_name)
    return best if best_score >= threshold else None


def _sparql(server_url: str, token: str | None, query: str) -> list[dict]:
    request = urllib.request.Request(
        f"{server_url.rstrip('/')}/sparql",
        data=json.dumps({"query": query}).encode("utf-8"),
        method="POST",
    )
    request.add_header("content-type", "application/json")
    if token:
        request.add_header("authorization", f"Bearer {token}")
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return json.loads(response.read())["rows"]
    except urllib.error.URLError as unreachable:
        raise AgentError(f"SPARQL query was unreachable: {unreachable}") from unreachable


def _unwrap_iri(term: str) -> str:
    return term[1:-1] if term.startswith("<") and term.endswith(">") else term


def _unwrap_literal(term: str) -> str:
    match = re.match(r'^"(.*)"', term)
    return match.group(1) if match else term


def _local_name(iri: str) -> str:
    return iri.rsplit("#", 1)[-1].rsplit("/", 1)[-1]


def _fetch_suppliers(server_url: str, token: str | None) -> list[tuple[str, str]]:
    rows = _sparql(
        server_url,
        token,
        "PREFIX gst: <https://graph-owl.dev/packs/gst#> "
        "SELECT DISTINCT ?supplier ?name WHERE { "
        "GRAPH ?g { ?supplier a gst:Supplier ; gst:supplierName ?name } }",
    )
    return [(_unwrap_iri(row["supplier"]), _unwrap_literal(row["name"])) for row in rows]


def _fetch_supplier_invoices(server_url: str, token: str | None, supplier_iri: str) -> list[str]:
    rows = _sparql(
        server_url,
        token,
        "PREFIX gst: <https://graph-owl.dev/packs/gst#> "
        f"SELECT DISTINCT ?invoice WHERE {{ GRAPH ?g {{ "
        f"?invoice a gst:PurchaseInvoice ; gst:issuedBy <{supplier_iri}> }} }}",
    )
    return sorted({_local_name(_unwrap_iri(row["invoice"])) for row in rows})


def _handle_supplier_query(name: str, server_url: str, token: str | None) -> dict:
    try:
        suppliers = _fetch_suppliers(server_url, token)
        matched = best_supplier_match(name, suppliers)
        if matched is None:
            return {
                "kind": "noMatch",
                "message": f'No supplier in the graph matches "{name}".',
            }
        supplier_iri, supplier_name = matched
        invoices = _fetch_supplier_invoices(server_url, token, supplier_iri)
    except AgentError as failed:
        return {"kind": "error", "message": str(failed)}

    count_text = f"{len(invoices)} invoice{'' if len(invoices) == 1 else 's'}"
    lines = [f"{count_text} for {supplier_name}:"] + [f"  {i}" for i in invoices]
    return {
        "kind": "answered",
        "questionNumber": None,
        "answer": "\n".join(lines) if invoices else f"{count_text} for {supplier_name}.",
    }


def _handle_ask(question_text: str, server_url: str, token: str | None) -> dict:
    supplier_name = extract_supplier_query(question_text)
    if supplier_name is not None:
        return _handle_supplier_query(supplier_name, server_url, token)
    matched = best_match(question_text)
    if matched is None:
        return {
            "kind": "noMatch",
            "message": "None of the fixed reconciliation questions this can answer look like that.",
        }
    try:
        rows = findings(server_url, token)
        found = answer(matched, rows)
    except AgentError as failed:
        return {"kind": "error", "message": str(failed)}

    result = {"kind": "answered", "questionNumber": matched, "answer": found.as_text()}

    base_url = os.environ.get("LLM_API_BASE_URL")
    model = os.environ.get("LLM_MODEL")
    if base_url and model:
        try:
            result["narration"] = narrate(
                found,
                base_url,
                model,
                os.environ.get("LLM_API_KEY"),
                fallback_base_url=os.environ.get("LLM_FALLBACK_BASE_URL", base_url),
                fallback_model=os.environ.get("LLM_FALLBACK_MODEL"),
            )
        except AgentError as failed:
            # The structured answer above is already complete — a failed
            # narration is not a failed answer.
            result["narrationError"] = str(failed)
    return result


class Handler(BaseHTTPRequestHandler):
    def log_message(self, format: str, *args: object) -> None:  # noqa: A002
        sys.stderr.write(f"{self.address_string()} {format % args}\n")

    def do_POST(self) -> None:  # noqa: N802
        if self.path != "/ask":
            self.send_response(404)
            self.end_headers()
            return
        length = int(self.headers.get("content-length", "0"))
        try:
            payload = json.loads(self.rfile.read(length) or b"{}")
            question_text = str(payload.get("question", ""))
        except (json.JSONDecodeError, TypeError):
            self.send_response(400)
            self.end_headers()
            return

        server_url = os.environ.get("GRAPH_OWL_SERVER", "http://localhost:8080")
        token = os.environ.get("GRAPH_OWL_TOKEN")
        body = json.dumps(_handle_ask(question_text, server_url, token)).encode("utf-8")

        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.send_header("access-control-allow-origin", "*")
        self.end_headers()
        self.wfile.write(body)

    def do_OPTIONS(self) -> None:  # noqa: N802
        self.send_response(204)
        self.send_header("access-control-allow-origin", "*")
        self.send_header("access-control-allow-methods", "POST, OPTIONS")
        self.send_header("access-control-allow-headers", "content-type")
        self.end_headers()


def main() -> int:
    port = int(os.environ.get("ASK_SERVER_PORT", "8090"))
    server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    print(f"ask-graphowl listening on http://127.0.0.1:{port}/ask", file=sys.stderr)
    server.serve_forever()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
