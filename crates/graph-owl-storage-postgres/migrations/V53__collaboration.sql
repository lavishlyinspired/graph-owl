-- Epic 35: collaboration — threads, proposals, announcements, reactions.
--
-- Every table references `assets (id) ON DELETE CASCADE` deliberately: this
-- project's delete is soft (a `deleted` flag on the row, per V4's own
-- comment), so an ordinary delete never fires this cascade at all — a
-- thread survives exactly as the plan's own acceptance criteria require
-- ("deleting an entity retains its threads for audit"). The cascade only
-- fires on a genuine hard delete, where "removes them" is also what the
-- plan asks for. One FK gives both behaviours for free.

CREATE TABLE threads (
    id          UUID PRIMARY KEY,
    about       UUID NOT NULL REFERENCES assets (id) ON DELETE CASCADE,
    -- A field or column FQN this thread is anchored to. NULL means the
    -- thread is about the entity as a whole.
    field       TEXT,
    created_by  TEXT NOT NULL REFERENCES users (id) ON DELETE RESTRICT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    resolved    BOOLEAN NOT NULL DEFAULT FALSE,
    resolved_by TEXT REFERENCES users (id) ON DELETE SET NULL,
    resolved_at TIMESTAMPTZ
);

CREATE INDEX threads_by_about ON threads (about, resolved);

CREATE TABLE posts (
    id         UUID PRIMARY KEY,
    thread_id  UUID NOT NULL REFERENCES threads (id) ON DELETE CASCADE,
    -- Always the authenticated principal — never client-supplied.
    author     TEXT NOT NULL REFERENCES users (id) ON DELETE RESTRICT,
    message    TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    edited_at  TIMESTAMPTZ,
    -- A tombstone, not a row removal — deleting must preserve thread
    -- structure rather than leave a hole a reply's thread_id points into.
    deleted    BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE INDEX posts_by_thread ON posts (thread_id, created_at);

CREATE TABLE proposals (
    id              UUID PRIMARY KEY,
    about           UUID NOT NULL REFERENCES assets (id) ON DELETE CASCADE,
    field           TEXT NOT NULL CHECK (field <> ''),
    current_value   TEXT,
    proposed_value  TEXT,
    rationale       TEXT NOT NULL CHECK (rationale <> ''),
    status          TEXT NOT NULL DEFAULT 'pending'
                    CHECK (status IN ('pending', 'accepted', 'rejected')),
    -- Decision 3: attribution on accept goes to the proposer, never the
    -- accepter — this column is what a later `updated_by` write reads.
    proposed_by     TEXT NOT NULL REFERENCES users (id) ON DELETE RESTRICT,
    decided_by      TEXT REFERENCES users (id) ON DELETE SET NULL,
    decided_at      TIMESTAMPTZ,
    -- Required on rejection — checked by the facade, not this CHECK, since
    -- the rule is conditional on status and a CHECK cannot see the
    -- previous row.
    decision_reason TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX proposals_by_about ON proposals (about, status);
CREATE INDEX proposals_by_proposer ON proposals (proposed_by);

CREATE TABLE announcements (
    id         UUID PRIMARY KEY,
    -- Visible on descendants too, if `about` is a container — Slice D's
    -- inheritance rule is evaluated at read time via the same
    -- `ancestors_of` walk ownership inheritance (Epic 11D) already uses,
    -- not stored redundantly on every descendant.
    about      UUID NOT NULL REFERENCES assets (id) ON DELETE CASCADE,
    message    TEXT NOT NULL CHECK (message <> ''),
    starts_at  TIMESTAMPTZ NOT NULL,
    ends_at    TIMESTAMPTZ NOT NULL CHECK (ends_at > starts_at),
    created_by TEXT NOT NULL REFERENCES users (id) ON DELETE RESTRICT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX announcements_by_about ON announcements (about, starts_at, ends_at);

-- Slice E: toggle semantics. The primary key itself is the idempotency —
-- reacting twice with the same kind is caught by a conflict the facade
-- reads as "already reacted" before deciding to delete rather than insert.
CREATE TABLE reactions (
    post_id    UUID NOT NULL REFERENCES posts (id) ON DELETE CASCADE,
    user_id    TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    kind       TEXT NOT NULL CHECK (kind IN ('helpful', 'agree', 'disagree', 'question')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (post_id, user_id, kind)
);
