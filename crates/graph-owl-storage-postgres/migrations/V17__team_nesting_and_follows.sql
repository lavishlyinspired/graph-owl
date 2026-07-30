-- Epic 11: the half of Slice B that was never built, plus Slice F.
--
-- Slice B is titled "Teams exist **and nest**" and specifies `parentOf` edges
-- with cycle detection at depths 1, 2 and 3. `V13` shipped the table with no
-- parent column, and the index recorded "Teams exist" — honest about what
-- landed, but it left Slice G's "deleting a team with child teams → 409"
-- unreachable, because there were no child teams to have.

-- **A self-referential nullable foreign key, not a join table.** A team has at
-- most one parent — that is what makes it a hierarchy rather than a graph — and a
-- join table would make two parents representable, which is the thing the cycle
-- checks then have to defend against on every read.
--
-- `ON DELETE RESTRICT`: a deleted parent must not silently orphan its children
-- into roots. Slice G's job is to make that refusal visible with a count, and
-- `SET NULL` here would take the decision away from it.
ALTER TABLE teams ADD COLUMN parent_team_id TEXT REFERENCES teams (id) ON DELETE RESTRICT;
CREATE INDEX teams_by_parent ON teams (parent_team_id) WHERE parent_team_id IS NOT NULL;

-- Self-parenting, refused by the database rather than only by the application.
-- The depth-1 cycle is the one a careless `UPDATE` creates, and it is the cheapest
-- possible check — deeper cycles need the ancestor walk and live in the adapter.
ALTER TABLE teams ADD CONSTRAINT teams_no_self_parent CHECK (id <> parent_team_id);

-- Slice F: a consumer records interest, and Epic 3's change events gain an
-- audience.
--
-- **The primary key is the idempotency.** Slice F requires a second follow to be
-- a `200` with one edge rather than a `409`: following something you already
-- follow is not an error, it is the state you asked for. `ON CONFLICT DO NOTHING`
-- against this key is what makes that true without a read-then-write race.
CREATE TABLE asset_followers (
    asset_id UUID NOT NULL REFERENCES assets (id) ON DELETE CASCADE,
    user_id  TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    followed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (asset_id, user_id)
);

-- "`GET /users/{id}/follows` paginated across entity types" reads in this
-- direction; the follower count on an entity read reads in the other, which the
-- primary key already serves.
CREATE INDEX asset_followers_by_user ON asset_followers (user_id);
