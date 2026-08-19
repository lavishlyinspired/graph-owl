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


@dataclass
class AgentRun:
    """One agent, woken by one event, with what it did and what it cost."""

    registry: Registry
    agent: str
    event: str
    writes: list[dict[str, Any]] = field(default_factory=list)
    refusals: list[dict[str, Any]] = field(default_factory=list)
    _tokens: int | None = None
    _cost: float | None = None

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
            self.refusals.append(
                {
                    "agent": self.agent,
                    "capability": capability,
                    "event": self.event,
                    "reason": "grant not held at the moment of the write",
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
