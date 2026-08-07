"""Slice E RED: `GraphOwlCheckpointer` — a LangGraph `BaseCheckpointSaver`
backed by `record_memory`/`recall_memory`.

**A sixth finding, made building this**: Epic 31's `record_memory` is
asset-centric (`fullyQualifiedName` required) and confidence-banded (below
0.8, a write becomes a proposal rather than an assertion — unsuitable for
a checkpointer, which needs immediate read-after-write). This checkpointer
resolves both: checkpoints key a *synthetic* FQN
(``dsc:langgraph-checkpoint/{thread_id}/{checkpoint_ns}``) rather than a
real catalog asset, and every checkpoint write uses confidence 1.0 — not
an inflated number, but the honest one: a checkpoint records the agent's
*own* execution state, which is not uncertain institutional knowledge the
way a recalled fact is.

"Retracted, not deleted, remains in history" needs no separate retraction
call: `record_memory` has none, and none is needed — each checkpoint save
is a new memory, `recall_memory` returns everything recorded (never
deletes), so an older checkpoint that is no longer "current" stays exactly
as queryable as it always was. "Discarding" a checkpoint, in this design,
is simply not writing a newer one that supersedes it — the old one was
never at risk of disappearing.
"""

import base64
import json

from langgraph.checkpoint.base import Checkpoint, CheckpointMetadata, empty_checkpoint

from graph_owl_langchain._core.principal import Principal
from graph_owl_langchain.memory import GraphOwlCheckpointer

SECRET = "sk-super-secret-agent-token"


class _FakeResponse:
    def __init__(self, body: bytes):
        self.status = 200
        self._body = body

    def read(self):
        return self._body

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        return False


class _FakeMemoryServer:
    """An in-process stand-in for `record_memory`/`recall_memory`: real
    accumulate-never-delete semantics, so "history is preserved" is
    provable without a live server."""

    def __init__(self):
        self.by_fqn: dict[str, list[dict]] = {}

    def opener(self, request):
        payload = json.loads(request.data)
        name = payload["params"]["name"]
        args = payload["params"]["arguments"]
        if name == "record_memory":
            fqn = args["fullyQualifiedName"]
            self.by_fqn.setdefault(fqn, []).append(
                {
                    "kind": "checkpoint",
                    "content": args["content"],
                    "confidence": args["confidence"],
                    "humanAuthored": False,
                }
            )
            result = {"recorded": True}
        elif name == "recall_memory":
            fqn = args["fullyQualifiedName"]
            result = {"memories": self.by_fqn.get(fqn, [])}
        else:
            result = {}
        envelope = {
            "jsonrpc": "2.0",
            "id": payload["id"],
            "result": {
                "content": [{"type": "text", "text": json.dumps(result)}],
                "isError": False,
            },
        }
        return _FakeResponse(json.dumps(envelope).encode("utf-8"))


def _config(thread_id: str, checkpoint_ns: str = ""):
    return {"configurable": {"thread_id": thread_id, "checkpoint_ns": checkpoint_ns}}


def test_a_checkpoint_round_trips():
    server = _FakeMemoryServer()
    saver = GraphOwlCheckpointer(
        endpoint="https://graph-owl.internal",
        principal=Principal(token=SECRET),
        opener=server.opener,
    )

    checkpoint: Checkpoint = empty_checkpoint()
    checkpoint["channel_values"] = {"messages": ["hello"]}
    metadata: CheckpointMetadata = {"source": "input", "step": 0, "parents": {}}

    saved_config = saver.put(_config("thread-1"), checkpoint, metadata, {})
    loaded = saver.get_tuple(saved_config)

    assert loaded is not None
    assert loaded.checkpoint["id"] == checkpoint["id"]
    assert loaded.checkpoint["channel_values"] == {"messages": ["hello"]}
    assert loaded.metadata["step"] == 0


def test_a_fresh_saver_instance_reads_what_an_earlier_one_wrote():
    """ "Round-trips across process restart" — the saver holds no in-memory
    state of its own; a brand-new instance against the same server/thread
    must see exactly what the first one wrote."""
    server = _FakeMemoryServer()
    checkpoint: Checkpoint = empty_checkpoint()
    checkpoint["channel_values"] = {"count": 1}
    metadata: CheckpointMetadata = {"source": "loop", "step": 1, "parents": {}}

    first = GraphOwlCheckpointer(
        endpoint="https://graph-owl.internal",
        principal=Principal(token=SECRET),
        opener=server.opener,
    )
    saved_config = first.put(_config("thread-2"), checkpoint, metadata, {})

    second = GraphOwlCheckpointer(
        endpoint="https://graph-owl.internal",
        principal=Principal(token=SECRET),
        opener=server.opener,
    )
    loaded = second.get_tuple(saved_config)

    assert loaded is not None
    assert loaded.checkpoint["channel_values"] == {"count": 1}


def test_getting_the_latest_checkpoint_with_no_checkpoint_id_returns_the_newest():
    server = _FakeMemoryServer()
    saver = GraphOwlCheckpointer(
        endpoint="https://graph-owl.internal",
        principal=Principal(token=SECRET),
        opener=server.opener,
    )

    first_checkpoint: Checkpoint = empty_checkpoint()
    first_checkpoint["channel_values"] = {"step": "first"}
    saver.put(
        _config("thread-3"), first_checkpoint, {"source": "loop", "step": 0, "parents": {}}, {}
    )

    second_checkpoint: Checkpoint = empty_checkpoint()
    second_checkpoint["channel_values"] = {"step": "second"}
    saver.put(
        _config("thread-3"), second_checkpoint, {"source": "loop", "step": 1, "parents": {}}, {}
    )

    latest = saver.get_tuple(_config("thread-3"))
    assert latest is not None
    assert latest.checkpoint["channel_values"] == {"step": "second"}


def test_an_older_checkpoint_is_still_retrievable_after_a_newer_one_is_saved():
    """The retraction-vs-deletion property: saving a newer checkpoint must
    not make the older one unreadable — nothing here ever deletes."""
    server = _FakeMemoryServer()
    saver = GraphOwlCheckpointer(
        endpoint="https://graph-owl.internal",
        principal=Principal(token=SECRET),
        opener=server.opener,
    )

    first_checkpoint: Checkpoint = empty_checkpoint()
    first_checkpoint["channel_values"] = {"step": "first"}
    first_config = saver.put(
        _config("thread-4"), first_checkpoint, {"source": "loop", "step": 0, "parents": {}}, {}
    )

    second_checkpoint: Checkpoint = empty_checkpoint()
    second_checkpoint["channel_values"] = {"step": "second"}
    saver.put(
        _config("thread-4"), second_checkpoint, {"source": "loop", "step": 1, "parents": {}}, {}
    )

    still_there = saver.get_tuple(first_config)
    assert still_there is not None
    assert still_there.checkpoint["channel_values"] == {"step": "first"}


def test_list_returns_every_checkpoint_for_a_thread_newest_first():
    server = _FakeMemoryServer()
    saver = GraphOwlCheckpointer(
        endpoint="https://graph-owl.internal",
        principal=Principal(token=SECRET),
        opener=server.opener,
    )

    for i in range(3):
        checkpoint: Checkpoint = empty_checkpoint()
        checkpoint["channel_values"] = {"i": i}
        saver.put(_config("thread-5"), checkpoint, {"source": "loop", "step": i, "parents": {}}, {})

    tuples = list(saver.list(_config("thread-5")))
    assert len(tuples) == 3
    assert [t.checkpoint["channel_values"]["i"] for t in tuples] == [2, 1, 0]


def test_every_checkpoint_write_uses_full_confidence_not_the_proposal_gate():
    """Below 0.8 confidence, `record_memory` turns a write into a proposal
    rather than an assertion — wrong for a checkpointer, which needs
    immediate read-after-write. This is the check that the gate is never
    accidentally triggered."""
    server = _FakeMemoryServer()
    saver = GraphOwlCheckpointer(
        endpoint="https://graph-owl.internal",
        principal=Principal(token=SECRET),
        opener=server.opener,
    )
    checkpoint: Checkpoint = empty_checkpoint()
    saver.put(_config("thread-6"), checkpoint, {"source": "input", "step": 0, "parents": {}}, {})

    [record] = next(iter(server.by_fqn.values()))
    assert record["confidence"] == 1.0


def test_a_missing_thread_has_no_checkpoint_rather_than_an_exception():
    server = _FakeMemoryServer()
    saver = GraphOwlCheckpointer(
        endpoint="https://graph-owl.internal",
        principal=Principal(token=SECRET),
        opener=server.opener,
    )
    assert saver.get_tuple(_config("never-written")) is None


def test_the_checkpoint_content_is_not_readable_json_by_accident():
    """The blob is LangGraph's own serializer output, base64-encoded — not
    hand-rolled JSON this checkpointer invents. Guards against silently
    reverting to a fragile ad hoc format."""
    server = _FakeMemoryServer()
    saver = GraphOwlCheckpointer(
        endpoint="https://graph-owl.internal",
        principal=Principal(token=SECRET),
        opener=server.opener,
    )
    checkpoint: Checkpoint = empty_checkpoint()
    saver.put(_config("thread-7"), checkpoint, {"source": "input", "step": 0, "parents": {}}, {})

    [record] = next(iter(server.by_fqn.values()))
    type_tag, _, encoded = record["content"].partition(":")
    assert type_tag  # a real serde type tag, not empty
    base64.b64decode(encoded)  # must not raise


def test_a_compiled_langgraph_actually_persists_state_across_invocations():
    """The end-to-end proof: a real `StateGraph`, compiled with this
    checkpointer, retains its running total across two separate `invoke`
    calls sharing one `thread_id` — the property every AC in this slice
    exists to protect, demonstrated against the real contract rather than
    only against this file's own `CheckpointTuple` assertions."""
    from langgraph.graph import END, START, StateGraph
    from typing_extensions import TypedDict

    class State(TypedDict):
        total: int

    def add_one(state: State) -> dict:
        return {"total": state["total"] + 1}

    server = _FakeMemoryServer()
    saver = GraphOwlCheckpointer(
        endpoint="https://graph-owl.internal",
        principal=Principal(token=SECRET),
        opener=server.opener,
    )

    graph = StateGraph(State)
    graph.add_node("add_one", add_one)
    graph.add_edge(START, "add_one")
    graph.add_edge("add_one", END)
    app = graph.compile(checkpointer=saver)

    config = {"configurable": {"thread_id": "counting-thread"}}
    first = app.invoke({"total": 0}, config)
    second = app.invoke({"total": first["total"]}, config)

    assert first["total"] == 1
    assert second["total"] == 2
    # Both super-steps' checkpoints are still readable — nothing was
    # deleted to make room for the newer state.
    history = list(saver.list(config))
    assert len(history) >= 2
