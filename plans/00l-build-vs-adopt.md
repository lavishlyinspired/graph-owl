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
| `horned-owl` | **LGPL-3.0** | OWL parsing and manipulation — RDF/XML, OWL/XML, Functional, Manchester | **Adopt out of process.** See below — the earlier "rejected" was wrong about both the licence and the options |

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
