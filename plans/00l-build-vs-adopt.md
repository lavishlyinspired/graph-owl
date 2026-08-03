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
| **openCypher grammar** | **Apache-2.0** | Official Cypher grammar: XML source, generated EBNF + ANTLR4, railroad diagrams | **Adopt the grammar** — see the Cypher section below |
| **openCypher TCK** | Apache-2.0 | Cucumber features defining Cypher behaviour | **Adopt as a conformance oracle** |
| `decypher` | EUPL-1.2 **OR** MIT **OR** Apache-2.0 | openCypher parser, `rowan`-based, error-resilient, returns `Unsupported` for unhandled productions | **Spike before committing** — active (2026-05-19) but `0.2.0-alpha.6`, and its own README says the AST is unstable until 0.2.0 |
| `open-cypher` | MIT | `pest` grammar derived from the openCypher EBNF | **Reject — abandoned.** Last published 2022-07-23 at v0.1.1. The *approach* is right; the crate is four years stale |
| `ocg` | Apache-2.0 | "100% openCypher-compliant graph database" | **Reject** — it is a database, so it owns storage and crosses the line above; and its repository is on `github.ibm.com`, which nobody outside IBM can audit |
| `antlr4rust` | BSD-3-Clause on crates.io, `NOASSERTION` on GitHub | ANTLR4 runtime for Rust | **Reject** — repository unpushed since 2023-02-14, and the two licence claims disagree |
| `antlr-rust-runtime` | BSD-3-Clause | Newer ANTLR4 Rust runtime | **Reject for now** — fresh (2026-08-01) but 2,439 downloads; too thin to put a query front end on |
| `pest` | MIT / Apache-2.0 | PEG parser generator | **Adopt** — 293M downloads; the generation target for the Cypher subset |
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


## Cypher: adopt the grammar, generate the parser

**Verified against crates.io, the openCypher repository and GitHub on 4 August
2026.** This section exists because "reuse openCypher" means two different
things and only one of them was ever in doubt.

1. **Reuse the openCypher *specification and grammar*.** Never in doubt, and
   `00i` rule 2 requires it: specifications are the source. The grammar is
   **Apache-2.0**, published as XML source with generated EBNF and ANTLR4.
2. **Reuse someone's openCypher *parser implementation*.** This is the question,
   and the answer in 2026 is **no** — not because building is better in
   principle, but because nothing in the Rust ecosystem is both maintained and
   stable enough to put a query front end on.

### The finding that changes the estimate

`07c-engine-lpg.md` records that a reference implementation carries a
**~10,000-line openCypher front end**. That number has been misread — including
by this project — as an estimate for Epic 7b Slice A. It is not. It is the cost
of a **complete** front end for the whole language: lexer, parser, validator and
diagnostics.

**Epic 7b's declared subset is eleven clause forms** — `MATCH`,
`OPTIONAL MATCH`, `WHERE`, `RETURN`, `WITH`, `UNWIND`, `ORDER BY`, `SKIP`,
`LIMIT`, `DISTINCT`, and variable-length patterns — with `CALL`, procedures,
`FOREACH`, path predicates and shortest-path explicitly out. That is a different
order of magnitude, and the plan should not be sized against the larger number.

### The decision

**Vendor the Apache-2.0 openCypher EBNF and generate a `pest` grammar
restricted to the declared subset.** Not a hand-written lexer and parser, and
not a third-party parser crate.

Three reasons, in order of weight:

1. **"A documented subset" stops being a promise and becomes a file.** The
   grammar *is* the declaration. Productions outside it do not parse, so the
   `Unsupported` answer the plan requires falls out of the parse error rather
   than needing a separate validator kept in step with the parser — which is
   two things that can disagree.
2. **No fragile dependency in a security-relevant path.** Authorization is
   compiled into the plan (Epic 7 Slice E), so the query front end is on the
   security path. `decypher` is `0.2.0-alpha.6` with an explicitly unstable AST;
   `open-cypher` has not been touched since 2022. `pest` is MIT/Apache-2.0 with
   293M downloads.
3. **The route is already proven.** `open-cypher` is exactly this — a pestfile
   derived from the openCypher EBNF. We take the approach and leave the
   abandoned crate.

**Adopt the TCK regardless of route.** It is Apache-2.0 Cucumber features that
say empirically what our subset supports, rather than us asserting it — the same
role `00k` gives specification conformance elsewhere.

### What would change this

`decypher` is the better answer the moment its AST stabilises: `rowan` gives
error-resilient parsing and column-accurate diagnostics for free, and it already
returns `Unsupported` rather than panicking, which is the exact shape Slice A
needs. **A one-hour spike against its API belongs before Slice A, not instead of
it** — and this section should be revisited when it reaches 0.2.0 proper.

What is *not* a reason to revisit: an ANTLR route. It would add a Java
build-time dependency to a Rust workspace and CI, on runtimes that are either
stale or barely adopted.
