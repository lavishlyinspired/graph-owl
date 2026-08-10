"""graph-owl document worker — Epic 21, decision 0.

Parses what graph-owl cannot (PDF today; OCR and multimodal later) and proposes
what those documents say. Every policy decision about that proposal is made by
graph-owl, not here.
"""

from .extract import MENTION_CONFIDENCE, ClaimExtractor, MentionExtractor, sentences
from .ocr import EndpointOcrModel, OcrError, OcrModel, OcrPdfParser, OvisOcrParser
from .parsers import (
    DocumentParser,
    ParseError,
    ParserRegistry,
    PdfParser,
    TextParser,
    UnsupportedMediaType,
)
from .worker import MEDIA_TYPES, DocumentOutcome, Worker, catalog_subjects

__all__ = [
    "MEDIA_TYPES",
    "MENTION_CONFIDENCE",
    "ClaimExtractor",
    "DocumentOutcome",
    "DocumentParser",
    "EndpointOcrModel",
    "MentionExtractor",
    "OcrError",
    "OcrModel",
    "OcrPdfParser",
    "OvisOcrParser",
    "ParseError",
    "ParserRegistry",
    "PdfParser",
    "TextParser",
    "UnsupportedMediaType",
    "Worker",
    "catalog_subjects",
    "sentences",
]
