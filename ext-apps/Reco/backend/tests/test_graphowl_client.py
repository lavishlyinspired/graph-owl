"""RED tests for reco-now's graph-owl ingestion client.

plans/118-reco-now-integration.md, Slice 1. Two layers, tested
differently: `rows_to_turtle` is pure and gets plain unit tests;
`import_document` talks HTTP and gets the same "a real local double, not
a mock" discipline connectors/python/tests/test_loader.py already uses —
not a live graph-owl-server, which is what the manual end-to-end
verification step (Slice 1's "Done when") is for.
"""

from __future__ import annotations

import json
import threading
from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, HTTPServer
from urllib.parse import parse_qs, urlparse

import pytest

from app.graphowl_client import IngestError, import_document, rows_to_turtle

ALL_PREDICATES = (
    "invoiceNumber", "supplierGstin", "supplierName", "taxableValue",
    "invoiceDate", "placeOfSupply", "hsnCode", "imsStatus", "reverseCharge",
    "noteType", "voucherType", "originalInvoiceNumber", "voucherNumber",
    "igst", "cgst", "sgst", "cess",
)


def _row(**overrides) -> dict:
    """A maximally-populated row — every one of the 17 fields has a real
    value, so the "everything lands" test needs no per-field setup, and
    the omission tests override only what they mean to blank out."""
    base = {
        "invoice_no": "INV-AUG-109",
        "supplier_gstin": "27AAAFN2938K1Z2",
        "supplier_name": "Nimbus Freight Logistics",
        "taxable": 22000,
        "invoice_date": "11-08-2026",
        "place_of_supply": "27-Maharashtra",
        "hsn": "9965",
        "ims_status": "Accepted",
        "reverse_charge": "No",
        "note_type": "Credit Note",
        "voucher_type": "Purchase",
        "original_invoice_no": "INV-AUG-101",
        "voucher_no": "VCH/2026/109",
        "igst": 3960,
        "cgst": 0,
        "sgst": 0,
        "cess": 0,
    }
    base.update(overrides)
    return base


class TestRowsToTurtle:
    def test_empty_rows_produce_an_empty_document(self):
        assert rows_to_turtle([], "books") == ""

    def test_fully_populated_row_carries_every_predicate(self):
        turtle = rows_to_turtle([_row()], "books")
        for predicate in ALL_PREDICATES:
            assert f"reco:{predicate}" in turtle, f"missing {predicate}"

    def test_books_kind_uses_the_books_class(self):
        turtle = rows_to_turtle([_row()], "books")
        assert "a reco:BooksInvoice" in turtle
        assert "reco:PortalInvoice" not in turtle

    def test_portal_kind_uses_the_portal_class(self):
        turtle = rows_to_turtle([_row()], "gstr2b")
        assert "a reco:PortalInvoice" in turtle
        assert "reco:BooksInvoice" not in turtle

    def test_unknown_kind_is_rejected(self):
        with pytest.raises(ValueError):
            rows_to_turtle([_row()], "not-a-real-kind")

    @pytest.mark.parametrize("blank", [None, "", float("nan")])
    def test_absent_field_is_omitted_not_written_as_a_blank_literal(self, blank):
        turtle = rows_to_turtle([_row(ims_status=blank, note_type=blank)], "books")
        # Negative, not merely "we didn't check for it": the predicate
        # must not appear at all — distinct from appearing with `""`,
        # which would claim "recorded as blank" for a fact never recorded.
        assert "reco:imsStatus" not in turtle
        assert "reco:noteType" not in turtle
        assert '""' not in turtle

    def test_zero_is_a_real_value_and_is_not_treated_as_absent(self):
        turtle = rows_to_turtle([_row(cgst=0, sgst=0)], "books")
        assert "reco:cgst" in turtle
        assert "reco:sgst" in turtle

    def test_a_real_non_nan_float_is_present_not_treated_as_absent(self):
        # pandas hands back float64 for a numeric column even when every
        # value is whole — a check that drops the isnan() half and
        # treats every float as absent would still pass every int-typed
        # fixture above and only show up on real upload data.
        turtle = rows_to_turtle([_row(taxable=22000.5)], "books")
        assert "reco:taxableValue" in turtle

    def test_whitespace_only_field_is_treated_as_absent(self):
        # A cell that round-tripped through a spreadsheet as spaces, not
        # truly empty — `value == ""` (no strip) would let this through
        # as a recorded-blank fact instead of an absent one.
        turtle = rows_to_turtle([_row(note_type="   ")], "books")
        assert "reco:noteType" not in turtle

    def test_invoice_number_with_slashes_is_percent_encoded_in_the_subject(self):
        # Real GST invoice numbers commonly look like "INF/23-24/0456" —
        # an un-encoded "/" would silently change the IRI's path shape.
        turtle = rows_to_turtle([_row(invoice_no="INF/23-24/0456")], "books")
        subject_line = next(line for line in turtle.splitlines() if line.startswith("<"))
        assert "INF/23-24/0456" not in subject_line
        assert "INF%2F23-24%2F0456" in subject_line

    def test_a_blank_field_beside_populated_ones_still_lets_the_rest_land(self):
        turtle = rows_to_turtle([_row(ims_status=None)], "books")
        assert "reco:supplierName" in turtle
        assert "reco:taxableValue" in turtle
        assert "reco:imsStatus" not in turtle

    def test_quote_backslash_and_newline_are_escaped(self):
        # Actual value: O"Brien \ Textiles<newline>Unit 2 — a real supplier
        # name and a real multi-line note can both hit this.
        turtle = rows_to_turtle(
            [_row(supplier_name='O"Brien \\ Textiles\nUnit 2')], "books"
        )
        assert '\\"Brien' in turtle  # the quote became \"
        assert "\\\\ Textiles" in turtle  # the single backslash became \\
        assert "Textiles\\nUnit" in turtle  # the real newline became \n
        # An unescaped quote/backslash would corrupt Turtle parsing past
        # that point — one subject block proves it did not.
        assert turtle.count("a reco:BooksInvoice") == 1

    def test_subjects_are_minted_under_the_pack_s_own_registered_namespace(self):
        # Regression: subjects were originally minted under a separate
        # "https://reconow.dev/data/..." namespace, which POST
        # /namespaces never declares — graph-owl refused every row with
        # "not in a namespace this store recognises" (found by actually
        # running the upload, not by reading the code). Only the pack's
        # own NAMESPACE is ever registered, so every subject must live
        # under it — the same shape packs/gst's own fixtures use
        # (`gst:pr-INV-1001`, not a separate data prefix).
        from app.graphowl_client import NAMESPACE

        turtle = rows_to_turtle([_row()], "books")
        subject_line = next(line for line in turtle.splitlines() if line.startswith("<"))
        assert subject_line.startswith(f"<{NAMESPACE}")

    def test_same_invoice_number_different_supplier_gives_distinct_subjects(self):
        rows = [
            _row(invoice_no="INV-9", supplier_gstin="27AAAFN2938K1Z2"),
            _row(invoice_no="INV-9", supplier_gstin="29AAECK4410L1Z7"),
        ]
        turtle = rows_to_turtle(rows, "books")
        subjects = [line for line in turtle.splitlines() if line.startswith("<")]
        assert len(subjects) == 2
        assert subjects[0] != subjects[1]


def _handler(received: list[dict], fail: bool):
    class Scripted(BaseHTTPRequestHandler):
        def do_POST(self):  # noqa: N802
            parsed = urlparse(self.path)
            length = int(self.headers.get("content-length", 0))
            raw = self.rfile.read(length) if length else b""
            received.append(
                {
                    "path": parsed.path,
                    "query": {k: v[0] for k, v in parse_qs(parsed.query).items()},
                    "raw": raw,
                    "auth": self.headers.get("authorization"),
                }
            )
            if fail:
                self.send_response(500)
                self.end_headers()
                self.wfile.write(b'{"detail":"deliberate failure"}')
                return
            body = json.dumps({"landed": ["x"], "skipped": [], "rejected": []}).encode("utf-8")
            self.send_response(200)
            self.send_header("content-type", "application/json")
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *args):  # silence test output
            pass

    return Scripted


@contextmanager
def _server(received: list[dict], fail: bool = False):
    server = HTTPServer(("127.0.0.1", 0), _handler(received, fail))
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_port}"
    finally:
        server.shutdown()
        thread.join()


class TestImportDocument:
    def test_posts_turtle_with_source_and_format_query_params(self):
        received: list[dict] = []
        with _server(received) as url:
            result = import_document(url, "reco-books-upload-1", "@prefix reco: <x> .\n")
        assert len(received) == 1
        call = received[0]
        assert call["path"] == "/graph/import/rdf"
        assert call["query"]["source"] == "reco-books-upload-1"
        assert call["query"]["format"] == "turtle"
        assert call["raw"] == b"@prefix reco: <x> .\n"
        assert result == {"landed": ["x"], "skipped": [], "rejected": []}

    def test_sends_bearer_token_when_given(self):
        received: list[dict] = []
        with _server(received) as url:
            import_document(url, "s", "text", token="abc123")
        assert received[0]["auth"] == "Bearer abc123"

    def test_omits_authorization_header_when_no_token_given(self):
        received: list[dict] = []
        with _server(received) as url:
            import_document(url, "s", "text")
        assert received[0]["auth"] is None

    def test_server_error_raises_ingest_error_naming_the_status(self):
        received: list[dict] = []
        with _server(received, fail=True) as url:
            with pytest.raises(IngestError, match="500"):
                import_document(url, "s", "text")

    def test_unreachable_server_raises_ingest_error(self):
        with pytest.raises(IngestError):
            import_document("http://127.0.0.1:1", "s", "text")
