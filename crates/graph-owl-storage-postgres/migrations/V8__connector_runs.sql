-- Epic 15: what a run did, kept after it finished.
--
-- A triggered run previously left no record: the report went back in the HTTP
-- response and nowhere else, so "did last night's sync work" was unanswerable
-- the moment the caller closed the connection. `15-connectors.md` lists run
-- history as a governance concern for exactly that reason — a catalog whose
-- freshness cannot be evidenced is a catalog nobody can trust a decision to.
CREATE TABLE connector_runs (
    id           UUID PRIMARY KEY,
    connector    TEXT NOT NULL,
    service_name TEXT NOT NULL,

    started_at   TIMESTAMPTZ NOT NULL,
    -- Nullable, and that is the point: a row with no `finished_at` is a run
    -- that started and never reported. A crashed run must be distinguishable
    -- from one that succeeded quickly, and a schema that only records
    -- completions cannot express the difference — the crash simply leaves no
    -- trace, which is the failure mode this table exists to remove.
    finished_at  TIMESTAMPTZ,

    created      INT NOT NULL DEFAULT 0,
    -- Decision 7's fingerprint skip. Reported separately because a run that
    -- wrote nothing because nothing changed and one that wrote nothing because
    -- it was broken are otherwise identical rows.
    skipped      INT NOT NULL DEFAULT 0,
    failed       INT NOT NULL DEFAULT 0,
    deleted      INT NOT NULL DEFAULT 0,

    -- The per-record reasons, not just the count. A run that reports only a
    -- number tells an operator something is wrong and nothing about what.
    failures     JSONB NOT NULL DEFAULT '[]'::jsonb,
    -- Set when deletion detection refused, carrying why. A refusal is a
    -- successful run that deliberately did nothing, and reading it as a failure
    -- sends someone looking for a fault that is not there.
    refusal      TEXT,

    triggered_by TEXT NOT NULL
);

-- The only query this table serves: the recent runs for a service, newest
-- first. History is read as a timeline, never searched.
CREATE INDEX connector_runs_recent ON connector_runs (service_name, started_at DESC);
