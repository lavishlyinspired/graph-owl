"""Slice B RED: rendering graph context to text plus metadata.

Decision 4 is the one property that matters most here: a derived fact must
be identifiable in the *text* the model reads, not only in metadata — an
LLM handed an inference as though it were an assertion states it as fact.
"""

from graph_owl_langchain._core.rendering import (
    IGNORE_BAND_CEILING,
    GraphContext,
    RelatedFact,
    render,
    visible_facts,
)


def test_page_content_names_the_asset():
    context = GraphContext(
        fully_qualified_name="warehouse.retail.orders",
        kind="table",
        description="Daily order totals.",
    )
    text, _ = render(context)
    assert "warehouse.retail.orders" in text
    assert "Daily order totals." in text


def test_a_derived_fact_is_labelled_inferred_in_the_text_itself():
    context = GraphContext(
        fully_qualified_name="warehouse.retail.orders",
        kind="table",
        description=None,
        facts=[RelatedFact(text="feeds warehouse.retail.revenue", derived=True)],
    )
    text, _ = render(context)
    # Exact line, not `in`: a substring check is satisfied by mutmut's own
    # "wrap the literal" string mutation, which would still contain
    # "[inferred]" as a substring of "XX[inferred] XX".
    assert "[inferred] feeds warehouse.retail.revenue" in text.splitlines()


def test_an_asserted_fact_is_not_labelled_inferred():
    context = GraphContext(
        fully_qualified_name="warehouse.retail.orders",
        kind="table",
        description=None,
        facts=[RelatedFact(text="feeds warehouse.retail.revenue", derived=False)],
    )
    text, _ = render(context)
    assert "[inferred]" not in text


def test_metadata_carries_the_structured_fields_text_does_not_have_to():
    context = GraphContext(
        fully_qualified_name="warehouse.retail.orders",
        kind="table",
        description=None,
        as_of="2026-01-01T00:00:00Z",
        facts=[
            RelatedFact(
                text="feeds revenue",
                derived=True,
                confidence=0.9,
                relationship="feeds",
                source="connector:snowflake",
            )
        ],
    )
    _, metadata = render(context)
    assert metadata["fullyQualifiedName"] == "warehouse.retail.orders"
    assert metadata["asOf"] == "2026-01-01T00:00:00Z"
    assert metadata["facts"][0] == {
        "text": "feeds revenue",
        "derived": True,
        "confidence": 0.9,
        "relationship": "feeds",
        "source": "connector:snowflake",
    }


def test_a_truncated_context_states_it_in_the_text_not_only_a_flag():
    """A budget-truncated context that reads as complete makes the model
    assert absence it never verified — the truncation notice belongs in
    page_content, mirroring decision 4's reasoning for `derived`."""
    context = GraphContext(
        fully_qualified_name="warehouse.retail.orders",
        kind="table",
        description=None,
        truncated=True,
    )
    text, metadata = render(context)
    assert (
        "[truncated] Not all related facts fit the response budget; "
        "this is a partial picture, not the complete one." in text.splitlines()
    )
    assert metadata["truncated"] is True


def test_an_untruncated_context_says_nothing_about_truncation():
    context = GraphContext(
        fully_qualified_name="warehouse.retail.orders", kind="table", description=None
    )
    text, _ = render(context)
    assert "truncated" not in text.lower()


def test_visible_facts_excludes_derived_facts_by_default():
    # The excluded fact sits in the middle, not last: a `continue` mutated
    # to `break` would silently drop everything after it too, and a fact
    # list with nothing following the exclusion could never catch that.
    facts = [
        RelatedFact(text="asserted-1", derived=False),
        RelatedFact(text="inferred", derived=True),
        RelatedFact(text="asserted-2", derived=False),
    ]
    kept = visible_facts(facts, include_derived=False, min_confidence=0.0)
    assert [f.text for f in kept] == ["asserted-1", "asserted-2"]


def test_visible_facts_includes_derived_facts_when_asked():
    facts = [
        RelatedFact(text="asserted", derived=False),
        RelatedFact(text="inferred", derived=True),
    ]
    kept = visible_facts(facts, include_derived=True, min_confidence=0.0)
    assert {f.text for f in kept} == {"asserted", "inferred"}


def test_visible_facts_excludes_below_the_ignore_band():
    # Same middle-of-the-list placement as the derived test, for the same
    # `continue`-vs-`break` reason.
    facts = [
        RelatedFact(text="strong-1", derived=False, confidence=0.9),
        RelatedFact(text="weak", derived=False, confidence=0.1),
        RelatedFact(text="strong-2", derived=False, confidence=0.9),
    ]
    kept = visible_facts(facts, include_derived=True, min_confidence=IGNORE_BAND_CEILING)
    assert [f.text for f in kept] == ["strong-1", "strong-2"]


def test_visible_facts_keeps_a_fact_landing_exactly_on_the_confidence_floor():
    """`<` vs `<=` at the floor is invisible unless something lands exactly
    on it — only strictly-below is excluded."""
    facts = [RelatedFact(text="exactly-at-floor", derived=False, confidence=IGNORE_BAND_CEILING)]
    kept = visible_facts(facts, include_derived=True, min_confidence=IGNORE_BAND_CEILING)
    assert [f.text for f in kept] == ["exactly-at-floor"]


def test_visible_facts_never_excludes_a_fact_with_no_confidence_at_all():
    """A lineage edge is not a memory and has no confidence band — the floor
    is a memory-band concept and must not silently drop plain edges."""
    facts = [RelatedFact(text="feeds revenue", derived=False, confidence=None)]
    kept = visible_facts(facts, include_derived=True, min_confidence=IGNORE_BAND_CEILING)
    assert [f.text for f in kept] == ["feeds revenue"]
