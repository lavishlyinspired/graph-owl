# Plan: 106 — agent trace hygiene (GST trace `019ff1cb-3960-7e21-8c1a-01c7fa06316a` follow-ups)

**Status**: shipped, 12 August 2026. All four slices landed: Slice 1
(`b70535c`), Slices 2+3a (`5e07c23`), Slice 3b (`0dfbfcd`), Slices 4a+4b
(`c67f29e`). `plans/105-mcp-tool-visibility-divergence.md`'s status
updated to match: D1 kept asset-scoped, D2 explain extended, D3 shipped
as `run_pack_query`. **Branch**: main, commit directly.
**Trigger**: review of a live LangSmith trace of the GST investigation agent surfaced four
independent gaps. `plans/105-mcp-tool-visibility-divergence.md` already dissected the
visibility thread — this plan executes its stated follow-ups under its "do not bundle" rule.
**Decisions taken before planning** (user-confirmed): D1 = keep memory/investigation
asset-scoped; D2 = extend `explain` to pack named graphs; D3 = named parameterized pack query.

**Four slices, four commits, own acceptance criteria each.**

## What the trace proved (evidence)

- Answer correctness: 10% under Notification 75/2019-CT (INV-2001, invoice date 2020-07-10,
  cap 10% effective 2020-01-01) — correct.
- `query_graph` (SPARQL) read the pack's named graphs; every task-shaped tool on the same
  subject refused with `"no such entity, or it is not visible to you"` — three separate root
  causes sharing one error string (documented in `105-mcp-tool-visibility-divergence.md`).
- `SELECT ?s ?p ?o { ?s gst:governedBy ?o }` returned zero rows: `gst:governedBy` is declared
  (`packs/gst/ontology.ttl:37`, `pack.toml`) but never instantiated. The agent answered the
  temporal question by in-context arithmetic, not a graph fact.
- `LANGSMITH_API_BASE_URL` in local `.env` was set to the unresolvable `api.langsmith.com`;
  `.env.example` carries no `LANGSMITH_*` block at all; the example docstring documents a
  different var name (`LANGSMITH_ENDPOINT`).

## Slice 1 — LangSmith env hygiene (config/docs, no code)

Local `.env` already corrected. Committed gaps remain:

- `.env.example` has no `LANGSMITH_*` block.
- The var name used locally (`LANGSMITH_API_BASE_URL`) differs from the one the example
  docstring documents (`LANGSMITH_ENDPOINT`).

Work: verify which env var the installed LangSmith SDK actually reads (the venv
`integrations/langchain` runs in), align `.env.example` to that name, add the tracing block
(commented, no key), and fix the docstring if the name it documents is wrong.

**AC**: `.env.example` carries the working var set; `git check-ignore .env` still true;
`gst_investigation_agent.py` docstring matches reality.

**Mutants**: none (no logic). TDD n/a — verification is a live check against the installed SDK.

## Slice 2 — `query_graph` diagnostics (MCP response)

Gap: `QueryAnswer` (`crates/graph-owl-mcp/src/lib.rs:603`) returns only `rows`/`truncated`;
`SparqlOutcome` already carries `facts_scanned`, `plan`, `ql_rewrite`, `refused_axioms`,
`alignments_used`, `variables` (`graph-owl-api/src/lib.rs:485-553`). The MCP handler
(`crates/graph-owl-mcp/src/catalog.rs:501-530`) drops all but two. In the trace this produced
the worst silent moment: `governedBy` → `[]` with no way to tell the law graph was never scanned.

Work: extend `QueryAnswer` with `factsScanned: usize`, `plan: Vec<String>`,
`variables: Vec<String>`, and `qlRewrite`/`refusedAxioms`/`alignmentsUsed` as
`Option`/`Vec` fields with `#[serde(skip_serializing_if)]` for backward compatibility; derive
`Serialize` on `QlRewrite`/`RefusedAxiom`/`AlignmentReviewEntry` in `graph-owl-api` (or build
compact MCP-side shapes if cross-crate derives are rejected). Populate in `catalog.rs`. The
jsonrpc no-op double (`jsonrpc.rs:416`) stays on `QueryAnswer::default()`.

**AC**: an MCP SELECT returns the new fields; a query whose pattern the planner could not bound
shows a single `?s ?p ?o` entry in `plan`; `qlRewrite` is absent when nothing rewrote (the
"silence is the signal" rule from `SparqlOutcome.ql_rewrite`); existing consumers of
`rows`/`truncated` unaffected.

**Mutants to kill**: a field wired to the wrong `SparqlOutcome` member; `skip_serializing_if`
dropped (schema bloat); `qlRewrite` reported when `None`.

## Slice 3a — memory/investigation stay asset-scoped (D1 = keep)

No gate change. The 105-mcp doc's open questions (pack-reload survival, cross-pack collision,
SPARQL-readability of a pack-subject memory) stay open; `recall_memory`/`record_investigation`
remain assets-table-scoped (`get_asset_by_fqn`), so pack subjects continue to fail for every
principal — that is now the stated design, not an accident.

Work: finish the SYSTEM_PROMPT steer that commit `93b6300` started (agent already steered to
`query_graph` for pack data; make it explicit that memory/investigation address catalog assets
only), and replace the generic `jsonrpc.rs:293` string with an honest per-tool reason — for
these two tools: "memory is scoped to catalog assets; pack entities are queried via
query_graph."

**AC**: a `recall_memory` on a pack subject returns an asset-scope message, not "not visible";
an eval question over the trace's data shows the agent no longer wastes turns on
`recall_memory` for invoices.

**Mutants to kill**: a mutant that reverts the specific string to the generic one.

## Slice 3b — `explain` over pack named graphs (D2 = extend)

`explain` is auth-only and calls `explain_fact` with `graph_owl_reasoning::Budget::default()`,
whose `include_graphs` is empty, so `reasoning_base` (`graph-owl-api/src/lib.rs:16806-16849`)
never loads `graph:import:*` and every pack-fact explain is `NotFound` regardless of principal.
`Budget` already has `include_graphs` — no contract change needed.

Work: `explain_fact` (or its MCP/HTTP callers) resolves the subject's source graph(s) (its
`cx` / `graph:import:{source}` namespace) and fills `Budget.include_graphs` from them, so
explaining a pack fact reasons over the graph that holds it. Distinct from 3a, separate commit.

**AC**: `explain(pr-INV-2001, gst:governedBy, gst:Rule36-4-2020)` returns a derivation (or an
honest "no such fact") once the fact exists, where today it is `NotFound` because the reasoner
never saw the graph; `GET /reasoning/explain` shares the behaviour; a fact in the default graph
explains unchanged.

**Mutants to kill**: a mutant that drops a named graph from `include_graphs` (derivation
becomes `Unknown` again); one that over-includes graphs (leaks facts across packs).

## Slice 4 — `governedBy` becomes answerable (D3 = named parameterized query)

**4a (pack content):** author `packs/gst/queries/provision-in-force.sparql` — parameterized by
an invoice subject; resolve its `invoiceDate`, then the OPTIONAL + `!BOUND` "latest
`effectiveFrom` ≤ date" idiom already proven in `amount-mismatch.sparql`, returning provision,
`capPercent`, `citation`, `effectiveFrom`. Register it alongside the six existing registered
queries. Add the parameterization mechanism the six queries lack: a declared placeholder
(e.g. `VALUES ?invoice { <{{binding}}> }` substitution performed before parse, so the bound
term is injected as syntax-level VALUES, never concatenated — no SPARQL injection surface).

**4b (agent surface):** a small P10-style MCP tool (e.g. `run_pack_query(pack, query,
bindings)`), mirroring the existing eight tools' `ToolDeclaration`/dispatch/catalog-handler/
jsonrpc-double pattern. Without it the agent has no way to call 4a by name and will keep
re-deriving the idiom or querying the non-existent edge.

**AC**: `provision-in-force` on `pr-INV-2001` (date 2020-07-10) returns `Rule36-4-2020`, 10%,
`Notification 75/2019-CT`; `scripts/verify-pack-load.sh` stays green; a non-existent binding is
a validation error, not a 500; Slice 2's `plan`/`factsScanned` in the tool response show the
law graph was scanned.

**Mutants to kill**: VALUES injection that substitutes into the wrong position; a mutant that
drops the `!BOUND` guard (the exact silent-trivial-true failure `amount-mismatch.sparql`'s
comment warns about).

## Ordering & process

1 → 2 (both independent) → 3a → 3b → 4a → 4b. No bundling; each slice is one commit with its
own RED test. Workspace suite via `cargo test --workspace -- --test-threads=2`; fmt→clippy→test
before `cargo mutants --file` scoped per touched crate. On landing, update
`105-mcp-tool-visibility-divergence.md`'s status to note which root causes are now resolved
(D1 = kept asset-scoped, D2 = explain extended).
