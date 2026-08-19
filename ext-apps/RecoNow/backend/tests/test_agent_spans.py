"""What an agent actually did, step by step.

**The gap this closes.** `wake_agents` created an `AgentRun` and immediately
recorded its empty summary — no agent did any work, and the "activity" screen
showed that an agent *would have* run. There was no history, no way to see what
was running, and nothing to report on.

**The shape follows current agent-observability practice**: every step is a
typed, inspectable span, and what is traced is *tool I/O and state changes*,
not only the model calls. A trace of model calls alone cannot answer the
question anyone actually asks after a bad outcome, which is "what did it look
at before it decided that".

Three span kinds, because collapsing them loses what a reader needs:

- **tool** — it went and got something. The input and output are evidence.
- **model** — it asked a model. Cost and latency belong here, and so does
  whether the answer was grounded.
- **decision** — it concluded something. The only kind a human argues with.
"""

from __future__ import annotations

import pytest

from app.agent_runtime import AgentRun, GrantRevoked, Registry, Subscription


def _run() -> AgentRun:
    registry = Registry()
    registry.subscribe(Subscription(agent="triage", event="reconciliation.finished"))
    registry.grant("triage", "propose")
    return AgentRun(registry=registry, agent="triage", event="reconciliation.finished")


class TestSpans:
    def test_a_tool_call_is_recorded_with_what_went_in_and_came_out(self):
        run = _run()

        with run.span("tool", "fetch_cases") as span:
            span.record(input={"period": "2026-03"}, output={"count": 12})

        assert run.spans[0]["kind"] == "tool"
        assert run.spans[0]["name"] == "fetch_cases"
        assert run.spans[0]["input"] == {"period": "2026-03"}
        assert run.spans[0]["output"] == {"count": 12}

    def test_every_span_carries_how_long_it_took(self):
        """Latency per step is what turns "the agent was slow" into "the graph
        query was slow"."""
        run = _run()
        with run.span("tool", "x"):
            pass

        assert run.spans[0]["ms"] >= 0

    def test_a_failing_span_is_recorded_as_failed_and_does_not_swallow_the_error(self):
        """**Log 100% of errors.** A step that failed silently is the one you
        need most, and an observability layer that eats the exception has
        destroyed the evidence."""
        run = _run()

        with pytest.raises(RuntimeError):
            with run.span("tool", "query_graph"):
                raise RuntimeError("graph unreachable")

        assert run.spans[0]["status"] == "failed"
        assert "unreachable" in run.spans[0]["error"]

    def test_spans_keep_their_order(self):
        run = _run()
        for name in ("first", "second", "third"):
            with run.span("tool", name):
                pass

        assert [s["name"] for s in run.spans] == ["first", "second", "third"]

    def test_a_model_span_carries_its_usage_and_whether_it_was_grounded(self):
        run = _run()
        with run.span("model", "summarise") as span:
            span.record(output={"text": "..."}, tokens=1200, cost=0.003, grounded=True)

        assert run.spans[0]["tokens"] == 1200
        assert run.spans[0]["grounded"] is True
        assert run.summary()["tokens"] == 1200

    def test_a_decision_span_is_what_a_human_argues_with(self):
        run = _run()
        with run.span("decision", "rank") as span:
            span.record(output={"top": "INV-1"}, because="largest unclaimed credit")

        assert run.spans[0]["kind"] == "decision"
        assert run.spans[0]["because"] == "largest unclaimed credit"


class TestRunLifecycle:
    def test_a_run_reports_running_until_it_finishes(self):
        run = _run()

        assert run.summary()["status"] == "running"
        run.finish()
        assert run.summary()["status"] == "completed"

    def test_a_run_that_failed_says_so_rather_than_completing(self):
        run = _run()
        run.finish(error="graph unreachable")

        assert run.summary()["status"] == "failed"
        assert run.summary()["error"]

    def test_a_finished_run_carries_its_elapsed_time(self):
        run = _run()
        run.finish()

        assert run.summary()["ms"] >= 0

    def test_a_run_has_an_id_so_two_runs_of_one_agent_stay_distinguishable(self):
        assert _run().id != _run().id

    def test_the_summary_counts_the_spans_by_kind(self):
        """An activity list showing "12 steps" tells a reader nothing. "3 tool
        calls, 1 model call, 1 decision" tells them what kind of run it was."""
        run = _run()
        with run.span("tool", "a"):
            pass
        with run.span("model", "b"):
            pass
        run.finish()

        assert run.summary()["span_counts"] == {"tool": 1, "model": 1}

    def test_a_refused_write_still_appears_in_the_trace(self):
        """A grant revoked mid-run is exactly the event an audit asks about,
        and it must be in the trace rather than only in a counter."""
        run = _run()
        run.registry.revoke("triage", "propose")

        with pytest.raises(GrantRevoked):
            run.write("propose", {"x": 1})

        assert any(s["kind"] == "refusal" for s in run.spans)
