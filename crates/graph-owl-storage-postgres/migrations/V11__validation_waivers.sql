-- Epic 5 / Epic 41: a violation somebody has accepted, on the record.
--
-- **Keyed on the finding's identity, not its row id.** `validation_results` is
-- replaced wholesale on every pass and every row gets a fresh UUID, so a waiver
-- pointing at a row id would survive exactly until the next run and then point
-- at nothing. The identity of a finding is what it is *about*: which shape,
-- which node, which path, which constraint.
CREATE TABLE validation_waivers (
    id          UUID PRIMARY KEY,

    shape           TEXT NOT NULL,
    focus_node      TEXT NOT NULL,
    -- Null for a combinator, matching `validation_results`. Part of the key, so
    -- `COALESCE` in the unique index below rather than a nullable column that
    -- silently permits duplicates.
    path            TEXT,
    constraint_kind TEXT NOT NULL,

    -- **Required.** A waiver without a reason is a violation deleted with extra
    -- steps: the next person to read the queue cannot tell an accepted risk
    -- from a forgotten one, and cannot judge whether the acceptance still holds.
    reason      TEXT NOT NULL CHECK (length(trim(reason)) > 0),
    waived_by   TEXT NOT NULL,
    waived_at   TIMESTAMPTZ NOT NULL DEFAULT now(),

    -- **Required, and that is the design.** A permanent waiver is a rule
    -- somebody switched off without switching it off — invisible in the shape,
    -- invisible in the queue, and never reviewed again. An expiry forces the
    -- acceptance back in front of a person.
    expires_at  TIMESTAMPTZ NOT NULL
);

-- One live waiver per finding. `COALESCE` because a null path would otherwise
-- compare unequal to itself and let the same finding be waived repeatedly.
CREATE UNIQUE INDEX validation_waivers_identity
    ON validation_waivers (shape, focus_node, COALESCE(path, ''), constraint_kind);

-- The queue joins on this for every row it renders.
CREATE INDEX validation_waivers_by_node ON validation_waivers (focus_node);
