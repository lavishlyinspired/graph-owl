"""RED tests for reco-now's graph-owl ingestion client.

plans/118-reco-now-integration.md Slice 1, corrected twice per
plans/119-architecture-audit.md — most recently (16 August 2026) by
merging reco-now's own extension pack entirely into `packs/gst`. There is
now exactly one GST pack; this client ingests everything under its
namespace, `gst:`, with no second prefix and no second pack registration.

Two layers, tested differently: `rows_to_turtle` is pure and gets plain
unit tests; `import_document`/`list_findings` talk HTTP and get the same
"a real local double, not a mock" discipline
connectors/python/tests/test_loader.py already uses — not a live
graph-owl-server, which is what the manual end-to-end verification step
(and ext-apps/Reco/scripts/verify-reconcile-parity.py) is for.
"""

from __future__ import annotations

import json
import threading
from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, HTTPServer
from urllib.parse import parse_qs, urlparse

import pytest

from app.graphowl_client import IngestError, import_document, list_findings, rows_to_turtle

#: Every field this client asserts as a `gst:` predicate. `supplier_gstin`
#: is deliberately excluded — it lands on the Supplier subject, not as a
#: direct predicate on the invoice (TestCanonicalLinking covers that).
ALL_PREDICATES = (
    "invoiceNumber", "supplierName", "taxableValue", "invoiceDate",
    "placeOfSupply", "hsnCode", "imsStatus", "reverseCharge", "noteType",
    "voucherType", "originalInvoiceNumber", "voucherNumber",
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
        # only format both this pack's dates and its own law dates can be
        # compared under with plain `<=`.
        turtle = rows_to_turtle([_row(invoice_date="07-08-2026")], "books")
        assert 'gst:invoiceDate "2026-08-07"' in turtle
        assert "07-08-2026" not in turtle

    def test_an_unparseable_date_is_passed_through_unchanged(self):
        # A malformed date is a data-quality problem to surface, not one
        # for this function to silently swallow or crash on.
        turtle = rows_to_turtle([_row(invoice_date="not-a-date")], "books")
        assert 'gst:invoiceDate "not-a-date"' in turtle

    def test_fully_populated_row_carries_every_predicate(self):
        turtle = rows_to_turtle([_row()], "books")
        for predicate in ALL_PREDICATES:
            assert f"gst:{predicate}" in turtle, f"missing gst:{predicate}"

    def test_nothing_is_ever_asserted_under_a_reco_prefix(self):
        # Regression guard: the first two versions of this pack
        # duplicated (v1) or partially duplicated (v2) packs/gst's own
        # vocabulary under a separate reco: namespace before both were
        # corrected. There is now exactly one pack and one prefix — this
        # pins that down so it can't quietly come back a third time.
        turtle = rows_to_turtle([_row()], "books")
        assert "reco:" not in turtle
        assert "@prefix reco" not in turtle

    def test_declares_the_gst_prefix_once(self):
        turtle = rows_to_turtle([_row()], "books")
        assert turtle.count("@prefix gst:") == 1
        assert "@prefix gst: <https://graph-owl.dev/packs/gst#>" in turtle

    def test_books_kind_mints_the_purchase_invoice_class(self):
        turtle = rows_to_turtle([_row()], "books")
        assert "a gst:PurchaseInvoice" in turtle
        assert "gst:Gstr2bInvoice" not in turtle

    def test_gstr2b_kind_mints_the_2b_invoice_class(self):
        turtle = rows_to_turtle([_row()], "gstr2b")
        assert "a gst:Gstr2bInvoice" in turtle
        assert "gst:PurchaseInvoice" not in turtle

    def test_unknown_kind_is_rejected(self):
        with pytest.raises(ValueError):
            rows_to_turtle([_row()], "not-a-real-kind")

    @pytest.mark.parametrize("blank", [None, "", float("nan")])
    def test_absent_field_is_omitted_not_written_as_a_blank_literal(self, blank):
        turtle = rows_to_turtle([_row(ims_status=blank, note_type=blank)], "books")
        assert "gst:imsStatus" not in turtle
        assert "gst:noteType" not in turtle
        assert '""' not in turtle

    @pytest.mark.parametrize("blank", [None, "", float("nan")])
    def test_absent_field_is_omitted_regardless_of_which_one(self, blank):
        # Every field goes through the same omission rule now that
        # there's only one predicate table — this pins that down for a
        # field that used to be gst:-mapped (supplierName) too, not just
        # the ones that used to be reco:-mapped.
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
        assert "gst:imsStatus" not in turtle

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
        assert turtle.count("a gst:PurchaseInvoice") == 1

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
        assert "gst:noteType" not in turtle

    def test_invoice_number_with_slashes_is_percent_encoded_in_the_subject(self):
        # Real GST invoice numbers commonly look like "INF/23-24/0456" —
        # an un-encoded "/" would silently change the IRI's path shape.
        turtle = rows_to_turtle([_row(invoice_no="INF/23-24/0456")], "books")
        subject_line = next(
            line for line in turtle.splitlines() if line.startswith("<https://graph-owl.dev/packs/gst#books-")
        )
        assert "INF/23-24/0456" not in subject_line
        assert "INF%2F23-24%2F0456" in subject_line

    def test_same_invoice_number_different_supplier_gives_distinct_subjects(self):
        rows = [
            _row(invoice_no="INV-9", supplier_gstin="27AAAFN2938K1Z2"),
            _row(invoice_no="INV-9", supplier_gstin="29AAECK4410L1Z7"),
        ]
        turtle = rows_to_turtle(rows, "books")
        subjects = [
            line for line in turtle.splitlines()
            if line.startswith("<https://graph-owl.dev/packs/gst#books-")
        ]
        assert len(subjects) == 2
        assert subjects[0] != subjects[1]


class TestCanonicalLinking:
    """packs/gst's finding queries (missing-in-gstr2b, amount-mismatch,
    tax-head-mismatch) don't join books-vs-2B by a shared literal key — they
    walk a real graph shape: a canonical subject linked to each per-source
    invoice via gst:recordedIn/gst:reflectedIn, and each per-source invoice
    linked to a real gst:Supplier subject via gst:issuedBy (confirmed by
    reading the queries in full, not assumed from field-level vocabulary
    compatibility). Without this, every one of packs/gst's finding queries
    would silently match zero of reco-now's subjects."""

    def test_supplier_subject_is_a_gst_supplier_with_its_gstin(self):
        turtle = rows_to_turtle([_row(supplier_gstin="27AAAFN2938K1Z2")], "books")
        assert "a gst:Supplier" in turtle
        assert 'gst:supplierGstin "27AAAFN2938K1Z2"' in turtle

    def test_supplier_gstin_lands_exactly_once_on_the_supplier_not_the_invoice(self):
        # It lives on the Supplier subject, reached via gst:issuedBy —
        # matching packs/gst's own queries (`?supplier gst:supplierGstin
        # ?gstin`, never `?purchase gst:supplierGstin ?gstin`).
        turtle = rows_to_turtle([_row()], "books")
        assert turtle.count("gst:supplierGstin") == 1

    def test_the_invoice_carries_an_issuedby_edge_to_the_supplier(self):
        turtle = rows_to_turtle([_row(supplier_gstin="27AAAFN2938K1Z2")], "books")
        assert "gst:issuedBy <https://graph-owl.dev/packs/gst#supplier-27AAAFN2938K1Z2>" in turtle

    def test_a_books_row_links_its_canonical_subject_via_recordedin(self):
        turtle = rows_to_turtle(
            [_row(invoice_no="INV-AUG-101", supplier_gstin="27AAAFN2938K1Z2")], "books"
        )
        assert (
            "<https://graph-owl.dev/packs/gst#invoice-27AAAFN2938K1Z2-INVAUG101>\n"
            "    gst:recordedIn <https://graph-owl.dev/packs/gst#books-27AAAFN2938K1Z2-INV-AUG-101> ."
        ) in turtle
        assert "gst:reflectedIn" not in turtle

    def test_a_gstr2b_row_links_its_canonical_subject_via_reflectedin(self):
        turtle = rows_to_turtle(
            [_row(invoice_no="INV-AUG-101", supplier_gstin="27AAAFN2938K1Z2")], "gstr2b"
        )
        assert (
            "<https://graph-owl.dev/packs/gst#invoice-27AAAFN2938K1Z2-INVAUG101>\n"
            "    gst:reflectedIn <https://graph-owl.dev/packs/gst#gstr2b-27AAAFN2938K1Z2-INV-AUG-101> ."
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
        # None of the 3 findings wired so far (PotentialMismatch,
        # AmountMismatch, TaxHeadMismatch) read gst:taxAmount off the
        # Gstr2bInvoice side — only itc-not-available/reverse-charge do,
        # and those are deferred (no itcAvailable field, and reverseCharge
        # values are "Yes"/"No" text, not the "R"/"N" codes those queries
        # filter on). Asserting a field nothing reads is speculative
        # scope, not a fix.
        turtle = rows_to_turtle([_row(igst=3960, cgst=0, sgst=0, cess=0)], "gstr2b")
        assert "gst:taxAmount" not in turtle

    def test_books_rows_carry_a_normalized_invoice_key(self):
        # packs/gst's keying-error guards (missing-in-gstr1.sparql,
        # missing-in-books.sparql) match "same invoice, any GSTIN" via
        # gst:invoiceKey — reused from reconciliation.normalize_invoice_no
        # rather than re-derived, so the two can never compute different
        # keys for what a human would call the same invoice number.
        turtle = rows_to_turtle([_row(invoice_no="inv-2024/001")], "books")
        assert 'gst:invoiceKey "INV2024001"' in turtle

    def test_the_canonical_subject_normalizes_the_invoice_number(self):
        # connectors/python/graph_owl_packs/gstr2b.py (the live-GSP
        # connector) already normalizes the invoice number before
        # building its canonical subject; this module didn't — found 16
        # August 2026 while extracting the shared gst_identity module.
        # Harmless while gstr2b.py was never called in production, but a
        # books upload for "INV-2024/001" and a live 2B pull for
        # "inv2024001" must land on the *same* canonical subject, or
        # every finding query joining through it silently matches nothing.
        turtle = rows_to_turtle(
            [_row(invoice_no="inv-2024/001", supplier_gstin="27X")], "books"
        )
        assert "<https://graph-owl.dev/packs/gst#invoice-27X-INV2024001>" in turtle

    def test_gstr2b_rows_also_carry_a_normalized_invoice_key(self):
        # missing-in-gstr1.sparql's own "GSTR-2B presence is conclusive
        # proof the supplier filed" guard reads gst:invoiceKey off the
        # Gstr2bInvoice side (`?availableIn2b a gst:Gstr2bInvoice ;
        # gst:invoiceKey ?key`) — its comment names the exact failure mode
        # of skipping this: "before this guard every 2B-matched invoice
        # with no 2A row was reported as one the supplier had never
        # filed." Omitting it here reintroduces precisely that bug for any
        # deployment with partial GSTR-1/2A coverage, which is the normal
        # case, not an edge one.
        turtle = rows_to_turtle([_row(invoice_no="inv-2024/001")], "gstr2b")
        assert 'gst:invoiceKey "INV2024001"' in turtle


def _gstr1_row(**overrides) -> dict:
    """A GSTR-2A/GSTR-1 declared-invoice row — packs/gst's ontology
    deliberately has no separate Gstr2aInvoice class (its own comment:
    "a revolving view over the same supplier-declared data"
    gst:Gstr1Invoice already carries), so a GSTR-2A upload ingests as
    gst:Gstr1Invoice, same as a GSTR-1 upload would."""
    base = {
        "invoice_no": "INV-AUG-113",
        "supplier_gstin": "29AAECK4410L1Z7",
        "supplier_name": "Kavya Cloud Systems LLP",
        "taxable": 180000,
        "invoice_date": "13-08-2026",
        "igst": 0,
        "cgst": 16200,
        "sgst": 16200,
        "cess": 0,
        "filed_date": "20-09-2026",
        "period": "082026",
    }
    base.update(overrides)
    return base


class TestGstr1Ingestion:
    """GSTR-2A/GSTR-1 support — closes the only_gstr2b/MissingInBooks gap
    from plans/119-architecture-audit.md §5c/§8 by giving the 4
    GSTR-1-anchored finding rules (already registered in packs/gst, never
    reachable before) the gst:Gstr1Invoice-shaped data they need."""

    def test_mints_the_gstr1_invoice_class(self):
        turtle = rows_to_turtle([_gstr1_row()], "gstr1")
        assert "a gst:Gstr1Invoice" in turtle
        assert "gst:PurchaseInvoice" not in turtle
        assert "gst:Gstr2bInvoice" not in turtle

    def test_carries_a_normalized_invoice_key(self):
        turtle = rows_to_turtle([_gstr1_row(invoice_no="inv-2024/001")], "gstr1")
        assert 'gst:invoiceKey "INV2024001"' in turtle

    def test_carries_a_combined_tax_amount(self):
        # missing-in-books.sparql reads gst:taxAmount off the declared
        # (Gstr1Invoice) side too, not just the books side.
        turtle = rows_to_turtle([_gstr1_row(igst=0, cgst=16200, sgst=16200, cess=0)], "gstr1")
        assert 'gst:taxAmount "32400"' in turtle

    def test_the_invoice_carries_an_issuedby_edge_to_the_supplier(self):
        turtle = rows_to_turtle([_gstr1_row(supplier_gstin="29AAECK4410L1Z7")], "gstr1")
        assert "gst:issuedBy <https://graph-owl.dev/packs/gst#supplier-29AAECK4410L1Z7>" in turtle

    def test_links_its_canonical_subject_via_appearsin_not_recordedin_or_reflectedin(self):
        turtle = rows_to_turtle(
            [_gstr1_row(invoice_no="INV-AUG-113", supplier_gstin="29AAECK4410L1Z7")], "gstr1"
        )
        assert (
            "<https://graph-owl.dev/packs/gst#invoice-29AAECK4410L1Z7-INVAUG113>\n"
            "    gst:appearsIn <https://graph-owl.dev/packs/gst#gstr1-29AAECK4410L1Z7-INV-AUG-113> ."
        ) in turtle
        assert "gst:recordedIn" not in turtle
        assert "gst:reflectedIn" not in turtle

    def test_the_same_invoice_shares_its_canonical_subject_with_the_books_side(self):
        # The whole point of the canonical link: a books upload and a
        # gstr1 upload for the same (gstin, invoice_no) must agree on
        # which subject is "this invoice", or gst:recordedIn and
        # gst:appearsIn would never meet.
        books = rows_to_turtle(
            [_row(invoice_no="INV-9", supplier_gstin="29AAECK4410L1Z7")], "books"
        )
        gstr1 = rows_to_turtle(
            [_gstr1_row(invoice_no="INV-9", supplier_gstin="29AAECK4410L1Z7")], "gstr1"
        )
        books_canonical = next(l for l in books.splitlines() if l.startswith("<") and "invoice-" in l)
        gstr1_canonical = next(l for l in gstr1.splitlines() if l.startswith("<") and "invoice-" in l)
        assert books_canonical == gstr1_canonical

    def test_links_to_a_filing_subject_carrying_filed_date_and_period(self):
        turtle = rows_to_turtle(
            [_gstr1_row(supplier_gstin="29AAECK4410L1Z7", filed_date="20-09-2026", period="082026")],
            "gstr1",
        )
        filing_iri = "https://graph-owl.dev/packs/gst#filing-29AAECK4410L1Z7-082026"
        assert f"gst:filedIn <{filing_iri}>" in turtle
        assert f"<{filing_iri}>\n    a gst:Gstr1Filing" in turtle
        # filedDate goes through the same DD-MM-YYYY -> ISO normalization
        # invoiceDate does, for the identical reason (law-provision date
        # comparisons are plain-string lexicographic).
        assert 'gst:filedDate "2026-09-20"' in turtle
        assert 'gst:period "082026"' in turtle

    def test_two_rows_from_the_same_supplier_and_period_share_one_filing_subject(self):
        rows = [
            _gstr1_row(invoice_no="INV-A", supplier_gstin="29AAECK4410L1Z7", period="082026"),
            _gstr1_row(invoice_no="INV-B", supplier_gstin="29AAECK4410L1Z7", period="082026"),
        ]
        turtle = rows_to_turtle(rows, "gstr1")
        assert turtle.count("a gst:Gstr1Filing") == 1


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
            result = import_document(url, "gst-books-upload-1", "@prefix gst: <x> .\n")
        assert len(received) == 1
        call = received[0]
        assert call["path"] == "/graph/import/rdf"
        assert call["query"]["source"] == "gst-books-upload-1"
        assert call["query"]["format"] == "turtle"
        assert call["raw"] == b"@prefix gst: <x> .\n"
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
    def test_gets_findings_scoped_to_the_gst_pack(self):
        received: list[dict] = []
        sample = [{"id": "f1", "label": "gst:AmountMismatch", "subject": "x"}]
        with _server(received, findings_response=sample) as url:
            result = list_findings(url)
        assert received[0]["path"] == "/findings"
        assert received[0]["query"]["pack"] == "gst"
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
