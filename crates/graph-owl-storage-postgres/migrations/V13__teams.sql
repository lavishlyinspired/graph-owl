-- Epic 11: teams, and ownership by one.
--
-- The epic is named "Users, **Teams** & Ownership" and read Shipped for weeks
-- with no team anywhere in the schema. Two lines were correctly blocked on it
-- and looked like their own problem — the console's owner display, and
-- Epic 41's violation assignment, which assigns to a `users.id` because a team
-- was not addressable.
CREATE TABLE teams (
    id           TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    -- Nullable, so an unowned team is expressible. A team with no stated
    -- purpose is a real state — it usually means somebody created it in a
    -- hurry — and forcing a description would get it filled with "team".
    description  TEXT,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Membership. A person may be in several teams: ownership follows the
-- organisation, and organisations are not trees.
CREATE TABLE team_members (
    team_id TEXT NOT NULL REFERENCES teams (id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    PRIMARY KEY (team_id, user_id)
);

CREATE INDEX team_members_by_user ON team_members (user_id);

-- **Ownership by team, beside ownership by person — not instead of.**
--
-- A second column rather than a polymorphic `(owner_kind, owner_id)` pair:
-- the two reference different tables, and a polymorphic key cannot be a
-- foreign key. Losing referential integrity to save a column would let a
-- deleted team stay named as an owner, which is exactly the silent-rot this
-- schema refuses everywhere else.
--
-- Both may be set. "The platform team owns this, and Priya is the person to
-- ask" is the normal case in a real estate, and a model that forced a choice
-- would push one of the two into a description field where nothing can query
-- it.
ALTER TABLE assets ADD COLUMN owner_team_id TEXT REFERENCES teams (id) ON DELETE SET NULL;
CREATE INDEX assets_owner_team ON assets (owner_team_id) WHERE owner_team_id IS NOT NULL;
