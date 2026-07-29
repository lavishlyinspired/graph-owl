-- Epic 41: who is fixing this violation.
--
-- **Keyed on the finding's identity, like a waiver, and for the same reason.**
-- `validation_results` is replaced wholesale every pass and every row gets a
-- fresh UUID, so an assignment pointing at a row id would survive until the
-- next run and then point at nothing — reading as the assignment having been
-- dropped rather than as a design error.
--
-- Separate from `validation_waivers` rather than a column on it, because the
-- two are different statements with different lifecycles: "somebody is fixing
-- this" and "somebody accepted this" can both be true, either can be true
-- alone, and only one of them expires.
CREATE TABLE validation_assignments (
    id          UUID PRIMARY KEY,

    shape           TEXT NOT NULL,
    focus_node      TEXT NOT NULL,
    path            TEXT,
    constraint_kind TEXT NOT NULL,

    -- A `users.id`. **Not** free text: an assignment to a name nobody can
    -- resolve is a queue row that looks worked and is not, and "what is on my
    -- plate" stops being answerable the first time somebody types a nickname.
    assignee    TEXT NOT NULL REFERENCES users(id),
    assigned_by TEXT NOT NULL,
    assigned_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- One assignment per finding: two owners is no owner.
CREATE UNIQUE INDEX validation_assignments_identity
    ON validation_assignments (shape, focus_node, COALESCE(path, ''), constraint_kind);

-- "What is on my plate" — the query this table exists to answer.
CREATE INDEX validation_assignments_by_assignee ON validation_assignments (assignee);
