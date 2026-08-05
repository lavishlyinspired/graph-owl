-- Epic 34 Slice C: lineage's missing middle. "Table A feeds table B" says
-- *that* data moved; the pipeline that moved it says *how* — the job, its
-- schedule, its run history — the same thing `query` already carries for a
-- single SQL transformation, for a multi-step job `query` cannot express as
-- one string.
--
-- No `ON DELETE` action needed: assets are never actually deleted by SQL
-- `DELETE`, only soft-deleted (`assets.deleted = true`), so the referenced
-- row always still exists and this column is never left dangling by a
-- deletion this schema performs. "A pipeline referenced by lineage resists
-- deletion" is an application-level guard (`Catalog::soft_delete_asset`),
-- not a constraint this column could express — a check *before* soft-delete
-- is not something a foreign key can do.
ALTER TABLE lineage_edges ADD COLUMN pipeline_asset_id UUID REFERENCES assets (id);

-- "Which pipelines does this table depend on becoming deletable" is the
-- query the guard runs; a partial index costs nothing on the overwhelming
-- majority of edges that carry no pipeline at all.
CREATE INDEX lineage_edges_by_pipeline ON lineage_edges (pipeline_asset_id)
    WHERE pipeline_asset_id IS NOT NULL;
