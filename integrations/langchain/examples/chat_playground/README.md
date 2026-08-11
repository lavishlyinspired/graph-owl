# chat playground (local dev tool — not shipped)

A live, streaming chat UI over the GST investigation agent from
`../gst_investigation_agent.py`, with one property that CLI-only tool
does not have: you can ask a second question while the first is still
running. Each question gets its own independent streaming run — no
locking, no queue.

**Never embedded in graph-owl-server or `ui/` (the shipped console).**
See `streaming.py`'s docstring for why: `graph_owl_langchain`'s own
`pyproject.toml` says "never a chain, an agent, or a runtime"
(`plans/00j-language-boundaries.md`), and this directory stays outside
that boundary the same way `gst_investigation_agent.py` already does.

## Run it

```bash
cd integrations/langchain
python3 -m venv .venv && source .venv/bin/activate   # if you don't already have one
pip install -e ".[langgraph]" langchain-openai fastapi uvicorn

export LLM_API_BASE_URL=...       # any OpenAI-compatible endpoint
export LLM_MODEL=...
export LLM_API_KEY=...            # if your endpoint needs one
export GRAPH_OWL_SERVER=http://localhost:8080
export GRAPH_OWL_TOKEN=...        # a token graph-owl-server accepts

python3 -m uvicorn examples.chat_playground.server:app --port 8899
```

Then serve `static/index.html` over HTTP (a browser's `fetch`/
`EventSource` from a `file://` page is unreliable in some browsers, so
this is the one extra step) and open it:

```bash
cd examples/chat_playground/static
python3 -m http.server 8898
# open http://127.0.0.1:8898/index.html
```

If the FastAPI server runs on a different host/port, set
`window.GRAPH_OWL_CHAT_SERVER = "http://your-host:port"` in the page's
`<head>` (or open the browser console and set it) before asking a
question — it defaults to `http://localhost:8899`.

## What it proves, and what it deliberately doesn't do yet

- **Proven**: `tests/test_chat_playground.py` (in `integrations/
  langchain/tests/`) runs two investigations concurrently with a
  scripted model and asserts neither's answer leaks into the other's
  stream — the actual property "ask while the first runs" depends on.
  Verified live too: two real HTTP requests against a running server, a
  browser asking two questions back to back, both streaming
  independently, confirmed via screenshot and zero console errors.
- **Not attempted**: multi-browser-tab fan-out to the same thread,
  reconnect-mid-stream beyond a full-text replay on connect, a
  tool-call trace in the UI (the `"update"` stream chunks exist in
  `streaming.py`'s output but `server.py` does not forward them to the
  browser yet), and persistence across a server restart (`_THREADS`
  is in-process memory only — this is a personal dev tool, not a
  service).
