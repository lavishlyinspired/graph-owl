"""Credential handling for every graph-owl surface this package exposes.

Decision 2 (`43-framework-integrations.md`): construction requires a
principal. There is no ambient default and no "if unset, use admin" — an
integration that quietly runs as a superuser is worse than one that fails to
construct.
"""

from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass, field


@dataclass(frozen=True)
class Principal:
    """A caller's identity, carried through to every MCP/REST call.

    ``token`` is excluded from the generated ``__repr__`` (``field(repr=False)``)
    so a principal can be logged, printed in a traceback, or interpolated into
    an error message without ever putting the credential where a human
    reading it would see it.

    ``refresh``, when given, is called at most once per request on a 401 —
    Slice C's "an expired token triggers one refresh and does not loop."
    ``None`` means no refresh is possible, and an expired token then fails
    the call rather than retrying forever.
    """

    token: str = field(repr=False)
    refresh: Callable[[], str] | None = field(default=None, repr=False)

    def __post_init__(self) -> None:
        if not self.token:
            raise ValueError("a principal's token must not be empty")
