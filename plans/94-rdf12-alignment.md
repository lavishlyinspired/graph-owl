# Plan: RDF 1.2 Alignment (Epic 94)

**Status**: Not started
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
:rel_abc123 rdf:reifies << :table_customers dsc:feeds :table_orders >> ;
            dsc:confidence 0.95 .
```

**The store rows are identical in both cases.** `rel_abc123` was already a
reifier — an identity standing for a proposition, with confidence attached to
the identity rather than to either endpoint. What Slice B adds is a serializer
that recognises the shape and names it. That is why the acceptance criteria
below include an unchanged flake count: if the number moves, this epic has
quietly become a model epic and decision 3 has been broken.

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

- [ ] `FlakeValue::TripleTerm` at discriminant 10, pinning test extended.
- [ ] A relationship serializes to `rdf:reifies` + a triple term, and parses back.
- [ ] A triple term in subject position is refused with an error naming why.
- [ ] A language-tagged literal round-trips with its tag **and** direction.
- [ ] An `rtl` literal keeps its direction through serialization — asserted with
      real Arabic or Hebrew text, not a placeholder.
- [ ] Nothing in the store changes by default: a catalogue run produces the same
      flake count as before this epic.

## Slices

### Slice A: The value variant

**RED**: the pinning test gains discriminant 10 and still passes for 0–9 — the
point is that appending changed nothing. A subject-position triple term is
refused.
**Done when**: criteria met, mutation report reviewed.

### Slice B: `rdf:reifies` on export

**RED**: a reified relationship serializes to a reifier and a triple term, and
a round trip reconstructs the same edge. Mutator watch: emitting the endpoints
without `rdf:reifies` must fail the round trip, because the result is then a
node with properties rather than a statement about a proposition.
**Done when**: criteria met, store flake count unchanged.

### Slice C: `rdf:dirLangString`

**RED**: an `rtl` literal survives storage and serialization with its direction
intact. The negative case matters as much: a plain string must not acquire a
direction, or every literal in the catalog gains a meaningless `ltr`.
**Done when**: criteria met, mutation report reviewed.

### Slice D: `rdf:reifies` at the query surface

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

**Check before starting**: whether Slice B's serializer needs `rdf-12` too. If
it emits Turtle as text it does not; if it builds `oxrdf` terms it does, and
then export and query share the gate rather than merely neighbouring it.

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
