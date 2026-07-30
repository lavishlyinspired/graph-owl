"""The graph-owl ingestion SDK — Epic 16 Slice E."""

from .contract import CONTRACT_VERSION, REQUIRED_PATHS
from .ingest import (
    MAX_ITEMS_PER_PUSH,
    EdgeDraft,
    EntityDraft,
    GraphOwlClient,
    GraphOwlError,
    IngestBuilder,
    IngestRequest,
    backoff_seconds,
    chunk,
    is_retryable,
)

__all__ = [
    "CONTRACT_VERSION",
    "MAX_ITEMS_PER_PUSH",
    "REQUIRED_PATHS",
    "EdgeDraft",
    "EntityDraft",
    "GraphOwlClient",
    "GraphOwlError",
    "IngestBuilder",
    "IngestRequest",
    "backoff_seconds",
    "chunk",
    "is_retryable",
]
