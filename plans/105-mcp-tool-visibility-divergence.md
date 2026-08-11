# Investigation: pack-subject visibility divergence across MCP tools

**Branch**: main. **Status**: investigated 11 August 2026; the three
follow-ups this doc named are now resolved by `plans/106-agent-trace-
hygiene.md`, shipped 12 August 2026. **D1 (root cause 1) = kept
asset-scoped by design** (Slice 3a, `5e07c23`): `recall_memory`/
`record_investigation` still resolve via `get_asset_by_fqn` and still
refuse a pack subject for every principal — that refusal now carries an
honest per-tool reason ("memory is scoped to catalog assets; pack
entities are queried via query_graph") instead of the generic visibility
string, and `SYSTEM_PROMPT` steers the agent away from trying. The
harder product questions this doc raised (pack-reload survival,
cross-pack collision, SPARQL-readability of a pack-subject memory) were
never in scope for that decision and remain genuinely open if this ever
needs revisiting. **D2 (root cause 2) = explain extended** (Slice 3b,
`0dfbfcd`): `explain_fact` now resolves a subject's own source graph(s)
and widens `Budget.include_graphs` before reasoning, so a pack fact
explains instead of reporting `NotFound` for a fact the reasoner was
never shown — bounded to the subject's own graphs, explicitly excluding
`graph:reasoning` (a regression the slice's own test caught: a subject
with a prior conclusion would otherwise feed it back in as if asserted).
**D3, not diagnosed here but the concrete fix for the trace's actual
failed question** (Slices 4a+4b, `c67f29e`): `gst:governedBy` had no
queryable edge at all, which is why the agent fell back to in-context
arithmetic for the trace's cap percentage. `run_pack_query` plus a
registered `provision-in-force` query makes that answerable from the
graph. `run_rule` and `find_evidence` were confirmed not bugs and
untouched, as this doc originally concluded.

Triggered by a live LangSmith trace review (`019ff1cb-...`): `query_graph`
(SPARQL) read `pr-INV-2001`'s data fine as `Principal::system()`;
`recall_memory`, `explain`, `run_rule`, and `find_evidence` all refused
the same subject with variants of `"no such entity, or it is not visible
to you"` in the same run. That reads, on its face, like one authorization
bug. It is not — three separate root causes, one shared error string.

## Root cause 1 — real, confirmed: an asset-only visibility gate

`recall_memory` (`CatalogContext::recall`, `crates/graph-owl-mcp/
src/catalog.rs:201-223`) and `record_investigation`'s write-gate
(`Catalog::gate_agent_write`, `crates/graph-owl-api/src/lib.rs:31936-
31960`) both resolve the subject via `Catalog::get_asset_by_fqn` — an
`assets`-table-only lookup — **before any policy check runs**. A pack
subject (`pr-INV-2001`, imported into `graph:import:gst-purchase-
register`, no `assets` row by design — `plans/105-domain-neutrality.md`)
fails that lookup unconditionally, for every principal including
`Principal::system()` (`is_admin: true`), because the code path never
reaches `get_asset_for` (where the admin bypass and any policy check
would live). The same shape affects `asset_context`, `explain_lineage`,
`analyze_impact`, `get_governance_context` — every Epic-14-vintage read
tool gates the same way.

**Why this is not simply "apply the `105y` fix here too."** `105y`
(per-named-graph policy) exists precisely so `scope_facts` can decide
*SPARQL* visibility for pack subjects. `recall_memory`/
`record_investigation` ask a different question first: not "may this
principal see this subject's facts" but "does a memory/investigation
system that is scoped to catalog assets even apply to this subject at
all." `plans/31-memory.md:146` and `plans/32-agent-capabilities.md:54,88`
frame memory and investigations as being about **enterprise metadata
assets** from original design — not a domain-neutrality gap Epic 105
left open, but a scope boundary nobody has revisited since. Whether an
agent should be able to persist a memory or an investigation finding
against an arbitrary pack subject (an invoice, not a table) is a real
product question, not a policy-wiring bug: it would mean a memory system
built for "this table has known data-quality issues" now also needs to
answer "what happens when two packs' subjects collide," "does a memory
on a pack subject survive the pack being reloaded," and "is a
`recall_memory` result on `pr-INV-2001` itself something SPARQL should be
able to read back." None of those are settled anywhere. Patching the
visibility check alone would let writes through without answering them.

**What a real fix needs, when someone picks this up**: either (a)
extend `get_asset_by_fqn`'s callers to also recognize a pack-namespace
subject and route to a parallel, graph-scoped check using the same
`admitted_by_vocabulary_namespace`/`named_graph_predicate_for` machinery
`105y` already built for SPARQL — the mechanical part is not hard, since
the pieces exist — or (b) decide memory/investigations stay
asset-scoped by design, and teach the agent (via `SYSTEM_PROMPT`, the
same lever the token-bloat fix in this session already pulled) not to
try them on pack subjects at all. Both are legitimate; neither should be
picked implicitly by whichever one is easier to code in the next five
minutes.

## Root cause 2 — different, pre-existing: `explain`'s reasoning scope

`explain` (`CatalogContext::explain`, `catalog.rs:679-710`) does **no
visibility check of any kind** — only `self.authenticated(principal)?`,
matching its own doc comment. It calls `Catalog::explain_fact`
(`graph-owl-api/src/lib.rs:14293-14321`) with `graph_owl_reasoning::
Budget::default()`, whose `include_graphs` is empty. `reasoning_base`
(`graph-owl-api/src/lib.rs:16806-16849`) only loads the *default* graph
(`cx: Some(None)`) plus `include_graphs` — so a pack subject's facts,
which live in `graph:import:{source}` (`cx: Some(...)`), are never
loaded into the reasoning input at all, for any principal. `reasoning::
explain` correctly reports `Explanation::Unknown` for a fact it was
never shown, which `explain_fact` maps to `CatalogError::NotFound` —
same wire text, unrelated cause.

**This is not new.** `GET /reasoning/explain` (`crates/graph-owl-server/
src/lib.rs:7695-7712`) has called `Budget::default()` identically since
before Epic 105, and `plans/105m-explain-tool.md:41-49` says the MCP
tool "inherits that route's existing posture rather than inventing a new
one" — a stated, deliberate scope boundary at the time, not an oversight.
Whether that posture should change now that packs are a real, first-class
data source is worth asking, but the fix (some way for a caller to name
which named graph(s) `explain` should reason over) is a different shape
of change than the visibility-gate fix above, and touches
`graph_owl_reasoning::Budget`'s own contract, not just an MCP wrapper.

## Not bugs, in this incident: `run_rule` and `find_evidence`

- `run_rule`'s only `NotFound` is a `(pack, label)` registry miss
  (`graph-owl-api/src/lib.rs:2506-2509`) — a wrong rule label, not a
  visibility question. Its actual data path runs the rule's own SPARQL
  through `Catalog::sparql`, which **does** correctly thread pack
  subjects through `scope_facts`/`admitted_by_vocabulary_namespace`
  (`plans/105p-run-rule-tool.md:41-60` already documents hitting and
  fixing exactly this distinction).
- `find_evidence` takes a `findingId` (`Uuid`), not a subject — its
  `NotFound` is "no finding with that UUID" (`graph-owl-api/
  src/lib.rs:2808-2822`), a legitimate empty/absent state if no rule has
  yet opened a finding against `pr-INV-2001`, not a refusal.

## What was actually done this session

- **Nothing patched.** The mechanical half of root cause 1 is buildable
  today (the `105y` machinery exists), but doing it without deciding the
  product question above would ship a policy hole or a false sense that
  the question is settled.
- The system-prompt fix already landed this session (`93b6300`, "cut
  wasted investigation turns") reduces how often an agent reaches for
  `recall_memory` on a pack subject in the first place, by steering it
  toward `query_graph` for pack data from the start — a real, if partial,
  mitigation that ships without answering the harder design question.

## What this deliberately does not do

Does not extend `recall_memory`/`record_investigation` to pack subjects,
does not change `explain`'s reasoning `Budget`, does not touch
`run_rule`/`find_evidence` (not bugs). Each is a real, separate follow-up
with its own acceptance criteria — bundling them under one rushed patch
because they share an error string would be the same mistake the
investigation itself was written to avoid making.
