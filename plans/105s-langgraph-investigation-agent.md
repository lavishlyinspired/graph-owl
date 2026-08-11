# Plan: a LangGraph investigation agent over the P10 tools — Epic 105 P11

**Branch**: main (committed directly, per `CLAUDE.md`)
**Status**: In progress, 11 August 2026 — by direct user instruction after
an architectural placement question ("use langchain langgraph with
python").

**Crates/packages**: `integrations/langchain/examples/` (new directory,
new files only — `graph_owl_langchain` itself is untouched).

## The placement question this had to answer first

The task list names P11 "One GraphOwl Agent (Python, LangGraph)". Building
it naively — inside `graph_owl_langchain`, or under the top-level
`examples/` — is blocked by three independent, pre-existing statements
already on record in this codebase:

1. `integrations/langchain/pyproject.toml`'s own `description`: "LangChain
   and LangGraph adapters over graph-owl: a retriever, a toolkit, and a
   checkpointer — **never a chain, an agent, or a runtime**."
2. `plans/00j-language-boundaries.md`: "LangChain, LangGraph, agent
   frameworks... are not part of graph-owl. They are clients of it."
3. `plans/ROADMAP.md`'s refusal table: "A hosted agent runtime or prebuilt
   agents | ... | **Never**."

And separately, `scripts/check-examples-purity.py` forbids any
`langchain`/`langgraph` import anywhere under the top-level `examples/`
tree (`CURATED_PACKAGES` allows only `graph_owl_sdk` and
`graph_owl_read_client`), which closes off that location too, on different
grounds (dependency purity, not the "never an agent" refusal).

**Asked the user rather than guessing.** The refusal is deliberate and
repeated three times independently — not the kind of thing to route around
silently. Given the choice between building outside the published package,
skipping to P12, or dropping P11 as out of scope, the answer was explicit:
build it, with LangChain and LangGraph, in Python.

**Resolved by placement, not by ignoring the refusal.**
`integrations/langchain/examples/` — a new directory **sibling to
`tests/`**, inside the same top-level folder as the package but outside
what ships:

```toml
[tool.setuptools.packages.find]
include = ["graph_owl_langchain*"]
```

`tests/` already sits there unshipped (`testpaths = ["tests"]`, no
`packages.find` entry for it); `examples/` follows the identical pattern.
Nothing under it is importable as `graph_owl_langchain.anything`, nothing
in it is published to PyPI, and the package's own refusal — "never a
chain, an agent, or a runtime" — stays true of the package. This directory
is not scanned by `scripts/check-examples-purity.py` either (that script's
`examples_dir` is `<repo-root>/examples/`, a different tree), so the
LangChain/LangGraph imports this needs are unblocked without loosening
that check's scope.

The framing that keeps this honest: **a worked reference for what a user's
own agent looks like, not a graph-owl product component** — the same
posture `examples/agent-triage/triage.py` already has for a
non-LangGraph, fixed-sequence "agent," just one level up in capability
(here the model chooses which tool to call next, rather than the script
choosing for it).

## What the agent proves that the existing reference apps don't

- `examples/gst-reconcile/reconcile_agent.py` (P9) is **deterministic,
  non-agentic**: a fixed `QUESTION_LABELS` table routes exactly 5
  questions (1–5) to a `/findings` query, with a model only narrating an
  already-complete answer.
- `packs/gst/eval/questions.md`'s own "Running it" section says outright:
  "There is no agent yet." Questions 12–15 are explicitly the ones "where
  the graph should beat retrieval" — un-covered by `QUESTION_LABELS`, and
  `scripts/verify-gst-reconciliation.sh` only asserts 1–11.
- **Question 14 is the flagship target**: "Which invoices would become
  compliant if the taxpayer paid them today?" (key: INV-1006 only).
  Answering it needs the model to *choose* which subjects to check and
  read `calculate_risk`'s `daysRemaining` correctly — non-negative means
  still curable by paying now, negative means the breach already
  happened and payment cannot undo it. `Catalog::obligations_from_rows`'s
  own doc comment already establishes the fact this reasoning rests on: "A
  row whose `to` (the discharging event) is already bound is not on the
  calendar" — so every obligation `calculate_risk` reports is,
  by construction, unpaid; the only judgment left is the sign of
  `daysRemaining`. No existing tool or script performs that read.

## What was built

- `integrations/langchain/examples/gst_investigation_agent.py`:
  - `SYSTEM_PROMPT` — states the one fact a model cannot get from the tool
    schema alone: `daysRemaining >= 0` means still payable into
    compliance, `< 0` means the breach is already final.
  - `investigate(model, tools, question) -> str` — the entire
    LangGraph-specific surface: `create_react_agent(model, tools,
    prompt=SYSTEM_PROMPT).invoke(...)`, returning the final message's
    text. No bespoke orchestration — the ReAct loop *is* the
    investigation, matching Decision 8 ("framework-agnostic core, thin
    framework shims") already on record for Epic 43.
  - `build_chat_model()` — real usage only, lazily imports
    `langchain_openai.ChatOpenAI` inside `main()` so the module (and the
    test file) never requires it. Env vars match `reconcile_agent.py`'s
    established names: `LLM_API_BASE_URL`, `LLM_MODEL`, `LLM_API_KEY`.
  - `main()` — CLI: `--question`, `--server`, `--token`, matching
    `reconcile_agent.py`'s own argument names for consistency between the
    two reference scripts.

## The RED test

`integrations/langchain/tests/test_gst_investigation_agent.py`, following
`test_langgraph_integration.py`'s own established pattern exactly (a
scripted `BaseChatModel` subclass emitting a fixed step sequence, and a
fake `opener` returning real P10 wire shapes) — proving the **wiring**
(toolkit → LangGraph's tool-calling loop → real MCP call shapes →
`investigate`'s return value), not a live model's reasoning, matching this
project's "prove the deterministic layer before the model" posture and
`test_reconcile_agent.py`'s own precedent of testing entirely against
in-process fakes with no live server or API key.

Two tests:

1. `test_the_agent_names_only_the_invoice_still_inside_its_window` — three
   scripted steps: `resolve_entity("unpaid GST purchases")` →
   `calculate_risk` on the one candidate whose obligation is still open →
   a final answer. Fake opener returns real `resolve_entity`/
   `calculate_risk` wire shapes (`candidates: [{fullyQualifiedName, kind,
   score}]`; `{obligations: [...], count}` with camelCase `daysRemaining`)
   built from `packs/gst/eval/questions.md`'s own key numbers (INV-1006,
   3 days remaining of the 180). Asserts both tool calls actually ran (not
   just that a plausible-looking string came back) and that the final
   answer names INV-1006.
2. `test_a_negative_days_remaining_is_not_reported_as_curable` — the
   mutator-rules gap `testing`'s mutation-aware planning flags directly: a
   model (or a future refactor) that reads `daysRemaining` with the wrong
   comparison direction, or that reports every open obligation
   regardless of sign, would pass test 1 alone. Scripts a
   `calculate_risk` response with `daysRemaining: -12` and asserts the
   final answer does **not** name that subject — the negative half
   `CLAUDE.md`'s mutation-testing lesson calls for on every "X derives Y"
   test.

## Mutation report

**Not run, and that turned out to match established precedent rather than
fall short of it.** `setup.cfg`'s `[mutmut] paths_to_mutate` is scoped to
`graph_owl_langchain/` only — the same boundary `[tool.mypy] files` already
draws. Neither `reconcile_agent.py` nor `triage.py` (the two prior
reference scripts) has ever had mutmut run against it either; grepping
`plans/` and `examples/` for `mutmut` finds nothing before this plan.

Tried it anyway and hit a real, structural reason it doesn't fit: mutmut
resolves a mutant's module key from the file's path relative to the
project root (`examples.gst_investigation_agent`), but the test imports it
as a bare top-level module via `sys.path.insert` —
`test_reconcile_agent.py`'s and `test_langgraph_integration.py`'s own
established pattern for a script with no package to belong to. The two
conventions disagree about the module's own name, so mutmut records zero
trampoline hits and stops rather than silently under-counting. Forcing
agreement would mean adding an `__init__.py` and switching every reference
script's test to a `from examples.gst_investigation_agent import ...`
absolute import — a repo-wide convention change with no test-quality
payoff, since `investigate()` itself is a three-line pass-through with no
branch for a mutant to hide in.

Coverage instead comes from the two behavioral tests: the full
`resolve_entity` → `calculate_risk` → answer path (proves the tool-calling
loop threads real results through), and `SYSTEM_PROMPT` reaching the
model's first call (the one line deleting `system_prompt=SYSTEM_PROMPT`
from the `create_agent(...)` call would be caught by).

## What this deliberately does not do

- **No new capability in `graph_owl_langchain` itself.** The toolkit
  (`GraphOwlToolkit`), already exposing all 8 P10 tools with zero
  toolkit-side changes (confirmed during P11 scoping — it builds every
  `StructuredTool` from a live `tools/list` manifest, no hand-listed tool
  names), is unmodified.
- **No real-LLM proof in this environment.** No `LLM_API_BASE_URL`/
  `LLM_MODEL`/`LLM_API_KEY` is configured here (checked:
  `env | grep -i "LLM_\|OPENAI\|DEEPSEEK\|ANTHROPIC"` returns nothing), so
  `build_chat_model()`'s real path is exercised by inspection and by
  `reconcile_agent.py`'s own established OpenAI-compatible-endpoint
  precedent, not by a live call from this session.
- **Does not implement P12 (the eval harness).** This slice answers one
  question by hand-wiring the scripted proof to it; automated scoring
  against all 15 questions with a Wilson interval is P12's own scope, not
  this one's.
