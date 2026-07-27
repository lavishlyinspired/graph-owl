-- The flake table and its four index orderings.
--
-- Owned by graph-owl-engine-postgres, migrated independently of the storage
-- adapter (which owns the entity tables that are the source of truth). Both
-- run against the same database, so this runner uses its own history table --
-- see `refinery_schema_history_engine` in the adapter.

-- Namespace codes are u16 in Rust (0..=65535). Postgres SMALLINT is signed
-- i16, topping out at 32767, so a runtime-allocated code above that would
-- overflow silently. INTEGER with an explicit range check is the honest
-- mapping: the bounds are u16's own, and a write outside them fails loudly
-- rather than storing a number no u16 can represent. The extra two bytes are
-- noise next to the TEXT sid columns that dominate every one of these indexes.
CREATE TABLE flakes (
    id           BIGSERIAL PRIMARY KEY,

    namespace_s  INTEGER  NOT NULL CHECK (namespace_s BETWEEN 0 AND 65535),
    sid_s        TEXT     NOT NULL,
    namespace_p  INTEGER  NOT NULL CHECK (namespace_p BETWEEN 0 AND 65535),
    sid_p        TEXT     NOT NULL,

    -- Pinned discriminants; see graph_owl_core::flake::value_type.
    -- 0=ref 1=str 2=bool 3=int 4=float 5=instant 6=json 7=bytes 8=uuid 9=duration
    value_type   SMALLINT NOT NULL CHECK (value_type BETWEEN 0 AND 9),

    -- A deterministic text encoding of the object, computed in Rust.
    --
    -- Two jobs. It gives the identity index something comparable for every
    -- value type without a per-type unique index, and it lets the POST
    -- ordering serve object lookups for *any* literal rather than only for
    -- strings -- indexing value_str alone would leave an integer- or
    -- instant-valued object lookup on a sequential scan.
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

    -- NULL is the default graph, holding core catalog facts. Deliberately not
    -- namespace 0: absence and "uninitialized" are different states, and code
    -- 0 is reserved to make the second detectable.
    cx_namespace INTEGER  CHECK (cx_namespace BETWEEN 0 AND 65535),
    cx_id        TEXT,

    t            BIGINT   NOT NULL,

    -- false is a retraction, not a delete. The asserting row stays.
    op           BOOLEAN  NOT NULL
);

-- SPOT, which also enforces idempotency.
--
-- These began as two indexes and were merged, because the identity constraint
-- necessarily leads with subject-then-predicate and a separate SPOT was
-- therefore a strict prefix of it -- redundant for lookups, and paid for on
-- every single write. The planner confirmed it: given both, it never chose the
-- narrower one.
--
-- Re-asserting an identical fact at the same t is a no-op, so a retried
-- projection converges instead of duplicating.
--
-- COALESCE rather than raw columns because Postgres treats NULLs as distinct
-- in a unique index, which would make every default-graph row unique from
-- every other and defeat the constraint entirely. -1 and '' are outside the
-- range the CHECK admits, so neither can collide with a real graph.
CREATE UNIQUE INDEX idx_flakes_spot ON flakes (
    namespace_s, sid_s, namespace_p, sid_p,
    value_type, value_key,
    COALESCE(cx_namespace, -1), COALESCE(cx_id, ''),
    t DESC, op
);

-- t DESC on every ordering so "current state" -- the newest flake per fact --
-- is a leading-edge read rather than a sort.
CREATE INDEX idx_flakes_psot ON flakes (namespace_p, sid_p, namespace_s, sid_s, t DESC);
CREATE INDEX idx_flakes_post ON flakes (namespace_p, sid_p, value_type, value_key, namespace_s, sid_s, t DESC);

-- OPST is partial: reverse traversal only means anything for references, and
-- indexing literal objects here would cost a fourth full index serving no
-- query that could use it.
CREATE INDEX idx_flakes_opst ON flakes (value_ref_ns, value_ref_id, namespace_p, sid_p, namespace_s, sid_s, t DESC)
    WHERE value_type = 0;

-- The transaction clock. One row, enforced by construction: the primary key
-- admits one value and the CHECK admits only TRUE, so a second row is
-- impossible rather than merely discouraged.
CREATE TABLE graph_clock (
    only_row BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (only_row),
    t        BIGINT NOT NULL
);

-- Starts at 0, so the first reserved t is 1 and t=0 means "before anything
-- happened" -- which makes an as-of query below the first transaction return
-- empty rather than returning the first state.
INSERT INTO graph_clock (only_row, t) VALUES (TRUE, 0);
