-- Identity and policy. Epic 11 (users), Epic 12 (auth), Epic 13 (authz).
CREATE TABLE users (
    id           TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    email        TEXT,
    is_admin     BOOLEAN NOT NULL DEFAULT FALSE,
    -- Connectors and agents authenticate as themselves, so `updated_by` names
    -- a connector rather than `system` (15-connectors.md decision 6).
    is_bot       BOOLEAN NOT NULL DEFAULT FALSE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE roles (
    name TEXT PRIMARY KEY
);

CREATE TABLE user_roles (
    user_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    role    TEXT NOT NULL REFERENCES roles (name) ON DELETE CASCADE,
    PRIMARY KEY (user_id, role)
);

-- Policies are stored as their JSON form so the pure evaluator in
-- graph-owl-authz stays the single definition of what a policy *is*.
CREATE TABLE policies (
    name  TEXT PRIMARY KEY,
    rules JSONB NOT NULL
);

CREATE TABLE role_policies (
    role   TEXT NOT NULL REFERENCES roles (name) ON DELETE CASCADE,
    policy TEXT NOT NULL REFERENCES policies (name) ON DELETE CASCADE,
    PRIMARY KEY (role, policy)
);

-- Ownership (Epic 11). An asset with no owner is a governance gap, which is
-- why this is a nullable column rather than a required one: the gap has to be
-- visible, not prevented by a constraint nobody can satisfy on import.
ALTER TABLE assets ADD COLUMN owner_id TEXT REFERENCES users (id) ON DELETE SET NULL;
CREATE INDEX assets_owner ON assets (owner_id) WHERE owner_id IS NOT NULL;
