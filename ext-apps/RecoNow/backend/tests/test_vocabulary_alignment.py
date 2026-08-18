"""A confirmed column mapping becomes a durable alignment — Plan 123 Slice G.

Every client's ERP calls things something different. Today that is a per-file
column mapping: made at upload, used once, thrown away. The next period, and
the next file, someone maps `Party Code → supplier GSTIN` again.

As an **alignment** it is a different kind of object — durable, reusable and
**inspectable**: "this client's `Party Code` *is* `gst:supplierGstin`" becomes
a fact in the graph with a source and a confidence, which a reviewer can
disagree with and a later upload can reuse. `graph-owl-ontology` already ships
the machinery (`MatchPredicate`, `AlignmentSource`, `Alignment`) and nothing
used it.

**Confidence is where the care goes.** A mapping a human confirmed is not the
same claim as one `_auto_map` guessed from a header string, and recording both
at 1.0 would destroy the distinction exactly where it matters — an automated
guess that silently becomes a durable, reused fact is how one bad header
propagates to every future period.
"""

from __future__ import annotations

import pytest

from app.vocabulary import alignment_requests

HEADERS = ["Party Code", "Bill Number", "Net Value", "Unmapped Column"]
MAPPING = {"supplier_gstin": 0, "invoice_no": 1, "taxable": 2}


def _by_left(requests: list[dict]) -> dict[str, dict]:
    return {r["left"]: r for r in requests}


class TestWhatIsAligned:
    def test_each_mapped_column_becomes_one_alignment(self):
        requests = alignment_requests(
            client_id="c-1", headers=HEADERS, mapping=MAPPING, confirmed_by_human=True
        )

        assert len(requests) == 3

    def test_an_alignment_points_the_client_term_at_the_pack_term(self):
        requests = _by_left(
            alignment_requests(
                client_id="c-1", headers=HEADERS, mapping=MAPPING, confirmed_by_human=True
            )
        )

        party = next(r for left, r in requests.items() if "Party" in left)
        assert party["right"].endswith("supplierGstin")

    def test_the_client_term_is_scoped_to_its_own_client(self):
        """Two clients may both call a column `Party Code` and mean different
        things. An unscoped term would make one client's vocabulary silently
        redefine another's."""
        one = alignment_requests(
            client_id="c-1", headers=HEADERS, mapping=MAPPING, confirmed_by_human=True
        )
        two = alignment_requests(
            client_id="c-2", headers=HEADERS, mapping=MAPPING, confirmed_by_human=True
        )

        assert {r["left"] for r in one}.isdisjoint({r["left"] for r in two})

    def test_an_unmapped_column_produces_no_alignment(self):
        """A column nobody mapped is not a claim about anything."""
        requests = alignment_requests(
            client_id="c-1", headers=HEADERS, mapping=MAPPING, confirmed_by_human=True
        )

        assert not any("Unmapped" in r["left"] for r in requests)

    def test_a_mapping_pointing_past_the_headers_is_skipped_not_crashed(self):
        """A stale template against a changed file. Losing one alignment is
        recoverable; failing the upload is not."""
        requests = alignment_requests(
            client_id="c-1",
            headers=["Only One"],
            mapping={"supplier_gstin": 0, "invoice_no": 7},
            confirmed_by_human=True,
        )

        assert len(requests) == 1


class TestConfidenceRecordsWhoDecided:
    def test_a_human_confirmed_mapping_is_curated_at_full_confidence(self):
        requests = alignment_requests(
            client_id="c-1", headers=HEADERS, mapping=MAPPING, confirmed_by_human=True
        )

        assert all(r["confidence"] == 1.0 for r in requests)
        assert all(r["source"]["kind"] == "human" for r in requests)

    def test_an_auto_mapping_is_computed_and_below_the_assert_threshold(self):
        """graph-owl asserts a direct triple only at >= 0.8 and puts anything
        in 0.5..0.8 in the review band. An automated header guess belongs in
        that band: durable enough to reuse, not so durable that nobody looks
        at it."""
        requests = alignment_requests(
            client_id="c-1", headers=HEADERS, mapping=MAPPING, confirmed_by_human=False
        )

        assert all(r["source"]["kind"] == "computed" for r in requests)
        assert all(0.5 <= r["confidence"] < 0.8 for r in requests)

    def test_the_two_are_never_recorded_identically(self):
        """The distinction is the point. A guess that becomes indistinguishable
        from a confirmation is how one bad header propagates to every period."""
        human = alignment_requests(
            client_id="c-1", headers=HEADERS, mapping=MAPPING, confirmed_by_human=True
        )
        auto = alignment_requests(
            client_id="c-1", headers=HEADERS, mapping=MAPPING, confirmed_by_human=False
        )

        assert human[0]["confidence"] != auto[0]["confidence"]
        assert human[0]["source"]["kind"] != auto[0]["source"]["kind"]


class TestTheShapeGraphOwlAccepts:
    def test_every_request_carries_the_fields_the_endpoint_requires(self):
        requests = alignment_requests(
            client_id="c-1", headers=HEADERS, mapping=MAPPING, confirmed_by_human=True
        )

        for request in requests:
            assert request["kind"] == "match"
            assert request["predicate"], request
            assert isinstance(request["confidence"], float)
            assert "kind" in request["source"]

    def test_a_column_mapping_is_a_close_match_not_an_exact_one(self):
        """`Party Code` holds a GSTIN *in this client's files*; it is not the
        same concept as `gst:supplierGstin` universally. `exactMatch` is
        transitive under SKOS, so overclaiming here would let two clients'
        unrelated columns become equivalent through the pack term."""
        requests = alignment_requests(
            client_id="c-1", headers=HEADERS, mapping=MAPPING, confirmed_by_human=True
        )

        assert all(r["predicate"].endswith("closeMatch") for r in requests)
