-- Agent runs, durably.
--
-- Agent activity lived in a capped module global and evaporated on restart.
-- For an agentic product the trace of what an agent read, decided and was
-- refused is the audit trail — it deserves the same durability as the cases
-- it was produced from. The full record (spans, writes, refusals, context)
-- is stored as JSONB; the flat columns exist only for filtering and
-- ordering the list view.
CREATE TABLE agent_run (
    id         TEXT PRIMARY KEY,
    agent      TEXT NOT NULL,
    event      TEXT NOT NULL,
    status     TEXT NOT NULL,
    record     JSONB NOT NULL,
    started_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX agent_run_started_at ON agent_run (started_at DESC);
