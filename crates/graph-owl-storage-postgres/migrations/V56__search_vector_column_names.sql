-- Phase 3 item 3.7 (Decision 5's "column names" tier, `08-engine-search.md`).
-- Same treatment `V46__search_vector_weight_d.sql` gave dashboards and their
-- charts: `search_vector` is a per-row GENERATED column with no way to reach
-- a child row, so a table's own columns are denormalized onto its row in the
-- weight-D slot, alongside chart names — both are "a child's name, findable
-- from the parent, ranked below description", the same tier by Decision 5's
-- own ordering. A generated column's expression cannot be altered in place,
-- so this is drop-and-re-add again, same as V46.
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
               to_tsvector(
                   'english',
                   coalesce(properties ->> 'chartNames', '')
                       || ' ' || coalesce(properties ->> 'columnNames', '')
               ),
               'D')
    ) STORED;

CREATE INDEX assets_search_vector ON assets USING GIN (search_vector);
