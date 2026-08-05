-- Epic 9 Slice D: OpenLineage import is idempotent by event id. Without a
-- place to remember which `run.runId` produced an edge, re-importing the
-- same run event would assert the same edge twice — this column is the
-- lookup a re-import checks before writing.
ALTER TABLE lineage_edges ADD COLUMN openlineage_event_id TEXT;

-- "Has this run already been imported" is the query idempotent import runs
-- on every event; a partial index costs nothing on the overwhelming
-- majority of edges that carry no OpenLineage origin at all.
CREATE INDEX lineage_edges_by_openlineage_event ON lineage_edges (openlineage_event_id)
    WHERE openlineage_event_id IS NOT NULL;
