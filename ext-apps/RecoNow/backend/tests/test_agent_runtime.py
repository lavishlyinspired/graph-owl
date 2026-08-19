"""Agents: what wakes them, what they may do, what each run cost.

Plan 123 §5. The safety rule (`grounding.py`) shipped first, deliberately —
an agent fleet without it is the thing the plan warns about. This is the rest:

**The trigger bus.** Revision 1's agents were on-demand and passive. An agent
that only runs when someone clicks is a feature; one that wakes on an event is
a colleague. Subscriptions are declared, matched against events, and an event
nobody subscribes to is not an error — most events have no listener and
treating that as a failure would fill the log with noise.

**Grants, revocable mid-run.** An agent's permissions are data, not code, so
they can be withdrawn while it is working. The plan's own AC: "revoking a
grant mid-run stops the write and records the refusal." A grant checked only
at dispatch is a grant that cannot be revoked at all.

**Cost per run, measured or not shown.** A number nobody measured is worse
than a blank: it invites a decision. `None` means unmeasured and renders as
"—", never as zero.
"""

from __future__ import annotations

import pytest

from app.agent_runtime import (
    AgentRun,
    GrantRevoked,
    Registry,
    Subscription,
)


def _registry() -> Registry:
    registry = Registry()
    registry.subscribe(Subscription(agent="triage", event="reconciliation.finished"))
    registry.subscribe(Subscription(agent="explainer", event="finding.created"))
    registry.subscribe(Subscription(agent="eligibility", event="finding.created"))
    return registry


class TestTheTriggerBus:
    def test_an_event_wakes_every_agent_subscribed_to_it(self):
        woken = _registry().woken_by("finding.created")

        assert sorted(woken) == ["eligibility", "explainer"]

    def test_an_event_nobody_subscribes_to_wakes_nothing_and_is_not_an_error(self):
        """Most events have no listener. Treating that as a failure would fill
        the log with noise and train everyone to ignore it."""
        assert _registry().woken_by("nothing.listens.to.this") == []

    def test_an_agent_is_not_woken_by_an_event_it_did_not_subscribe_to(self):
        """The negative that matters: a bus waking everything on every event
        is not a bus, and would pass a test that only checked the positive."""
        assert "triage" not in _registry().woken_by("finding.created")

    def test_subscribing_twice_wakes_the_agent_once(self):
        """A restart that re-registers subscriptions must not double every
        agent's work."""
        registry = _registry()
        registry.subscribe(Subscription(agent="triage", event="reconciliation.finished"))

        assert registry.woken_by("reconciliation.finished") == ["triage"]


class TestGrants:
    def test_an_agent_with_a_grant_may_act(self):
        registry = _registry()
        registry.grant("triage", "propose")

        assert registry.may("triage", "propose") is True

    def test_an_agent_without_a_grant_may_not(self):
        """Deny by default. An agent that can act until someone forbids it is
        an agent nobody configured."""
        assert _registry().may("triage", "propose") is False

    def test_a_revoked_grant_is_refused_from_that_moment(self):
        registry = _registry()
        registry.grant("triage", "propose")
        registry.revoke("triage", "propose")

        assert registry.may("triage", "propose") is False

    def test_revoking_one_capability_leaves_the_others(self):
        registry = _registry()
        registry.grant("triage", "propose")
        registry.grant("triage", "read")
        registry.revoke("triage", "propose")

        assert registry.may("triage", "read") is True

    def test_revoking_mid_run_stops_the_write_and_records_the_refusal(self):
        """The plan's own acceptance criterion, verbatim. A grant checked only
        at dispatch is a grant that cannot be revoked at all — the run is
        already past the check by the time anyone clicks."""
        registry = _registry()
        registry.grant("triage", "propose")
        run = AgentRun(registry=registry, agent="triage", event="reconciliation.finished")

        registry.revoke("triage", "propose")

        with pytest.raises(GrantRevoked):
            run.write("propose", {"anything": True})

        assert run.refusals
        assert run.refusals[0]["capability"] == "propose"

    def test_a_write_within_a_live_grant_succeeds_and_is_recorded(self):
        registry = _registry()
        registry.grant("triage", "propose")
        run = AgentRun(registry=registry, agent="triage", event="reconciliation.finished")

        run.write("propose", {"case": "c-1"})

        assert run.writes == [{"capability": "propose", "payload": {"case": "c-1"}}]
        assert run.refusals == []


class TestCost:
    def test_an_unmeasured_run_reports_none_never_zero(self):
        """A cost of zero is a claim that the run was free. `None` says nobody
        measured, which is the truth and invites the right question."""
        run = AgentRun(registry=_registry(), agent="triage", event="e")

        assert run.summary()["tokens"] is None
        assert run.summary()["cost"] is None

    def test_a_measured_run_reports_what_was_measured(self):
        run = AgentRun(registry=_registry(), agent="triage", event="e")
        run.record_usage(tokens=1200, cost=0.0031)

        summary = run.summary()
        assert summary["tokens"] == 1200
        assert summary["cost"] == 0.0031

    def test_usage_accumulates_across_several_calls_in_one_run(self):
        run = AgentRun(registry=_registry(), agent="triage", event="e")
        run.record_usage(tokens=1000, cost=0.002)
        run.record_usage(tokens=500, cost=0.001)

        assert run.summary()["tokens"] == 1500
        assert run.summary()["cost"] == pytest.approx(0.003)

    def test_the_summary_names_the_event_that_woke_the_run(self):
        """Without it an activity screen shows work with no cause, and nobody
        can tell a scheduled run from one a user triggered."""
        run = AgentRun(registry=_registry(), agent="triage", event="reconciliation.finished")

        assert run.summary()["event"] == "reconciliation.finished"
