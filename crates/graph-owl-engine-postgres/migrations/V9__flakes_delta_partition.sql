-- Epic 102: split the flake table into a read-optimised main partition and a
-- write-optimised delta, unified by a view so every existing reader keeps
-- working unchanged.
--
-- **Built on an explicit override, not because the trigger fired** — see
-- `plans/102-read-write-partitions.md`'s status line. Epic 37a Slice C
-- measured flat write throughput (53,641-57,919 flakes/s, no degrading
-- trend) across 1M-10M synthetic flakes under the default `shared_buffers`.
-- The mechanism below is real regardless; whether adopting it is a net win
-- is a separate, later measurement (`102-read-write-partitions.md` decision
-- 5), not assumed here.
--
-- `ALTER TABLE ... RENAME TO` moves zero data and rebuilds zero indexes --
-- `flakes_main` keeps every row and all four index orderings the original
-- `flakes` table had, under their original index names (`idx_flakes_spot`
-- and friends), so nothing that names those indexes needs to change.
ALTER TABLE flakes RENAME TO flakes_main;

-- Same columns as `flakes_main`, deliberately unconstrained by its four
-- index orderings -- "minimal, append order only" per the plan's own
-- table. The primary key is the append-order cursor compaction reads by;
-- it is its own independent sequence, not a continuation of
-- `flakes_main`'s, because nothing in this system ever compares an `id`
-- across rows -- it exists for row identity within one table, not as a
-- global ordering key (see `FLAKE_COLUMNS` in `graph-owl-engine-postgres`,
-- which never selects it).
CREATE TABLE flakes_delta (
    id           BIGSERIAL PRIMARY KEY,

    namespace_s  INTEGER  NOT NULL CHECK (namespace_s BETWEEN 0 AND 65535),
    sid_s        TEXT     NOT NULL,
    namespace_p  INTEGER  NOT NULL CHECK (namespace_p BETWEEN 0 AND 65535),
    sid_p        TEXT     NOT NULL,

    value_type   SMALLINT NOT NULL CHECK (value_type BETWEEN 0 AND 9 OR value_type = 11),

    value_key    TEXT     NOT NULL,

    value_ref_ns INTEGER  CHECK (value_ref_ns BETWEEN 0 AND 65535),
    value_ref_id TEXT,
    value_str    TEXT,
    value_bool   BOOLEAN,
    value_int    BIGINT,
    value_float  DOUBLE PRECISION,
    value_inst   TIMESTAMPTZ,
    value_json   JSONB,
    value_bytes  BYTEA,
    value_uuid   UUID,
    value_lang   TEXT,
    value_dir    TEXT     CHECK (value_dir IN ('ltr', 'rtl')),

    cx_namespace INTEGER  CHECK (cx_namespace BETWEEN 0 AND 65535),
    cx_id        TEXT,

    t            BIGINT   NOT NULL,
    op           BOOLEAN  NOT NULL
);

-- The one index delta keeps: idempotency. Without it a retried projection
-- would duplicate every flake in delta instead of converging, exactly the
-- property `flakes_main`'s own SPOT index protects (`V1__create_flakes.sql`).
-- Everything else -- POST, PSOT, OPST -- is deliberately absent; delta is
-- meant to stay small and recently-written, where a sequential scan over it
-- costs little, and paying to maintain four indexes on every insert here
-- would defeat the entire point of the split.
CREATE UNIQUE INDEX idx_flakes_delta_spot ON flakes_delta (
    namespace_s, sid_s, namespace_p, sid_p,
    value_type, value_key,
    COALESCE(cx_namespace, -1), COALESCE(cx_id, ''),
    t DESC, op
);

-- The union every existing reader queries transparently. `current_state_query`
-- and `push_live_flakes` both already resolve "newest row per fact" via
-- `DISTINCT ON ... ORDER BY t DESC` over `FROM flakes` -- pointing that at a
-- view spanning both partitions makes current-state resolution span them too
-- (`102-read-write-partitions.md` decision 2) with no change to either query
-- builder. A plain `UNION ALL` view is not itself insertable, which is
-- deliberate: `write()` targets `flakes_delta` explicitly, so there is
-- exactly one place a row is written, matching `graph-owl-engine-postgres`'s
-- own stated invariant.
--
-- **Named columns, not `SELECT *`** — found the hard way, not designed
-- around in advance: `flakes_main`'s physical column order has
-- `value_lang`/`value_dir` appended at the *end* (from `V8`'s
-- `ALTER TABLE ... ADD COLUMN`, which always appends), while
-- `flakes_delta`'s fresh `CREATE TABLE` above places them in their logical
-- position instead. `UNION ALL SELECT *` matches columns positionally, not
-- by name, so the two tables' differing physical orders paired `cx_namespace`
-- (integer) against `value_lang` (text) and failed outright: "UNION types
-- integer and text cannot be matched". An explicit list matches by name
-- regardless of either table's physical column order, and stays correct the
-- next time a migration appends a column to one table via `ALTER TABLE` --
-- which will not, by construction, reorder anything this view depends on.
CREATE VIEW flakes AS
    SELECT id, namespace_s, sid_s, namespace_p, sid_p, value_type, value_key,
           value_ref_ns, value_ref_id, value_str, value_bool, value_int,
           value_float, value_inst, value_json, value_bytes, value_uuid,
           value_lang, value_dir, cx_namespace, cx_id, t, op
    FROM flakes_main
    UNION ALL
    SELECT id, namespace_s, sid_s, namespace_p, sid_p, value_type, value_key,
           value_ref_ns, value_ref_id, value_str, value_bool, value_int,
           value_float, value_inst, value_json, value_bytes, value_uuid,
           value_lang, value_dir, cx_namespace, cx_id, t, op
    FROM flakes_delta;
