-- Epic 16 Slice C: batch is a job, not a request.
--
-- Decision 2: a 500k-row file cannot be request/response, so the upload returns
-- a handle and this row is the answer — polled until it settles.
CREATE TABLE ingest_jobs (
    id     UUID PRIMARY KEY,
    format TEXT NOT NULL,

    -- `queued` | `running` | `succeeded` | `partial` | `failed`. The vocabulary
    -- is owned by `graph-owl-connectors`' `JobState`, which is where the
    -- distinction that matters is argued: a job that landed 400k rows and
    -- rejected 100k is `partial`, and calling it `failed` would make a client
    -- re-push 400k rows to retry 100k.
    state  TEXT NOT NULL,

    rows_read BIGINT NOT NULL DEFAULT 0,
    accepted  BIGINT NOT NULL DEFAULT 0,
    rejected  BIGINT NOT NULL DEFAULT 0,

    -- The per-row reasons, each carrying the line number in the submitted file.
    -- Bounded by the error cap: a report of 500k identical parse errors is a
    -- report nobody reads, which is what the cap exists to prevent.
    failures JSONB NOT NULL DEFAULT '[]'::jsonb,

    -- Why it stopped before the end of the file. NULL means it read to the end,
    -- so the counts describe a result rather than a prefix.
    halt_reason TEXT,

    -- **A request, not an order.** Only the worker can stop cleanly and report
    -- what landed; killing it from outside would leave counts describing a
    -- moment nobody observed.
    cancel_requested BOOLEAN NOT NULL DEFAULT FALSE,

    submitted_by TEXT NOT NULL,

    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- The last time the worker said it was alive. Without it a process that
    -- died mid-job leaves a row reading `running` forever and a client waiting
    -- for an answer that will never come — and with no scheduler in this
    -- project (Epic 15 decision 5), a heartbeat read on poll is what makes
    -- "stopped reporting" observable at all.
    heartbeat_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at  TIMESTAMPTZ
);

-- The reaper's only query: jobs still claiming to run, oldest heartbeat first.
CREATE INDEX ingest_jobs_live ON ingest_jobs (heartbeat_at) WHERE finished_at IS NULL;
