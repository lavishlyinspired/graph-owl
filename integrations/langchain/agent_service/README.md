# agent service

The backend the console's **Agent** tab calls, and a live, streaming chat
UI over the same investigation agent from `../gst_investigation_agent.py`
(displayed as "Reconciliation Agent" — the only agent today, with room
to add more later). You can ask a second question while the first is
still running: each question gets its own independent streaming run, no
locking, no queue.

**Still a separate Python process — never ported into `graph-owl-server`
(Rust) or the engine.** See `streaming.py`'s docstring: `graph_owl_
langchain`'s own `pyproject.toml` says "never a chain, an agent, or a
runtime" (`plans/00j-language-boundaries.md`). The console's React
frontend is a *consumer* of this service over HTTP, the same relationship
it already has with `graph-owl-server` itself — not this service becoming
part of the engine. This directory used to live under `examples/` as a
standalone dev tool; it moved to sit directly under `integrations/
langchain/` once the console started depending on it directly, but the
process boundary hasn't changed.

## Run it

```bash
cd integrations/langchain
python3 -m venv .venv && source .venv/bin/activate   # if you don't already have one
pip install -e ".[langgraph]" langchain-openai fastapi uvicorn

# This process does no .env loading of its own — it only sees what the
# launching shell exports. The repo root's .env already has
# LLM_API_BASE_URL/LLM_MODEL/LLM_API_KEY, so source it rather than
# retyping them:
set -a; source ../../.env; set +a
export GRAPH_OWL_SERVER=http://localhost:8080

python3 -m uvicorn agent_service.server:app --port 8899
```

The console sends its own signed-in user's access token with every
request (`Authorization: Bearer ...`), so an investigation run from the
console sees exactly what that user is authorized to see — this service
never uses a fixed identity for console-originated questions. If you're
calling `/questions` some other way with no token of its own (the
standalone `static/index.html` page below, or a raw `curl`), set
`GRAPH_OWL_TOKEN` in the environment as a fallback.

**Also required when the console itself runs with OIDC off**
(`OIDC_ISSUER=`, the open-mode demo config): with no OIDC provider there
is no signed-in access token for the browser to forward, so every
console-originated question hits this fallback too — set
`GRAPH_OWL_TOKEN` to any non-empty placeholder in that case.
graph-owl-server ignores the token's actual value in open mode (every
request resolves to the system principal regardless), so the value
itself carries no meaning there; this check is agent_service's own gate,
not graph-owl-server's.

### Standalone chat page (no console needed)

Still works, for quick manual testing without the full console running.
Serve `static/index.html` over HTTP (a browser's `fetch`/`EventSource`
from a `file://` page is unreliable in some browsers, so this is the one
extra step) and open it:

```bash
export GRAPH_OWL_TOKEN=...   # required here - no browser session to draw a token from
cd agent_service/static
python3 -m http.server 8898
# open http://127.0.0.1:8898/index.html
```

If the FastAPI server runs on a different host/port, set
`window.GRAPH_OWL_CHAT_SERVER = "http://your-host:port"` in the page's
`<head>` (or open the browser console and set it) before asking a
question — it defaults to `http://localhost:8899`.

## What it proves, and what it deliberately doesn't do yet

- **Proven**: `tests/test_agent_service.py` runs two investigations
  concurrently with a scripted model and asserts neither's answer leaks
  into the other's stream — the actual property "ask while the first
  runs" depends on. It also proves real per-token delta forwarding (a
  scripted model implementing `_stream`/`_astream` for real, not just
  `BaseChatModel`'s single-blob fallback) and that a turn following a
  tool call gets a paragraph break rather than running on into the
  previous turn's last word. `tests/test_agent_service_server.py` runs a
  real `uvicorn` server and proves each question authenticates as its own
  caller's token, not a shared one. Verified live too: two real HTTP
  requests against a running server, a browser asking two questions back
  to back, both streaming independently, confirmed via screenshot and
  zero console errors; a real DeepSeek model answering a real question
  against real GST data with clean, correctly-paragraphed prose (no
  tool-output leakage — a real bug found and fixed live, see
  `streaming.py`'s `isinstance(chunk, AIMessage)` check), each of 7 real
  tool calls rendered as a live "Using X…" → "✓ Used X" badge in the
  console's Agent tab.
- **Not built on `create_agent`**: replaced with a hand-rolled
  `.astream()`-based ReAct loop (`run_investigation_stream`) — see that
  function's own docstring in `streaming.py` for why; the short version
  is that `create_agent`'s model node always calls `.ainvoke(...)`, never
  `.astream(...)`, so no `stream_mode` the outer graph requests
  guarantees a real token delta reaches the caller.
- **Not attempted**: multi-browser-tab fan-out to the same thread,
  reconnect-mid-stream beyond a full-text-and-activity replay on connect,
  and persistence across a server restart (`_THREADS` is in-process
  memory only). The DeepSeek `reasoning_content` upstream bug
  (`langchain-ai/langchain` issues #34166, #37174) remains open and can
  still 400 a sufficiently long investigation on its second-or-later
  tool-calling turn — not fixable here without patching a third-party
  library or switching off reasoning mode.
