-- Epic 20 x Epic 42 Slice D: drift, made HTTP-queryable. The CLI's `drift`
-- command (Epic 20 Slice E) already computes this — declared vs. live,
-- distinguishing "someone edited outside the declarations" from "the file
-- changed and was never applied" — but only against whatever machine ran
-- it. This table is where a pushed report becomes something the console can
-- list and act on.
--
-- One row per (asset, field): the partial unique index below is what makes
-- pushing idempotent the same way `resolution_queue` (V23) is — a second
-- push of the same still-pending drift does not duplicate it, but a drift
-- that was applied or ignored and then reappears gets a fresh pending row,
-- because that is a new instance of the problem, not the same one still
-- open.
CREATE TABLE drift_reports (
    id              UUID PRIMARY KEY,
    asset_id        UUID NOT NULL REFERENCES assets (id) ON DELETE CASCADE,
    field           TEXT NOT NULL,
    kind            TEXT NOT NULL CHECK (kind IN ('live_edited', 'unapplied')),
    live_value      TEXT,
    declared_value  TEXT,
    status          TEXT NOT NULL DEFAULT 'pending'
                    CHECK (status IN ('pending', 'applied', 'ignored')),
    reported_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    decided_at      TIMESTAMPTZ,
    decided_by      TEXT,
    -- Set on `ignored`, matching Epic 17/21's identical rule (Epic 42
    -- decision 3): a decision with no reason teaches nothing and is
    -- unauditable later.
    reason          TEXT
);

CREATE UNIQUE INDEX drift_reports_pending_unique
    ON drift_reports (asset_id, field)
    WHERE status = 'pending';

CREATE INDEX drift_reports_status ON drift_reports (status, reported_at);
