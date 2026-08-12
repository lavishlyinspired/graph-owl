"""Streaming investigation runs, one per thread — the core the FastAPI
server in `server.py` wraps.

**A real backend the console's "Agent" tab talks to directly, not a
throwaway example.** Originally built as a standalone dev tool
(`examples/chat_playground/`); the console now embeds a chat panel that
calls this service's HTTP API over the network, which is why it moved to
`agent_service/`, a sibling of `examples/` rather than a child of it.
What has **not** changed: this is still a separate Python process, run
independently of `graph-owl-server` (the Rust binary) — `graph_owl_
langchain`'s own `pyproject.toml` says "never a chain, an agent, or a
runtime" (`plans/00j-language-boundaries.md`, the ROADMAP's refusal
table), and porting a LangGraph tool-calling loop into Rust would mean
building an agent runtime inside the engine, which is exactly the layer
that refusal exists to hold the line on. The console's React frontend is
a *consumer* of this service (the same relationship it already has with
graph-owl-server itself), not this service becoming part of the engine.
Nothing in this file imports anything under `crates/`, and nothing under
`crates/` imports this — the process boundary is the language boundary,
unchanged by where the directory sits.

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

**Not built on `langchain.agents.create_agent`, on purpose — found
live, 12 August 2026.** The first version called `create_agent(...)
.astream(stream_mode=["updates", "messages"])`, matching
https://docs.langchain.com/oss/python/langgraph/streaming's own
documented contract. It worked in every scripted test (a scripted model
implementing `_stream` really does forward per-chunk deltas through that
path) but showed no visible token-by-token typing against the real
model in the console — text arrived as one block. Root cause, found by
reading `create_agent`'s own source
(`langchain/agents/factory.py::_execute_model_async`): its model node
always calls `await model_.ainvoke(messages)`, never `.astream(...)`.
Whether that still produces incremental deltas depends on LangChain's
callback-triggered auto-streaming kicking in for the specific
model/provider — for this deployment's DeepSeek endpoint, it did not.
Rather than depend on that indirection, this module calls
`model.astream(messages)` directly, in a hand-rolled ReAct loop: stream
the model's response, merge the `AIMessageChunk`s as they arrive
(`+=`, which LangChain's own chunk type supports for both `content` and
`tool_call_chunks`), yield each content delta immediately, then either
stop (no tool calls) or invoke each tool call directly
(`tool.ainvoke(call)`, which returns a `ToolMessage` with the right
`tool_call_id` set automatically) and loop. This is a deliberately
minimal reimplementation of what `create_agent` already does, with one
difference — the model call streams for real — not a wholesale rebuild
of agentic tool-calling.

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

    `"message"` is a text delta from the model — a UI appends each one
    to what it has already shown, the same way Cursor/Claude/ChatGPT's
    own chat views grow a response token by token rather than replacing
    it. `"update"` is a tool-call lifecycle event — `data` is always one
    of two small, JSON-serializable shapes a UI can render as a clean
    "Using <tool>…" / "✓ <tool>" line, never the tool's own raw output:

    - `{"phase": "tool_call", "tool": name, "args": {...}}`
    - `{"phase": "tool_result", "tool": name, "ok": bool}`
    """

    kind: str  # "message" | "update"
    text: str = ""
    data: Any = None


#: The exact substring the OpenAI SDK's rendered error carries for the
#: known, open upstream bug (langchain-ai/langchain issues #34166,
#: #37174): a DeepSeek thinking-mode model 400s a request that echoes
#: back a prior tool-calling turn without that turn's own
#: `reasoning_content`, because `ChatOpenAI`'s message serialization
#: does not round-trip that field. Narrow and literal on purpose — this
#: must trigger a fallback for exactly this bug, never for an unrelated
#: 400 (a malformed tool call, say) that happens to also be worth
#: knowing about as a real failure.
_REASONING_CONTENT_ERROR_MARKER = "reasoning_content"


async def run_investigation_stream(
    model: Any,
    tools: list[Any],
    question: str,
    thread_id: str,  # noqa: ARG001 - bookkeeping key for the caller (server.py), not used here; kept in the signature so a future checkpointer-backed version has an obvious place to plug in
    system_prompt: str,
    recursion_limit: int = DEFAULT_RECURSION_LIMIT,
    fallback_model: Any | None = None,
) -> AsyncIterator[StreamChunk]:
    """Run one question through a hand-rolled tool-calling loop, yielding
    progress — including real token deltas — as it happens rather than
    blocking for the final answer.

    Two calls to this function, awaited concurrently (`asyncio.gather`,
    or two independently-scheduled tasks — which is what `server.py`
    does per incoming question), run their own independent loops.
    Neither shares state with the other: each has its own local
    `messages` list, and the only shared objects (`model`, `tools`) are
    the same kind of concurrency-safe HTTP-backed clients any two
    coroutines already share safely.

    # Raises

    `RuntimeError` if the model has not stopped calling tools after
    `recursion_limit` turns — surfaced, not swallowed, matching
    `gst_investigation_agent.py`'s own `GraphRecursionError` posture: a
    silent partial answer at the cap would look like a real conclusion.
    """
    from langchain_core.messages import (
        AIMessage,
        AIMessageChunk,
        HumanMessage,
        SystemMessage,
        ToolMessage,
    )

    tools_by_name = {tool.name: tool for tool in tools}
    model_with_tools = model.bind_tools(tools) if tools else model
    # Bound once, up front, rather than re-bound on every fallback — the
    # tool set never changes mid-investigation, only which underlying
    # model is answering.
    fallback_with_tools = (
        fallback_model.bind_tools(tools) if fallback_model and tools else fallback_model
    )
    active_model = model_with_tools
    messages: list[Any] = [SystemMessage(content=system_prompt), HumanMessage(content=question)]

    for turn in range(recursion_limit):
        accumulated: AIMessage | None = None
        started_this_turns_prose = False
        attempted_fallback_this_turn = False
        while True:
            try:
                async for piece in active_model.astream(messages):
                    if not isinstance(piece, AIMessage):
                        continue
                    # A model that streams natively yields many AIMessageChunks,
                    # merged via `+` as they arrive. A model with no `_stream`
                    # override falls back to exactly one plain AIMessage (not a
                    # Chunk) carrying the whole response — nothing to merge, and
                    # `+` is not defined on the base type, so that single piece
                    # is used as-is rather than accumulated.
                    if isinstance(piece, AIMessageChunk):
                        accumulated = piece if accumulated is None else accumulated + piece
                    else:
                        accumulated = piece
                    if piece.content:
                        # A turn after the first (i.e. one that follows at least
                        # one tool call) starts a new paragraph rather than
                        # butting directly up against the previous turn's last
                        # word — found live, 12 August 2026: "...tables
                        # relatedThe catalog has 15 tables..." with no separator
                        # at all between two consecutive turns' prose.
                        if turn > 0 and not started_this_turns_prose:
                            yield StreamChunk(kind="message", text="\n\n")
                        started_this_turns_prose = True
                        yield StreamChunk(kind="message", text=piece.content)
                break
            except Exception as exc:
                # A narrow, named recovery for one known, open upstream
                # bug (langchain-ai/langchain #34166, #37174) — a
                # thinking-mode model 400s once a prior tool-calling
                # turn's `reasoning_content` isn't echoed back, which
                # `ChatOpenAI`'s own message serialization never does.
                # Guarded on all four conditions: a fallback exists, this
                # turn hasn't already tried it, this turn isn't already
                # running on the fallback, and no content was received
                # yet this attempt (a 400 arrives before any chunk is
                # streamed, so a mid-stream failure — accumulated not
                # None — is a different, real problem and must not be
                # retried against a different model with a half-built
                # response already in flight). Anything not matching all
                # four re-raises, so an unrelated failure (rate limit,
                # real outage) still fails the investigation rather than
                # being silently absorbed.
                should_fall_back = (
                    fallback_with_tools is not None
                    and not attempted_fallback_this_turn
                    and active_model is not fallback_with_tools
                    and accumulated is None
                    and _REASONING_CONTENT_ERROR_MARKER in str(exc)
                )
                if not should_fall_back:
                    raise
                attempted_fallback_this_turn = True
                active_model = fallback_with_tools
                yield StreamChunk(
                    kind="update",
                    data={"phase": "model_fallback", "reason": "reasoning_content"},
                )

        if accumulated is None:
            return
        messages.append(accumulated)

        tool_calls = accumulated.tool_calls
        if not tool_calls:
            return

        for call in tool_calls:
            yield StreamChunk(
                kind="update",
                data={"phase": "tool_call", "tool": call["name"], "args": call["args"]},
            )
            tool = tools_by_name.get(call["name"])
            if tool is None:
                result = ToolMessage(
                    content=f"no such tool: {call['name']}", tool_call_id=call["id"]
                )
                ok = False
            else:
                result = await tool.ainvoke(call)
                ok = True
            messages.append(result)
            yield StreamChunk(
                kind="update", data={"phase": "tool_result", "tool": call["name"], "ok": ok}
            )

    raise RuntimeError(f"agent did not stop calling tools after {recursion_limit} turns")
