-- rdf:langString / rdf:dirLangString -- Epic 94 Slice C.
--
-- Reuses `value_str` for the lexical form: the discriminant already tells a
-- plain `String` row (value_type=1) apart from a `LangString` row
-- (value_type=11) on read, so a second text column would only duplicate what
-- `value_type` already says. `value_lang`/`value_dir` are the two components
-- a plain string does not carry.
--
-- Both columns arrive together, matching `flake.rs`'s own decision 4: sizing
-- for a language tag alone and adding direction later would migrate every
-- multilingual label ever written.
ALTER TABLE flakes ADD COLUMN value_lang TEXT;
ALTER TABLE flakes ADD COLUMN value_dir  TEXT CHECK (value_dir IN ('ltr', 'rtl'));

-- Widened to admit 11 (LangString) specifically, not simply raised to 11 --
-- 10 (TripleTerm) stays excluded on purpose. Epic 94 decision 3 is that a
-- triple term is synthesized at query time and never written to the store;
-- a `BETWEEN 0 AND 11` constraint would silently stop enforcing that the
-- moment this migration ran, turning a database-level safety net into a
-- gap nobody noticed removing.
ALTER TABLE flakes DROP CONSTRAINT flakes_value_type_check;
ALTER TABLE flakes ADD CONSTRAINT flakes_value_type_check
    CHECK (value_type BETWEEN 0 AND 9 OR value_type = 11);

-- The predicate registry (V3) declares each predicate's own `value_type`
-- and carries the identical constraint, checked separately by Postgres
-- from the `flakes` table above — found only by actually trying to define
-- an `rdf:langString`-valued predicate against a real database, not by
-- reading the schema. Same widening, same reason: 11 admitted, 10 still
-- excluded.
ALTER TABLE predicates DROP CONSTRAINT predicates_value_type_check;
ALTER TABLE predicates ADD CONSTRAINT predicates_value_type_check
    CHECK (value_type BETWEEN 0 AND 9 OR value_type = 11);
