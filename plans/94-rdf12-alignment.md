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
