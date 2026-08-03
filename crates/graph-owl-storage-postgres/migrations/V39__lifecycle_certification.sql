-- Lifecycle and certification — Epic 26.
--
-- **Two orthogonal axes** (decision 3). An asset can be Active-uncertified,
-- Active-certified, or Deprecated-certified — still trustworthy, and going
-- away. One column each rather than one combined state, because collapsing them
-- loses exactly the distinction somebody deciding whether to build on it needs.

-- Defaulting to `active`, not `draft`: every asset already in this catalog got
-- there from a connector or a deliberate write, and retroactively marking the
-- whole estate `draft` would make the state meaningless on the day it shipped.
-- Hand-created assets can be created as drafts explicitly.
ALTER TABLE assets
    ADD COLUMN lifecycle TEXT NOT NULL DEFAULT 'active'
        CHECK (lifecycle IN ('draft', 'active', 'deprecated', 'retired'));

-- The whole deprecation as one document, because its parts are meaningless
-- apart: a reason with no timestamp, or a successor with no reason, are states
-- nobody should be able to store. `NULL` means "not deprecated", which is also
-- what `lifecycle <> 'deprecated'` says — and the two are kept in step by the
-- one method that writes both.
ALTER TABLE assets ADD COLUMN deprecation JSONB;

-- Retired assets are excluded from search by default, so the index that serves
-- discovery should not carry them.
CREATE INDEX assets_by_lifecycle ON assets (lifecycle) WHERE lifecycle <> 'active';

CREATE TABLE certification_types (
    id                    UUID PRIMARY KEY,
    name                  TEXT NOT NULL UNIQUE CHECK (name <> ''),
    description           TEXT,

    -- **Decision 1: certification expires.** An unexpiring trust stamp becomes
    -- a lie within a year. The default is per type because a security review
    -- and a freshness check do not age at the same rate.
    default_validity_days INT NOT NULL CHECK (default_validity_days > 0),

    -- Open text, one row per kind. An enum would mean a release per
    -- organization: what counts as evidence — "a passing freshness test", "the
    -- owner confirmed in writing", "SOC2 control 4.1" — is theirs to name.
    required_evidence     TEXT[] NOT NULL DEFAULT '{}',

    version_major         INT NOT NULL DEFAULT 0,
    version_minor         INT NOT NULL DEFAULT 1,
    updated_by            TEXT NOT NULL DEFAULT 'system',
    created_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at            TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- **An empty allowlist means anyone**, deliberately: a type nobody has
-- restricted is one the organization has not decided about, and refusing every
-- issuance would make defining a type useless until somebody also configured
-- issuers. The restriction is opt-in, and a row here is the opt-in.
CREATE TABLE certification_type_issuers (
    type_id      UUID NOT NULL REFERENCES certification_types (id) ON DELETE CASCADE,
    principal_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    PRIMARY KEY (type_id, principal_id)
);

CREATE TABLE certifications (
    id         UUID PRIMARY KEY,
    -- FQN for the same reason `tag_labels` uses one: a column is addressed that
    -- way and is certifiable.
    target_fqn TEXT NOT NULL CHECK (target_fqn <> ''),
    type_id    UUID NOT NULL REFERENCES certification_types (id) ON DELETE RESTRICT,

    -- **Decision 4: issued by a principal, not a system.** Accountability
    -- requires a name. Automated certification stays possible — the issuer is
    -- then the bot principal that granted it, which is still a name.
    issuer     TEXT NOT NULL REFERENCES users (id) ON DELETE RESTRICT,
    criteria   TEXT,

    issued_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- **Required, with no default at this level.** The facade fills it from the
    -- type's validity when a caller omits it; a nullable column here would let
    -- a bug write an unexpiring certification, which is the one thing decision
    -- 1 refuses.
    expires_at TIMESTAMPTZ NOT NULL,

    -- One live certification per (target, type). A renewal supersedes rather
    -- than accumulating, so "when does my Gold expire" has one answer — but the
    -- superseded rows stay, because the history of who vouched for what and
    -- when is the point.
    superseded_by UUID REFERENCES certifications (id) ON DELETE SET NULL
);

CREATE INDEX certifications_by_target ON certifications (target_fqn) WHERE superseded_by IS NULL;
-- The recertification queue reads this way: what expires inside the window.
CREATE INDEX certifications_by_expiry ON certifications (expires_at) WHERE superseded_by IS NULL;

CREATE TABLE certification_evidence (
    certification_id UUID NOT NULL REFERENCES certifications (id) ON DELETE CASCADE,
    kind             TEXT NOT NULL CHECK (kind <> ''),
    -- What was pointed at — a test id, a document URL, a ticket. Free text
    -- because the things an organization treats as evidence live in systems
    -- this catalog does not model.
    reference        TEXT NOT NULL,
    PRIMARY KEY (certification_id, kind, reference)
);
