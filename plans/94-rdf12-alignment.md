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

## Explicitly deferred

- **Nested triple terms beyond one level** → the `Box` admits them; nothing
  needs them. Revisit when a real annotation-of-an-annotation appears.
- **RDF 1.2 Turtle/TriG syntax** → those documents are Working Draft. Epic 9
  emits the RDF 1.1 syntaxes; the 1.2 surface syntax follows its own spec to
  Recommendation.
- **`rdf:reifies` written into the store** → decision 3. The trigger is a SPARQL
  query needing to match triple-term patterns.
