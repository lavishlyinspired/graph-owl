-- Epic 34 Slice A: the weight-D slot `V6__asset_search_vector.sql`'s own
-- comment reserved for "a later field" is used for the first time — a
-- dashboard's own row carries its charts' denormalized names
-- (`properties->>'chartNames'`, kept in sync by `Catalog::upsert_asset`
-- whenever a chart under it changes), so a dashboard is findable by a
-- chart's name without a per-row search join `search_vector` has no way to
-- express (it is a GENERATED column, computed from that row alone).
--
-- A generated column's expression cannot be altered in place — Postgres has
-- no `ALTER COLUMN ... SET EXPRESSION`. Drop and re-add is the only path,
-- which also means dropping and rebuilding the GIN index over it.
DROP INDEX assets_search_vector;
ALTER TABLE assets DROP COLUMN search_vector;

ALTER TABLE assets
    ADD COLUMN search_vector tsvector GENERATED ALWAYS AS (
        setweight(to_tsvector('english', translate(coalesce(name, ''), '._-', '   ')), 'A')
        || setweight(
               to_tsvector('english', translate(coalesce(fully_qualified_name, ''), '._-', '   ')),
               'B')
        || setweight(to_tsvector('english', coalesce(description, '')), 'C')
        || setweight(
               to_tsvector('english', coalesce(properties ->> 'chartNames', '')),
               'D')
    ) STORED;

CREATE INDEX assets_search_vector ON assets USING GIN (search_vector);
