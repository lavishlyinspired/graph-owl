"""``GraphOwlCheckpointer`` — a LangGraph `BaseCheckpointSaver` backed by
Epic 31's `record_memory`/`recall_memory` MCP tools.

**A sixth finding, made building this** (`43-framework-integrations.md`):
`record_memory` is asset-centric (`fullyQualifiedName` required) and
confidence-banded — below 0.8, a write becomes a *proposal* rather than an
assertion, which is wrong for a checkpointer that needs immediate
read-after-write. Resolved here, not worked around invisibly:

- Checkpoints key a **synthetic** FQN
  (``dsc:langgraph-checkpoint/{thread_id}/{checkpoint_ns}``), not a real
  catalog asset. This is a deliberate, visible trade-off: a checkpoint
  "asset" can appear in `search_assets`/`get_asset_context` results if a
  deployment does not filter the ``langgraph-checkpoint`` namespace, and
  this package has no way to suppress that from the client side.
- Every write uses **confidence 1.0**, which is honest rather than
  inflated: a checkpoint records the agent's own execution state, not
  uncertain institutional knowledge the way a recalled fact is, so it is
  never gated into the proposal queue.

**"Retracted, not deleted, remains in history" needs no retraction call at
all.** `record_memory` has none, and `recall_memory` returns everything
ever recorded — nothing here is ever deleted, so an older checkpoint
stays exactly as queryable after a newer one is saved as it was before.
"Discarding" a checkpoint, in this design, is simply not saving a newer
one that supersedes it.

Serialization reuses LangGraph's own `serde` (`JsonPlusSerializer` by
default via `BaseCheckpointSaver.__init__`) rather than a hand-rolled
format — the wire bytes are base64-encoded into `record_memory`'s
``content`` string field, prefixed with the serializer's own type tag.

**A seventh finding, found running this against a real `graph-owl-server`
(8 August 2026), not just this file's own mocked tests**: two more
prerequisites `record_memory` enforces, beyond the confidence gate above.
Every one of the 9 tests in ``tests/test_memory.py`` — including the
compiled-`StateGraph` proof — passes against a mocked MCP server; the same
calls against a real one refused with two different errors in sequence
until both were satisfied:

1. **The target FQN must already exist as a real, readable asset.**
   ``record_memory`` against a never-created synthetic FQN answers *"this
   agent cannot read `X`, so it cannot write to it"* — a checkpoint's
   backing entity is not implicitly created by writing to it, the way a
   filesystem path is. This package cannot create one either: there is no
   MCP tool for asset creation (`crates/graph-owl-mcp/src/write.rs`'s six
   write tools are `propose_metadata_change`, `record_memory`,
   `record_investigation`, `create_glossary_term`, `create_quality_test`,
   `link_lineage` — none of them "create an asset"). The only way to
   create one is the REST ingest API (`POST /ingest`), which this package
   does not currently call from `memory.py`.
2. **The calling principal needs the `recordMemory` capability, granted
   explicitly.** Even against a real, existing, fully-readable asset, the
   write still refuses with *"this action requires the `recordMemory`
   capability, which this agent has not been granted"* until an admin
   grants it — and granting is **admin-only, human-only** by Epic 32's own
   design (`PUT /agents/{agent_id}/grant`, gated the same way
   `set_agent_grant`'s own doc states: "Grants are admin-only and
   human-only; the agent is named in the path so an audit reads who
   changed whose capabilities"). This is not a gap to work around — it is
   the exact property Epic 32 exists to guarantee, and a checkpointer that
   found a way to self-grant would be defeating it.

**So this checkpointer has a genuine, unautomatable deployment
prerequisite**, and pretending otherwise would be worse than stating it:
before first use, a human admin must (a) ensure a real asset exists at
the FQN this checkpointer will write to — one per thread, or a single
shared "checkpoints" asset, an operator's choice — and (b) grant that
asset-holding principal the `recordMemory` capability. Everything *after*
that point — round-tripping, ordering, retraction-as-non-deletion,
confidence — is what this file's own tests (run against a mock standing
in for a server that has already granted both) actually verify, and did
verify correctly the moment the real prerequisites were met by hand.
"""

from __future__ import annotations

import base64
import builtins
from collections.abc import Iterator, Sequence
from typing import Any

from langchain_core.runnables import RunnableConfig
from langgraph.checkpoint.base import (
    BaseCheckpointSaver,
    Checkpoint,
    CheckpointMetadata,
    CheckpointTuple,
    get_checkpoint_id,
)

from graph_owl_langchain._core.client import GraphOwlClient
from graph_owl_langchain._core.principal import Principal

#: Never a proposal — see the module doc for why this is the honest value,
#: not an inflated one.
CHECKPOINT_CONFIDENCE = 1.0


def _fqn(thread_id: str, checkpoint_ns: str) -> str:
    suffix = f"/{checkpoint_ns}" if checkpoint_ns else ""
    return f"dsc:langgraph-checkpoint/{thread_id}{suffix}"


class GraphOwlCheckpointer(BaseCheckpointSaver[str]):
    """LangGraph state, persisted as Epic 31 memories.

    ``principal`` has no default (decision 2) and, per Epic 32's own
    requirement, must be an agent-scoped credential — this package cannot
    verify that structurally (a token's holder is a server-side fact), so
    it is on the caller to construct this with the right principal, the
    same way every other surface in this package requires.
    """

    def __init__(
        self,
        endpoint: str,
        principal: Principal,
        opener: Any = None,
    ) -> None:
        super().__init__()
        self._client = GraphOwlClient(endpoint=endpoint, principal=principal, opener=opener)

    def put(
        self,
        config: RunnableConfig,
        checkpoint: Checkpoint,
        metadata: CheckpointMetadata,
        new_versions: Any,
    ) -> RunnableConfig:
        configurable = config["configurable"]
        thread_id = configurable["thread_id"]
        checkpoint_ns = configurable.get("checkpoint_ns", "")
        record = {
            "checkpoint": checkpoint,
            "metadata": metadata,
            "parent_checkpoint_id": configurable.get("checkpoint_id"),
        }
        type_tag, data = self.serde.dumps_typed(record)
        content = f"{type_tag}:{base64.b64encode(data).decode('ascii')}"

        self._client.call_tool(
            "record_memory",
            {
                "fullyQualifiedName": _fqn(thread_id, checkpoint_ns),
                "content": content,
                "rationale": f"LangGraph checkpoint {checkpoint['id']} for thread {thread_id}",
                "confidence": CHECKPOINT_CONFIDENCE,
            },
        )
        return {
            "configurable": {
                "thread_id": thread_id,
                "checkpoint_ns": checkpoint_ns,
                "checkpoint_id": checkpoint["id"],
            }
        }

    def put_writes(
        self,
        config: RunnableConfig,
        writes: Sequence[tuple[str, Any]],
        task_id: str,
        task_path: str = "",
    ) -> None:
        # Pending/intermediate writes for a still-running super-step.
        # Recorded the same way a checkpoint is — a memory, never deleted —
        # keyed under the owning checkpoint so `get_tuple` can attach them.
        configurable = config["configurable"]
        thread_id = configurable["thread_id"]
        checkpoint_ns = configurable.get("checkpoint_ns", "")
        checkpoint_id = configurable["checkpoint_id"]
        # `[*writes]`, not `list(writes)`: a bare `list(...)` call inside a
        # class that overrides the name `list` as a method is unambiguous
        # to Python at runtime (bare names never resolve through `self`),
        # but confuses static analysis, so the spread form sidesteps the
        # question entirely rather than relying on that distinction.
        type_tag, data = self.serde.dumps_typed([*writes])
        content = f"{type_tag}:{base64.b64encode(data).decode('ascii')}"
        self._client.call_tool(
            "record_memory",
            {
                "fullyQualifiedName": _fqn(thread_id, checkpoint_ns) + f"/writes/{checkpoint_id}",
                "content": content,
                "rationale": f"pending writes for task {task_id}",
                "confidence": CHECKPOINT_CONFIDENCE,
            },
        )

    def get_tuple(self, config: RunnableConfig) -> CheckpointTuple | None:
        configurable = config["configurable"]
        thread_id = configurable["thread_id"]
        checkpoint_ns = configurable.get("checkpoint_ns", "")
        records = self._recorded_checkpoints(thread_id, checkpoint_ns)
        if not records:
            return None

        wanted = get_checkpoint_id(config)
        chosen = (
            next((r for r in records if r["checkpoint"]["id"] == wanted), None)
            if wanted
            else max(records, key=lambda r: r["checkpoint"]["id"])
        )
        if chosen is None:
            return None

        parent_id = chosen.get("parent_checkpoint_id")
        return CheckpointTuple(
            config={
                "configurable": {
                    "thread_id": thread_id,
                    "checkpoint_ns": checkpoint_ns,
                    "checkpoint_id": chosen["checkpoint"]["id"],
                }
            },
            checkpoint=chosen["checkpoint"],
            metadata=chosen["metadata"],
            parent_config=(
                {
                    "configurable": {
                        "thread_id": thread_id,
                        "checkpoint_ns": checkpoint_ns,
                        "checkpoint_id": parent_id,
                    }
                }
                if parent_id
                else None
            ),
            pending_writes=None,
        )

    def list(
        self,
        config: RunnableConfig | None,
        *,
        filter: dict[str, Any] | None = None,
        before: RunnableConfig | None = None,
        limit: int | None = None,
    ) -> Iterator[CheckpointTuple]:
        if config is None:
            return
        configurable = config["configurable"]
        thread_id = configurable["thread_id"]
        checkpoint_ns = configurable.get("checkpoint_ns", "")
        records: list[dict[str, Any]] = sorted(
            self._recorded_checkpoints(thread_id, checkpoint_ns),
            key=lambda r: r["checkpoint"]["id"],
            reverse=True,
        )
        if before is not None:
            cutoff = get_checkpoint_id(before)
            records = [r for r in records if r["checkpoint"]["id"] < cutoff]
        if limit is not None:
            records = records[:limit]

        for record in records:
            parent_id = record.get("parent_checkpoint_id")
            yield CheckpointTuple(
                config={
                    "configurable": {
                        "thread_id": thread_id,
                        "checkpoint_ns": checkpoint_ns,
                        "checkpoint_id": record["checkpoint"]["id"],
                    }
                },
                checkpoint=record["checkpoint"],
                metadata=record["metadata"],
                parent_config=(
                    {
                        "configurable": {
                            "thread_id": thread_id,
                            "checkpoint_ns": checkpoint_ns,
                            "checkpoint_id": parent_id,
                        }
                    }
                    if parent_id
                    else None
                ),
                pending_writes=None,
            )

    def _recorded_checkpoints(
        self, thread_id: str, checkpoint_ns: str
    ) -> builtins.list[dict[str, Any]]:
        result = self._client.call_tool(
            "recall_memory", {"fullyQualifiedName": _fqn(thread_id, checkpoint_ns)}
        )
        memories = result.get("memories") or []
        records: list[dict[str, Any]] = []
        for memory in memories:
            type_tag, _, encoded = memory["content"].partition(":")
            data = base64.b64decode(encoded)
            records.append(self.serde.loads_typed((type_tag, data)))
        return records
