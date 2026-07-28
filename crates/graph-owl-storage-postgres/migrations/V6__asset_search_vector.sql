-- Epic 8 Slice A: search over what a person actually types.
--
-- A GENERATED column rather than a detached index. The vector is written in the
-- same transaction as the row it describes, so it cannot drift from its source
-- and there is nothing to reindex, retry or reconcile. The event-driven indexer
-- Slice B was scoped for exists to keep a *detached* index in step; a column has
-- no detachment to repair. See `08-engine-search.md` decision 10.
--
-- `translate(..., '._-', '   ')` before tokenising. Postgres's parser treats
-- `upi_transactions` as one token, so a search for `transactions` would not
-- find it and a search for the whole identifier would only match on an exact
-- string. Splitting on the three separators identifiers actually use makes the
-- index hold the words a person would type. The FQN gets the same treatment,
-- which is what makes `hdfc-core retail` a usable query.
--
-- Weights A/B/C, in that order:
--   A  name         — what someone is looking for
--   B  FQN          — how they disambiguate two things with the same name
--   C  description  — where a match is plausible but weak
-- D is deliberately left unused so a later field (Epic 24's glossary terms) can
-- rank below description without redefining the three that already exist.
--
-- `to_tsvector` with an explicit configuration is IMMUTABLE, which the
-- generated-column expression requires; the single-argument form is not,
-- because it depends on `default_text_search_config`.
ALTER TABLE assets
    ADD COLUMN search_vector tsvector GENERATED ALWAYS AS (
        setweight(to_tsvector('english', translate(coalesce(name, ''), '._-', '   ')), 'A')
        || setweight(
               to_tsvector('english', translate(coalesce(fully_qualified_name, ''), '._-', '   ')),
               'B')
        || setweight(to_tsvector('english', coalesce(description, '')), 'C')
    ) STORED;

-- GIN rather than GiST: this table is read far more often than written, and GIN
-- answers a lexeme lookup without the recheck GiST's lossy signatures force.
-- The write cost GIN trades for that is paid on catalogue runs, which are
-- batched and off the request path.
CREATE INDEX assets_search_vector ON assets USING GIN (search_vector);
