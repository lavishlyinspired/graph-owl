-- Epic 11 Slice C: entities have owners — plural, and of two kinds.
--
-- `00c-domain-model.md`: "**Single-owner models fail immediately** — every real
-- asset has a producing team and an accountable individual."
--
-- **Two dead columns are dropped here.** `V5` added `assets.owner_id` and `V13`
-- added `assets.owner_team_id`, and *neither was ever read or written* — verified
-- across the whole tree before dropping: the only reference anywhere was a comment
-- in the MCP adapter noting that `Asset` carries no owner field. They are worse
-- than useless: two columns that look like the answer to "who owns this", hold
-- nothing, and cannot express the plural model the domain requires. Leaving them
-- would guarantee somebody writes to one and wonders why the console disagrees.
ALTER TABLE assets DROP COLUMN IF EXISTS owner_id;
ALTER TABLE assets DROP COLUMN IF EXISTS owner_team_id;

-- Ownership as a join table, with the target split across two foreign keys.
--
-- Same reasoning as `V13` gave for owner columns and `V15` for memory links: a
-- polymorphic `(kind, id)` pair cannot be a foreign key, and losing referential
-- integrity to save a column lets a deleted principal stay named as an owner —
-- which is the silent rot this schema refuses everywhere else. Here it would be
-- worse than elsewhere, because the entire value of recording an owner is that
-- somebody can be *asked*, and a dangling name cannot be asked.
CREATE TABLE asset_owners (
    asset_id UUID NOT NULL REFERENCES assets (id) ON DELETE CASCADE,

    user_id TEXT REFERENCES users (id) ON DELETE CASCADE,
    team_id TEXT REFERENCES teams (id) ON DELETE CASCADE,

    -- Exactly one. `<>` on two NULL-tests is the concise spelling of "one or the
    -- other but not both", and it also rejects the row with neither — which a
    -- pair of one-sided checks would let through.
    CONSTRAINT asset_owners_one_principal CHECK (
        (user_id IS NOT NULL) <> (team_id IS NOT NULL)
    ),

    -- **Submitted order is preserved, and that is a correctness requirement, not
    -- presentation.** Validation failures are reported by index — Slice C
    -- specifies `owners[1].id` — so a read that returned owners in a different
    -- order than they were sent would make the index name the wrong entry, and a
    -- client would "fix" the owner that was fine.
    ordinal INT NOT NULL,

    PRIMARY KEY (asset_id, ordinal)
);

-- One principal cannot own the same asset twice. Two partial unique indexes
-- rather than one composite, because the principal lives in whichever column
-- applies and NULLs are not equal in a unique index — so `(asset, NULL)` rows
-- would all count as distinct.
CREATE UNIQUE INDEX asset_owners_user_identity
    ON asset_owners (asset_id, user_id) WHERE user_id IS NOT NULL;
CREATE UNIQUE INDEX asset_owners_team_identity
    ON asset_owners (asset_id, team_id) WHERE team_id IS NOT NULL;

-- "Assets filterable by owner" (Slice E) reads in this direction, and the
-- ownership-gap report reads the absence of it.
CREATE INDEX asset_owners_by_user ON asset_owners (user_id) WHERE user_id IS NOT NULL;
CREATE INDEX asset_owners_by_team ON asset_owners (team_id) WHERE team_id IS NOT NULL;
