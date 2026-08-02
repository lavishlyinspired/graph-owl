"""The extraction wire contract — Epic 21.

These are cross-language tests wearing Python clothes. Every assertion here has
a counterpart in the Rust suite, and the ones that matter most are the ones
where the two languages could disagree without either being obviously wrong.
"""

from __future__ import annotations

import hashlib
import json
from datetime import datetime, timezone

from graph_owl_sdk.extraction import (
    ASSERT_THRESHOLD,
    CATALOG_PREDICATES,
    SURFACE_THRESHOLD,
    Claim,
    ExtractionResult,
    ParsedDocument,
    Provenance,
    Section,
    TextSpan,
    decide,
    review_queue,
    submit_extraction,
)


class FakeClient:
    def __init__(self, answer=None):
        self.sent = []
        self.answer = answer if answer is not None else {"outcome": "recorded"}

    def request(self, method, path, body=None, *args, **kwargs):
        self.sent.append((method, path, body))
        return self.answer


def document(text="The orders service is append-only.") -> ParsedDocument:
    return ParsedDocument(source_id="runbook.md", media_type="markdown", text=text)


def claim(confidence=0.6) -> Claim:
    return Claim(
        subject="prod.orders",
        predicate="description",
        object="append-only",
        confidence=confidence,
        provenance=Provenance(
            source_id="runbook.md",
            extractor="pdf-worker",
            extractor_version="1",
            evidence=TextSpan(4, 18),
            extracted_at=datetime(2026, 8, 2, tzinfo=timezone.utc),
        ),
    )


# ── the fingerprint must agree with Rust, or idempotence never fires ───────


def test_the_fingerprint_is_sha256_of_the_utf8_text():
    """**The one value both languages compute independently.**

    Rust's ``content_fingerprint`` is ``sha256(bytes)`` rendered lowercase hex.
    If Python disagreed — by hashing the wire JSON, or by using a different
    encoding — every re-submission would look like a new document, idempotence
    would silently never fire, and a PDF worker would re-run OCR over an
    unchanged corpus forever while reporting success.
    """
    text = "The orders service is append-only."

    assert (
        document(text).fingerprint()
        == hashlib.sha256(text.encode("utf-8")).hexdigest()
    )


def test_the_fingerprint_follows_content_not_identity():
    same = ParsedDocument("a.md", "markdown", "identical")
    other = ParsedDocument("b.pdf", "application/pdf", "identical")

    assert same.fingerprint() == other.fingerprint()


def test_a_single_byte_edit_changes_the_fingerprint():
    """A fingerprint that missed small edits would skip re-extraction on exactly
    the corrections people make."""
    assert document("append-only").fingerprint() != document("append-ony").fingerprint()


def test_a_non_ascii_document_fingerprints_over_its_utf8_bytes():
    """The case where a plausible alternative (hashing the str, or latin-1) gives
    a different answer than Rust."""
    text = "café serves crêpes"

    assert (
        document(text).fingerprint()
        == hashlib.sha256(text.encode("utf-8")).hexdigest()
    )


# ── spans are byte offsets, which is where the two languages diverge ───────


def test_a_span_resolves_over_bytes_not_characters():
    """**Python indexes strings by character and Rust by byte.** A span computed
    character-wise would point at the wrong words in any document containing an
    accent — silently, and only in the documents most likely to be interesting.
    """
    text = "café is fine"

    # 'café' is five bytes, so the byte span 0..5 is the whole word.
    assert TextSpan(0, 5).resolve(text) == "café"
    # Character-wise, 0..4 would also be 'café'. Byte-wise it splits the é.
    assert TextSpan(0, 4).resolve(text) is None


def test_a_span_past_the_end_resolves_to_none_rather_than_raising():
    """A worker that miscounts must not crash the caller — it gets a None and
    can report the claim it could not evidence."""
    assert TextSpan(0, 500).resolve("short") is None


def test_an_inverted_span_resolves_to_none():
    assert TextSpan(9, 2).resolve("some text") is None


# ── the wire shape ─────────────────────────────────────────────────────────


def test_every_key_on_the_wire_is_camel_case():
    """The server rejects unknown fields, so a snake_case key is a 400 rather
    than a silently ignored value. That has bitten this project twice."""
    body = claim().wire()

    assert set(body) == {"subject", "predicate", "object", "confidence", "provenance"}
    assert set(body["provenance"]) == {
        "sourceId",
        "extractor",
        "extractorVersion",
        "extractedAt",
        "evidence",
    }
    assert set(document().wire()) == {"sourceId", "mediaType", "text"}


def test_sections_are_omitted_when_there_are_none():
    """The Rust side skips serializing an empty section list, and a parser that
    recovers no structure is not degraded — merely less specific."""
    assert "sections" not in document().wire()


def test_sections_are_sent_when_a_parser_recovered_them():
    parsed = ParsedDocument(
        "r.md", "markdown", "# H\nbody", sections=[Section("H", TextSpan(0, 8))]
    )

    body = parsed.wire()

    assert body["sections"] == [{"heading": "H", "span": {"start": 0, "end": 8}}]


def test_a_timestamp_is_sent_with_an_explicit_offset():
    """A naive datetime serializes without one and is rejected — a confusing
    failure to debug from the far side of a process boundary."""
    sent = claim().wire()["provenance"]["extractedAt"]

    assert sent.endswith("+00:00"), sent


def test_the_whole_submission_is_json_serialisable():
    body = ExtractionResult(claims=[claim()]).wire()

    json.dumps(body)


# ── the client calls ───────────────────────────────────────────────────────


def test_submit_posts_the_document_and_the_result_together():
    """The worker is the only party that read the source, so the parsed text
    travels with the claims — every evidence span is an offset into it."""
    client = FakeClient()

    submit_extraction(client, document(), ExtractionResult([claim()]), "pdf-worker", "1")

    method, path, body = client.sent[0]
    assert (method, path) == ("POST", "/extraction/runs")
    assert body["extractor"] == "pdf-worker"
    assert body["extractorVersion"] == "1"
    assert body["document"]["text"] == document().text


def test_the_review_queue_survives_being_an_array():
    """``_send`` narrows to a dict, which would turn the queue into ``{}`` — and
    an empty queue looks exactly like "nothing is waiting for you"."""
    client = FakeClient(answer=[{"id": "c1", "evidence": "the orders service"}])

    assert review_queue(client) == [{"id": "c1", "evidence": "the orders service"}]


def test_an_unexpected_queue_shape_yields_nothing_rather_than_crashing():
    assert review_queue(FakeClient(answer={"error": "nope"})) == []


def test_a_decision_always_states_its_verdict():
    """No default in either direction: true asserts what nobody approved, false
    rejects what nobody refused."""
    client = FakeClient()

    decide(client, "c1", confirmed=False)

    _, path, body = client.sent[0]
    assert path == "/extraction/claims/c1/decision"
    assert body == {"confirmed": False}


# ── the policy is documented here and enforced there ───────────────────────


def test_the_bands_match_the_ones_graph_owl_applies():
    """Mirrored for a worker's convenience, never as the decision. If these ever
    disagree with the server the server wins — which is why nothing in this SDK
    branches on them."""
    assert ASSERT_THRESHOLD == 0.8
    assert SURFACE_THRESHOLD == 0.5


def test_the_mirrored_vocabulary_holds_the_catalog_predicates():
    assert "description" in CATALOG_PREDICATES
    assert "isFriendsWith" not in CATALOG_PREDICATES
