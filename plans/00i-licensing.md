# graph-owl — Licensing & Clean-Room Rules

**Status**: Binding on every implementation session.
**Scope**: all code, tests, fixtures, comments, commit messages, and plan files.
**Not legal advice.** These are engineering rules that keep the project clear of a question a lawyer would otherwise have to answer.

## The situation in one paragraph

Two reference implementations were studied during planning. Their clones live under `.claude/docs/referenceRepo/`, which is gitignored and never committed. **Neither is permissively licensed throughout**, and one is not open source at all:

| Reference | Shape of the licence | Consequence |
|---|---|---|
| Engine reference | **Source-available, not open source.** A non-compete grant blocks using the work to offer a hosted database/graph/ledger service. Converts to a permissive licence four years after each version ships | The code may be read. It may not be reused, vendored, or copied |
| Catalog reference | **Permissive core**, but three subdirectories — the web UI, the ingestion/connector framework, and the agent-protocol server — carry a **community licence with a non-compete** | The three most heavily studied directories are the three least freely licensed |

**graph-owl contains no code from either.** That is the entire basis on which neither non-compete binds this project, and it is a property that has to be actively maintained during implementation, not just asserted once during planning.

## Why this is a live risk during implementation, not just planning

Copyright protects **expression**, not ideas. Reading an architecture and reaching the same conclusion is not infringement; transcribing a function, a constants table, or a test fixture is. The risk is highest in exactly the moments that feel most harmless:

- Getting stuck on an algorithm, opening the reference for "just a look", and typing what you saw.
- Copying a table of magic numbers because they look authoritative.
- Lifting error message strings, log formats, or config key names.
- Reusing test fixtures or golden files.

One instance of this already happened during planning and was caught and reverted: a cache-sizing table was reproduced near-verbatim from a doc comment in a source-available file, **including its rationale**. It was thin material and would probably never have mattered — and it was still copying, and it is still gone. Assume the same failure mode will present itself during implementation, because it will.

## The rules

### 1. Do not open reference source while writing the corresponding graph-owl code

The single most effective rule, and the only one that is mechanically checkable. Study happens in a separate session from implementation. If a reference file has been open in the current session, the corresponding graph-owl file is not written in that session.

### 2. Specifications are the source; implementations are not

Everything the engine layer needs is a published standard, and the standard is always the better reference anyway — it is normative, versioned, and has conformance suites:

| Capability | Read this |
|---|---|
| RDF, SPARQL, OWL 2 RL, SHACL, SKOS | W3C recommendations |
| openCypher / GQL | openCypher spec; ISO/IEC 39075 |
| Bolt wire protocol, PackStream | The published protocol specification (CC-BY-SA) |
| JSON-LD, DCAT, PROV-O | W3C recommendations |
| Problem details, HTTP semantics | RFC 9457, RFC 9110 |
| Index orderings (SPOT/PSOT/POST/OPST) | The triple-store literature — this predates both references by decades |

**If a capability has a spec, the spec is the only permitted reference.** No exceptions, and no "the spec is unclear so I checked how they did it".

### 3. Never copy, in any quantity

- Source, whole or partial, including "adapted" or translated between languages. Translating Java or Rust into different Rust is a derivative work, not a rewrite.
- **Constant tables, thresholds, tuning percentages, size classes, timeouts, retry counts.** Every magic number in graph-owl must be derivable from a stated reason in a plan. If the reason is "the reference used this", the number is not permitted — and it was never justified for this system anyway.
- Error strings, log message formats, metric names, config key names, CLI flag spellings.
- Test fixtures, golden files, sample datasets.
- Comments, doc comments, README prose.

### 4. Every non-obvious number carries its reasoning

A constant with a written justification cannot have been copied thoughtlessly, which makes this rule a control rather than a formality. `plans/` is where the justification lives. A PR introducing an unexplained threshold is incomplete.

### 5. Never name either reference in a committed artifact

Pre-existing rule, restated here because it is now also a licensing hygiene rule. Not in code, comments, commit messages, plan files, tests, or fixtures. Write the pattern and the reasoning, never the source. Named detail for local use lives in `.claude/docs/`, which is gitignored.

### 6. Convergent design is expected — say so, do not hide it

Where an independent design lands on the same decomposition as a reference, that is **evidence the design is right**, and the plans say so explicitly (see `07c-engine-lpg.md` and `31-memory.md`, both of which correct earlier novelty claims). Convergence reached independently is not infringement. Pretending it did not happen is worse than useless: it removes the record that would show the design was reasoned rather than copied.

### 7. Third-party dependencies are audited before they land

`cargo deny` in CI with an allowlist. Permissive licences only (MIT, Apache-2.0, BSD, ISC, Unicode, Zlib). **Copyleft and source-available dependencies are rejected by default** — a GPL crate would impose its terms on the binary, and a source-available one reintroduces exactly the non-compete this document exists to avoid.

## On crate and protocol names

**A generic technical term is not protectable, and near-universal Rust convention is not copying.** `core`, `api`, `server`, `query`, `cli`, `storage` are what every Rust workspace calls those crates. Independent projects converge on them because they are the obvious names.

**Protocol names used descriptively are nominative use.** `graph-owl-bolt` describes the protocol the crate speaks, exactly as `graph-owl-rdf-io` describes the serializations it handles. The protocol's specification is published under a permissive documentation licence and certified independent implementations exist. Naming the crate after what it implements is the clearest possible name and the most defensible.

Two guardrails do apply:

- **Never imply endorsement, origin, or affiliation.** "graph-owl speaks the Bolt protocol" is descriptive and fine. Branding, logos, or wording that suggests a partnership is not.
- **The product name must remain clearly distinct.** `graph-owl` is, and no crate carries another project's product name as its own.

## What to do when genuinely stuck

In order:

1. **The specification.** Nine times in ten it is there, and it is normative where an implementation is merely one interpretation.
2. **A permissively licensed implementation** — Apache-2.0 or MIT, checked before reading. Even then: read for understanding, then write from understanding, and attribute if anything is genuinely derived.
3. **Ask.** A design question routed to a human costs an hour. A licensing question discovered at acquisition due diligence costs considerably more.

**Never**: open the source-available or community-licensed reference to unblock an implementation task. That is precisely the moment the rule exists for.
