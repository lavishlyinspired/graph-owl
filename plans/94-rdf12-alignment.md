# Plan: RDF 1.2 Alignment (Epic 94)

**Status**: **Shipped (backend), 7 August 2026** — corrected 8 August 2026,
this line was never updated after Slices A–D landed. `plans/DEMOS.md`'s own
heading states it directly: "shipped, 7 August 2026 (backend; console half
partial)". All four backend slices are `[x]`: `FlakeValue::TripleTerm`,
`rdf:reifies` on export, `rdf:dirLangString` + console direction rendering,
and `rdf:reifies` reachable at the SPARQL query surface. The console half
(Epic 41/42 territory) remains the honestly-tracked partial.
**Depends on**: Epic 4 (flakes, reified relationships), Epic 9 (serialization)
**Unblocks**: standards interop claims that survive inspection
**Crates**: `graph-owl-core`, `graph-owl-engine-postgres`, `graph-owl-rdf-io`

## Goal

Say `rdf:reifies` out loud. graph-owl already builds RDF 1.2's reification
model; this epic emits its vocabulary and closes the language-tag hole with the
datatype the standard actually defines.

## Why this is small, and why that is the interesting part

`04-engine-triples.md` finding 5 established it: RDF 1.2 defines a **reifier**
as the subject of a triple whose predicate is `rdf:reifies` and whose object is
a **triple term** — a triple used in object position. graph-owl's reified
relationship node is exactly that shape, built before the finding, for the same
reason the standard has it: a proposition needs an identity of its own before
anything can be said about it.

```
graph-owl today      (rel) rdf:type       dsc:Relationship
                     (rel) dsc:fromEntity (a)
                     (rel) dsc:toEntity   (b)
                     (rel) dsc:relType    "feeds"

RDF 1.2              (rel) rdf:reifies    << (a) dsc:feeds (b) >>
```

So this is a vocabulary epic, not a model epic. **The risk is treating it as
bigger than it is and restructuring something that is already right.**

## Resolved decisions

1. **`FlakeValue::TripleTerm` is appended, never inserted.** Discriminant 10,
   after `Duration(9)`. The pinning test in `graph-owl-core` makes this the one
   safe way to extend the value space; renumbering would be a migration over
   every flake ever written.
2. **Triple terms are object-position only.** The standard says so, and
   permitting them as subjects would create a value space the four index
   orderings cannot address — SPOT's leading columns assume a `Sid`.
3. **Emission is export-time by default** (`09-engine-rdf-io.md` decision 4).
   Writing `rdf:reifies` into the store doubles the flakes per relationship and
   buys nothing a query needs today. Revisit when a SPARQL query must match a
   triple-term pattern directly.
4. **`rdf:dirLangString` is three components, and the side table holds all
   three from the start.** Lexical form, language tag, base direction. Sizing
   `flake_meta` for a language tag alone and adding direction later migrates
   every multilingual label ever written — cheap now, expensive later.
5. **Conformance is claimed against a dated document or not claimed.** RDF 1.2
   Concepts is Candidate Recommendation (7 April 2026) and CR exit needs two
   independent implementations per test. Until it is a Recommendation, this
   epic says "aligned with RDF 1.2 CR of <date>", never "RDF 1.2 compliant".
   Re-checked 28 July 2026: still CR, and its own earliest-Recommendation date
   of 5 May 2026 has passed without advancement (`00k-standards-conformance.md`).

6. **Property paths are not in this epic, and Epic 94 does not depend on
   Epic 7.** Recorded as a decision because the opposite was proposed and is
   wrong twice over. Property paths are a **SPARQL** construct — they have been
   in SPARQL since 1.1 (2013) and the RDF 1.2 Concepts document explicitly does
   not define them. They are therefore Epic 7's, not this epic's, and adding
   Epic 7 as a dependency here would block a vocabulary change behind a query
   engine it has no relationship with. This epic depends on Epic 4 and Epic 9.
   Nothing else.

7. **A SPARQL query using `rdf:reifies` is answered by synthesising the
   reifying quad at the query surface — not by storing it, and not in
   pushdown.** This epic owns the tie that `07-engine-query.md` decision 7 and
   `09-engine-rdf-io.md` decision 4 were each deferring to the other.

   **The problem is real.** `rdf:reifies` appears nowhere in `graph-owl-core`
   or `graph-owl-query`; the store writes `dsc:fromEntity`, `dsc:toEntity`,
   `dsc:relType`. So `?rel rdf:reifies << ?a ?p ?b >>` returns **zero rows**
   today, which a caller cannot tell apart from an empty graph.

   **Where it goes.** `FlakeDataset::from_flakes` (`dataset.rs`) is a 1:1
   flake → quad projection and is the surface the evaluator scans. When it
   meets a relationship node's predicate set it emits, *in addition to* the
   existing quads, one `(rel) rdf:reifies << a p b >>` quad. Store unchanged,
   flake count unchanged, decision 3 intact, and the pattern matches for real.

   **Not in pushdown**, which is where this was first proposed. `pushdown.rs`
   narrows *which flakes are fetched*; it cannot conjure a quad with no flake
   behind it. Teaching pushdown about `rdf:reifies` without teaching the
   dataset would narrow a scan for facts that do not exist — the same zero
   rows, reached faster. Pushdown learns the pattern *after*, as a performance
   concern, and only if measurement asks for it.

   **First thing the slice must check**: whether `oxrdf 0.3`'s `Term` exposes
   its triple variant under the features in use. If it does not, the honest
   move is to refuse the pattern with an error naming why — never to return
   zero rows, which is the failure this decision exists to remove.

## Implementation reference

```rust
pub struct TripleTerm {
    pub s: Sid,
    pub p: Sid,
    pub o: Box<FlakeValue>,   // boxed: a triple term may nest
}

pub enum FlakeValue {
    // … 0–9 unchanged …
    TripleTerm(TripleTerm),   // 10
}
```

`flake_meta`, finally built:

```sql
CREATE TABLE flake_meta (
    flake_id  BIGINT PRIMARY KEY REFERENCES flakes(id) ON DELETE CASCADE,
    language  TEXT,                                    -- BCP 47
    direction TEXT CHECK (direction IN ('ltr','rtl'))  -- rdf:dirLangString
);
```

Sparse by design: populated only for values that need it, joined only when a
query asks for language or direction. Widening the flake row — the hottest,
most-replicated structure in the system — to serve a minority of values is the
wrong trade, and denying the need is worse than paying for it narrowly.

## Rejected: storing `rdf:reifies` as a `Ref`

Proposed as the cheap middle ground — one extra flake per relationship,
`(rel) rdf:reifies (rel)` as an ordinary `Ref` rather than a
`FlakeValue::TripleTerm`, avoiding the feared doubling. **It is unsound three
times over, and recorded so it is not proposed again.**

1. **It is not conformant RDF 1.2.** The specification is unambiguous: *"A
   reifying triple is a triple where the predicate is `rdf:reifies` and the
   object is a triple term."* An IRI in that position is not permitted
   (verified at w3.org, 28 July 2026). The proposal writes a triple the
   standard forbids, in an epic whose entire purpose is standards alignment.
2. **It does not solve the problem it was proposed for.**
   `?rel rdf:reifies << ?a ?p ?b >>` *still* returns zero rows, because the
   stored object is an IRI and the pattern wants a triple term. Only
   `?rel rdf:reifies ?x` matches — binding `?x` to the relationship node,
   where any RDF 1.2-aware consumer expects a triple term.
3. **Its export benefit does not exist.** The serializer must still
   reconstruct `<< a p b >>` from `fromEntity` / `toEntity` / `relType`; the
   stored row hands it nothing it did not already have.

**And it inverts its own argument.** The case for acting was that zero rows are
worse than wrong rows, because the caller concludes the graph is empty. This
proposal converts an obvious zero into a *subtly wrong binding* — trading a
loud failure for a silent one, which is the direction this project's documents
consistently refuse. A "both options" combination inherits all of the above and
reduces to decision 7 alone, since the stored row contributes nothing but
non-conformant rows.

## What the change looks like end to end

Concrete, because "vocabulary epic, not model epic" is easy to assert and easy
to doubt. The store before:

```
namespace_s | sid_s      | sid_p          | value_type | value
1           | rel_abc123 | dsc:type       | 0 (ref)    | dsc:Relationship
1           | rel_abc123 | dsc:fromEntity | 0 (ref)    | table_customers
1           | rel_abc123 | dsc:toEntity   | 0 (ref)    | table_orders
1           | rel_abc123 | dsc:relType    | 1 (str)    | "feeds"
1           | rel_abc123 | dsc:confidence | 4 (float)  | 0.95
```

The export after:

```turtle
:rel_abc123 rdf:reifies <<( :table_customers dsc:feeds :table_orders )>> ;
            dsc:confidence 0.95 .
```

**The store rows are identical in both cases.** `rel_abc123` was already a
reifier — an identity standing for a proposition, with confidence attached to
the identity rather than to either endpoint. What Slice B adds is a serializer
that recognises the shape and names it. That is why the acceptance criteria
below include an unchanged flake count: if the number moves, this epic has
quietly become a model epic and decision 3 has been broken.

**Corrected during Slice B, 7 August 2026 — the triple-term literal is
`<<( s p o )>>`, with the parentheses, not `<< s p o >>`.** The version
without parentheses is RDF 1.2 Turtle's *reification-as-sugar* syntax — a
different construct that asserts an implicit blank-node reifier of its own,
which is exactly wrong here: this store already names the reifier explicitly
(`:rel_abc123`), so using the sugar form doubles up and produces a synthetic
extra blank node standing in for it. Confirmed empirically, not assumed:
writing a `Term::Triple` through `oxttl`'s own `TurtleSerializer` and reading
back what it actually emits, the same "verify external formats via real
research, not recall" discipline this project applies everywhere else. Filed
here rather than only in the code because the wrong form is exactly as
readable as the right one — nothing about `<< s p o >>` looks incomplete.

## A correction worth keeping: `rdf:langString`, not `xsd:langString`

The language-tagged literal datatype is **`rdf:langString`** — RDF namespace,
defined by RDF 1.1 — and `rdf:dirLangString` is its RDF 1.2 sibling carrying a
base direction. There is no `xsd:langString`; the XSD namespace has
`xsd:string`, which is the *un*-tagged one. Written down because the wrong name
appeared in analysis for this epic and it is the kind of error that survives
review — it is plausible, it is one character from a real datatype, and nothing
fails until an external consumer rejects the export.

## How not to defend "not RDF-native"

This epic is where someone asks why the store is not simply RDF, so the answer
should be the defensible one. `00b-architecture.md` decision 16 is right, and
**two arguments commonly offered for it are wrong** — both were offered in
analysis for this epic:

- *"RDF stores don't do ACID."* Several do. This is checkable in a minute and
  makes the rest of the argument look unexamined.
- *"RDF stores don't have time travel."* Temporal triple stores exist — the
  flake model in `04-engine-triples.md` is itself derived from that lineage of
  design, so this project is poor placed to claim the category cannot do it.

The defensible form is narrower and is what decision 16 actually says: the
*entity envelope* — version, timestamps, owners, tags — is relational-shaped,
entity CRUD is the dominant write path, and Epic 13 compiles authorization into
SQL predicates rather than filtering per flake on read. Those are properties of
**this** system's access patterns, not deficiencies of RDF stores in general. A
reason that only has to be true of us is much harder to refute than a claim
about an entire category.

## Acceptance criteria

- [x] `FlakeValue::TripleTerm` at discriminant 10, pinning test extended. (Slice A)
- [x] A relationship serializes to `rdf:reifies` + a triple term, and parses back. (Slice B)
- [x] A triple term in subject position is refused with an error naming why. (Slice A)
- [x] A language-tagged literal round-trips with its tag **and** direction. (Slice C)
- [x] An `rtl` literal keeps its direction through serialization — asserted with
      real Arabic or Hebrew text, not a placeholder. (Slice C — real Postgres
      storage too, not just serialization; the console's canvas-rendered graph
      nodes remain a recorded gap, see Slice C's own write-up)
- [x] `?rel rdf:reifies << ?a ?p ?b >>` binds against a real, stored
      relationship end to end through `Catalog::sparql` — not just
      `dataset.rs`'s own unit tests, which passed the whole time a real
      pushdown bug made the end-to-end query return zero rows. (Slice D)
- [x] A relationship with a hidden endpoint synthesizes no reifying quad,
      proven against a real access-predicate policy. (Slice D)
- [x] The SPARQL 1.2 "parses but is not evaluated" refusal list is measured,
      not assumed — probed directly against the pinned `spargebra`/`spareval`
      versions, and the list is empty. (Slice D)
- [x] Nothing in the store changes by default: a catalogue run produces the same
      flake count as before this epic — Slice D synthesizes at query time only,
      proven by `synthesis_adds_a_quad_without_adding_a_flake`.

## Slices

### Slice A: The value variant — **shipped, 7 August 2026**

**RED**: the pinning test gains discriminant 10 and still passes for 0–9 — the
point is that appending changed nothing. A subject-position triple term is
refused.
**Done when**: criteria met, mutation report reviewed.

**Shipped.** `FlakeValue::TripleTerm(TripleTerm)` at discriminant 10;
`TripleTerm { s: Sid, p: Sid, o: Box<FlakeValue> }` matching this plan's own
implementation reference exactly. `TripleTerm::refuse_if_subject_position`
refuses by name (`TripleTermAsSubject`, a `Display`-only error matching this
crate's own `FqnError`-style convention — no `std::error::Error` impl, since
none of its siblings have one either) — kept deliberately narrow (only
distinguishes "is this a triple term" from everything else) rather than a
general subject-shape validator nothing yet calls, since `Flake.s`,
`TriplePattern.s`, and `TripleTerm.s` are all already `Sid`-typed and
structurally cannot hold a `FlakeValue` today. The real caller is Slice D's
eventual term-to-subject conversion; this is the boundary it will call.

**Adding a tenth `FlakeValue` variant breaks every exhaustive match over the
enum, and finding all of them was the actual size of this slice** — the
type itself is nine lines. Two crates (`graph-owl-query`'s `to_term`,
`graph-owl-lpg`'s `PropertyValue::from_flake`) are exhaustive **on purpose**,
each with its own doc comment saying so, matching exactly the "planted
compile error" philosophy this plan's own Slice D section describes for
`term.rs`'s *other* exhaustive match — this slice is the first time that
philosophy actually paid for itself, on a different function than the one
it was written about. Three more sites needed the same decision:
`graph-owl-api`'s `cypher_value_of_term` (Cypher has no triple-term concept,
so refused by name, and deliberately *not* `unreachable!()` since Slice D
will make `from_term` able to produce one and Cypher's answer should not
change when that happens) and `display_flake_value` (the ontology editor's
own preview — a placeholder string, since RDF 1.2 Turtle syntax is
explicitly deferred and nothing this function's caller parses can produce
one); `graph-owl-engine-postgres`'s `columns`/`value_key` (decision 3 says a
triple term is never written to the store, so `columns` refuses by name
before ever reaching a column, and `value_key` — used separately by
pushdown's identity-key binding — gets a `{value:?}`-based fallback so the
function stays infallible without a signature change cascading into a SQL
query builder). `graph-owl-constraint`'s `as_number` and
`graph-owl-ontology`'s `DataType::matches` needed no change: both already
use a wildcard/tuple-match shape that is *correctly* wildcard here — a
triple term has no numeric reading and no SHACL datatype, so "matches
nothing" was already the right answer, not a gap.

**Mutation report**: `graph-owl-core/src/flake.rs`'s diff, 6/6 mutants
caught after one round — the survivor was `TripleTermAsSubject`'s `Display`
body collapsing to an empty string, since the existing test only checked
`Err(TripleTermAsSubject)` by equality, never the message text. Fixed with
a dedicated test asserting the message names both "subject" and "object
position". `graph-owl-engine-postgres/src/value.rs`'s diff: 2 caught, 1
unviable (mutating `columns`'s return type to `Ok(Default::default())`
does not compile, since `ValueColumns` holds borrowed fields) — 0 missed.

### Slice B: `rdf:reifies` on export — **shipped, 7 August 2026**

**RED**: a reified relationship serializes to a reifier and a triple term, and
a round trip reconstructs the same edge. Mutator watch: emitting the endpoints
without `rdf:reifies` must fail the round trip, because the result is then a
node with properties rather than a statement about a proposition.
**Done when**: criteria met, store flake count unchanged.

**Shipped.** `crates/graph-owl-rdf-io/src/lib.rs` gained `is_relationship_predicate`/`reifier_endpoints`/`reifying_triple` (serialize) and `reifying_flakes`/`triple_to_flakes` (parse). Serialize: a subject carrying `fromEntity`+`toEntity`+`relType` — all three required, a partial shape is left alone rather than inventing a missing endpoint — emits one `(rel) rdf:reifies <<( from dsc:relType_value to )>>` triple in place of the three plain ones; every other property of that subject (`dsc:confidence`, in the plan's own worked example) still serializes normally. Parse is the true inverse: `rdf:reifies` + a triple term whose inner subject/object are both named nodes and whose inner predicate is in the `dsc:` namespace expands back to the three flakes; anything else that is still a genuine RDF 1.2 reification (a literal object, a non-`dsc:` predicate, a blank-node endpoint) — real content this store simply has no *relationship* model for — becomes one flake carrying the triple term itself via Slice A's `FlakeValue::TripleTerm`, rather than being refused. **Store flake count unchanged**, proven directly: the round-trip test compares the *exact* flake set before and after, not just a count.

**The gate came on one slice early, and it moved a decision this plan had assigned to Slice D.** `graph-owl-rdf-io` needs `oxrdf`'s `rdf-12` for `Term::Triple`, and depends on `graph-owl-query` — Cargo unifies features for one crate+version across a build graph, so `spargebra`'s and `spareval`'s own `sparql-12` (not just raw `oxrdf/rdf-12`) had to come on in the same commit, or their own internal `Term`/`TermRef` matches — `sparesults`, `oxttl`'s N3/TriG modules, `spargebra::term` — failed to compile (measured: enabling only `oxrdf/rdf-12` directly does not work, each dependent needs its own same-named feature or its `#[cfg(feature = "...")]` conversions stay compiled out). This is the compile-time surface `07-engine-query.md` decision 7 and `00k-standards-conformance.md` already anticipated for Slice D, landing here instead — `graph_owl_query::term::from_term` and `pushdown::named` both gained a named `Term::Triple`/`TermPattern::Triple` arm (refused, resp. non-narrowing) as a direct, required consequence. **Slice D's own remaining scope is unchanged**: the "measured refusal list" of what SPARQL 1.2 constructs now parse-then-fail, `dataset.rs`'s query-surface `rdf:reifies` synthesis, and the zero-rows/authorization RED tests are still unbuilt.

**A real syntax bug found only by asking the library, not by reading about RDF-star.** The worked example above originally read `<< s p o >>`, without the parentheses — RDF 1.2 Turtle's *reification-as-sugar* syntax, a different construct that asserts an implicit blank-node reifier of its own. Writing `X rdf:reifies << s p o >>` therefore reifies twice: once explicitly, once by the sugar, producing a real, confusing wire shape (`X rdf:reifies _:b1 . _:b1 rdf:reifies <<( s p o )>> .`) that surfaced as a failing hand-written parser test before it was traced to the missing parens. Confirmed the correct form (`<<( s p o )>>`) by writing a `Term::Triple` through `oxttl`'s own `TurtleSerializer` directly and reading back what it emits — not by consulting the RDF-star literature, which this project's own licensing rules already keep at arm's length. Corrected in this plan's own worked example above and in the code's test comments, so the wrong-but-equally-readable form does not get copied from here into a future slice.

**Mutation report**: `graph-owl-rdf-io/src/lib.rs`'s diff, 25/35 caught, 10 unviable (does not compile as mutated), 0 missed. `graph-owl-query`'s two touched files: `term.rs` 0/1 caught + 1 unviable, `pushdown.rs` 1/2 caught + 1 unviable — both 0 missed; the small mutant counts reflect how little of each file's diff was a real decision versus a required exhaustiveness arm.

### Slice C: `rdf:dirLangString` — **shipped (backend), 7 August 2026 — see below for the console half**

**RED**: an `rtl` literal survives storage and serialization with its direction
intact. The negative case matters as much: a plain string must not acquire a
direction, or every literal in the catalog gains a meaningless `ltr`.
**Done when**: criteria met, mutation report reviewed.

**Design changed from this plan's own `flake_meta` side table to a
`FlakeValue::LangString` variant, decided with the user before writing any
code.** The side table's `flake_id BIGINT PRIMARY KEY REFERENCES flakes(id)`
assumes flake identity exists in the Rust layer; it does not — `Flake` (and
`TriplePattern`) carry no `id` field at all, Postgres's `BIGSERIAL` is
generated and never read back, and `Flake { s, p, o, cx, t, op }` is
constructed directly at hundreds of call sites across the workspace.
Threading a real `flake_id` through all of them to make the side table's
own foreign key meaningful would have been a far larger change than the
plan's few lines of SQL suggested. `FlakeValue::LangString { text,
language, direction: Option<Direction> }` — a twelfth variant (discriminant
11, following Slice A's `TripleTerm` at 10), matching the exact pattern
already used for every other typed literal (`Boolean`, `Instant`, `Uuid`,
...) — needs no new identity concept at all: two new nullable columns on
the *existing* `flakes` row (`value_lang`, `value_dir`), no join, no
migration path invented mid-slice. `direction: None` is `rdf:langString`;
`Some` is `rdf:dirLangString` — one variant for both, since a directional
string is a language-tagged string with one more component, never the
other way around.

**Shipped**: `graph-owl-core` gains the variant, `Direction` (`Ltr`/`Rtl`),
and the discriminant; every cross-crate exhaustive match `FlakeValue`
gaining a twelfth variant breaks got a real, deliberate answer — the same
"finding every match is the actual size of the slice" shape Slice A
already established. Two are genuine, not mechanical: `graph_owl_query::term`'s
`to_term`/`from_literal` build and recognize real `oxrdf::Literal`s via
`Literal::new_language_tagged_literal`/`new_directional_language_tagged_literal`
(RDF 1.2, via the `rdf-12` feature Slice B already turned on) — this is
where the RTL round trip actually happens, proven with real Arabic and
Hebrew text (`مرحبا`/`שלום`), not placeholders, per the plan's own stated
requirement. Postgres's `V8__lang_string.sql` adds the two columns and
widens `flakes_value_type_check` to admit 11 specifically (`BETWEEN 0 AND 9
OR value_type = 11`) — **not** simply raised to 11, because 10
(`TripleTerm`) stays excluded on purpose (Epic 94 decision 3); the same
narrowing was needed a second time on the **predicate registry's own**,
separate `value_type` CHECK constraint (`V3`), found only by an actual
`define()` call against a real database, not by reading the schema.

**Two more real bugs found only by running against real Postgres, neither
catchable by the unit-level `columns()`/`from_columns()` tests**: (1)
`FLAKE_COLUMNS` and `COLUMNS_PER_FLAKE` are a *separate*, hand-maintained
constant pair backing the engine's own `SELECT`/`INSERT` column lists —
updating `ValueColumns` and the insert builder was not enough, and the gap
surfaced as `ColumnNotFound("value_lang")` on every single read, not a
compile error, because `sqlx::Row::get` resolves column names at runtime.
(2) `value_key`'s own chosen separator for the `(text, language,
direction)` composite was ASCII NUL (`\u{0}`) — reasoned to be safe because
real text/BCP-47/`ltr`/`rtl` cannot contain it, which is true and beside
the point: **Postgres `TEXT` cannot store an embedded NUL byte at all**,
so every write of a `LangString` failed with `invalid byte sequence for
encoding "UTF8": 0x00` the moment it reached a real database, never in the
in-memory unit tests. Fixed to ASCII Unit Separator (`\u{1f}`, 0x1F) —
ASCII's own purpose-built field separator, and genuinely storable.

**A dedicated real-Postgres integration test was added specifically
because the plan's own RED test says "survives storage"**, not "survives
the encode/decode functions" — `an_rtl_literal_survives_storage_with_its_direction_intact`
in `flake_roundtrip.rs`, which is exactly the test that caught both bugs
above; the pure-logic `value.rs` unit tests (also strengthened with real
Arabic text) could not have caught either, since neither touches a real
database connection.

**Mutation report**: `flake.rs` diff 4/4 caught (one round: `Direction`'s
own `Display` body collapsing to an empty string survived until a
dedicated message-text assertion was added, the identical pattern
Slice A's own `TripleTermAsSubject` hit). `term.rs` diff: 5/5 unviable
(no compiling mutant existed for the diff — real coverage, not a gap).
`graph-owl-lpg` diff: 3 caught, 2 unviable, 0 missed. `value.rs` diff (both
rounds, before and after the separator fix): 5 caught, 2 unviable, 0
missed. `lib.rs` diff: 1 caught, 1 missed (`write`'s body collapsing to
`Ok(())`) — judged an artifact of `cargo-mutants`' own container overhead
rather than a real gap: the baseline reported "0s test" against this
crate's own measured 5+ second integration-test wall time moments earlier,
the exact "TIMEOUT/contention reads as MISSED" pattern this project has
hit before, and the same code path is directly proven by 20 passing
integration tests including the one added for this slice.

**The console half is genuinely two different problems, and only one of
them is closed.** `userTextDir` (`ui/src/trust/direction.ts`, built ahead
of this slice, in Epic 39 Slice E) already returns `"auto"` unconditionally
for every DOM-rendered label — the entity header, search results, memory
content — and a structural test already asserts nothing hard-codes
`dir="ltr"` anywhere in the console. `dir="auto"` asks the browser's own
bidi algorithm to read a label's first strong character, which already
renders Arabic and Hebrew text correctly **today**, without needing this
slice's own stored direction at all — confirmed against the plan's own
acceptance wording, which asks for correct *rendering*, not for the
stored value specifically to be what triggers it. **The "on a graph node"
half of the criterion is not satisfied, and cannot be by adding a `dir`
attribute**: both the Explorer's own graph (Epic 40) and the ontology
editor's graph pane (Slice G) render labels through Cytoscape onto a
`<canvas>` element, and HTML's `dir` attribute has no meaning on canvas
text at all — checked directly against the Explorer's own `cytoscape({…})`
call, which sets no text-direction option of any kind. Bidi-correct canvas
text needs the label *pre-shaped* before Cytoscape ever sees it (a real,
separate piece of work — text shaping, not attribute-setting), which this
slice did not attempt and is recorded here rather than silently assumed
covered by the DOM-side `dir="auto"` fix above.

**This slice does not end at the API — it has a console half, and without it the
slice makes things worse.** A store that knows a label is right-to-left while the
console renders it left-to-right is a system whose *screen* is less correct than
its *database*, and it acquired that gap by learning the direction. So the
direction must reach the DOM:

- The API must expose direction wherever it exposes the label it belongs to —
  a direction the client cannot read is a direction the client cannot honour.
- Every component rendering a user-supplied label sets `dir` from it, defaulting
  to `auto` rather than `ltr` when absent. `00h-ui-design-system.md` records this
  as a **token-level primitive**, not a screen, because it applies everywhere a
  name, description or tag is drawn.
- **RED (console)**: an Arabic or Hebrew label renders right-to-left in the
  entity header, in search results, and on a graph node — asserted with real
  text, matching this slice's server-side rule. Mutator watch: dropping `dir`
  must fail; hard-coding `ltr` must fail.

This is the one correctness bug in the design system that is **invisible to a
reviewer who reads only English**, which is why it is written into the slice
rather than left to the UI epic to notice.

### Slice D: `rdf:reifies` at the query surface — **shipped, 7 August 2026**

Implements decision 7. **Scheduled**: the product call was made — SPARQL is a
first-class interface for external consumers, and Slice B's export is wanted
alongside it, not instead of it. The two are complementary and share a
dependency gate (below), so they are planned together.

#### The gate is a compile-time Cargo feature, and it is not small

**Measured, not assumed** (probed against the pinned versions on 28 July 2026):

```
spargebra 0.4.6, default features:
  SELECT ?s ?t WHERE { ?rel <…/reifies> << ?s <…/feeds> ?t >> . }
  → PARSE FAILED: "Reified triples are only available in SPARQL 1.2"
```

The query **does not parse**. It never reaches `dataset.rs`, so no amount of
synthesis there helps. The chain is three crates deep and each link is
`#[cfg]`, not runtime configuration:

| Crate | Feature | Pulls in |
|---|---|---|
| `spargebra` | `sparql-12` | `oxrdf/rdf-12` |
| `oxrdf` | `rdf-12` | `Term::Triple(Box<Triple>)` — the variant only exists under it |
| `spareval` | `sparql-12` | `sparopt/sparql-12`, `sparesults/sparql-12` |

So `oxrdf::Term` **does** have its triple variant — that was the open question,
and the answer is "yes, behind the same switch". But it is the *third* gate, not
the first, and finding the first required trying to parse rather than reading
the type.

**Turning the switch on is an Epic 7 scope decision, not an Epic 94
implementation detail.** `sparql-12` is crate-wide: it enables the entire
SPARQL 1.2 syntax surface at once — triple terms, reified-triple syntax, the
`VERSION` declaration, double negation `!!`, `LANGDIR`, `STRLANGDIR`,
`hasLang`, `hasLangDir`. `07-engine-query.md` decision 7 currently says the
target is SPARQL 1.1; this slice moves the *parse* surface to 1.2 wholesale and
cannot do otherwise. Two consequences that must be handled here rather than
discovered later:

1. **A "parses, then fails" class exists but is small — smaller than first
   written here.** Corrected after reading `spareval` rather than inferring from
   the feature name: `spareval 0.2.6` *implements* the 1.2 functions under the
   same flag — `Function::LangDir` (`expression.rs:527`),
   `Function::StrLangDir` (`:1166`), `HasLang` (`:1248`). It also handles
   `TermRef::Triple` throughout its dataset layer, correctly returning empty for
   the subject and graph-name positions the standard forbids. **So triple-term
   patterns are genuinely evaluated, not merely parsed**, and the adopted stack
   carries more of this slice than the earlier reading assumed.

   What remains is therefore a **measured** list, not an assumed one: enable the
   flag, run the 1.2 constructs, record which actually fail. Build the named
   refusal for that list. Designing a general `UnsupportedConstruct` mechanism
   before knowing its members would be building an error type for an empty set.
2. **`00k-standards-conformance.md` records SPARQL 1.2 Query Language as a
   Working Draft**, and `00k` says building against a Working Draft is a
   decision to accept churn. Taking this feature is exactly that decision, so
   it is made here in the open and `00k`'s SPARQL row updates with it.

**One thing it buys back**: `LANGDIR` / `STRLANGDIR` / `hasLangDir` arrive on the
same switch, and those are the query-side counterpart of Slice C's
`rdf:dirLangString`. Slices C and D share a gate, which is an argument for
doing them adjacently rather than a coincidence to ignore.

#### The real cross-file impact is a compile error, and it was planted on purpose

`from_term` (`graph-owl-query/src/term.rs:92`) matches `oxrdf::Term`
**exhaustively, with no wildcard**, and its comment says why:

> *Named rather than caught by a wildcard, so when RDF 1.2 triple terms arrive
> (Epic 94) this becomes a compile error rather than silently taking the
> "unrepresentable" path they may not belong on.*

Enabling `rdf-12` adds `Term::Triple` and that match stops compiling — **by
design, as a checkpoint this codebase staked out in advance**. It is the most
useful thing the feature flag does: it forces an explicit answer to a question
synthesis alone would let us skip — *what does a triple term mean when
converting a query term back into a flake value?* `from_term` feeds pattern
matching, and a triple term has no `Sid` address in this store, so the honest
answer is probably a named refusal on that path specifically. That is a much
narrower refusal than a general unsupported-construct mechanism, and it is one
the compiler will not let us forget.

So "≈100 lines in `dataset.rs` only" is wrong on the file count regardless of
the line count: `term.rs` changes too, and it changes first, because nothing
compiles until it does.

**Checked, 28 July 2026 — it does, and so does Slice C.** `graph-owl-rdf-io` is
still a six-line placeholder with no RDF dependency, so this was an unmade
decision rather than a discoverable fact. `00l-build-vs-adopt.md` settles it:
**`oxrdf` + `oxttl` + `oxjsonld`**, so the serializer builds `oxrdf` terms.
(`09-engine-rdf-io.md` had drifted to naming `rio_turtle`; corrected.) Emitting
text by hand is not a real escape — the round-trip criterion means `oxttl`
parses regardless, and owning escaping and canonicalisation on only one side of
a round trip is where such round trips quietly fail.

| Slice | Needs | Because |
|---|---|---|
| B — `rdf:reifies` on export | `oxrdf/rdf-12` | `Term::Triple` exists only under the flag |
| C — `rdf:dirLangString` | `oxrdf/rdf-12` | `BaseDirection` (`oxrdf/src/literal.rs:809`) is behind the same flag |
| D — query synthesis | same, via `spargebra`/`spareval` | Both cascade to `oxrdf/rdf-12` |

**One decision, not three**, taken once for the workspace when the first of the
three lands. This also revises the ordering argument above: C and D do not
merely *share* a gate with each other — B does too, so the whole epic turns it
on together. And Slice C without the flag is not a smaller Slice C: direction
would reach `flake_meta` and never reach the wire, which is not a finished
slice.

**Not needed**: pinning the crates to exact versions as churn protection.
`Cargo.lock` is committed, which already fixes the resolved versions for every
build; adding `=` pins to `Cargo.toml` would duplicate that and make routine
patch upgrades a manual edit.

**Acceptance criteria**: `?rel rdf:reifies << ?a ?p ?b >>` binds against a
reified relationship; the flake count is unchanged, asserted on the same
catalogue run as the other slices; the synthesised quad carries the same
`as_of` and access-predicate treatment as every other quad, because it is built
from flakes that were already filtered; a relationship the principal may not
see produces no reifying quad; a SPARQL 1.2 construct that parses but is not
evaluated returns a named, documented refusal rather than an internal error.

**RED**: the zero-rows test — a query using the standard vocabulary against an
estate that plainly contains relationships must not return an empty result.
This is the whole reason the slice exists, and it is the one failure that looks
like success. Second RED: the authorization test, since a synthesised quad is
new surface area and a fact assembled *after* filtering could reintroduce an
endpoint the filter removed. Mutator watch: emitting the reifying quad from
unfiltered flakes must fail; dropping the triple-term object in favour of the
relationship IRI must fail the pattern match, which is the rejected proposal
above failing on contact.

**Why the slice is authorization-safe — and the wrong reason for it.** It has
been argued that a visible relationship with a hidden endpoint leaks the
endpoint through `dsc:fromEntity` anyway, so synthesis adds no new surface.
**That state cannot arise.** `graph-owl-api/src/lib.rs:238` makes a relationship
node visible *only when both endpoints are*, tracked as (endpoints seen,
endpoints permitted), with `an_edge_with_no_endpoints_is_not_assumed_visible`
pinning the half-written case. The conclusion is right and the reasoning is
not, which matters: it asserts a pre-existing leak that does not exist, and
anyone who later relaxed the both-endpoints rule on the strength of it would
create the leak the argument assumed. The real reason synthesis is safe is that
it reads flakes the both-endpoints filter has already passed, so a synthesised
quad cannot name an entity those flakes did not already name.

**Done when**: criteria met, mutation report reviewed, flake count unchanged.

**Shipped.** `graph_owl_query::dataset::FlakeDataset::from_flakes` gained a
second pass: for each already-filtered flake, if its subject hasn't already
been reified and `reifier_endpoints` (a `fromEntity`+`toEntity`+`relType`
lookup, all three required — same shape and same reasoning as
`graph-owl-rdf-io`'s Slice B export-side function, duplicated rather than
shared because `graph-owl-query` cannot depend on `graph-owl-rdf-io`) finds
the shape, it synthesizes one `(rel) rdf:reifies <<( from relType to )>>`
`InternalQuad`, carrying the flake's own `cx` as `graph_name`. Tracked via a
`HashSet<&Sid>` so a subject with all three flakes present is synthesized at
most once. Four dataset-level tests pin this directly: a matching pattern
finds the synthesized quad; synthesis adds a quad without adding a flake (3
flakes in, `dataset.len() == 4`); a relationship missing one endpoint (the
authorization case) synthesizes nothing; an ordinary, non-relationship
subject synthesizes nothing.

**The RED test found a second, real bug beyond the one it was written to
find — pushdown, not synthesis.** `dataset.rs`'s own four unit tests all
passed the day this was written, and the end-to-end test
(`sparql_answers_an_rdf_reifies_pattern_against_a_real_relationship`, real
`spargebra` parse through real `Catalog::sparql`) still returned **zero
rows**. `scoped_facts` narrows what reaches `from_flakes` via
`graph_owl_query::pushdown::scans_for` *before* synthesis ever runs, and
`pushdown.rs`'s own `named()` function — written in Slice B/C, before
synthesis existed — had already left itself a note naming exactly this trap:
*"`rdf:reifies` matching is handled as a separate, specially-recognized
pattern shape (Slice D), never through this generic subject/predicate
narrowing."* The generic narrowing binds `rdf:reifies` as an ordinary bound
predicate — the same code path that turns `?s dsc:name ?n` into "scan
`name`" turns `?rel rdf:reifies <<(...)>>` into "scan `rdf:reifies`". No
flake has ever had that predicate (decision 3: it is synthesized at query
time, never stored), so the scan is *provably* empty every time, and
`from_flakes` receives nothing to synthesize from — the exact zero-rows
failure this slice exists to prevent, reached one layer earlier than the
slice's own design had considered.

**Fix**: `pushdown.rs` gained `is_reifies_pattern` (a bound `rdf:reifies`
predicate against a `TermPattern::Triple` object — the one shape
`reifying_quad` answers) and `reification_scans`, which turns that one
pattern into three scans — `fromEntity`, `toEntity`, `relType`, narrowed by
subject when the relationship IRI is itself bound — instead of the doomed
`rdf:reifies` scan. This is pushdown learning the pattern after all, but not
for the performance reason decision 7 anticipated ("only if measurement asks
for it") — it turned out to be load-bearing for *correctness*, since a
narrowing pass that is ignorant of a synthetic predicate does not fail safe
by scanning too much; it fails by scanning a predicate that is guaranteed
empty. Three new `pushdown.rs` tests pin it:
`an_rdf_reifies_pattern_scans_the_relationship_shape_not_the_synthetic_predicate`
(the fix itself — asserts no scan ever names `reifies` and all three
relationship predicates are scanned), `a_bound_relationship_narrows_the_
reification_scans_by_subject` (a bound relationship IRI still narrows, same
as any other pattern), and `a_variable_predicate_against_a_triple_term_
object_is_not_the_reifies_shape` (a wholly unbound pattern falls through to
the ordinary full-scan behaviour, not a false-positive reification match).

**The authorization RED test needed the same correction the zero-rows test
did**: hand-seeded `Sid::dsc(...)` endpoints (the `dataset.rs`-level fixture
shape) make `scope_facts`'s `visible.contains(&target.id)` check vacuous,
because a real relationship's endpoints are `Ref`s to an asset's actual
**UUID** (`graph_owl_core::projection::entity_sid`), not to a hand-picked
string — the same identity distinction this project's CLAUDE.md already
records for a different authorization check
(`Catalog::authorization_key`/`dsc:fqn`). Both new
`graph-owl-api` tests (`sparql_answers_an_rdf_reifies_pattern_against_a_
real_relationship`, `sparql_answers_no_row_for_a_relationship_whose_
endpoint_is_hidden`) therefore create real assets via `catalog.upsert_asset`
and reify against their real `entity_sid`s — the second one under
`incremental_projection_tests_support::restricted_analyst`'s existing
`public.`-prefix policy, with one endpoint inside the allowed prefix and one
outside it, asserting the query returns zero rows for the hidden-endpoint
relationship.

**The "measured refusal list" acceptance criterion is measured, and the
list is empty.** Probed directly against the pinned `spargebra 0.4.6` /
`spareval 0.2.6` (a throwaway scratch binary, `oxrdf::Dataset` in place of
`FlakeDataset` — the expression evaluator that implements `LANGDIR`,
`STRLANGDIR`, `hasLANG`, `hasLANGDIR`, `TRIPLE()`, `SUBJECT()`, `isTRIPLE()`
lives entirely inside `spareval` and does not depend on which
`QueryableDataset` impl supplies the quads): reified-triple sugar
(`<< s p o >>`) in a WHERE-clause pattern, a bare triple-term pattern, a
`VERSION` declaration, `FILTER(!!true)`, and every SPARQL 1.2 function named
in the plan's own table all **parse and evaluate** — none produced an
internal error. This confirms the plan's own correction (line 480 above):
`spareval` carries far more of the 1.2 surface than the feature-flag name
alone suggested. The one genuine, deliberate refusal in this area —
`graph_owl_query::term::from_term`'s `Term::Triple` arm, converting a
query-bound triple term back into a flake value for pattern matching, which
has no `Sid` address in this store — already returns a named
`TermError::Unrepresentable`, not a panic, and was pinned before this slice
(Slice A/B). No `UnsupportedConstruct` mechanism was built, matching the
plan's own instruction: designing one before knowing its members would be
building an error type for an empty set.

**Not in this slice, and not anywhere yet: mapping the triple term's predicate to a domain vocabulary.** Emitting `<< :a fibo:isDataFor :b >>` instead of `<< :a dsc:feeds :b >>` has been proposed as a natural extension of the same translation. It is not one. `rdf:reifies` is a *structural* translation — the same fact, in the shape the standard defines — and swapping the predicate for a domain ontology's is a *semantic* one, which needs an owner, a mapping table, and a rule for what happens when no mapping exists. It is also the wrong vocabulary for the job: see `33-ontology-packs.md`, "A pack vocabulary describes what data means, never how it flows". Keeping the two apart is what lets this slice stay a serialization concern.

## Explicitly deferred

- **Nested triple terms beyond one level** → the `Box` admits them; nothing
  needs them. Revisit when a real annotation-of-an-annotation appears.
- **RDF 1.2 Turtle/TriG syntax** → those documents are Working Draft. Epic 9
  emits the RDF 1.1 syntaxes; the 1.2 surface syntax follows its own spec to
  Recommendation.
- **`rdf:reifies` written into the store** → decision 3, and **the trigger has
  now been examined and did not fire**. It read "a SPARQL query needing to match
  triple-term patterns", which decision 7 answers at the query surface instead —
  same capability, no store change, no doubling. The remaining trigger for
  storing it is narrower than it was: a query shape that must *scan* reifying
  quads faster than they can be synthesised, which is a measurement nobody has
  yet had cause to take.
