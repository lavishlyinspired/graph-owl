-- Epic 18 Slice A: registered webhook receivers. `secret` holds raw key
-- material (an HMAC shared secret, or an Ed25519 public verifying key) —
-- never a hash of it, since HMAC verification needs the raw key to recompute
-- the same MAC the sender did. `Storage::webhook_secret` is the one read
-- path that touches this column; every other read goes through
-- `webhook_endpoints_public`, which does not select it at all.
CREATE TABLE webhook_endpoints (
    id             UUID PRIMARY KEY,
    path           TEXT NOT NULL UNIQUE CHECK (path <> ''),
    source         TEXT NOT NULL CHECK (source <> ''),
    scheme         TEXT NOT NULL CHECK (scheme IN ('hmac_sha256', 'ed25519')),
    scheme_header  TEXT NOT NULL,
    -- Only meaningful for hmac_sha256; NULL for ed25519.
    scheme_prefix  TEXT,
    mapping        TEXT NOT NULL,
    event_filter   TEXT[] NOT NULL DEFAULT '{}',
    enabled        BOOLEAN NOT NULL DEFAULT TRUE,
    secret         BYTEA,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);
