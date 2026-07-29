-- Epic 29 Slice A: an asserted lineage edge.
--
-- A separate table rather than a row in `entity_relationships`, for one reason
-- that decides it: **uniqueness is on (from, to, relationship, source)**, and
-- `entity_relationships` is unique on the triple alone. The same pair asserted
-- by a person and by a connector are two facts that must coexist — automation
-- is often wrong about lineage a human knows, and a human is often out of date
-- about lineage automation observes. Sharing the table would make one silently
-- overwrite the other, with the winner decided by run order.
CREATE TABLE lineage_edges (
    id             UUID PRIMARY KEY,

    -- Assets, not the pre-Epic-2 `tables`. This is what the catalog actually
    -- holds, and lineage between anything else is lineage nobody can navigate
    -- to. ON DELETE CASCADE so a hard-deleted asset does not leave an edge
    -- pointing at nothing — a dangling edge renders as a node with no name.
    from_asset_id  UUID NOT NULL REFERENCES assets (id) ON DELETE CASCADE,
    to_asset_id    UUID NOT NULL REFERENCES assets (id) ON DELETE CASCADE,

    -- `feeds` or `derivedFrom`. Flow and provenance are kept apart deliberately
    -- (`00c`): lineage explainability needs both, and one column holding either
    -- makes "how did this get here" unanswerable.
    relationship   TEXT NOT NULL,

    source         TEXT NOT NULL,
    -- The SQL that produced the edge. The whole reason an edge is believable:
    -- one carrying its query can be checked, one without is an assertion a
    -- reader must take on trust.
    query          TEXT,
    description    TEXT,

    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by     TEXT NOT NULL,

    -- Self-lineage is refused here as well as in the facade. The facade returns
    -- the better error; this makes the rule true of the data even if a future
    -- write path forgets to ask.
    CONSTRAINT lineage_is_not_reflexive CHECK (from_asset_id <> to_asset_id),
    CONSTRAINT lineage_edge_identity UNIQUE (from_asset_id, to_asset_id, relationship, source)
);

-- The two walks: downstream from a node, and upstream to it. Both directions
-- are first-class — "what breaks if I change this" and "where did this number
-- come from" are the same graph read in opposite directions.
CREATE INDEX lineage_downstream ON lineage_edges (from_asset_id);
CREATE INDEX lineage_upstream ON lineage_edges (to_asset_id);
