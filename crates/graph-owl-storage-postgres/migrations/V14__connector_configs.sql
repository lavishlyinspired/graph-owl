-- Epic 41 Slice F: a connector somebody configured once, rather than a
-- connection string pasted into every run.
--
-- **The secret is a separate column, and nothing that reads a config selects
-- it.** `ConnectorConfig` has no field to put it in — see the port — so
-- returning it is not a mistake somebody can make in a handler. A `redacted`
-- flag on a struct that still carries the value is one `Debug` derive away from
-- a log line with a password in it.
CREATE TABLE connector_configs (
    id           UUID PRIMARY KEY,
    connector    TEXT NOT NULL,
    service_name TEXT NOT NULL,

    -- Everything a reader may see: host, port, database, which schemas to
    -- include. Free-form because each connector's shape differs and Epic 41's
    -- `SchemaForm` renders it from the connector's own JSON Schema.
    settings     JSONB NOT NULL DEFAULT '{}'::jsonb,

    -- Write-only. Nullable so "configured, no credential yet" is expressible —
    -- a connector reachable without one is a real case, and a NOT NULL column
    -- would force an empty string that reads as a secret.
    --
    -- **Not encrypted here**, and that is stated rather than implied:
    -- encryption at rest is the deployment's (`00g` — disk or tablespace
    -- encryption), because a key managed by this application would live beside
    -- the ciphertext and protect nothing. What this schema guarantees is that
    -- the value never leaves through the API.
    secret       TEXT,

    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- One configuration per service per connector: two would make "which
-- credential did last night's run use" unanswerable.
CREATE UNIQUE INDEX connector_configs_identity
    ON connector_configs (connector, service_name);
