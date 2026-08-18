"""A case must be able to show the facts it rests on, not just a count.

Reco Now stored `evidence_count` and displayed "4 fact(s) cited", which
tells a reviewer that evidence exists without letting them see it. The
facts live in graph-owl, keyed by the finding's subject IRI, and that is
the whole point of the integration: a case that cannot show its evidence
is an assertion.

`case_from_finding` is pure, so the shaping is tested against the captured
real finding output rather than a running server.
"""

from __future__ import annotations

import json
import pathlib

from app.main import evidence_for_subject


FINDINGS = json.loads((pathlib.Path(__file__).parent / "fixtures" / "real_findings.json").read_text())

AMOUNT_SUBJECT = "https://graph-owl.dev/packs/gst#books-27AABCS1429B1Z8-INV-MAR-011"


def test_the_facts_behind_a_case_are_returned_with_their_predicates():
    facts = evidence_for_subject(FINDINGS, AMOUNT_SUBJECT, "gst:AmountMismatch")

    by_predicate = {f["predicate"]: f["value"] for f in facts}
    assert by_predicate["gst:taxableValue"] in {"180000.0", "180500"}
    assert by_predicate["gst:supplierGstin"] == "27AABCS1429B1Z8"
    assert by_predicate["gst:invoiceNumber"] == "INV-MAR-011"


def test_the_statutory_citation_is_among_the_facts():
    """Rule 36(4)'s cap is read from the graph, and the provision that set it
    is cited in the evidence. A reviewer defending a number needs that."""
    facts = evidence_for_subject(FINDINGS, AMOUNT_SUBJECT, "gst:AmountMismatch")

    citations = [f["value"] for f in facts if f["predicate"] == "gst:citation"]
    assert citations == ["Notification 40/2021-CT"]


def test_each_fact_names_the_variable_the_rule_bound_it_to():
    """`claimed` vs `filed` is what makes two identical predicates readable as
    two sides of a comparison rather than a duplicate."""
    facts = evidence_for_subject(FINDINGS, AMOUNT_SUBJECT, "gst:AmountMismatch")

    variables = {f["var"] for f in facts}
    assert {"claimed", "filed"} <= variables


def test_the_right_finding_is_chosen_when_one_subject_has_two():
    """INV-MAR-011 carries both an AmountMismatch and a TaxHeadMismatch on the
    same subject. Asking for one must not return the other's facts."""
    tax_head = evidence_for_subject(FINDINGS, AMOUNT_SUBJECT, "gst:TaxHeadMismatch")

    variables = {f["var"] for f in tax_head}
    assert "bookedIgst" in variables
    assert "claimed" not in variables


def test_an_unknown_subject_yields_no_facts_rather_than_someone_elses():
    assert evidence_for_subject(FINDINGS, "https://example.test/nope", "gst:AmountMismatch") == []
