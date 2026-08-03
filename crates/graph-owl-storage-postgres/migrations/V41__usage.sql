-- Usage and popularity — Epic 28.
--
-- **A time series with rollups, aggregated on read** (decision 1). Storing a
-- pre-computed popularity would go stale silently — the same reasoning as Epic
-- 26's certification status. Raw observations answer "what happened"; rollups
-- answer "how much", and only the rollups survive pruning.

CREATE TABLE usage_observations (
    id           UUID PRIMARY KEY,

    -- **FQN, and deliberately not a foreign key.** An observation about a table
    -- nobody has catalogued yet is still worth keeping (Slice A): the connector
    -- may simply not have run. A foreign key would discard exactly the usage
    -- that tells you something is missing from the catalog.
    asset_fqn    TEXT NOT NULL CHECK (asset_fqn <> ''),

    -- Prefixed at the boundary — `principal:alice` or `opaque:alice` — so a
    -- catalog user and an unresolved warehouse identity of the same name do not
    -- collide. They are different consumers until resolution says otherwise.
    consumer_key TEXT NOT NULL CHECK (consumer_key <> ''),
    operation    TEXT NOT NULL
                 CHECK (operation IN ('read', 'write', 'delete', 'schemaRead')),

    occurred_at  TIMESTAMPTZ NOT NULL,
    row_count    BIGINT,
    duration_ms  BIGINT,

    -- The engine's identifier for the query, **not its text**. This is what
    -- makes re-ingesting a log file idempotent.
    query_id     TEXT,

    -- **Off by default, and dropped at the boundary when off** (decision 2).
    -- Query bodies contain literals — customer identifiers, filter values — so
    -- ingesting them is a data-protection decision rather than a default. The
    -- difference between not storing data and storing-then-hiding it is the
    -- whole point: this column stays null unless a deployment opted in.
    query_text   TEXT
);

-- The dedup key. Partial, because `query_id` is optional — an engine that does
-- not supply one still produces usable observations, they just cannot be
-- deduplicated.
CREATE UNIQUE INDEX usage_observations_dedup
    ON usage_observations (asset_fqn, query_id)
    WHERE query_id IS NOT NULL;

-- Pruning reads this way: everything older than the window, oldest first.
CREATE INDEX usage_observations_by_age ON usage_observations (occurred_at);
CREATE INDEX usage_observations_by_asset ON usage_observations (asset_fqn, occurred_at DESC);

-- Daily, per (asset, consumer, operation). **These survive pruning** (decision
-- 4): per-query rows at warehouse scale are enormous, and the aggregate is what
-- every question actually asks.
CREATE TABLE usage_rollups (
    asset_fqn    TEXT NOT NULL,
    consumer_key TEXT NOT NULL,
    day          DATE NOT NULL,
    operation    TEXT NOT NULL
                 CHECK (operation IN ('read', 'write', 'delete', 'schemaRead')),
    count        BIGINT NOT NULL DEFAULT 0,
    total_rows   BIGINT,
    PRIMARY KEY (asset_fqn, consumer_key, day, operation)
);

CREATE INDEX usage_rollups_by_asset ON usage_rollups (asset_fqn, day DESC);

-- **The last access per asset, kept separately so pruning cannot erase it.**
-- Slice E's sharp criterion: pruning raw observations must not blank
-- `last_accessed`, which is the single most useful signal there is. Deriving it
-- from the raw table would do exactly that the first time the window passed.
CREATE TABLE usage_last_accessed (
    asset_fqn   TEXT PRIMARY KEY,
    occurred_at TIMESTAMPTZ NOT NULL
);
