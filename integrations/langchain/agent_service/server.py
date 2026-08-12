"""The FastAPI shim around `streaming.run_investigation_stream` — the
service the console's Agent tab calls.

Run it directly:

    cd integrations/langchain
    pip install fastapi uvicorn langchain-openai
    LLM_API_BASE_URL=... LLM_MODEL=... GRAPH_OWL_SERVER=http://localhost:8080 \\
        python -m uvicorn agent_service.server:app --port 8899

The console's own access token travels with every request (see `ask`
below) and becomes the principal the agent's tools act as — an
investigation run from the console sees exactly what its signed-in user
is authorized to see, not a fixed service identity. `GRAPH_OWL_TOKEN` in
the environment is a fallback for running this service directly (as
`examples/chat_playground/static/index.html` still does, or its own
successor) without a browser session to draw a token from.

**Why "ask while the first still runs" needs no locking.** `POST
/questions` returns as soon as a background `asyncio.Task` is scheduled
— it does not await the investigation. The caller can `POST` a second
question immediately; the first task keeps running independently.
Nothing here serializes two investigations against each other, because
nothing they touch is shared mutable state per-question (each question
gets its own `asyncio.Queue` in `_THREADS`).

**In-memory only, single-process.** `_THREADS` lives in process memory
and is lost on restart — there is no persistence story yet, matching
this service's current scope (one question, one answer, not a saved
conversation history). CORS stays permissive (`allow_origins=["*"]`)
deliberately: the real authorization boundary is the token each request
carries through to graph-owl-server itself, not this service's own CORS
policy — a caller with no valid token gets nothing useful back regardless
of origin.
"""

from __future__ import annotations

import asyncio
import json
import os
import sys
import uuid
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

# `agent_service` moved out from under `examples/` to sit directly under
# `integrations/langchain/`, a sibling of both `graph_owl_langchain/` (the
# package) and `examples/` (where `gst_investigation_agent.py` still
# lives) — so, unlike before the move, these are two genuinely different
# directories and need their own sys.path entries rather than one shared
# insert covering both imports below.
_LANGCHAIN_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(_LANGCHAIN_ROOT))
sys.path.insert(0, str(_LANGCHAIN_ROOT / "examples"))

from gst_investigation_agent import (  # noqa: E402
    SYSTEM_PROMPT,
    build_chat_model,
    build_fallback_chat_model,
)

from agent_service.files import get_file, store_file  # noqa: E402
from agent_service.reconcile_uploaded import reconcile_uploaded_files  # noqa: E402
from agent_service.streaming import run_investigation_stream  # noqa: E402
from graph_owl_langchain._core.principal import Principal  # noqa: E402
from graph_owl_langchain.tools import GraphOwlToolkit  # noqa: E402

try:
    from fastapi import FastAPI, Header, HTTPException
    from fastapi.middleware.cors import CORSMiddleware
    from fastapi.responses import StreamingResponse
except ImportError as missing:  # pragma: no cover - exercised only by running the server
    raise SystemExit(
        "fastapi and uvicorn are needed to run the server "
        "(pip install fastapi uvicorn) — streaming.py itself has no such "
        "dependency and is unaffected"
    ) from missing


@dataclass
class _Thread:
    question: str
    status: str = "running"  # "running" | "done" | "error"
    text: str = ""
    error: str | None = None
    activity: list[dict[str, Any]] = field(default_factory=list)
    queue: asyncio.Queue = field(default_factory=asyncio.Queue)
    # [{fileId, name}], for the UI's attached-file chips
    files: list[dict[str, str]] = field(default_factory=list)


_THREADS: dict[str, _Thread] = {}


def _files_context_note(files: list[dict[str, str]]) -> str:
    """Turns the attached files into a short, explicit note prepended to
    the question — the only way the model learns a real file ID exists
    to pass to `reconcile_uploaded_files`, since IDs are server-assigned
    UUIDs the model could never guess. Empty when there are no
    attachments, so a question with none reads exactly as it always
    did."""
    if not files:
        return ""
    lines = "\n".join(f'- {f["fileId"]}: "{f["name"]}"' for f in files)
    return (
        "The user has attached the following file(s) to this question. "
        "Use their exact IDs (not their names) with any tool that takes "
        f"a file ID:\n{lines}\n\n"
    )


async def _run_and_publish(thread_id: str, question: str, token: str) -> None:
    """The background task `POST /questions` schedules and does not await.

    `token` is the caller's own — passed in by `ask` below, never read
    from the environment here. The toolkit built from it means every tool
    call this investigation makes reaches graph-owl-server as that
    specific caller, subject to their own authorization, not a shared
    service-level identity.

    Publishes each chunk onto the thread's own queue as it arrives, so a
    concurrently-running `GET .../stream` sees it immediately; also
    accumulates the full text onto `_Thread.text` so a stream that
    connects *after* the run finished (or a reconnect) can still read
    the complete answer rather than an empty tail.
    """
    thread = _THREADS[thread_id]
    try:
        model = build_chat_model()
        fallback_model = build_fallback_chat_model()
        toolkit = GraphOwlToolkit(
            endpoint=os.environ.get("GRAPH_OWL_SERVER", "http://localhost:8080"),
            principal=Principal(token=token),
        )
        # The uploaded-file comparison tool is only offered when a
        # question actually has files attached — an investigation with
        # nothing attached sees exactly the same tool set it always did,
        # rather than a permanently larger one the model has to reason
        # past on every question.
        tools = toolkit.tools() + ([reconcile_uploaded_files] if thread.files else [])
        question_with_files = _files_context_note(thread.files) + question
        async for chunk in run_investigation_stream(
            model,
            tools,
            question_with_files,
            thread_id,
            SYSTEM_PROMPT,
            fallback_model=fallback_model,
        ):
            if chunk.kind == "message":
                thread.text += chunk.text
            elif chunk.kind == "update":
                thread.activity.append(chunk.data)
            await thread.queue.put(chunk)
        thread.status = "done"
    except Exception as exc:  # noqa: BLE001 - surfaced to the UI, not swallowed; asyncio.CancelledError is a BaseException, unaffected
        thread.status = "error"
        thread.error = str(exc)
    finally:
        await thread.queue.put(None)  # sentinel: stream ends


app = FastAPI(title="graph-owl agent service")
app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_methods=["*"],
    allow_headers=["*"],
)


def _caller_token(authorization: str | None) -> str | None:
    """The bearer token this specific request carried, or `None` if it
    carried none. A missing header is not an error here — `ask` falls
    back to `GRAPH_OWL_TOKEN` for the standalone-tool case — so this
    stays a pure extraction, no raising."""
    if authorization and authorization.lower().startswith("bearer "):
        return authorization[len("bearer ") :].strip()
    return None


@app.post("/questions")
async def ask(
    body: dict[str, Any], authorization: str | None = Header(default=None)
) -> dict[str, str]:
    question = body.get("question", "").strip()
    if not question:
        raise HTTPException(status_code=400, detail="question is required")
    token = _caller_token(authorization) or os.environ.get("GRAPH_OWL_TOKEN")
    if not token:
        raise HTTPException(
            status_code=401,
            detail="no bearer token supplied, and GRAPH_OWL_TOKEN is not set as a fallback",
        )
    files: list[dict[str, str]] = []
    for file_id in body.get("fileIds", []):
        record = get_file(file_id)
        if record is None:
            raise HTTPException(status_code=404, detail=f"no such uploaded file: {file_id}")
        files.append({"fileId": record.file_id, "name": record.name})
    thread_id = str(uuid.uuid4())
    _THREADS[thread_id] = _Thread(question=question, files=files)
    asyncio.create_task(_run_and_publish(thread_id, question, token))
    return {"threadId": thread_id}


@app.post("/files")
async def upload_file(body: dict[str, Any]) -> dict[str, Any]:
    """Stores an uploaded file's raw text content in memory, keyed by a
    fresh UUID the caller then attaches to a question's `fileIds` (see
    `ask` above) or reads back via `GET /files/{file_id}` for a preview.
    No auth on this route deliberately: the file itself carries no
    graph-owl authorization semantics (it's the user's own local file,
    not a catalog asset), and the token that matters — the caller's own
    — is only meaningful once it's used against graph-owl-server, which
    happens at `ask` time, not at upload time.
    """
    name = body.get("name", "").strip()
    content = body.get("content")
    if not name or content is None:
        raise HTTPException(status_code=400, detail="name and content are required")
    if len(content) > 5_000_000:
        raise HTTPException(status_code=413, detail="file too large (5,000,000 character limit)")
    record = store_file(name, body.get("contentType", "text/plain"), content)
    return {
        "fileId": record.file_id,
        "name": record.name,
        "contentType": record.content_type,
        "size": len(record.content),
    }


@app.get("/files/{file_id}")
async def read_file(file_id: str) -> dict[str, Any]:
    record = get_file(file_id)
    if record is None:
        raise HTTPException(status_code=404, detail="no such uploaded file")
    return {
        "fileId": record.file_id,
        "name": record.name,
        "contentType": record.content_type,
        "content": record.content,
        "size": len(record.content),
    }


@app.get("/questions")
async def list_threads() -> list[dict[str, Any]]:
    return [
        {"threadId": tid, "question": t.question, "status": t.status, "text": t.text}
        for tid, t in _THREADS.items()
    ]


@app.get("/questions/{thread_id}/stream")
async def stream(thread_id: str) -> StreamingResponse:
    thread = _THREADS.get(thread_id)
    if thread is None:
        raise HTTPException(status_code=404, detail="no such question")

    def _done_event() -> str:
        payload = {"kind": "done", "status": thread.status, "error": thread.error}
        return f"data: {json.dumps(payload)}\n\n"

    async def events():
        # Replay what already happened before this connection opened —
        # a browser tab switched away and back must not lose the
        # transcript so far. Activity (tool calls) first, then the text
        # accumulated so far: an approximation of true arrival order (the
        # two are not stored pre-interleaved), acceptable for a
        # reconnect/late-join view where exact ordering matters far less
        # than for the live stream.
        for activity in thread.activity:
            yield f"data: {json.dumps({'kind': 'update', 'data': activity})}\n\n"
        if thread.text:
            yield f"data: {json.dumps({'kind': 'message', 'text': thread.text})}\n\n"
        if thread.status != "running":
            yield _done_event()
            return
        while True:
            chunk = await thread.queue.get()
            if chunk is None:
                yield _done_event()
                return
            if chunk.kind == "message":
                yield f"data: {json.dumps({'kind': 'message', 'text': chunk.text})}\n\n"
            elif chunk.kind == "update":
                # A clean, structured tool-call lifecycle event (see
                # streaming.py's StreamChunk docstring for the two
                # `data` shapes) — a UI renders this as "Using <tool>…"
                # then "✓ <tool>", never the tool's own raw output.
                yield f"data: {json.dumps({'kind': 'update', 'data': chunk.data})}\n\n"

    return StreamingResponse(events(), media_type="text/event-stream")
