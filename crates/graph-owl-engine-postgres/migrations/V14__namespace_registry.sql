-- Namespaces definable at runtime, so a domain can bring its own vocabulary
-- without a release.
--
-- The sibling of `predicates` (V3), and it exists for the same reason one
-- level up: V3 let an organisation define new *predicates*, but only ever
-- inside a namespace the binary already knew, because `Sid::from_iri` scans a
-- fixed compile-time array and `namespace_iri()` is a `match` returning
-- `&'static str`. A vocabulary in any other namespace could not become a graph
-- subject or predicate at all.
--
-- The evidence that this was a real limit rather than a theoretical one: the
-- CUI, SNOMED CT and RxNorm namespaces were added to `graph-owl-core` as Rust
-- constants for one domain's ingestion work. This table is what that domain
-- should have been able to use instead.
CREATE TABLE namespaces (
    -- The stored half of a Sid. Constrained to the runtime range because
    -- everything below it belongs to the binary: letting a deployment claim
    -- `dsc:` or `rdf:` would redefine the catalog's own vocabulary for its
    -- flakes while every other part of the system still read the shipped
    -- meaning. `namespace::RUNTIME_START` is 1024 and `NOT_FOUND` is 65535.
    code        INTEGER PRIMARY KEY CHECK (code BETWEEN 1024 AND 65534),

    -- The IRI prefix the code stands for. UNIQUE because two codes for one
    -- IRI would make resolution depend on load order — the same IRI would
    -- become one code or the other according to which row was read first.
    iri         TEXT    NOT NULL UNIQUE CHECK (iri <> ''),

    -- Who asked for it. A namespace outlives the pack that introduced it (its
    -- flakes are still readable), so this is provenance, never a foreign key
    -- that could cascade a delete into rewriting history.
    declared_by TEXT    NOT NULL,

    declared_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- **A code is never reassigned, and this is the constraint that says so.**
-- A Sid is stored as a bare (code, local) pair, so a code that changes meaning
-- silently rewrites every flake already carrying it — and time travel makes
-- that corruption permanent rather than transient. PRIMARY KEY on `code` plus
-- UNIQUE on `iri` together make both directions of the mapping immutable once
-- written; the application refuses the update, and the table refuses it again.
COMMENT ON TABLE namespaces IS
    'Runtime namespace registry. Codes 1024+ only; both code and IRI are '
    'unique, so the mapping is immutable once written — reassigning either '
    'would change the meaning of flakes already stored.';
