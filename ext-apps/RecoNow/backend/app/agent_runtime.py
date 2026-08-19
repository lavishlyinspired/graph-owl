"""What wakes an agent, what it may do, and what each run cost — Plan 123 §5.

The safety rule (`grounding.py`) shipped first, deliberately: an agent fleet
without it is the thing the plan warns about. This is the rest of the runtime.

**The trigger bus.** An agent that only runs when someone clicks is a feature;
one that wakes on an event is a colleague. Subscriptions are declared and
matched; an event nobody subscribes to is **not an error**, because most
events have no listener and treating that as a failure fills the log with
noise that trains everyone to ignore it.

**Grants, revocable mid-run.** Permissions are data, not code, so they can be
withdrawn while an agent is working. The plan's own acceptance criterion:
*"revoking a grant mid-run stops the write and records the refusal."* A grant
checked only at dispatch is a grant that **cannot be revoked at all** — the
run is already past the check by the time anyone clicks. So every write
re-checks.

**Cost per run, measured or not shown.** `None` means nobody measured and
renders as "—". Zero is a claim that the run was free, and a number nobody
measured is worse than a blank because it invites a decision.
"""

from __future__ import annotations

import time
import uuid
from collections import Counter
from contextlib import contextmanager
from dataclasses import dataclass, field
from typing import Any


class GrantRevoked(PermissionError):
    """An agent attempted a write its grant no longer permits."""


@dataclass(frozen=True)
class Subscription:
    """One agent's interest in one kind of event."""

    agent: str
    event: str


class Registry:
    """Who wakes on what, and who may do what.

    Deliberately in-process and simple: this is the *shape* of the runtime,
    and a durable subscription store belongs in graph-owl's own event
    infrastructure (`graph-owl-events` already defines `EventSink` and
    `ChangeEvent`) rather than in a second implementation here.
    """

    def __init__(self) -> None:
        self._subscriptions: set[Subscription] = set()
        self._grants: set[tuple[str, str]] = set()

    def subscribe(self, subscription: Subscription) -> None:
        # A set, so a restart that re-registers subscriptions does not double
        # every agent's work.
        self._subscriptions.add(subscription)

    def woken_by(self, event: str) -> list[str]:
        """Agents subscribed to `event`, sorted so the order is stable.

        An event with no subscribers returns an empty list rather than
        raising: silence is the normal case.
        """
        return sorted(s.agent for s in self._subscriptions if s.event == event)

    def grant(self, agent: str, capability: str) -> None:
        self._grants.add((agent, capability))

    def revoke(self, agent: str, capability: str) -> None:
        self._grants.discard((agent, capability))

    def may(self, agent: str, capability: str) -> bool:
        """Deny by default. An agent that can act until someone forbids it is
        an agent nobody configured."""
        return (agent, capability) in self._grants


class Span:
    """One step, while it is happening. `record` attaches what it found."""

    def __init__(self, kind: str, name: str) -> None:
        self.data: dict[str, Any] = {"kind": kind, "name": name, "status": "ok"}

    def record(self, **fields: Any) -> None:
        self.data.update({k: v for k, v in fields.items() if v is not None})


@dataclass
class AgentRun:
    """One agent, woken by one event: what it looked at, what it decided, what
    it cost, and what it was refused.

    **Every step is a typed, inspectable span**, and what is traced is tool
    input/output and decisions — not only the model calls. A trace of model
    calls alone cannot answer the question anyone actually asks after a bad
    outcome, which is *what did it look at before it decided that*.
    """

    registry: Registry
    agent: str
    event: str
    writes: list[dict[str, Any]] = field(default_factory=list)
    refusals: list[dict[str, Any]] = field(default_factory=list)
    spans: list[dict[str, Any]] = field(default_factory=list)
    context: dict[str, Any] = field(default_factory=dict)
    id: str = field(default_factory=lambda: uuid.uuid4().hex[:12])
    _tokens: int | None = None
    _cost: float | None = None
    _started: float = field(default_factory=time.monotonic)
    _status: str = "running"
    _error: str | None = None
    _ms: int | None = None

    @contextmanager
    def span(self, kind: str, name: str):
        """Time one step and keep it, whatever happens.

        **A failing span is recorded and the exception re-raised.** Log every
        error: a step that failed silently is the one you need most, and an
        observability layer that eats the exception has destroyed the evidence
        it exists to keep.
        """
        entry = Span(kind, name)
        started = time.monotonic()
        try:
            yield entry
        except Exception as exc:  # noqa: BLE001
            entry.data["status"] = "failed"
            entry.data["error"] = str(exc)
            raise
        finally:
            entry.data["ms"] = int((time.monotonic() - started) * 1000)
            if entry.data.get("tokens") or entry.data.get("cost"):
                self.record_usage(
                    tokens=int(entry.data.get("tokens") or 0),
                    cost=float(entry.data.get("cost") or 0.0),
                )
            self.spans.append(entry.data)

    def finish(self, *, error: str | None = None) -> None:
        self._ms = int((time.monotonic() - self._started) * 1000)
        self._status = "failed" if error else "completed"
        self._error = error

    def write(self, capability: str, payload: dict[str, Any]) -> None:
        """Perform a write, if the grant still permits it **now**.

        Re-checked per write rather than once at dispatch, which is what makes
        a grant revocable at all.

        # Raises

        `GrantRevoked`, after recording the refusal — for an agentic product
        the record of what an agent was refused is worth more than the record
        of what it produced.
        """
        if not self.registry.may(self.agent, capability):
            reason = "grant not held at the moment of the write"
            self.refusals.append(
                {
                    "agent": self.agent,
                    "capability": capability,
                    "event": self.event,
                    "reason": reason,
                }
            )
            # In the trace, not only in a counter: a grant revoked mid-run is
            # exactly the event an audit asks about.
            self.spans.append(
                {
                    "kind": "refusal",
                    "name": capability,
                    "status": "refused",
                    "error": reason,
                    "ms": 0,
                }
            )
            raise GrantRevoked(
                f"{self.agent} may not {capability} — grant not held at the moment of the write"
            )
        self.writes.append({"capability": capability, "payload": payload})

    def record_usage(self, *, tokens: int, cost: float) -> None:
        """Accumulate: one run may make several model calls, and reporting
        only the last would understate every multi-call run."""
        self._tokens = (self._tokens or 0) + tokens
        self._cost = (self._cost or 0.0) + cost

    def summary(self) -> dict[str, Any]:
        return {
            "id": self.id,
            "status": self._status,
            "error": self._error,
            "ms": self._ms,
            # "12 steps" tells a reader nothing; "3 tool calls, 1 model call,
            # 1 decision" tells them what kind of run it was.
            "span_counts": dict(Counter(s["kind"] for s in self.spans)),
            "agent": self.agent,
            # Without the event an activity screen shows work with no cause,
            # and nobody can tell a scheduled run from one a user triggered.
            "event": self.event,
            "writes": len(self.writes),
            "refusals": len(self.refusals),
            # `None`, never 0 — see the module docstring.
            "tokens": self._tokens,
            "cost": self._cost,
        }


#: The subscriptions Plan 123 §5's own table names. Declared as data so the
#: bus's wiring is readable in one place rather than spread across handlers.
DEFAULT_SUBSCRIPTIONS = [
    Subscription(agent="ingestion", event="file.uploaded"),
    Subscription(agent="validation", event="mapping.confirmed"),
    Subscription(agent="triage", event="reconciliation.finished"),
    # Woken by the same event as triage: it needs the period's cases, and
    # those exist exactly when the reconciliation has finished.
    Subscription(agent="vendor", event="reconciliation.finished"),
    # The one agent that genuinely needs the graph: "is this supplier a repeat
    # offender" is a question about other periods, which this period's rows
    # cannot answer.
    Subscription(agent="risk", event="reconciliation.finished"),
    Subscription(agent="explainer", event="finding.created"),
    Subscription(agent="eligibility", event="finding.created"),
    Subscription(agent="drift", event="gstr2a.pulled"),
    Subscription(agent="close", event="period.closing"),
    Subscription(agent="pattern", event="analytics.run"),
]


def default_registry() -> Registry:
    registry = Registry()
    for subscription in DEFAULT_SUBSCRIPTIONS:
        registry.subscribe(subscription)
    return registry


__all__ = [
    "DEFAULT_SUBSCRIPTIONS",
    "AgentRun",
    "GrantRevoked",
    "Registry",
    "Subscription",
    "default_registry",
]
