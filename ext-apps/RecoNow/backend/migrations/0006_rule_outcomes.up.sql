-- What each rule concluded on the last reconciliation, as the engine reported
-- it — passed, flagged, or not evaluated.
--
-- Reco Now inferred "this check is off" from which files had been uploaded.
-- That was a Python guess sitting beside the engine's own execution record,
-- and the two could disagree without anything noticing. graph-owl now reports
-- per-rule outcomes directly (it probes each rule's declared requirements
-- before running it), so this table stores what the engine actually said
-- rather than what the app supposed.
--
-- `unmet` lists the classes or predicates a not-evaluated rule needed and did
-- not find, which is what turns "not checked" into an actionable sentence.
CREATE TABLE rule_outcome (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    client_id   UUID NOT NULL REFERENCES client(id) ON DELETE CASCADE,
    period_id   UUID NOT NULL REFERENCES period(id) ON DELETE CASCADE,
    label       TEXT NOT NULL,
    governed_by TEXT,
    status      TEXT NOT NULL,
    found       INTEGER NOT NULL DEFAULT 0,
    unmet       JSONB NOT NULL DEFAULT '[]'::jsonb,
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- One row per rule per period: a re-run replaces what it last concluded.
    UNIQUE (client_id, period_id, label)
);
