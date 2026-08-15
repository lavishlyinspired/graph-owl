"""gst_identity: the GST subject-encoding rules shared by every module that
mints packs/gst subjects — gstr2b.py (the live-GSP connector) and reco-now's
graphowl_client.py (ext-apps/Reco). Extracted 16 August 2026 after they were
found to have drifted: graphowl_client.py's canonical subject used the raw
invoice number where gstr2b.py already normalized it first — harmless while
nothing called gstr2b.py, but would have silently split one real invoice
into two canonical subjects the moment both modules wrote to the same
store."""

from __future__ import annotations

from graph_owl_packs.gst_identity import (
    canonical_local_name,
    invoice_key,
    subject_suffix,
    supplier_local_name,
    turtle_literal,
)


class TestInvoiceKey:
    def test_strips_case_and_punctuation(self):
        assert invoice_key("inv-2024/001") == "INV2024001"

    def test_preserves_leading_zeros(self):
        # INV-001 and INV-1 are different invoices in plenty of numbering
        # schemes — stripping leading zeros risks matching the wrong one.
        assert invoice_key("INV-001") == "INV001"
        assert invoice_key("INV-1") == "INV1"

    def test_transliterates_accented_characters_instead_of_deleting_them(self):
        # A plain [^A-Z0-9] strip on the un-normalized string would delete
        # "É" outright rather than reduce it to "E" — a silently different,
        # wrong key for the same human-readable invoice number.
        assert invoice_key("INVÉ-001") == "INVE001"

    def test_none_is_empty(self):
        assert invoice_key(None) == ""


class TestSubjectSuffix:
    def test_alphanumeric_passes_through(self):
        assert subject_suffix("27AAAFN2938K1Z2") == "27AAAFN2938K1Z2"

    def test_a_slash_is_percent_encoded_not_substituted(self):
        # Substituting "/" with "-" would make "INV/1" and "INV-1" collide
        # into one subject — two different invoices merged silently.
        assert subject_suffix("INV/1") == "INV%2F1"
        assert subject_suffix("INV-1") == "INV-1"


class TestTurtleLiteral:
    def test_escapes_backslash_before_quote(self):
        # Escaping the quote first would double-escape the backslash this
        # test introduces before it.
        assert turtle_literal('a\\"b') == 'a\\\\\\"b'

    def test_escapes_newline_and_tab(self):
        assert turtle_literal("a\nb\tc") == "a\\nb\\tc"


class TestCanonicalAndSupplierNames:
    def test_canonical_local_name_normalizes_the_invoice_number(self):
        assert canonical_local_name("29AAECK4410L1Z7", "inv-2024/001") == (
            "invoice-29AAECK4410L1Z7-INV2024001"
        )

    def test_supplier_local_name(self):
        assert supplier_local_name("27AAAFN2938K1Z2") == "supplier-27AAAFN2938K1Z2"

    def test_same_invoice_different_punctuation_shares_one_canonical_name(self):
        # The whole reason invoice_key exists: a books upload and a live
        # GSP pull writing the "same" invoice under differently-punctuated
        # numbers must still land on one canonical subject.
        a = canonical_local_name("27X", "INV-2024/001")
        b = canonical_local_name("27X", "inv2024001")
        assert a == b
