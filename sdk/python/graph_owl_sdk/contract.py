"""The contract this SDK was built against — Epic 16 Slice E.

Kept as values rather than prose so the drift test can compare them. An SDK that
silently keeps working against a contract it was not built for is the failure
mode, not the success.
"""

CONTRACT_VERSION = "0.1.0"

REQUIRED_PATHS = (
    "/ingest",
    "/ingest/batch",
    "/ingest/jobs/{id}",
)
