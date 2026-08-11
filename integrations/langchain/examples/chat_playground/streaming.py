"""Streaming investigation runs, one per thread — the core the FastAPI
server in `server.py` wraps.

**Lives outside `graph_owl_langchain` for the same reason
`gst_investigation_agent.py` does** (see that module's own docstring):
the package's `pyproject.toml` says "never a chain, an agent, or a
runtime" (`plans/00j-language-boundaries.md`, the ROADMAP's refusal
table). This whole `chat_playground/` directory is a local dev tool, not
a shipped surface — nothing here is reachable from `graph-owl-server` or
the console (`ui/`), and it stays that way by construction: nothing in
this file imports anything under `crates/` or `ui/`, and nothing in
those trees imports this.

**Framework-agnostic on purpose** (Decision 8, `43-framework-
integrations.md`: "framework-agnostic core, thin framework shims"):
nothing here imports FastAPI, so this module is testable and reusable
without a web framework at all — `server.py` is the only file that
knows an HTTP server exists.

**Why a generator, not `investigate()`'s return-the-final-string
shape.** `gst_investigation_agent.py`'s `investigate()` blocks until the
whole tool-calling loop finishes and returns one string — fine for a
CLI, wrong for a UI that wants to show live progress and, per the
"ask a new question while the first still runs" requirement, run
several of these loops at once without one blocking another.
`agent.astream(stream_mode=["updates", "messages"])` is exactly that:
"messages" for the model's own text as it is produced, "updates" for
node-level progress (a tool call and its result) — see
https://docs.langchain.com/oss/python/langgraph/streaming for the
stream_mode contract this relies on.

**Concurrency safety, stated explicitly rather than assumed.**
`thread_id` goes into `config["configurable"]` — LangGraph's own
correlation mechanism, so a checkpointer (none is configured here) would
keep each thread's state separate. What actually makes two concurrent
calls to `run_investigation_stream` safe here is simpler: each call
creates its own local `agent`/execution state via `create_agent(...)`,
and neither shares mutable state with the other — the only shared
objects are `model` and `tools`, which a real chat model client (e.g.
`ChatOpenAI`) and `GraphOwlToolkit`'s tools are both safe to call
concurrently from multiple coroutines, the same way any HTTP client is.
`thread_id` is this module's own bookkeeping key (which stream belongs
to which question), not a LangGraph state-isolation requirement — stated
here so a future change does not assume a checkpointer is doing work
that was never wired in.
"""

from __future__ import annotations

from collections.abc import AsyncIterator
from dataclasses import dataclass
from typing import Any

#: Same bound and same reasoning as `gst_investigation_agent.py`'s
#: `DEFAULT_RECURSION_LIMIT` — a loop reacting to a failure must state
#: explicitly what makes it terminate (`CLAUDE.md`'s build/test-loop
#: section). Not re-derived independently; this playground runs the same
#: agent shape against the same tool surface, so the same bound applies.
DEFAULT_RECURSION_LIMIT = 40


@dataclass(frozen=True)
class StreamChunk:
    """One piece of progress from a running investigation.

    `kind` mirrors the two `stream_mode`s requested: `"message"` is a
    text fragment from the model (a UI renders this directly, token by
    token if the underlying model streams natively, or as one chunk
    otherwise — `BaseChatModel`'s own default `stream()` falls back to a
    single complete chunk when a subclass implements only `_generate`).
    `"update"` is a node-level event, typically a tool call and its
    result — carried in `data` rather than `text`, since it is
    structured detail a UI may show separately (a "thinking" trace),
    not prose to append to the answer.
    """

    kind: str  # "message" | "update"
    text: str = ""
    data: Any = None


async def run_investigation_stream(
    model: Any,
    tools: list[Any],
    question: str,
    thread_id: str,
    system_prompt: str,
    recursion_limit: int = DEFAULT_RECURSION_LIMIT,
) -> AsyncIterator[StreamChunk]:
    """Run one question through a real tool-calling loop, yielding
    progress as it happens rather than blocking for the final answer.

    Two calls to this function, awaited concurrently (`asyncio.gather`,
    or two independently-scheduled tasks — which is what `server.py`
    does per incoming question), run their own independent tool-calling
    loops. Neither call blocks the other; a UI can start a second
    question while the first is still streaming.
    """
    from langchain.agents import create_agent

    agent = create_agent(model, tools, system_prompt=system_prompt)
    config = {
        "configurable": {"thread_id": thread_id},
        "recursion_limit": recursion_limit,
    }
    async for stream_mode, payload in agent.astream(
        {"messages": [("user", question)]},
        config=config,
        stream_mode=["updates", "messages"],
    ):
        if stream_mode == "messages":
            chunk, _metadata = payload
            text = getattr(chunk, "content", "") or ""
            if text:
                yield StreamChunk(kind="message", text=text)
        elif stream_mode == "updates":
            yield StreamChunk(kind="update", data=payload)
