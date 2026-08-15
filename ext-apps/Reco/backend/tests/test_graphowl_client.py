"""RED tests for reco-now's graph-owl ingestion client.

plans/118-reco-now-integration.md Slice 1, corrected per
plans/119-architecture-audit.md §3.1/§6: reco-now's pack is an EXTENSION of
packs/gst, not a parallel copy of it. 11 of the 17 fields reuse `gst:`
predicates and `gst:PurchaseInvoice`/`gst:Gstr2bInvoice` directly; only the
6 fields packs/gst's ontology doesn't have get a `reco:` predicate.

Two layers, tested differently: `rows_to_turtle` is pure and gets plain
unit tests; `import_document` talks HTTP and gets the same "a real local
double, not a mock" discipline connectors/python/tests/test_loader.py
already uses — not a live graph-owl-server, which is what the manual
end-to-end verification step (Slice 1's "Done when") is for.
"""

from __future__ import annotations

import json
import threading
from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, HTTPServer
from urllib.parse import parse_qs, urlparse

import pytest

from app.graphowl_client import IngestError, import_document, list_findings, rows_to_turtle

#: Fields reco-now shares with packs/gst — asserted as `gst:` predicates.
GST_PREDICATES = (
    "invoiceNumber", "supplierGstin", "supplierName", "taxableValue",
    "invoiceDate", "placeOfSupply", "reverseCharge",
    "igst", "cgst", "sgst", "cess",
)
#: The 6 fields packs/gst's ontology does not have — reco-now's own.
RECO_PREDICATES = (
    "hsnCode", "imsStatus", "noteType", "voucherType",
    "voucherNumber", "originalInvoiceNumber",
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

    def test_invoice_date_is_normalized_to_iso_8601(self):
        # Reco's own CSVs (and _normalize's output) carry dates as
        # DD-MM-YYYY strings — main.py's own sample data uses "07-08-2026"
        # for 7 August 2026. packs/gst's finding queries compare
        # gst:invoiceDate against gst:effectiveFrom lexicographically as
        # plain strings (amount-mismatch.sparql: `FILTER (?from <= ?date)`,
        # no xsd:date cast) — verified live: with the raw DD-MM-YYYY string
        # asserted, every comparison against an ISO "20XX-01-01" provision
        # failed silently (no exception, just zero results), because "2"
        # sorts after "0" character-by-character. ISO YYYY-MM-DD is the
        # only format both this pack's dates and packs/gst's law dates can
        # be compared under with plain `<=`.
        turtle = rows_to_turtle([_row(invoice_date="07-08-2026")], "books")
        assert 'gst:invoiceDate "2026-08-07"' in turtle
        assert "07-08-2026" not in turtle

    def test_an_unparseable_date_is_passed_through_unchanged(self):
        # A malformed date is a data-quality problem to surface, not one
        # for this function to silently swallow or crash on.
        turtle = rows_to_turtle([_row(invoice_date="not-a-date")], "books")
        assert 'gst:invoiceDate "not-a-date"' in turtle

    def test_fully_populated_row_carries_every_predicate_under_the_right_prefix(self):
        turtle = rows_to_turtle([_row()], "books")
        for predicate in GST_PREDICATES:
            assert f"gst:{predicate}" in turtle, f"missing gst:{predicate}"
        for predicate in RECO_PREDICATES:
            assert f"reco:{predicate}" in turtle, f"missing reco:{predicate}"

    def test_shared_fields_never_land_under_the_reco_prefix(self):
        # The regression this fix exists for: a shared field must not be
        # asserted twice under two unrelated predicates.
        turtle = rows_to_turtle([_row()], "books")
        for predicate in GST_PREDICATES:
            assert f"reco:{predicate}" not in turtle, f"gst:{predicate} duplicated as reco:{predicate}"

    def test_declares_both_prefixes(self):
        turtle = rows_to_turtle([_row()], "books")
        assert "@prefix gst: <https://graph-owl.dev/packs/gst#>" in turtle
        assert "@prefix reco: <https://reconow.dev/pack#>" in turtle

    def test_books_kind_mints_the_shared_gst_purchase_invoice_class(self):
        turtle = rows_to_turtle([_row()], "books")
        assert "a gst:PurchaseInvoice" in turtle
        assert "gst:Gstr2bInvoice" not in turtle
        assert "reco:BooksInvoice" not in turtle  # the old, duplicated class

    def test_gstr2b_kind_mints_the_shared_gst_2b_invoice_class(self):
        turtle = rows_to_turtle([_row()], "gstr2b")
        assert "a gst:Gstr2bInvoice" in turtle
        assert "gst:PurchaseInvoice" not in turtle
        assert "reco:PortalInvoice" not in turtle  # the old, duplicated class

    def test_unknown_kind_is_rejected(self):
        with pytest.raises(ValueError):
            rows_to_turtle([_row()], "not-a-real-kind")

    @pytest.mark.parametrize("blank", [None, "", float("nan")])
    def test_absent_reco_field_is_omitted_not_written_as_a_blank_literal(self, blank):
        turtle = rows_to_turtle([_row(ims_status=blank, note_type=blank)], "books")
        assert "reco:imsStatus" not in turtle
        assert "reco:noteType" not in turtle

    @pytest.mark.parametrize("blank", [None, "", float("nan")])
    def test_absent_gst_field_is_also_omitted_not_written_as_a_blank_literal(self, blank):
        # The omission rule applies uniformly regardless of which prefix a
        # field ends up under — this pins that down explicitly.
        turtle = rows_to_turtle([_row(supplier_name=blank, place_of_supply=blank)], "books")
        assert "gst:supplierName" not in turtle
        assert "gst:placeOfSupply" not in turtle

    def test_zero_is_a_real_value_and_is_not_treated_as_absent(self):
        turtle = rows_to_turtle([_row(cgst=0, sgst=0)], "books")
        assert "gst:cgst" in turtle
        assert "gst:sgst" in turtle

    def test_a_blank_field_beside_populated_ones_still_lets_the_rest_land(self):
        turtle = rows_to_turtle([_row(ims_status=None)], "books")
        assert "gst:supplierName" in turtle
        assert "gst:taxableValue" in turtle
        assert "reco:imsStatus" not in turtle

    def test_quote_backslash_and_newline_are_escaped(self):
        # Actual value: O"Brien \ Textiles<newline>Unit 2 — a real supplier
        # name and a real multi-line note can both hit this. supplierName
        # is now a gst: field, so this also proves escaping isn't tied to
        # one particular namespace's predicates.
        turtle = rows_to_turtle(
            [_row(supplier_name='O"Brien \\ Textiles\nUnit 2')], "books"
        )
        assert '\\"Brien' in turtle  # the quote became \"
        assert "\\\\ Textiles" in turtle  # the single backslash became \\
        assert "Textiles\\nUnit" in turtle  # the real newline became \n
        # An unescaped quote/backslash would corrupt Turtle parsing past
        # that point — one subject block proves it did not.
        assert turtle.count("a gst:PurchaseInvoice") == 1

    def test_same_invoice_number_different_supplier_gives_distinct_subjects(self):
        rows = [
            _row(invoice_no="INV-9", supplier_gstin="27AAAFN2938K1Z2"),
            _row(invoice_no="INV-9", supplier_gstin="29AAECK4410L1Z7"),
        ]
        turtle = rows_to_turtle(rows, "books")
        # The per-source invoice subject specifically (not the Supplier or
        # canonical blocks, which every row also emits now).
        subjects = [
            line for line in turtle.splitlines() if line.startswith("<https://reconow.dev/pack#books-")
        ]
        assert len(subjects) == 2
        assert subjects[0] != subjects[1]

    def test_subjects_are_minted_under_the_pack_s_own_registered_namespace(self):
        # Regression: subjects were originally minted under a separate
        # "https://reconow.dev/data/..." namespace, which POST
        # /namespaces never declares — graph-owl refused every row with
        # "not in a namespace this store recognises" (found by actually
        # running the upload, not by reading the code). Only the pack's
        # own NAMESPACE is ever registered, so every subject must live
        # under it — the same shape packs/gst's own fixtures use
        # (`gst:pr-INV-1001`, not a separate data prefix). Instance
        # identity stays under reco:'s own namespace even though the
        # *type* and most *predicates* are now gst: — a subject's IRI and
        # its rdf:type are independent facts.
        from app.graphowl_client import NAMESPACE

        turtle = rows_to_turtle([_row()], "books")
        subject_line = next(line for line in turtle.splitlines() if line.startswith("<"))
        assert subject_line.startswith(f"<{NAMESPACE}")

    def test_a_real_non_nan_float_is_present_not_treated_as_absent(self):
        # pandas hands back float64 for a numeric column even when every
        # value is whole — a check that drops the isnan() half and
        # treats every float as absent would still pass every int-typed
        # fixture above and only show up on real upload data.
        turtle = rows_to_turtle([_row(taxable=22000.5)], "books")
        assert "gst:taxableValue" in turtle

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
        subject_line = next(
            line for line in turtle.splitlines() if line.startswith("<https://reconow.dev/pack#books-")
        )
        assert "INF/23-24/0456" not in subject_line
        assert "INF%2F23-24%2F0456" in subject_line


class TestCanonicalLinking:
    """packs/gst's finding queries (missing-in-gstr2b, amount-mismatch,
    tax-head-mismatch) don't join books-vs-2B by a shared literal key — they
    walk a real graph shape: a canonical subject linked to each per-source
    invoice via gst:recordedIn/gst:reflectedIn, and each per-source invoice
    linked to a real gst:Supplier subject via gst:issuedBy (confirmed by
    reading amount-mismatch.sparql/tax-head-mismatch.sparql/
    missing-in-gstr2b.sparql in full — not assumed from field-level
    vocabulary compatibility, which is what the first pass at this got
    wrong). Without this, every one of packs/gst's finding queries would
    silently match zero of reco-now's subjects."""

    def test_supplier_subject_is_a_gst_supplier_with_its_gstin(self):
        turtle = rows_to_turtle([_row(supplier_gstin="27AAAFN2938K1Z2")], "books")
        assert "a gst:Supplier" in turtle
        assert 'gst:supplierGstin "27AAAFN2938K1Z2"' in turtle

    def test_supplier_gstin_no_longer_lands_directly_on_the_invoice(self):
        # It now lives on the Supplier subject, reached via gst:issuedBy —
        # matching packs/gst's own queries (`?supplier gst:supplierGstin
        # ?gstin`, never `?purchase gst:supplierGstin ?gstin`). Asserting
        # it in both places would be exactly the kind of redundant,
        # unrelated-looking duplication plans/119-architecture-audit.md
        # §3.1 already found and fixed once.
        turtle = rows_to_turtle([_row()], "books")
        # supplierGstin must appear exactly once in the whole document —
        # on the Supplier subject — not additionally on the invoice.
        assert turtle.count("gst:supplierGstin") == 1

    def test_the_invoice_carries_an_issuedby_edge_to_the_supplier(self):
        turtle = rows_to_turtle([_row(supplier_gstin="27AAAFN2938K1Z2")], "books")
        assert "gst:issuedBy <https://reconow.dev/pack#supplier-27AAAFN2938K1Z2>" in turtle

    def test_a_books_row_links_its_canonical_subject_via_recordedin(self):
        turtle = rows_to_turtle(
            [_row(invoice_no="INV-AUG-101", supplier_gstin="27AAAFN2938K1Z2")], "books"
        )
        assert (
            "<https://reconow.dev/pack#invoice-27AAAFN2938K1Z2-INV-AUG-101>\n"
            "    gst:recordedIn <https://reconow.dev/pack#books-27AAAFN2938K1Z2-INV-AUG-101> ."
        ) in turtle
        assert "gst:reflectedIn" not in turtle

    def test_a_gstr2b_row_links_its_canonical_subject_via_reflectedin(self):
        turtle = rows_to_turtle(
            [_row(invoice_no="INV-AUG-101", supplier_gstin="27AAAFN2938K1Z2")], "gstr2b"
        )
        assert (
            "<https://reconow.dev/pack#invoice-27AAAFN2938K1Z2-INV-AUG-101>\n"
            "    gst:reflectedIn <https://reconow.dev/pack#gstr2b-27AAAFN2938K1Z2-INV-AUG-101> ."
        ) in turtle
        assert "gst:recordedIn" not in turtle

    def test_the_same_invoice_gets_the_same_canonical_subject_regardless_of_kind(self):
        # Not asserted in one call (rows_to_turtle only ever sees one kind
        # at a time — books and gstr2b are two separate uploads/imports) —
        # but the IRI must be deterministic from (gstin, invoice_no) alone,
        # kind-independent, or the two imports would mint two different
        # canonical subjects and gst:recordedIn/gst:reflectedIn would never
        # meet on one subject the way amount-mismatch.sparql requires.
        books = rows_to_turtle([_row(invoice_no="INV-9", supplier_gstin="27X")], "books")
        gstr2b = rows_to_turtle([_row(invoice_no="INV-9", supplier_gstin="27X")], "gstr2b")
        books_canonical = next(l for l in books.splitlines() if l.startswith("<") and "invoice-" in l)
        gstr2b_canonical = next(l for l in gstr2b.splitlines() if l.startswith("<") and "invoice-" in l)
        assert books_canonical == gstr2b_canonical

    def test_books_row_carries_a_combined_tax_amount(self):
        turtle = rows_to_turtle([_row(igst=3960, cgst=0, sgst=0, cess=0)], "books")
        assert 'gst:taxAmount "3960"' in turtle

    def test_combined_tax_amount_sums_all_four_components(self):
        turtle = rows_to_turtle([_row(igst=0, cgst=8550, sgst=8550, cess=100)], "books")
        assert 'gst:taxAmount "17200"' in turtle

    def test_combined_tax_amount_treats_an_absent_component_as_zero(self):
        turtle = rows_to_turtle([_row(igst=3960, cgst=None, sgst="", cess=float("nan"))], "books")
        assert 'gst:taxAmount "3960"' in turtle

    def test_gstr2b_rows_do_not_carry_a_combined_tax_amount(self):
        # None of the 3 findings wired in this slice (PotentialMismatch,
        # AmountMismatch, TaxHeadMismatch) read gst:taxAmount off the
        # Gstr2bInvoice side — only itc-not-available/reverse-charge do,
        # and those are deferred (Reco has no itcAvailable field at all,
        # and its reverse_charge values are "Yes"/"No" text, not the "R"/
        # "N" codes those queries filter on). Asserting a field nothing
        # reads is speculative scope, not a fix.
        turtle = rows_to_turtle([_row(igst=3960, cgst=0, sgst=0, cess=0)], "gstr2b")
        assert "gst:taxAmount" not in turtle


def _handler(received: list[dict], fail: bool, findings_response: list | None = None):
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

        def do_GET(self):  # noqa: N802
            parsed = urlparse(self.path)
            received.append(
                {
                    "path": parsed.path,
                    "query": {k: v[0] for k, v in parse_qs(parsed.query).items()},
                    "raw": b"",
                    "auth": self.headers.get("authorization"),
                }
            )
            if fail:
                self.send_response(500)
                self.end_headers()
                self.wfile.write(b'{"detail":"deliberate failure"}')
                return
            body = json.dumps(findings_response if findings_response is not None else []).encode(
                "utf-8"
            )
            self.send_response(200)
            self.send_header("content-type", "application/json")
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *args):  # silence test output
            pass

    return Scripted


@contextmanager
def _server(received: list[dict], fail: bool = False, findings_response: list | None = None):
    server = HTTPServer(("127.0.0.1", 0), _handler(received, fail, findings_response))
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


class TestListFindings:
    def test_gets_findings_scoped_to_the_reco_pack(self):
        received: list[dict] = []
        sample = [{"id": "f1", "label": "gst:AmountMismatch", "subject": "x"}]
        with _server(received, findings_response=sample) as url:
            result = list_findings(url)
        assert received[0]["path"] == "/findings"
        assert received[0]["query"]["pack"] == "reco"
        assert result == sample

    def test_sends_bearer_token_when_given(self):
        received: list[dict] = []
        with _server(received, findings_response=[]) as url:
            list_findings(url, token="abc123")
        assert received[0]["auth"] == "Bearer abc123"

    def test_empty_findings_list_is_a_real_empty_list_not_an_error(self):
        received: list[dict] = []
        with _server(received, findings_response=[]) as url:
            result = list_findings(url)
        assert result == []

    def test_server_error_raises_ingest_error(self):
        received: list[dict] = []
        with _server(received, fail=True) as url:
            with pytest.raises(IngestError, match="500"):
                list_findings(url)
