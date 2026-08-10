"""The worker's behaviour, tested against a recording client.

No server here on purpose: what these tests pin is what the worker *sends*, and
the wire shape is the contract. The Rust integration suite proves the server
honours it; this proves the worker speaks it.
"""

from __future__ import annotations

import json
from pathlib import Path

import pytest
from graph_owl_worker import (
    MENTION_CONFIDENCE,
    MentionExtractor,
    ParserRegistry,
    TextParser,
    UnsupportedMediaType,
    Worker,
    sentences,
)
from graph_owl_worker.worker import catalog_subjects


class RecordingClient:
    """Captures what it was asked to send, and answers as graph-owl would."""

    def __init__(self, answers: list | None = None) -> None:
        self.sent: list[tuple[str, str, object]] = []
        self._answers = answers or []

    def request(self, method, path, body=None, *args, **kwargs):
        self.sent.append((method, path, body))
        if self._answers:
            return self._answers.pop(0)
        return {"outcome": "recorded", "runId": "r1", "asserted": 0, "surfaced": 1}


RUNBOOK = """\
# Orders service

The orders service is append-only and owned by payments.

## Rollback

Revert the migration, then restart.
"""


def write(tmp_path: Path, name: str, body: str) -> Path:
    path = tmp_path / name
    path.write_text(body, encoding="utf-8")
    return path


# ── the sentence splitter, and the bug that made it silent ──────────────────


def test_a_fully_qualified_name_does_not_end_a_sentence():
    """**The failure this rule exists for was total and silent.**

    Splitting on every period tears ``svc.db.orders`` into three fragments, so
    no sentence ever contains the subject and the extractor returns *nothing* —
    which reads exactly like a document that mentions nothing.
    """
    text = "The svc.db.orders table is append-only. It is fine."

    spans = sentences(text)

    assert len(spans) == 2, [text.encode()[s:e] for s, e in spans]
    first = text.encode()[spans[0][0] : spans[0][1]].decode()
    assert "svc.db.orders" in first, first


def test_a_period_followed_by_whitespace_does_end_a_sentence():
    """The negative half. A splitter that never split would pass the test above
    and be equally useless — every claim's evidence would be the whole document.
    """
    assert len(sentences("One. Two. Three.")) == 3


def test_the_final_sentence_without_a_terminator_is_still_a_sentence():
    """Real documents end without punctuation more often than not."""
    spans = sentences("A complete thought. An unterminated one")

    assert len(spans) == 2
    assert spans[-1][1] == len("A complete thought. An unterminated one")


# ── parsing ────────────────────────────────────────────────────────────────


def test_headings_become_sections_that_resolve_against_the_text():
    parsed = TextParser().parse("r.md", "markdown", RUNBOOK.encode())

    headings = [section.heading for section in parsed.sections]
    assert headings == ["Orders service", "Rollback"]
    for section in parsed.sections:
        assert section.span.resolve(parsed.text) is not None, section


def test_a_section_stops_where_the_next_heading_begins():
    parsed = TextParser().parse("r.md", "markdown", RUNBOOK.encode())

    first = parsed.sections[0].span.resolve(parsed.text)

    assert "append-only" in first
    assert "Revert the migration" not in first


def test_invalid_utf8_is_read_lossily_rather_than_refused():
    """A runbook with one mangled byte is still a runbook."""
    parsed = TextParser().parse("b.md", "markdown", b"orders \xf0\x28\x8c table")

    assert "orders" in parsed.text
    assert "table" in parsed.text


def test_an_unconfigured_media_type_is_refused_by_name():
    """"install the pdf extra" and "this PDF is corrupt" are different things
    for an operator to do, so they are different failures."""
    with pytest.raises(UnsupportedMediaType, match="application/pdf"):
        ParserRegistry().parse("s.pdf", "application/pdf", b"%PDF-1.7")


def test_text_parsing_works_with_no_optional_parser_installed():
    """Decision 4 one level down: the format runbooks are actually written in
    needs nothing extra."""
    parsed = ParserRegistry().parse("n.txt", "text", b"just notes")

    assert parsed.text == "just notes"


# ── extraction ─────────────────────────────────────────────────────────────


def test_a_named_entity_in_a_descriptive_sentence_produces_a_claim():
    parsed = TextParser().parse("r.md", "markdown", RUNBOOK.encode())

    result = MentionExtractor().extract(parsed, ["prod.orders"])

    assert len(result.claims) == 1, result.claims
    assert result.claims[0].subject == "prod.orders"
    assert "append-only" in result.claims[0].object


def test_an_entity_nobody_mentioned_produces_nothing():
    """The negative case. An extractor that claimed something about every known
    entity regardless of the text would pass the test above."""
    parsed = TextParser().parse("r.md", "markdown", RUNBOOK.encode())

    result = MentionExtractor().extract(parsed, ["prod.invoices"])

    assert result.claims == []


def test_a_claim_can_never_reach_the_assert_band():
    """**A name matched in prose is evidence, not proof.** An extractor claiming
    enough confidence to assert would be writing to the graph on the strength of
    a string match, and would be believed."""
    assert MENTION_CONFIDENCE < 0.8
    assert MENTION_CONFIDENCE >= 0.5, "and it must still be worth reviewing"


def test_every_claim_carries_a_span_that_resolves_to_its_sentence():
    """Provenance that does not resolve makes a claim unverifiable, which is the
    one thing decision 5 refuses."""
    parsed = TextParser().parse("r.md", "markdown", RUNBOOK.encode())

    result = MentionExtractor().extract(parsed, ["prod.orders"])

    evidence = result.claims[0].provenance.evidence.resolve(parsed.text)
    assert evidence is not None
    assert "append-only" in evidence


# ── the wire shape ─────────────────────────────────────────────────────────


def test_the_worker_sends_camel_case_keys_the_server_expects():
    """**The contract is the wire names, not the Python attribute names.** A
    test built from the dataclasses would pass even if every key drifted."""
    client = RecordingClient()
    worker = Worker(client)

    worker.process(
        Path(__file__).parent / "fixtures" / "runbook.md", ["prod.orders"]
    )

    _, path, body = client.sent[-1]
    assert path == "/extraction/runs"
    assert set(body) == {"document", "result", "extractor", "extractorVersion"}
    assert set(body["document"]) >= {"sourceId", "mediaType", "text"}
    claim = body["result"]["claims"][0]
    assert set(claim) == {
        "subject",
        "predicate",
        "object",
        "confidence",
        "provenance",
    }
    assert set(claim["provenance"]) == {
        "sourceId",
        "extractor",
        "extractorVersion",
        "extractedAt",
        "evidence",
    }
    assert set(claim["provenance"]["evidence"]) == {"start", "end"}


def test_the_whole_body_is_json_serialisable():
    """It crosses a process boundary as JSON. A value that cannot be serialised
    fails at the socket, in a traceback that names none of this."""
    client = RecordingClient()

    Worker(client).process(
        Path(__file__).parent / "fixtures" / "runbook.md", ["prod.orders"]
    )

    json.dumps(client.sent[-1][2])


def test_the_path_is_the_source_id_so_a_rerun_is_recognised():
    """A generated id would make every run look like a first run, and
    idempotence would silently never fire — expensive, because the whole point
    of the fingerprint is to skip an OCR pass."""
    client = RecordingClient()
    path = Path(__file__).parent / "fixtures" / "runbook.md"

    Worker(client).process(path, ["prod.orders"])
    Worker(client).process(path, ["prod.orders"])

    first, second = client.sent[0][2], client.sent[1][2]
    assert first["document"]["sourceId"] == second["document"]["sourceId"] == str(path)


def test_an_unreadable_document_fails_that_document_not_the_run(tmp_path):
    """A batch of a thousand runbooks must not be lost to one bad file."""
    good = write(tmp_path, "good.md", RUNBOOK)
    write(tmp_path, "mystery.xyz", "no parser knows this")
    client = RecordingClient()

    outcomes = Worker(client).process_tree(tmp_path, ["prod.orders"])

    # The unknown extension is not even attempted — it is not in MEDIA_TYPES —
    # so the run is one document and it succeeded.
    assert [o.source_id for o in outcomes] == [str(good)]


def test_image_extensions_route_to_the_media_types_the_ocr_parser_claims(tmp_path):
    """The extension-to-media-type map decides whether a worker even attempts
    a file; an image extension left out of it is silently skipped forever —
    the same failure `.xyz` demonstrates above, just for a format that does
    have a parser once `--ocr` is on."""
    from graph_owl_worker.worker import MEDIA_TYPES

    assert MEDIA_TYPES[".png"] == "image/png"
    assert MEDIA_TYPES[".jpg"] == "image/jpeg"
    assert MEDIA_TYPES[".jpeg"] == "image/jpeg"
    assert MEDIA_TYPES[".webp"] == "image/webp"


def test_an_unsupported_media_type_is_reported_as_a_failed_document(tmp_path):
    pdf = write(tmp_path, "scan.pdf", "%PDF-1.7 not really")
    client = RecordingClient()

    outcome = Worker(client).process(pdf, ["prod.orders"])

    assert outcome.outcome == "failed"
    assert "application/pdf" in outcome.detail["reason"]


def test_already_extracted_is_reported_rather_than_read_as_an_empty_result():
    """A retrying worker needs to tell "graph-owl already had this" from
    "graph-owl found nothing in it". They look identical if you only count."""
    client = RecordingClient(answers=[{"outcome": "alreadyExtracted", "runId": "r1"}])

    outcome = Worker(client).process(
        Path(__file__).parent / "fixtures" / "runbook.md", ["prod.orders"]
    )

    assert outcome.outcome == "alreadyExtracted"


def test_subjects_come_from_the_catalog():
    client = RecordingClient(
        answers=[{"data": [{"fullyQualifiedName": "prod.orders", "kind": "service"}]}]
    )

    assert catalog_subjects(client) == ["prod.orders"]


def test_a_catalog_page_without_fqns_yields_nothing_rather_than_crashing():
    """The worker reads a page it did not shape. A missing key is a server that
    changed, not a reason to die mid-run."""
    client = RecordingClient(answers=[{"data": [{"kind": "service"}]}])

    assert catalog_subjects(client) == []
