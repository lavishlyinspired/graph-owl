-- Epic 32: agent capabilities, proposals, and the activity log.
--
-- Three tables, and the third is the one that makes the other two auditable.

-- What one agent may do.
--
-- `agent_id` references `users` because decision 1 requires a *distinct bot
-- principal per agent*, never a shared service account: attribution is the
-- entire basis of trust here, and two agents behind one identity means a bad
-- conclusion cannot be traced to the thing that drew it.
CREATE TABLE agent_grants (
    id                UUID PRIMARY KEY,
    agent_id          TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    -- The capability names, as the closed enum in `graph-owl-core::agent`
    -- spells them. Stored as text rather than a Postgres enum so that
    -- *removing* one does not need a migration that rewrites a type still
    -- referenced by history — and removal is the direction this set is
    -- expected to move.
    capabilities      TEXT[] NOT NULL DEFAULT '{}',
    -- NULL is the whole estate. An empty string would be a scope admitting
    -- nothing, which is a different statement, so the column is nullable
    -- rather than defaulted.
    scope_fqn_prefix  TEXT,
    max_writes        INTEGER NOT NULL DEFAULT 60,
    window_seconds    INTEGER NOT NULL DEFAULT 3600,
    expires_at        TIMESTAMPTZ,
    granted_by        TEXT NOT NULL REFERENCES users (id),
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),

    -- A limit of zero would refuse every write while looking like a grant, and
    -- a window of zero would make the rate check divide an empty interval.
    CONSTRAINT agent_grants_positive_limit CHECK (max_writes > 0 AND window_seconds > 0),
    -- One grant per agent. Two rows would make "what may this agent do" a
    -- union nobody wrote down, and a revocation would have to find every row
    -- to be a revocation at all.
    CONSTRAINT agent_grants_one_per_agent UNIQUE (agent_id)
);

CREATE INDEX agent_grants_agent_idx ON agent_grants (agent_id);

-- An agent's suggestion, awaiting a human.
CREATE TABLE agent_proposals (
    id             UUID PRIMARY KEY,
    agent_id       TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    target_fqn     TEXT NOT NULL,
    capability     TEXT NOT NULL,
    change         JSONB NOT NULL,
    -- Required, not nullable: a suggestion an agent cannot justify is one a
    -- reviewer cannot evaluate, and a queue of unjustified suggestions is a
    -- queue nobody works.
    rationale      TEXT NOT NULL,
    confidence     DOUBLE PRECISION NOT NULL,
    status         TEXT NOT NULL DEFAULT 'open',
    -- What the agent reasoned against. A proposal whose target has since moved
    -- is stale and must not silently overwrite whatever happened in between.
    base_major     INTEGER NOT NULL,
    base_minor     INTEGER NOT NULL,
    decided_by     TEXT REFERENCES users (id),
    decided_at     TIMESTAMPTZ,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT agent_proposals_confidence_range CHECK (confidence >= 0 AND confidence <= 1),
    CONSTRAINT agent_proposals_rationale_present CHECK (length(trim(rationale)) > 0),
    CONSTRAINT agent_proposals_status CHECK (
        status IN ('open', 'accepted', 'rejected', 'superseded')
    ),
    -- A decided proposal names who decided it and when; an open one names
    -- neither. Half a decision is a row nobody can audit.
    CONSTRAINT agent_proposals_decision_complete CHECK (
        (status = 'open' AND decided_by IS NULL AND decided_at IS NULL)
        OR (status <> 'open' AND decided_by IS NOT NULL AND decided_at IS NOT NULL)
    )
);

CREATE INDEX agent_proposals_agent_idx ON agent_proposals (agent_id, created_at DESC);
CREATE INDEX agent_proposals_target_idx ON agent_proposals (target_fqn);
-- The reviewer's queue: open proposals, oldest first, because a suggestion
-- that has waited longest is the one most likely to go stale.
CREATE INDEX agent_proposals_open_idx ON agent_proposals (created_at)
    WHERE status = 'open';

-- Every agent write attempt, including the refused ones.
--
-- **Refusals are recorded, and that is the point of this table existing
-- separately from history.** An agent repeatedly attempting un-granted writes
-- is either misconfigured or doing something nobody intended; a log of only
-- successes shows neither. It is also what makes the rate limit survive a
-- restart: the count comes from here rather than from a counter in a process
-- that a deploy resets — which is exactly when a runaway agent would get its
-- budget back.
CREATE TABLE agent_activity (
    id          UUID PRIMARY KEY,
    agent_id    TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    capability  TEXT NOT NULL,
    target_fqn  TEXT NOT NULL,
    outcome     TEXT NOT NULL,
    -- Present on a refusal, naming which rule refused. NULL otherwise.
    refusal     TEXT,
    at          TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT agent_activity_outcome CHECK (outcome IN ('applied', 'proposed', 'refused')),
    -- A refusal without a reason is an audit row nobody can act on, and a
    -- success carrying one is a contradiction.
    CONSTRAINT agent_activity_refusal_iff_refused CHECK (
        (outcome = 'refused' AND refusal IS NOT NULL)
        OR (outcome <> 'refused' AND refusal IS NULL)
    )
);

-- The rate-limit query: this agent, this capability, since a cutoff.
CREATE INDEX agent_activity_window_idx ON agent_activity (agent_id, capability, at DESC);
-- The audit view: everything one agent did, newest first.
CREATE INDEX agent_activity_agent_idx ON agent_activity (agent_id, at DESC);
