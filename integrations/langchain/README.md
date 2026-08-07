# graph-owl-langchain

LangChain and LangGraph adapters over graph-owl's MCP surface: a retriever,
a toolkit, and a checkpointer. Never a chain, an agent, or a runtime — see
`plans/43-framework-integrations.md` for why the boundary is drawn there.

## Install

```bash
pip install "graph-owl-langchain[langchain]"
```

Add `[langgraph]` too for `GraphOwlCheckpointer`, or to drive `GraphOwlToolkit`'s
tools through a LangGraph agent as shown below.

## Quickstart: retrieval in under twenty lines

```python
from graph_owl_langchain.retrievers import GraphOwlRetriever
from graph_owl_langchain._core.principal import Principal

retriever = GraphOwlRetriever(
    endpoint="http://localhost:8080",
    principal=Principal(token="<your agent token>"),
)

docs = retriever.invoke("customer churn model")
for doc in docs:
    print(doc.page_content)
```

Each returned `Document` is one asset's context — lineage and recalled
memory rendered as text, confidence-filtered at `min_confidence` (default
`0.5`, the same ignore-band ceiling the console uses) — ready to drop into
any LangChain prompt or chain.

## The toolkit: let an agent call graph-owl itself

```python
from langgraph.prebuilt import create_react_agent
from graph_owl_langchain.tools import GraphOwlToolkit
from graph_owl_langchain._core.principal import Principal

toolkit = GraphOwlToolkit(
    endpoint="http://localhost:8080",
    principal=Principal(token="<your agent token>"),
)
agent = create_react_agent(model, toolkit.tools())
```

`toolkit.tools()` is built from the live server's own `tools/list` manifest
— no tool is hand-declared here, so a server that adds or removes a tool
changes what the agent can do without a release of this package.

## The checkpointer: LangGraph state as graph-owl memory

```python
from graph_owl_langchain.memory import GraphOwlCheckpointer
from graph_owl_langchain._core.principal import Principal

checkpointer = GraphOwlCheckpointer(
    endpoint="http://localhost:8080",
    principal=Principal(token="<your agent token>"),
)
graph = builder.compile(checkpointer=checkpointer)
```

**Before first use**, a human admin must (a) create a real catalog asset at
the FQN the checkpointer writes to (`dsc:langgraph-checkpoint/{thread_id}`,
via `POST /ingest`) and (b) grant that asset's principal the `recordMemory`
capability (`PUT /agents/{agent_id}/grant`). Both are admin-only and
human-only by Epic 32's own design — see `graph_owl_langchain/memory.py`'s
module docstring for the full finding. This is a genuine deployment
prerequisite, not a bug: nothing in this package can, or should, work
around it.

## Authentication and token refresh

`Principal` takes an optional `refresh` callable, invoked at most once per
request on a 401:

```python
Principal(token=get_token(), refresh=get_token)
```

## Development

```bash
python3 -m venv .venv
.venv/bin/pip install -e ".[dev]"
.venv/bin/pytest              # full suite; live-server tests skip without one
.venv/bin/ruff check .
.venv/bin/mypy
```

To run the live-server tests too: `scripts/verify-langchain.sh` from the
repository root starts a real Postgres and `graph-owl-server`, then runs
this package's suite with `GRAPH_OWL_TEST_ENDPOINT` set — the same job CI
runs on every PR (`.github/workflows/ci.yml`, `langchain-integration`).

## What this package will never do

Per `plans/00j-language-boundaries.md`: no chain, no agent loop, no
prompt template, no LLM client. It renders graph-owl's data as LangChain
primitives (`Document`, `BaseTool`, `BaseCheckpointSaver`) and nothing more
— composing them into an agent is the caller's decision, not this
package's.
