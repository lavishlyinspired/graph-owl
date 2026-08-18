"""A reconciliation reads its own period's graphs and no others.

Found by running two periods: an April period holding only a payment ledger
reported `gst:GoodsReceiptTiming` as **passed**, because *March* had supplied
goods-receipt data. That is the "checked, clean" lie the three-state rule
outcome exists to prevent, reintroduced one level up — and it reported 38
findings that were mostly another period's.

Ingest was always scoped (one named graph per client, period and kind).
Evaluation was not.
"""

from __future__ import annotations

from app.main import PACK_GRAPHS, period_source_name, reconcile_scope

CLIENT = "418d04ba-b8dd-4080-befe-f00cbb604a27"
MARCH = "b52c73c0-0c16-4394-8739-eb856dd520ba"
APRIL = "5e50e0e6-ccab-4cc9-886d-2637f034fb03"


def test_two_periods_of_one_client_never_share_a_graph():
    march = set(reconcile_scope(CLIENT, MARCH, ["books", "gstr2b"]))
    april = set(reconcile_scope(CLIENT, APRIL, ["books", "gstr2b"]))

    assert march & april == set(PACK_GRAPHS), (
        "the only graphs two periods may share are the pack's own vocabulary and law"
    )


def test_two_clients_never_share_a_graph():
    other = "29aaaaa0-0000-0000-0000-000000000000"
    a = set(reconcile_scope(CLIENT, MARCH, ["books"]))
    b = set(reconcile_scope(other, MARCH, ["books"]))

    assert a & b == set(PACK_GRAPHS)


def test_the_scope_names_every_uploaded_kind():
    scope = reconcile_scope(CLIENT, MARCH, ["books", "gstr2b", "payments", "grn"])

    assert len([g for g in scope if g not in PACK_GRAPHS]) == 4


def test_a_kind_that_was_not_uploaded_is_not_in_scope():
    """The scope is what this period *has*, not what it could have. Naming an
    absent kind would put an empty graph in scope and change nothing — but it
    would also mean the scope no longer describes the run."""
    scope = reconcile_scope(CLIENT, MARCH, ["books", "gstr2b"])

    assert not any("grn" in g for g in scope)
    assert not any("payments" in g for g in scope)


def test_the_pack_is_always_in_scope():
    """Rule 36(4)'s cap lives in the pack's law graph. A run that could not
    read it would stop finding amount mismatches altogether — the scope must
    narrow period data without hiding the law."""
    for kinds in ([], ["books"], ["books", "gstr2b", "payments", "grn"]):
        assert set(PACK_GRAPHS) <= set(reconcile_scope(CLIENT, MARCH, kinds))


def test_the_source_name_matches_what_the_ingest_writes():
    """One construction, used by both. Two would drift, and the symptom would
    be a reconciliation scoped to graphs that do not exist — every rule
    silently not-evaluated."""
    import hashlib

    expected_hash = hashlib.sha256(f"{CLIENT}:{MARCH}:books".encode()).hexdigest()[:12]

    assert period_source_name(CLIENT, MARCH, "books") == f"reco-{expected_hash}-books"


def _finding(invoice, label="gst:ITCNotAvailable", gstin=None):
    """The shape `list_findings` actually returns: the invoice number and the
    supplier are **evidence bindings**, not top-level fields. Writing these
    tests against invented top-level keys is what let the first implementation
    read `finding["invoice_no"]`, get None for every finding, and drop them
    all while the tests passed."""
    evidence = [{"predicate": "gst:invoiceNumber", "value": invoice, "var": "number"}]
    if gstin is not None:
        evidence.insert(0, {"predicate": "gst:supplierGstin", "value": gstin, "var": "gstin"})
    return {"label": label, "subject": f"urn:{invoice}", "summary": "", "evidence": evidence}


class TestFindingsBelongToTheirPeriod:
    """Rule outcomes were scoped before the findings they came from were.

    April, whose GSTR-2B carries no ITC-eligibility column at all, reported
    `gst:ITCNotAvailable` as NOT EVALUATED while simultaneously reporting
    89,800 of blocked ITC, which was March's.
    """

    def test_a_finding_for_an_invoice_this_period_does_not_have_is_dropped(self):
        from app.main import findings_for_period

        assert findings_for_period(
            [_finding("INV-MAR-006", gstin="19AABCP8087C1ZV")],
            {("27AABCS1429B1Z8", "INVAPR001")},
        ) == []

    def test_a_finding_for_an_invoice_this_period_has_is_kept(self):
        from app.main import findings_for_period

        kept = findings_for_period(
            [_finding("INV-MAR-006", gstin="19AABCP8087C1ZV")],
            {("19AABCP8087C1ZV", "INVMAR006")},
        )
        assert len(kept) == 1

    def test_matching_uses_the_same_normalised_key_as_everything_else(self):
        from app.main import findings_for_period

        kept = findings_for_period(
            [_finding("INV/2026/1", gstin="27AABCS1429B1Z8")],
            {("27AABCS1429B1Z8", "INV20261")},
        )
        assert len(kept) == 1

    def test_a_finding_with_no_gstin_matches_on_the_invoice_alone(self):
        """`gst:ITCNotAvailable` used to bind no supplier. Dropping such a
        finding for lack of a GSTIN would lose exactly the blocked-credit
        cases this whole area exists to surface."""
        from app.main import findings_for_period

        kept = findings_for_period([_finding("INV-MAR-006")], {("19AABCP8087C1ZV", "INVMAR006")})
        assert len(kept) == 1

    def test_a_finding_naming_another_supplier_is_dropped(self):
        """Two suppliers can share an invoice number. Keeping a finding whose
        supplier this period does not have would attach one client's blocked
        credit to another's invoice."""
        from app.main import findings_for_period

        assert findings_for_period(
            [_finding("001", gstin="29AACCS9460D1Z4")], {("27AABCS1429B1Z8", "001")}
        ) == []

    def test_an_empty_period_keeps_nothing(self):
        from app.main import findings_for_period

        assert findings_for_period([_finding("INV-1")], set()) == []
