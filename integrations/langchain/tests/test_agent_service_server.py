"""`agent_service.server` — the FastAPI shim's own HTTP behavior, as
opposed to `test_agent_service.py`'s coverage of the framework-agnostic
streaming core it wraps.

Runs a real `uvicorn.Server` in-process against a real `httpx.AsyncClient`
rather than `starlette.testclient.TestClient` — found live while building
this: `TestClient` does not keep one persistent event loop across separate
`.post()`/`.get()` calls, so a fire-and-forget `asyncio.create_task`
scheduled inside a request handler gets silently dropped between calls.
A real server, one event loop, real HTTP — the same thing production
actually does.
"""

from __future__ import annotations

import asyncio
import os
import sys
from pathlib import Path
from typing import Any

import httpx
import pytest
import uvicorn
from langchain_core.language_models.chat_models import BaseChatModel
from langchain_core.messages import AIMessage
from langchain_core.outputs import ChatGeneration, ChatResult

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

os.environ.setdefault("GRAPH_OWL_TOKEN", "fallback-service-token")

from agent_service import server as agent_server  # noqa: E402


class _Scripted(BaseChatModel):
    steps: list = []
    calls: list = []

    def _generate(
        self, messages, stop=None, run_manager: Any = None, **kwargs: Any
    ) -> ChatResult:
        step = len(self.calls)
        self.calls.append(step)
        return ChatResult(generations=[ChatGeneration(message=self.steps[step])])

    @property
    def _llm_type(self) -> str:
        return "scripted"

    def bind_tools(self, tools: Any, **kwargs: Any) -> _Scripted:
        return self


class _RecordingToolkit:
    """Records the token each construction was given — this is the whole
    point of the test: a real toolkit is built fresh per question, using
    whichever principal that specific request supplied."""

    seen_tokens: list[str] = []

    def __init__(self, endpoint: str, principal: Any) -> None:
        _RecordingToolkit.seen_tokens.append(principal.token)

    def tools(self) -> list[Any]:
        return []


@pytest.fixture(autouse=True)
def _patched(monkeypatch):
    _RecordingToolkit.seen_tokens = []
    monkeypatch.setattr(
        agent_server, "build_chat_model", lambda: _Scripted(steps=[AIMessage(content="ok")])
    )
    monkeypatch.setattr(agent_server, "GraphOwlToolkit", _RecordingToolkit)
    agent_server._THREADS.clear()
    yield


async def _run_server_and(port: int, body_coro):
    config = uvicorn.Config(agent_server.app, host="127.0.0.1", port=port, log_level="warning")
    srv = uvicorn.Server(config)
    task = asyncio.create_task(srv.serve())
    for _ in range(50):
        if getattr(srv, "started", False):
            break
        await asyncio.sleep(0.05)
    try:
        return await body_coro(f"http://127.0.0.1:{port}")
    finally:
        srv.should_exit = True
        await task


def test_each_question_uses_its_own_callers_token_not_a_shared_one():
    async def body(base: str):
        async with httpx.AsyncClient() as client:
            r1 = await client.post(
                f"{base}/questions",
                json={"question": "q1"},
                headers={"authorization": "Bearer caller-token-A"},
            )
            r2 = await client.post(
                f"{base}/questions",
                json={"question": "q2"},
                headers={"authorization": "Bearer caller-token-B"},
            )
            for r in (r1, r2):
                assert r.status_code == 200, r.text
            # give the background tasks a moment to construct their toolkit
            await asyncio.sleep(0.3)
        return None

    asyncio.run(_run_server_and(8951, body))

    assert set(_RecordingToolkit.seen_tokens) == {"caller-token-A", "caller-token-B"}, (
        "each question must authenticate as its own caller, not a shared service token: "
        f"{_RecordingToolkit.seen_tokens}"
    )


def test_a_question_with_no_token_falls_back_to_the_service_token():
    """Backward compatible with the standalone-playground use case
    (README.md's own documented `GRAPH_OWL_TOKEN` env-var flow) — the
    console always sends its own token, but running this service directly
    still works exactly as before."""

    async def body(base: str):
        async with httpx.AsyncClient() as client:
            r = await client.post(f"{base}/questions", json={"question": "q"})
            assert r.status_code == 200, r.text
            await asyncio.sleep(0.3)
        return None

    asyncio.run(_run_server_and(8952, body))

    assert _RecordingToolkit.seen_tokens == ["fallback-service-token"]
