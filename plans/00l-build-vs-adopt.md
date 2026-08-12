# 00l — Build vs Adopt

**Status**: standing reference. Binds every engine epic.
**Verified against crates.io and the projects' own repositories on 28 July 2026.**

## The question this answers

"Why write a SPARQL engine, a reasoner and an RDF layer when libraries exist?"

It is the right question and it did not previously have a written answer, which
meant each epic implicitly answered it again. The short version:

> **Adopt everything above the storage line. Build everything below it.**

The line is not aesthetic. It is where this project's three differentiators
live.

**Corrected 28 July 2026.** The first version of this document claimed "every
library takes a graph and returns a graph", and used that to argue the whole
evaluator had to be built. That is true of `oxigraph::Store` and false of the
component crates underneath it — `spareval` evaluates against a
**`QueryableDataset` trait the caller implements**. The scan stays on our side
of the line, so the differentiators are preserved *and* the evaluator is
adopted. The line moved; it did not disappear.

## What is actually available

Checked, with licences, because this project's `cargo deny` allowlist is
permissive-only (MIT, Apache-2.0, BSD, ISC, Unicode, Zlib) and **copyleft is
rejected by default** (`00i`).

### Rust

| Crate | Licence | Does | Verdict |
|---|---|---|---|
| `spargebra` | Apache-2.0 / MIT | Full SPARQL 1.1 + 1.2 parser → standard algebra | **Adopt** — `07` decision 8 |
| `sparopt` | Apache-2.0 / MIT | Algebra optimizer | **Adopt** — generic rewrites we would otherwise write |
| `spareval` | Apache-2.0 / MIT | **SPARQL evaluator over a caller-supplied `QueryableDataset`** — plus `ServiceHandler` for federation and custom aggregates | **Adopt — this is the finding that shrinks Epic 7** |
| `sparesults` | Apache-2.0 / MIT | SPARQL results serialization (JSON, XML, CSV) | **Adopt** — the wire formats clients expect |
| `oxrdf`, `oxttl`, `oxjsonld` | Apache-2.0 / MIT | RDF terms, Turtle/N-Triples, JSON-LD | **Adopt** — Epic 9 |
| `oxrdfxml` | Apache-2.0 / MIT | RDF/XML parsing | **Adopt** — same Oxigraph project/maintainer already trusted for `oxrdf`/`oxttl`/`oxjsonld`; checked 8 August 2026 (0.2.3, updated 2026-04-11, 690k downloads) before adding, for Epic 33's FIBO import (Phase 3 item 3.9) — the crate description already scoped "RDF/XML import/export" as this module's mandate, unbuilt until now |
| `oxsdatatypes` | Apache-2.0 / MIT | XSD datatypes | **Adopt** where `FlakeValue` needs XSD semantics |
| `reasonable` | BSD-3-Clause | OWL 2 RL via Datalog, *subset* of rules, Python bindings | **Evaluate** — see below |
| `whelk-rs` | BSD-3-Clause | **OWL EL reasoner** | **Evaluate seriously** — Epic 98 |
| `oxigraph` | Apache-2.0 / MIT | Complete store + SPARQL engine | **Test oracle only** — owns storage |
| **openCypher grammar** | **Apache-2.0** | Official Cypher grammar: XML source, generated EBNF + ANTLR4, railroad diagrams | **Adopt as the specification.** Generating a parser from it is the *fallback*, not the plan — see below |
| **openCypher TCK** | Apache-2.0 | Cucumber features defining Cypher behaviour | **Adopt as a conformance oracle**, whichever parser wins |
| `cypher-parser` | MIT | Lexer + parser + AST + **separable** pluggable executor; positioned errors | **Spike first.** `Shopify/cypher-parser`, v0.8.1 (2026-07-09), 10 versions in six weeks, 4,380 downloads. Real organizational backing and a visible repository. Take the parser, ignore the executor — planning and execution stay ours |
| `tree-sitter-cypher` | MIT | tree-sitter grammar, consumable from Rust via the `tree-sitter` runtime (MIT, 30.9M downloads) | **Spike second for 7b; adopt for Epic 41 regardless.** Incremental and error-tolerant, which is exactly right for an editor — and a hazard for a *gate*. See the CST caveat below |
| `decypher` | EUPL-1.2 **OR** MIT **OR** Apache-2.0 | openCypher parser, `rowan`-based, error-resilient, returns `Unsupported` for unhandled productions | **Spike third.** Active (2026-05-19), typed AST, but `0.2.0-alpha.6` and its own README says the AST is unstable until 0.2.0 |
| `opencypher` | MIT / Apache-2.0 | Hand-written openCypher parser, typed **span-annotated** AST — on paper the best API fit of all of these | **Blocked, not rejected.** `rockstar/opencypher` returns **404** while the account exists: the repository is private or deleted. The source cannot be audited, the licence claim cannot be checked against the code, and there is nowhere to file an issue. 87 downloads, first published 2026-07-11. **Revisit the moment the repository is public** |
| `open-cypher` | MIT | `pest` grammar derived from the openCypher EBNF | **Reject — abandoned.** Last published 2022-07-23 at v0.1.1 |
| `ocg` | Apache-2.0 | "100% openCypher-compliant graph database" | **Reject** — a database, so it owns storage and crosses the line above; and its repository is on `github.ibm.com`, unauditable |
| Apache AGE | Apache-2.0 | Mature openCypher parser, transforms Cypher into PostgreSQL query trees | **Reject on architecture, not quality.** It lowers to a *Postgres* query tree and a Postgres planner; we lower to our own `QueryAst` and our own planner. Extracting the parser would import C and PostgreSQL assumptions and cost more than it saves |
| `antlr4rust` | BSD-3-Clause on crates.io, `NOASSERTION` on GitHub | ANTLR4 runtime for Rust | **Reject** — repository unpushed since 2023-02-14, and the two licence claims disagree |
| `antlr-rust-runtime` | BSD-3-Clause | Newer ANTLR4 Rust runtime | **Reject for now** — fresh (2026-08-01) but 2,439 downloads; too thin to put a query front end on |
| `pest` | MIT / Apache-2.0 | PEG parser generator | **Fallback only** — 293M downloads, and the generation target *if every parser candidate fails the spike* |
| `rust-mcp-sdk` | MIT | MCP server/client SDK over `rust-mcp-schema`; stdio, SSE and streamable-HTTP transports | **Spike, and it is a real candidate we did not check first.** v1.0.1, 43 versions, ~215k downloads, repository visible and pushed 2026-08-03. Epic 14's transport was hand-written before this check ran — see below. **Re-measured 9 August 2026: this was the wrong crate to have spiked** — a different, unrelated third-party crate of a confusingly similar name to `rmcp` below, which is the one `00j-language-boundaries.md` actually named. See the MCP section below |
| `rmcp` | Apache-2.0 | **Official** Rust SDK for MCP, maintained by the `modelcontextprotocol` org itself — protocol types, macros, and every transport (stdio, child-process, streamable-HTTP, with optional OAuth) behind 27 independently-selectable feature flags | **Adopt, feature-gated to schema types plus stdio — see the MCP section below.** 3.1.2, updated 2026-08-07, 19.3M downloads, `github.com/modelcontextprotocol/rust-sdk`. This is the crate `00j-language-boundaries.md` (line 46) named as the reason MCP could stay Rust, described there as "at 1.x" — it is now at 3.x. The 4 August spike never checked it, checking `rust-mcp-sdk` instead |
| `rust-igraph` | **GPL-2.0-or-later** | Graph algorithms: PageRank, centrality, WCC, cycle detection — everything Epic 38 wants | **Rejected on licence, and it is the clearest case for why licence is a gate rather than a score.** Copyleft: adopting it relicenses graph-owl. 175 downloads. Use `petgraph` |
| `petgraph` | MIT / Apache-2.0 | Graph data structures and algorithms | **Adopt for Epic 38's arithmetic.** 451M downloads. The reified-edge objection does not bite: Epic 38 already specifies "pure algorithms over an in-memory projection the caller supplies", so the projection hides the two-hop encoding and `petgraph` sees an ordinary graph |
| `sonyflake` / `snowflake` | MIT / Apache-2.0 | Distributed unique **id** generation | **Not applicable — a terminology collision.** A graph-owl *flake* is a fact tuple `{s, p, o, cx, t, op}` in the Datomic sense, not a Snowflake id. Recorded so the suggestion is not made a third time |
| `horned-owl` | **LGPL-3.0** | OWL parsing and manipulation — RDF/XML, OWL/XML, Functional, Manchester | **Adopt out of process.** See below — the earlier "rejected" was wrong about both the licence and the options |
| `async-openai` | MIT | OpenAI API client — chat, embeddings, and every other endpoint, with a caller-settable `api_base` (`OpenAIConfig::with_api_base`) | **Adopt, embeddings surface only — Epic 31's semantic ranking term.** 0.41.3, updated 2026-07-31, 7.0M downloads, `github.com/64bit/async-openai`. Checked because the user explicitly asked for a provider-agnostic client rather than a hardcoded vendor call: `with_api_base` is exactly the seam that makes one client work against OpenAI itself, a self-hosted LiteLLM proxy, or a self-hosted vLLM instance — all three speak the same OpenAI-compatible `/v1/embeddings` wire shape, so nothing provider-specific is hardcoded here. Only the embeddings request/response types are used; chat, images, audio, assistants and the rest of the SDK's surface are simply unused code this crate does not call |

### Python

`00j` puts Python out of process, so nothing here can be a library dependency —
only a separate service or an offline tool.

| Library | Licence | Does |
|---|---|---|
| `rdflib` | W3C Software (permissive) | RDF + SPARQL, mature, slow |
| `pySHACL` | Apache-2.0 | SHACL validation |
| `OWL-RL` | W3C Software | OWL 2 RL over rdflib |
| `owlready2` | **LGPL-3** | OWL + bundled HermiT (DL reasoning) |

**Python's real use here is not runtime.** It is offline conformance checking:
run the same ontology through `OWL-RL` or `owlready2` and diff against
graph-owl's derivations. That is a test asset, and a licence that would be a
problem in the binary is not a problem in a test script that ships to nobody.

## `spareval` changes what Epic 7 is

The Oxigraph ecosystem is four separable crates, not one engine:

```
SPARQL text → spargebra → algebra → sparopt → optimized algebra
                                                    │
                                                    ▼
                                              spareval  ──calls──▶  QueryableDataset
                                                    │                (WE implement this)
                                                    ▼
                                              sparesults → wire format
```

`spareval` does not own a store. It calls back into a `QueryableDataset` the
caller supplies. **That is the whole difference**, and it means:

| We supply | And therefore keep |
|---|---|
| `QueryableDataset` over flakes | Index selection across the four orderings — the pattern-to-index decision is inside our scan |
| A dataset constructed *at* an `as_of` | Time travel, because the dataset only ever exposes one resolved state |
| The access predicate applied inside the scan | Authorization, because `spareval` only ever sees permitted rows |
| A `ServiceHandler` | Epic 101's allow-list, budget and outbound filtering |

**Nothing is given up.** The three differentiators live in the scan, and the
scan is the trait we implement. What is adopted is parsing, optimisation, join
execution, expression evaluation, aggregates and result serialisation — which
is most of the ~29,000 lines a full SPARQL layer costs, and none of it is
specific to this project.

**What must still be checked before committing:**

1. **Budgets.** Nothing surveyed bounds anything. A `QueryableDataset` can count
   its own scans and refuse past a limit, but whether that yields a clean
   truncation or a mid-query error needs testing. Epic 7's `Tracker` may have to
   wrap rather than live inside.
2. **Freshness stamping** (Epic 4 decision 8) is ours to add around the result,
   which is fine — it is metadata, not evaluation.
3. **Error quality.** `07` decision 8 promises "MINUS is not supported yet"
   rather than a parse error. With a complete evaluator that message class
   mostly disappears — a good problem.

## The storage line, precisely

Every library above takes *a graph* and returns *a graph* or *results*. That is
the correct design for a library and it is exactly why none can carry this
project's differentiators:

| Differentiator | Why a library cannot provide it |
|---|---|
| **Time travel** | `as_of` filters `t` **during the scan**. A library handed a materialised graph has already lost the time dimension — it received one state, not a history |
| **Authorization** | The access predicate compiles **into** the scan (Epic 13). A library handed a graph receives it unfiltered, and filtering results afterwards is precisely the leak Demo 2 exists to demonstrate is closed |
| **Explainability** | Epic 6 requires derivation chains. `reasonable` returns triples; the chain that produced them is not in its output |

So the split is:

```
  ADOPT   parse · RDF terms · serialize · algebra · reasoning algorithms
  ─────────────────── the storage line ───────────────────
  BUILD   flake scan · as_of · access predicate · derivation chains · budgets
```

## The pattern that lets us adopt reasoners anyway

The line above looks like it rules out `reasonable` and `whelk-rs`. It does not,
because of the order of operations:

1. graph-owl resolves flakes into a plain fact set, **applying `as_of` and the
   access predicate first**.
2. That already-filtered fact set goes to the library.
3. Derived facts come back into the overlay.

The library never sees what the caller may not see, and never sees a state other
than the one asked for — because the filtering happened *before* it was called.
Its lack of time-travel and authorization stops mattering.

**What is still lost: explanations.** `reasonable` returns facts, not chains,
and Epic 6 requires chains. Three options, and the third is what this project
should do:

| Option | Cost |
|---|---|
| Adopt, drop explanations | Loses a stated differentiator. No |
| Build everything | Slow, and re-implements a well-tested rule set |
| **Adopt for bulk, re-derive for explanation** | Materialise with the library; when someone asks *why* a specific fact holds, re-derive that one fact locally with tracking on |

The third works because explanation is **rare and single-fact** while
materialisation is **constant and whole-graph**. Optimising them separately is
correct rather than a compromise: a chain is needed for one fact at a time, at
human speed.

## `horned-owl` and LGPL — the earlier answer was wrong twice

The first version of this document said "LGPL-3.0 / GPL-3.0 … rejected by
policy. Would impose its terms on the binary." Both halves were wrong.

**Wrong about the licence.** The published crate metadata says **LGPL-3.0**, not
a GPL dual. That distinction is the whole point of the LGPL: unlike the GPL, it
**explicitly permits linking from software under other licences**, including
proprietary. It does not "impose its terms on the binary".

**Wrong about the options.** The real friction is narrower: LGPL requires that a
user be able to **relink** with a modified version of the library. Rust links
statically by default, which makes that awkward — you would have to ship object
files or otherwise enable substitution. That awkwardness, not infection, is why
most Rust licence policies exclude LGPL wholesale, and it is why `00i`'s
allowlist does.

**But there is a clean, standard answer: a separate process.** LGPL obligations
attach to linking. A distinct binary communicating over a pipe or socket is not
linking, and the boundary is unambiguous — this is the ordinary way LGPL
software is used from a differently-licensed program.

`00j` already establishes out-of-process as this project's pattern for anything
that "needs a library ecosystem Rust does not have". An OWL syntax parser is
exactly that.

**What it buys, and it is not small.** OWL ontologies ship in syntaxes nothing
else in the permissive Rust ecosystem reads: the financial vocabulary is
RDF/XML, and large medical ontologies are OWL Functional Syntax. Without
`horned-owl` those parsers are ours to write, for four syntaxes, to import
content this project does not otherwise need to understand deeply.

**Recommendation**: an `graph-owl-owl-import` **sidecar binary** wrapping
`horned-owl`, invoked for ontology import and emitting the project's own
representation. Import is a batch operation at human cadence, so process
overhead is irrelevant. The main binary stays permissive-only and `cargo deny`
stays as strict as it is.

**What this needs from a human**: `00i` is a licensing document with real
consequences, and adding an LGPL component in *any* form is a decision to make
deliberately rather than one an implementation session should take. The analysis
is here; the choice is not mine.

## Decisions

1. **No parser is written for a standard we did not invent.** SPARQL, Turtle,
   JSON-LD, XSD all have permissive Rust parsers.
2. **A copyleft dependency is rejected regardless of quality.** `horned-owl` is
   the best OWL manipulation library in Rust and is LGPL/GPL. It does not enter
   the binary. If OWL syntax parsing is needed beyond what the permissive crates
   give, it is written or found elsewhere.
3. **A reasoning algorithm may be adopted; the scan may not.** Epics 6, 95, 98
   evaluate `reasonable` and `whelk-rs` against the pattern above before writing
   an engine. **Epic 98 in particular should not start until `whelk-rs` has been
   evaluated** — an EL reasoner is weeks of work and a BSD-3 one already exists.
4. **Every adopted library gets a differential test against our own path**
   where both exist, and against a Python implementation where ours is the only
   Rust one. An external oracle cannot share a misunderstanding with the code
   under test, which is the entire value.
5. **Adoption is revisited when a library's licence changes.** `cargo deny` in
   CI enforces this rather than a reviewer remembering.

## What we build, and why it is not reinvention

After the above, what remains is genuinely this project's:

- **The flake store** — time-travelling and authorization-filterable. No library
  offers this shape.
- **Query evaluation over flakes** — the planner maps standard algebra onto four
  index orderings and a compiled access predicate. The algebra is adopted; the
  mapping cannot be.
- **Derivation chains** — the explanation contract.
- **Budget enforcement** — every operation bounded, truncation reported. No
  surveyed library bounds anything.

That is a much smaller list than "a SPARQL engine, a reasoner and an RDF layer",
and stating it is the point of this document.


## Cypher: adopt a parser if one survives a spike; generate only if none does

**Revised 4 August 2026, after a second pass over the ecosystem found three
crates the first pass missed.** The earlier version of this section committed to
"vendor the EBNF and generate a `pest` grammar". That was premature. It is
recorded here rather than deleted, because the reason it was wrong is
instructive: the first search returned the *hyphenated* `open-cypher`
(abandoned 2022) and the search was never repeated for the unhyphenated
`opencypher`, so a whole cohort of 2026 crates went unseen.

### The correction to the estimate stands

`07c` records a reference implementation carrying a **~10,000-line** openCypher
front end. That figure has been read — including by this project — as an
estimate for Epic 7b Slice A. It is not: it is the cost of a **complete** front
end for the whole language. Epic 7b's subset is **eleven clause forms**. Do not
size the slice against the larger number.

### The decision

> **Adopt an existing Rust Cypher parser if it passes a controlled spike against
> Epic 7b's subset and the TCK. Generate our own from the official grammar only
> if every candidate fails.**

Building a parser is the last resort, and generating one is the second-to-last.

**Spike order, and it is not the order of API elegance:**

1. **`cypher-parser`** — MIT, `Shopify/cypher-parser`, active, visible, and its
   parse and execute layers are separable so we can take the first and leave the
   second. Strongest combination of licence, auditability and backing.
2. **`tree-sitter-cypher`** — MIT, visible, on the mature `tree-sitter` runtime.
3. **`decypher`** — permissive option available, typed AST, but alpha.
4. **`opencypher`** — **blocked** while its repository 404s. Best API fit on
   paper; unauditable in practice.
5. **`pest` from the vendored EBNF** — only if 1–4 all fail.

### Why auditability outranks API fit here

`opencypher` is the nicest-looking option — a typed, span-annotated AST is
exactly what a lowering layer wants. It is nevertheless the one candidate that
cannot currently be adopted, because **its repository does not resolve.** We
cannot read what it does, cannot check the MIT claim against the source, cannot
see its history, and cannot report a bug.

That is the same objection this document already raised against `ocg`, and
applying it inconsistently would make it worthless. `00i` requires a licence to
be checked *before* reading an implementation; a crate with no readable
implementation cannot clear that bar however good its documentation is.

**This is a blocking condition, not a verdict on the code.** If the repository
becomes public, `opencypher` moves to the front of the spike order.

### The tree-sitter caveat, which cuts both ways

`tree-sitter-cypher` is **more** attractive than a first reading suggests and
**less** attractive for this specific slice, for the same underlying reason.

It produces a **CST of untyped, string-named nodes**, and it is deliberately
**error-tolerant** — it recovers from malformed input and returns a partial tree
with `ERROR` and `MISSING` nodes in it.

- For **Epic 41's query workbench** that is close to ideal: incremental
  re-parsing, syntax highlighting and live diagnostics all fall out of it, and
  one grammar then serves the editor and the engine.
- For **Epic 7b** it is a hazard, because 7b's parser is a **gate**: it decides
  whether a query is inside the supported subset. A parser that recovers rather
  than refusing makes that decision by omission — the adapter must walk the tree
  hunting for error nodes, and a single missed check is a malformed query
  lowering silently into a plan. A typed AST gives that check to the compiler.

So the sensible outcome may well be **both**: a typed parser for the engine, and
`tree-sitter-cypher` for the editor. Two parsers is normally a smell; here they
answer different questions, and the TCK is what keeps them honest about
agreeing.

### The spike itself

One corpus, every candidate, same assertions — otherwise this is four
impressions rather than a comparison. The corpus is Epic 7b's declared subset
plus a malformed set:

`MATCH` · `OPTIONAL MATCH` · `WHERE` · `RETURN` · `WITH` · `UNWIND` ·
`ORDER BY` · `SKIP`/`LIMIT` · `DISTINCT` · variable-length paths · node and
relationship properties · expressions · aggregates · **malformed queries with
known error positions**

Judged on, in order:

1. **Auditability and licence** — a gate, not a score. Fails here, out.
2. **Subset coverage** — how much of the eleven forms parses at all.
3. **Refusal behaviour** — what an out-of-subset construct produces. A specific
   "unsupported" beats a generic parse error; a silent partial parse is
   disqualifying for the engine path.
4. **Diagnostics** — line and column on malformed input, per Slice A's criteria.
5. **AST usability for lowering** — typed and exhaustive-matchable, or untyped.
6. **Maintenance and dependency weight.**

### The spike ran. Here is what it found.

**4 August 2026.** Harness and corpus preserved at
`spikes/cypher-parsers-2026-08/` (excluded from the workspace). 32 queries: 22
inside Epic 7b's subset, 6 out of subset, 4 malformed.

| | in-subset parsed | out-of-subset refused | malformed refused | diagnostics |
|---|---|---|---|---|
| `decypher` 0.2.0-alpha.6 | **22 / 22** | 0 / 6 | 4 / 4 | positioned |
| `tree-sitter-cypher` 0.2.6 | **22 / 22** | 1 / 6 | 4 / 4 | **row + column** |
| `cypher-parser` 0.8.1 | 20 / 22 | 6 / 6 | 4 / 4 | byte offset |

**Result: `cypher-parser` is disqualified, and the spike is why.** It had the
best provenance of the three — MIT, the real Shopify organisation, a visible
repository, active releases — and it fails on the two things graph-owl needs
most:

1. **It cannot parse relationship properties.**
   `MATCH (a)-[r:FEEDS {confidence: 0.9}]->(b)` →
   *"expected `]` to close a relationship pattern"*. Edge properties are the
   defining LPG feature and the entire reason `07c` says this mapping is cheap.
   A Cypher front end that cannot express them cannot express the thing this
   product is best at.
2. **It cannot parse float literals at all.** `0.8` fails in a `WHERE`, in a
   list, everywhere; only integers parse. `dsc:confidence` is a float, and every
   lineage and reasoning threshold in this system is one.

Its 6/6 on out-of-subset refusals is also worth less than it looks: the
refusals are generic syntax errors — `CREATE …` produces *"expected `MATCH`"* —
which does **not** name the API to use instead, and Slice A's RED requires
exactly that.

**`decypher` and `tree-sitter-cypher` "failing" the out-of-subset column is not
a defect.** They are full openCypher parsers, so `CREATE` and `CALL` are valid
Cypher and parse correctly. The subset restriction is *our* policy, not theirs,
so the gate belongs in our adapter — which is where it should be anyway, because
only we can write "use `POST /assets` instead". `decypher`'s `analyze` (HIR)
layer also accepts writes, so it does not supply the gate either.

**Corrected**: the first run reported tree-sitter as giving no positions. That
was a bug in the harness — it passed `accepted` as the `positioned` flag, so a
refusal always recorded "no position". tree-sitter reports **row and column** on
every `ERROR` and `MISSING` node, which is precisely what Slice A asks for. The
harness is fixed; the correction is recorded because the original claim was
published in a commit message.

### The decision, now evidence-backed

**Adopt `decypher` for the engine path, subject to its alpha status.** It is the
only candidate that parses the whole declared subset including edge properties
and floats, it yields a typed AST that lowering can match exhaustively, and it
reports positions. Its `Unsupported` behaviour for unhandled productions is the
shape Slice A wants.

**The alpha risk is real and is managed, not ignored.** Its README says the AST
is unstable until 0.2.0. The mitigation is that **the adapter is the only thing
that touches its AST** — lowering goes `decypher AST → our CypherAst →
QueryAst`, so an upstream AST change is confined to one file with its own tests.
That boundary is worth having regardless of which parser wins.

**Adopt `tree-sitter-cypher` for Epic 41's workbench**, as originally reasoned:
incremental, error-tolerant, row/column diagnostics, and highlight/injection
queries already in the crate. Its error-tolerance remains the reason it is not
the engine parser.

**`pest` is no longer the fallback for Slice A** — no candidate failed in a way
that requires generating our own. It stays listed only in case `decypher` is
abandoned before 0.2.0.


## MCP: the transport was built before this document was consulted

**4 August 2026, recorded as a process failure rather than a technical one.**

Epic 14's JSON-RPC transport was hand-written — framing, batch handling,
notification suppression, method dispatch, MCP content rendering — roughly 300
lines with 24 tests. **`rust-mcp-sdk` was never checked for.** It is MIT, at
v1.0.1 with 43 releases and ~215k downloads, its repository is public and was
pushed the day before this was written, and it exists to do exactly that job on
top of a type-safe `rust-mcp-schema`.

This is the rule in `CLAUDE.md` — *search for an existing crate before writing
one* — failing on the epic that most obviously called for it. It is written down
here rather than quietly fixed, because the useful artifact is the miss.

**What the SDK would bring**, and it is not nothing: protocol conformance
maintained upstream as MCP versions move, type-safe schema objects instead of
hand-built `serde_json::json!`, and a **stdio transport for free** — which this
project will want, because stdio is how most MCP clients actually connect and
`POST /mcp` serves none of them.

**Why it was not swapped in immediately.** The SDK ships `McpHttpServer` and
owns its transport. graph-owl's MCP endpoint sits at `POST /mcp` on the existing
axum server, *behind* the `Auth` extractor, admission control, the observability
middleware and RFC 9457 error rendering. Adopting the SDK means either giving up
that integration or writing a custom `Transport` against `McpDispatch` — a real
piece of work, not a dependency swap. Ripping out working, tested code at the
end of a long session to half-finish a migration would be worse than scheduling
it honestly.

### The spike ran, and the answer is the schema crate — not the SDK

**4 August 2026.** Measured, not read off a README:

| | transitive crates | downloads | what it gives |
|---|---|---|---|
| `rust-mcp-sdk` | **205** — including `aws-lc-sys`, a large C crypto library | 215k | transports, server framework, **and it owns the transport** |
| `rust-mcp-schema` | **15**, every one already in this tree | **684k** | the MCP types and *every protocol version* |

**Adopt `rust-mcp-schema`. Do not adopt the SDK.** The SDK's weight is almost
entirely the transports, and the transport is the one part graph-owl must keep:
`POST /mcp` sits behind the `Auth` extractor, admission control, observability
and RFC 9457 rendering. Paying 190 extra crates and a C crypto dependency to be
handed back a transport we would then have to fight is the wrong trade.

**The spike found a live bug, which is the argument for adopting anything here.**
`jsonrpc.rs` advertised a hand-written `PROTOCOL_VERSION = "2024-11-05"`. MCP had
moved on **three revisions** — `2025-03-26`, `2025-06-18`, `2025-11-25` — and a
string constant cannot tell you it has gone stale. A client negotiating against a
version that old either refuses outright or silently degrades. That is precisely
the maintenance the schema crate exists to absorb, and precisely what
hand-writing costs you six months later.

Fixed: the version now comes from `ProtocolVersion::latest()`, with a test that
asserts it is **not** the old constant — so the assertion says something rather
than tautologising.

**Still to do, and specified rather than pretended:** the response bodies are
still hand-built `serde_json::json!`. Moving them to the crate's typed
`InitializeResult`, `ListToolsResult` and `CallToolResult` is mechanical, buys
compile-time conformance, and is the obvious next increment. Stdio remains
unserved — it is how most MCP clients actually connect, and `POST /mcp` reaches
none of them; the framing is newline-delimited JSON, so it does not need the SDK.

*(The spike described here has now run; its result is the section above.)*

### Re-measured 9 August 2026: the spike checked the wrong crate

**The 4 August spike never checked the crate `00j-language-boundaries.md` (line
46) actually named.** That document's whole argument for keeping MCP in Rust
rests on "there is an official Rust SDK (`rmcp`), maintained by the Model
Context Protocol organization... at 1.x." The spike above measured
`rust-mcp-sdk` — a different, third-party crate with a confusingly similar
name — and never measured `rmcp` at all. Recorded as the same class of process
failure the section above already owns, not a new one: the rule is *search for
an existing crate*, and this time the crate was already named in a standing
document and still wasn't the one checked.

**Measured against crates.io, 9 August 2026:**

| | licence | version | updated | downloads | repository |
|---|---|---|---|---|---|
| `rmcp` | Apache-2.0 | **3.1.2** | 2026-08-07 (two days ago) | **19.3M** | `github.com/modelcontextprotocol/rust-sdk` — the official org |
| `rust-mcp-schema` (currently adopted) | MIT | 0.10.3 | 2026-06-24 | 717k | third-party |
| `rust-mcp-sdk` (the crate actually spiked on 4 August) | MIT | 1.0.1 | 2026-08-03 | 215k | third-party, unrelated to `rmcp` |

`00j`'s "at 1.x" is stale — `rmcp` is at 3.x — but the update only strengthens
its argument: still the reference implementation, still the official org, now
with five more major versions of active maintenance behind it.

**`rust-mcp-schema`, the crate actually adopted, is behind the deployed spec.**
Its protocol-version ceiling is `2025-11-25` (`ProtocolVersion::latest()` in
`graph-owl-mcp` resolves to this). MCP has a newer stable revision
(2026-07-28) that `rust-mcp-schema` cannot express — stateless HTTP, expanded
discovery, MRTR, and subscriptions. `rmcp`'s own test suite already exercises
these by name (`test_mrtr_behavior`, `test_sep_2260_stream_routing`,
`test_stateless_server_requests`, `test_streamable_http_stale_session`),
which is direct evidence it tracks the spec where the currently-adopted crate
does not.

**The transport-ownership objection that rejected `rust-mcp-sdk` does not
apply the same way to `rmcp`, because `rmcp` gates its transports behind
independent feature flags rather than bundling them into one SDK.** Checked
directly against its published `Cargo.toml`: with `default-features = false`
and only `["macros", "schemars"]` selected, the only new dependencies are
`pastey`, `rmcp-macros` and `schemars` — none of `reqwest`, `oauth2`,
`jsonwebtoken`, or the `aws-lc-sys` weight the SDK spike flagged. That gives
the same job `rust-mcp-schema` does today (typed protocol structs), from the
crate that is both official and current.

**It also gives the stdio transport this project is missing, without pulling
in any HTTP-server feature.** `transport-io` (stdio) and
`transport-child-process` sit behind their own flags, needing only
`tokio/io-std` and `tokio-util/codec` — both already implied by this
workspace's tokio usage. Enabling them does not enable `server-side-http` or
`transport-streamable-http-server`, so nothing here competes with or replaces
the existing axum-mounted `POST /mcp`; a stdio entry point would be a second,
independent transport onto the same dispatch, not a rewrite of the first.

**Revised decision: adopt `rmcp` (feature-gated to `macros` + `schemars` +
`transport-io`), replacing `rust-mcp-schema`, and use it to add the missing
stdio transport.** `POST /mcp` keeps its own hand-owned transport behind
`Auth`, admission control and RFC 9457 rendering — that part of the 4 August
conclusion still holds and is not reopened. Tracked as Epic 14 Slice H
(`plans/14-mcp-activation.md`); **implemented 9 August 2026** — see that
plan's own Slice H section for the full account.

**One correction found only by compiling, not by reading the feature
table**: `transport-io`'s `stdio()` function needs `transport-io` *and*
one of `client`/`server` enabled — `schemars` alone does not reach it.
`server` was added (it already implies `schemars`, so nothing here is
redundant with the plan above) rather than `client`, since graph-owl is the
MCP *server* gaining a second transport, not a client. The actual verified
feature set: `["macros", "server", "transport-io"]` for `graph-owl-mcp`
(schema types only), and `["server", "transport-io"]` for `graph-owl-server`
(the new `graph-owl-mcp-stdio` binary, which needs the `stdio()` primitive
but not `rmcp`'s own macros). Confirmed via `cargo check`: no new
transitive dependency beyond what the `macros`/`schemars` measurement above
already found — `server`'s own additions (`pastey`, `uuid`) were already
covered.

### The openCypher TCK's own harness need — checked, blocked on scope rather than licence

**4 August 2026, for Epic 7b Slice A2.** Two separate adoption questions, both
checked:

**The TCK corpus itself**: `github.com/opencypher/openCypher/tree/main/tck`,
Apache-2.0, resolves, maintained by the openCypher org. Passes every gate — this
is data (Gherkin scenarios), not a competing implementation, and the plan
already called for adopting it regardless of Slice A's parsing route.

**A Gherkin harness for Rust**: `cucumber` (`crates.io/crates/cucumber`,
`github.com/cucumber-rs/cucumber`), `MIT OR Apache-2.0`, ~16M downloads,
published within the last few months as of this check. Passes licence,
auditability and maintenance. **Not yet adopted, because the blocker found
during this check is not the crate** — it is that the TCK's own fixture format
(`Given an empty graph` + `CREATE` statements, or a named fixture built the same
way) has no way to seed state in an engine whose Cypher surface refuses `CREATE`
by design. Running it needs a second, harness-only interpreter translating a
`CREATE`-shaped fixture into flakes directly — real scope, not a dependency
swap, and specified in `07b-engine-cypher.md`'s Slice A2 section rather than
rushed alongside it. **Blocked is not rejected**: both the corpus and the
harness crate are the right choice whenever that translator is built.

### PackStream/Bolt server crates — checked, not adopted

**4 August 2026, for Epic 7d Slice A.** Searched crates.io for a PackStream
codec and a Bolt server framework before writing either by hand, per the
standing rule. Candidates and why each was set aside:

| Crate | Licence | Why not |
|---|---|---|
| `packs` | non-standard | Fails the licence gate outright |
| `packstream` | Apache-2.0 | Last published 2019-06-20 — seven years stale, an explicit co-dependency of an abandoned Neo4j driver |
| `boltwire` | MIT OR Apache-2.0 | A *client* driver (decision 1 rules out anything graph-owl would connect *out* with); 16 downloads |
| `kglite-bolt-server`, `meshdb-bolt` | MIT | Named "for kglite"/"for Mesh" — built as another project's internal dependency, not offered as a general-purpose library |
| `boltr` | MIT OR Apache-2.0 | The closest fit on paper — see below |

**`boltr` got a real spike, not just a licence check**, because it is the one
candidate that could plausibly replace this whole epic's transport and
session layer: `BoltServer::builder(backend).serve(addr)` plus a
`BoltBackend` trait, a `PackStream` codec, chunked framing and pluggable auth,
all in ~38 transitive crates with no C dependency. It builds cleanly. **Not
adopted**, on the maintenance gate specifically: 4 releases in a 7-week burst
(20 Feb – 11 Apr 2026), nothing published or pushed since, 4 GitHub stars,
2,509 total downloads. That is a young, effectively single-maintainer project
with no track record past its own initial build — for a wire protocol that
carries authentication credentials, `00i` rule 4's "ask a human when stuck"
applies to the *decision to trust it* as much as to any code question. Worth
revisiting if it gains real adoption or a second maintainer; recorded here so
that reconsideration starts from what was actually checked, not from zero.

**Slice A is hand-rolled against the spec instead**, and that is not a
fallback so much as the actually-correct call: `00i` names "the published
Bolt/PackStream spec" as an authorised source in its own right, and unlike a
full query grammar, PackStream's type system is small — roughly a dozen
marker-byte forms with three size classes each. The size where hand-rolling
stopped being reasonable for Cypher (a full grammar) is not the size here.

### SPARQL Protocol client (outbound `SERVICE` calls) — checked late, for Epic 101 Slice B

**5 August 2026, checked *after* Slice B was already written** (the check
should have run before — see `plans/101-sparql-federation.md`'s own note and
the memory this cost). Searched crates.io for a standalone SPARQL 1.1
Protocol client crate that could replace `federation.rs`'s hand-wired
`reqwest` call plus `sparesults` parse:

| Crate | Licence | Why not |
|---|---|---|
| `sparql-client` | BSD-2-Clause | Repository is a self-hosted Phabricator instance (`devcentral.nasqueron.org`), not a host that resolves the way GitHub does for this kind of check — fails the auditability gate the same way `opencypher`/`ocg` did |
| `sparql` | Unlicense | Version `0.0.0` — a reserved name, not a released crate |
| `oxigraph` | MIT OR Apache-2.0 | Does the thing — see below — but at the cost of the whole embedded triple store this project already chose not to adopt |

**`oxigraph` itself has the reference implementation**, and it is worth
reading even though the crate is not adopted:
`lib/oxigraph/src/sparql/http.rs` implements exactly this handler, using
`oxhttp::Client`, **POST with `Content-Type: application/sparql-query`**
(not GET-with-query-string, which is what Slice B originally shipped), an
`Accept` header naming both JSON and XML, and a configurable global timeout.
It is not exported as a separate crate — it is embedded in the full
`oxigraph` distribution and uses that crate's own internal types, so there is
nothing here to depend on. This is the same shape as the Apache AGE finding
in this document's introduction: the capability is *architecturally*
entangled with a full system graph-owl does not want, even though the
capability itself is small.

**Not adopted, for the reason already established for `oxigraph` in this
project**: pulling in the full embeddable store to reuse one internal module
would import a second query engine and a second storage model this project
does not use, for a few hundred lines it can read instead. Confirmed against
the actual spec
(https://www.w3.org/TR/sparql11-protocol/#query-via-post-direct) that
POST-directly with `application/sparql-query` is one of the three
spec-defined submission methods — the spec, not Oxigraph's choice, is what
`federation.rs`'s doc comment cites as the reason. **Slice B's HTTP call was
rewritten** from GET-with-query-string to POST-directly with
content-negotiated response parsing (`QueryResultsFormat::from_media_type`
against the response's own `Content-Type`, rather than assuming JSON) as a
direct result of this check — a genuine correctness improvement (GET embeds
an unbounded `SERVICE` pattern in a URL, which a real proxy can reject on
length) that would have shipped as a known gap had the check run on schedule.

### OWL 2 QL query rewriting (PerfectRef or equivalent) — checked before Epic 99's slices were written

**5 August 2026, checked before any implementation, per this document's own
standing rule.** Searched crates.io for a permissively-licensed Rust crate
implementing OWL 2 QL query rewriting (the PerfectRef/Presto/Rapid family of
algorithms — expanding a query against a TBox into a union of queries a plain
database can answer, the DL-Lite literature's own name for what
`99-owl-ql-reasoning.md` calls "rewriting the query instead of deriving
facts"):

| Name searched | Result |
|---|---|
| `owl-ql`, `dl-lite`, `perfectref`, `owl2ql`, `query-rewriting`, `whelk`, `whelk-rs`, `owlrs` | Do not exist on crates.io |
| `reasonable` | Exists (BSD-3-Clause, real, 138k downloads) — but it is an **OWL 2 RL** reasoner (see "The pattern that lets us adopt reasoners anyway" above), the profile Epic 6 already ships its own engine for. It materialises facts; it does not rewrite queries, and QL's entire value proposition (first-order rewritable to SQL, §"What QL is genuinely good for" in the plan) is precisely the thing RL-shaped tools do not do |
| `sparql-algebra` (`rust-rdf/sparql.rs`) | Unlicense, but version `0.0.0` — a reserved name, not a released crate, the same signal `sparql` (`0.0.0`) already carried in the SPARQL Protocol check above |
| `horned-owl` | LGPL-3.0, already evaluated above for a different purpose (OWL syntax import) — an ontology *parser*, not a query rewriter, so irrelevant to this question even where its licence were acceptable |

**No candidate exists.** This is a build case, and decision 1 in this
document ("no parser is written for a standard we did not invent") does not
apply here the way it did for SPARQL/Turtle/Cypher — QL rewriting is not a
parser for a syntax; it is an algorithm over the algebra `graph-owl-query`'s
already-adopted `spargebra` parses. The adopted piece (parsing to standard
SPARQL algebra) is already in place per `99-owl-ql-reasoning.md` decision 1;
what remains unbuilt is genuinely novel to this system — expanding algebra
nodes against `graph-owl-ontology`'s subclass/subproperty axioms and handing
the result back to the same planner, bounded by a rewrite-specific budget
(branch count and depth, not the fact-count/iteration shape
`graph-owl-reasoning::Budget` uses for RL's fixpoint, which answers a
different question).

### Portable archive container format (`tar` + `zstd`) — checked before Epic 37b's Slice A

**5 August 2026, checked before implementation.** Epic 37b needs a
container format for the export archive: a manifest plus several
newline-delimited-JSON sections, compressed, with a checksum per section.
This is exactly the shape a container/codec format solves, not something
with a standard *specification* the way Cypher or SPARQL has one — decision
1 ("no parser for a spec we did not write") is about protecting against
copying an implementation of someone else's grammar; a tar layout and a
compression algorithm are neither.

| Name | Licence | Newest version | Downloads (5 Aug 2026) | Repository |
|---|---|---|---|---|
| `tar` | MIT OR Apache-2.0 | 0.4.46 (18 May 2026) | 203,203,529 | github.com/composefs/tar-rs |
| `zstd` | MIT | 0.13.3 (20 Feb 2025) | 346,578,369 | github.com/gyscos/zstd-rs |

Both permissive, both maintained, both among the most-downloaded crates in
the entire Rust ecosystem for their respective jobs — hand-rolling either a
tar writer or a Zstandard encoder would be reimplementing a binary format
with no specification-fidelity requirement pulling the other way. **Adopted,
no spike needed** — this is not a correctness-sensitive parser on a security
path (`00i` rule 4's heightened bar), it is a well-understood, extremely
widely used container format with a huge user base that would have already
surfaced any correctness problem in `tar`'s round-trip behaviour.

### Process RSS measurement (`sysinfo`) — checked before Epic 37a Slice F

**8 August 2026, checked before implementation.** Slice F's soak test needs
the test binary's own resident memory, sampled repeatedly over up to an
hour, to assert RSS growth stays under 10%. `/proc/self/status` parsing
would work on the Linux CI runner this test's nightly job actually runs on,
but not on macOS, where this test's dev-loop sanity runs (short-duration,
`GRAPH_OWL_SOAK_SECONDS` override) actually happen — a hand-rolled
Linux-only reader would silently pass CI and be unusable locally.

| Name | Licence | Newest version | Downloads (8 Aug 2026) | Repository |
|---|---|---|---|---|
| `sysinfo` | MIT | 0.39.6 (9 Jul 2026) | 179,750,316 | github.com/GuillaumeGomez/sysinfo |

Permissive, heavily used, actively maintained. **Adopted, no spike needed**
— this is an operational-measurement dependency (test-only, in
`graph-owl-server`'s `[dev-dependencies]`), not a parser on a correctness or
security path; the only property that matters is "reads the right number
for this OS," which is exactly the cross-platform portability a hand-rolled
`/proc` reader would not have.

### Python OpenAPI read-client generator (`openapi-python-client`) — checked before Epic 36 Slice D

**8 August 2026, checked before implementation.** Slice D (the browse
reference app) needs a generated Python client for `GET` endpoints — the
existing `graph_owl_sdk` package only covers the push/ingest surface
(`ingest.py`), and Slice D's own acceptance criterion ("uses the generated
client, never hand-rolled HTTP") rules out a hand-written `requests` wrapper.
`sdk/typescript` already establishes the pattern of generating from
`openapi.json` (via `openapi-typescript`) rather than hand-writing a client;
this is the same decision for Python.

| Name | Licence | Newest version | Downloads (8 Aug 2026) | Repository |
|---|---|---|---|---|
| `openapi-python-client` | MIT | 0.29.0 (30 May 2026) | 3,305,634/month | github.com/openapi-generators/openapi-python-client |

Permissive, actively maintained, heavily used (the de facto standard Python
OpenAPI-3-to-client generator, the same role `openapi-typescript` already
fills for the TypeScript SDK). **Adopted, no spike needed** — a client
generator is a build tool operating on an already-adopted spec format
(OpenAPI, which this project already generates from), not a parser on a
correctness or security path; the property that matters is "produces a
client that calls the documented endpoints correctly," which is exactly
what a widely-used, actively-maintained generator is exercised against by
its own enormous user base.

Regenerate after any `openapi.json` change:

```
openapi-python-client generate --path openapi.json --meta none \
  --output-path sdk/python/graph_owl_read_client/src/graph_owl_read_client
```

**Packaged as its own distribution, in a `src/` layout — found necessary
while wiring it up, not designed in advance.** `--meta none` outputs the
package flat at `--output-path`, with no `pyproject.toml` of its own.
`graph_owl_sdk` and `graph_owl_read_client` cannot share `sdk/python`'s
existing `pyproject.toml`: that file declares `dependencies = []`
deliberately (a hand-written adapter SDK with zero runtime deps to
resolve), and `graph_owl_read_client` genuinely needs `attrs`/`httpx`/
`python-dateutil` (declared in its own
`sdk/python/graph_owl_read_client/pyproject.toml`) — folding them into one
distribution would give every `graph_owl_sdk`-only consumer three
dependencies they never asked for. A flat `pyproject.toml` placed directly
inside the generated package directory does not work either: setuptools'
`packages.find` searches *inside* the directory the `pyproject.toml`
lives in, so `include = ["graph_owl_read_client*"]` found nothing (the
`graph_owl_read_client` package would need to be a subdirectory of
itself). The standard fix is the standard layout: `pyproject.toml` at
`sdk/python/graph_owl_read_client/`, the actual package one level down at
`sdk/python/graph_owl_read_client/src/graph_owl_read_client/`, and
`[tool.setuptools.packages.find] where = ["src"]`.

## ERPNext / Frappe — the first live-source connector (Epic 105 P2)

| Name | Licence | Role | Activity (checked 10 Aug 2026) |
|---|---|---|---|
| `frappe/frappe` | **MIT** | The framework: DocType metadata, the `/api/resource` REST surface | pushed 10 Aug 2026, 10.5k★ |
| `frappe/frappe_docker` | **MIT** | Official container images and compose files | pushed 9 Aug 2026, 2.5k★ |
| `frappe/erpnext` | **GPL-3.0** | The application: accounting, inventory, sales | pushed 10 Aug 2026, 37.9k★ |
| `resilient-tech/india-compliance` | **GPL-3.0** | Where GST actually lives — **not** core ERPNext | pushed 10 Aug 2026, 262★ |

**Adopt as a connector target. The copyleft gate is not engaged, and it is worth being precise about why**, because this document's own rule rejected `rust-igraph` for exactly the licence ERPNext carries.

- **No linking and no vendoring.** graph-owl speaks HTTP to a separately-deployed ERPNext. GPL-3.0's copyleft attaches to derivative works, not to a program that talks to one over a socket. This project already has that shape twice: the OCR model endpoint (`plans/21b`) and the `whelk-rs` EL sidecar.
- **The surface actually depended on is Frappe's, which is MIT** — the `/api/resource` convention and the DocType metadata endpoints are framework, not application.
- **`cargo deny` is untouched.** No Rust dependency changes; the permissive-only allowlist stays exactly as it is.

**The rule that keeps it that way, and it must not be relaxed: never vendor a doctype definition, schema file or fixture derived from ERPNext into this repository.** The connector *discovers* the schema from a live instance at run time. That is the licence-safe path and also the better engineering — a vendored schema drifts from the instance it claims to describe, silently.

**Why this target rather than a fixture pack.** `packs/hospitality` and `packs/gst` were written by this project, so they fit the platform by construction — weak evidence for domain neutrality. A connector that reads DocType metadata proves the platform accepts a schema **nobody designed for it**, which is the strongest available test of `plans/105-domain-neutrality.md`'s claim.

**Kept out of CI.** The `frappe_docker` stack is MariaDB + Redis + web + worker + scheduler, and first-site creation is slow. The fixture-based GST pack stays the CI path (fast, deterministic, no ERP); ERPNext is the live-source demo — the same split the OCR worker already draws between a scripted endpoint and a real model.

## `pypdfium2` — scanned-PDF rasterization for `OcrPdfParser` (Epic 21 follow-up, Slice 3)

| Name | Licence | Newest version | Repository | Activity |
|---|---|---|---|---|
| `pypdfium2` | `BSD-3-Clause, Apache-2.0, dependency licenses` | 5.12.1 | github.com/pypdfium2-team/pypdfium2 | pushed 9 Aug 2026, 806 stars, not archived |

Checked 10 August 2026. Both named licences are on the permissive
allowlist; the third clause ("dependency licenses") refers to the bundled
PDFium binary's own third-party components, which are the same Chromium
`third_party` set PDFium always ships with — permissive throughout, not a
copyleft dependency hiding behind the package's own licence field.
Repository resolves and is active (pushed yesterday relative to this
check). **Adopt** — this is exactly the "does it actually do the thing"
case: `pypdfium2` renders a PDF page to a raster image via prebuilt
PDFium bindings, which is what `OcrPdfParser` needs before handing a page
to the vision-model seam `ocr.py` already built in Slice 1, and writing a
PDF rasterizer from scratch is squarely the kind of already-solved,
non-differentiating problem this document exists to keep off this
project's plate. No spike: rasterization is not on a correctness or
security path the way a parser or reasoner is — the property that matters
is "produces a bitmap of the page," verified directly by the fixture PDF
in `verify-ocr-worker.sh` (Slice 4), not by a multi-candidate corpus
comparison.

## Temporal validity-period resolution — Epic 105 P8's temporal engine (§7)

Checked 11 August 2026, before writing `graph-owl-resolution::temporal`.
Searched crates.io for `chronoutil`, `temporal-rs` and a handful of
`*-interval`/`*-validity` names: nothing exists at the scale this needs —
"given N candidate `[effectiveFrom, effectiveTo)` windows, which one is in
force at date X" is too small and too shaped by this project's own flake
model (windows come from graph queries, not typed Rust structs handed in)
to be a reusable library concern. The closest general-purpose crates
(`chronoutil`, calendar/duration helpers) solve date arithmetic, which
`chrono` — already a workspace dependency — already provides; nothing
solves the *resolution* question. **Build** — the same conclusion
`passes_span`/`SimilarityBand` already reached for span conditions, and for
the same reason: a five-line resolution rule over already-parsed dates does
not clear the bar for an external dependency.

## `langchain` (umbrella) + `langchain-openai` — Epic 105 P11's investigation agent

| Name | Licence | Newest version | Repository | Activity |
|---|---|---|---|---|
| `langchain-core` | MIT | 1.5.3 | github.com/langchain-ai/langchain | active, already adopted (Epic 43) |
| `langgraph` | MIT | 1.2.10 | github.com/langchain-ai/langgraph | active, already adopted (Epic 43) |
| `langchain` | MIT | 1.3.14 | github.com/langchain-ai/langchain | active |
| `langchain-openai` | MIT | 1.4.3 | github.com/langchain-ai/langchain | active |

Checked 11 August 2026, by direct user instruction to use the latest
LangChain/LangGraph for P11's reference agent. All four MIT, same
`langchain-ai` org, repositories resolve and are readable. **Adopt** —
writing a ReAct tool-calling loop or an OpenAI-compatible chat-model client
from scratch is exactly the already-solved, non-differentiating problem
this document exists to keep off this project's plate, and doing so would
also duplicate what Epic 43 already chose to depend on.

`langchain-core`/`langgraph` were already adopted at Epic 43; this entry
is the re-check the "every epic, every slice" rule calls for, and it
surfaced a real change: `langgraph.prebuilt.create_react_agent` (what
Epic 43's own `test_langgraph_integration.py` uses) is deprecated as of
LangGraph v1.0 in favour of `langchain.agents.create_agent`, which pulls
in the `langchain` umbrella package as a new dependency. P11's agent uses
the new, non-deprecated API; Epic 43's existing test was left alone
(already-shipped, already-tested, and the deprecation is a warning, not a
failure — migrating it is a separate, unrelated refactor this slice did
not need to make).

`langchain-openai` is not added to `pyproject.toml`'s package extras: it
is only `examples/gst_investigation_agent.py`'s own real-usage dependency
(imported lazily, only inside `build_chat_model()`), not something
`graph_owl_langchain` itself, or its test suite, needs — the same
reasoning `reconcile_agent.py`'s stdlib-only design already established
for its own optional narration path.

## `petgraph` in `graph-owl-analytics` — declared, never used; found via a live-agent code review

`plans/38-graph-analytics.md` decision 5 says "Adopt `petgraph`... for the
arithmetic." `graph-owl-analytics/Cargo.toml` has carried
`petgraph.workspace = true` since Epic 38 shipped. **Checked 11 August
2026, after an unrelated review of a live agent trace raised it**: `grep`
across every `.rs` file in the crate finds zero uses of `petgraph` —
`CsrGraph` (`row_offsets`/`col_indices`, `graph-owl-analytics/src/
lib.rs:31-34`) is a hand-rolled compressed-sparse-row structure, and every
algorithm (degree centrality, connected components, `PageRank`, orphan
detection) iterates it directly, not a `petgraph::Graph`. This is not
"hand-rolled instead of the adopted library" in the usual sense of a
choice revisited later — the dependency was declared and then never
referenced even once, which no `cargo check`/clippy warning catches (an
unused `Cargo.toml` entry is silent; `cargo udeps` or equivalent is the
tool that would have caught it, and this project does not run one).

**First removed the dead declaration**, then, on direct follow-up ("but
we should use petgraph, isnt?"), adopted it for real rather than leaving
the gap only documented. The two candidate scopes were evaluated
separately, because they carry very different risk:

- **The algorithms themselves** (`petgraph::algo::page_rank`,
  `petgraph::algo::connected_components`) were checked against
  `graph-owl-analytics`'s own contract and **rejected for a full swap**.
  `page_rank(graph, damping, nb_iter)` takes a fixed iteration count with
  no convergence/tolerance signal at all — no equivalent of this crate's
  `PageRankResult.converged: bool`, which exists specifically so
  iteration exhaustion is never silently reported as a converged answer.
  `connected_components(g) -> usize` returns a bare count — no equivalent
  of `Components`'s per-node assignment, `orphans()`, or
  `filter_edge_types` tracking, all three load-bearing for this crate's
  own governance-reporting use. Swapping these in would have been a
  behavioural regression on shipped, tested output, not a refactor.
- **The adjacency storage** (`CsrGraph`'s hand-rolled `row_offsets`/
  `col_indices`) was adopted. `petgraph::csr::Csr`'s own internal layout
  (`column: Vec<NodeIndex<Ix>>`, `row: Vec<usize>`) is, read directly from
  its source, the identical representation — this was a genuine case of
  "someone already built this," not a coincidence worth re-implementing.
  `CsrGraph` (`graph-owl-analytics/src/lib.rs`) now wraps
  `petgraph::csr::Csr<(), (), Directed, NodeId>`; `project()` builds it via
  `Csr::with_nodes` + `add_edge` rather than `Csr::from_sorted_edges`,
  because the latter sizes itself from the edge list alone and would
  silently drop a node with no edges at all — exactly the orphan case
  `connected_components`'s own `size_one_components_are_the_orphan_set`
  test exists to catch. `degree_centrality`, `connected_components`, and
  `pagerank` — the algorithm bodies, convergence logic, dangling-node
  redistribution, and deterministic tie-breaking — are untouched.

**The differential test this document's own rule 4 asks for was the
crate's existing 30-test suite, unchanged.** Every assertion, including
the mutation-testing-derived ones pinning `<` vs `<=` in union-find and
the closed-form `PageRank` value, passed against the new internals with
zero test-assertion edits — only construction-site code changed. One gap
did surface under mutation testing (`cargo mutants --in-diff`, scoped to
the change): the new `PartialEq for CsrGraph` (needed because
`petgraph::csr::Csr` has no `PartialEq` of its own) had two survivors,
`eq -> true` and its `&&` mutated to `||`, both invisible to every
existing test because they only ever assert two *equal* projections —
never two different ones. Fixed with one negative test
(`an_empty_adjacency_is_not_equal_to_a_non_empty_one`); re-run: 19
caught, 2 unviable, 0 missed.

`38-graph-analytics.md`'s "Adopt `petgraph`... for the arithmetic" line
(not one of its six numbered decisions — a separate library-choice note)
is still not literally what was built: the arithmetic stayed hand-rolled,
deliberately, for the convergence- and component-detail reasons above.
What actually happened is closer to "adopt `petgraph` for storage, keep
the arithmetic" — corrected directly in that plan's own text rather than
left to drift, since the point of writing this down here is so the next
reader does not have to reconcile the two documents themselves.

## `toml`, for `GET /packs/available` reading `pack.toml` server-side

**Adopted.** `MIT OR Apache-2.0` (`crates.io/api/v1/crates/toml`, checked
2026-08-13 — the plain `curl` without a User-Agent header returned
crates.io's own API-access-policy error, not a licence result; retried
with one and got a real answer, recorded here rather than skipped),
`toml-rs` org, `1.1.4` published 2026-07-28, 811M downloads, real
`github.com/toml-rs/toml` repository that resolves. The canonical Rust
TOML parser — Cargo itself is built on it — for a need with no
architectural nuance to get wrong: read a pack's `[pack]` header
(`id`/`label`/`description`) off disk to list packs available to
install, before this server had ever read a `pack.toml` itself (every
pack-installing step to date was the separate `graph-owl-load-pack`
Python process, `connectors/python/graph_owl_packs/loader.py`, using
`tomllib` — 3.11's own standard library, no crate needed on that side).
Writing a hand-rolled TOML reader for four known keys was briefly
considered and rejected on the same reasoning `00i` already applies
project-wide: a parser is exactly the shape of thing to adopt rather than
half-implement, and a partial TOML reader would be wrong about escaping
and multi-line strings the moment a pack author used either.
