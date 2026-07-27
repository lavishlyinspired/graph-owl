-- When each transaction time happened.
--
-- `t` is a logical clock, which is what makes "the state after change N"
-- well-defined. But nobody asks a question in logical time -- they ask "what
-- did this look like on 1 January", and without this table that question has
-- no answer at all.
--
-- Deliberately a separate table rather than a column on `flakes`: the mapping
-- is one row per transaction, not one per flake, and denormalising it onto the
-- hottest table in the system would repeat the same timestamp across every
-- flake of a wide projection.
CREATE TABLE graph_transactions (
    t  BIGINT      PRIMARY KEY,
    -- Set by Postgres, not by the caller. A Rust-side timestamp would let two
    -- application servers with drifting clocks write a `t` ordering that
    -- disagrees with the wall-clock ordering, and time-travel would then
    -- return a state that never existed.
    at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Resolving an as-of timestamp is "the newest transaction at or before this
-- instant", which is a backwards scan from the leading edge.
CREATE INDEX idx_graph_transactions_at ON graph_transactions (at DESC);
