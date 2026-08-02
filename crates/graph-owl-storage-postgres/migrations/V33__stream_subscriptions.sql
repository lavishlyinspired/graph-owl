-- Epic 19 Slice A: durable broker subscriptions. `secret` holds raw
-- credential material (SASL password, Pulsar auth token) when the broker
-- needs one — never a hash, since some schemes need the raw value to
-- authenticate. `Storage::stream_subscription_secret` is the one read path
-- that touches this column; every other read selects an explicit column
-- list that omits it.
CREATE TABLE stream_subscriptions (
    id               UUID PRIMARY KEY,
    broker_kind      TEXT NOT NULL CHECK (broker_kind IN ('kafka_protocol', 'pulsar')),
    -- Kafka/Redpanda's bootstrap servers, or Pulsar's service URL — one
    -- column, since exactly one of the two is ever meaningful for a given
    -- broker_kind and a second nullable column would just be unused half
    -- the time.
    broker_address   TEXT NOT NULL CHECK (broker_address <> ''),
    topic            TEXT NOT NULL CHECK (topic <> ''),
    consumer_group   TEXT NOT NULL CHECK (consumer_group <> ''),
    mapping          TEXT NOT NULL,
    start_position   TEXT NOT NULL CHECK (start_position IN ('earliest', 'latest', 'timestamp', 'offset')),
    -- Only meaningful for the matching start_position; NULL otherwise.
    start_timestamp  TIMESTAMPTZ,
    start_offset     BIGINT,
    max_in_flight    INTEGER NOT NULL CHECK (max_in_flight > 0),
    poison_threshold INTEGER NOT NULL CHECK (poison_threshold > 0),
    enabled          BOOLEAN NOT NULL DEFAULT TRUE,
    secret           BYTEA,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- One registered subscription per (topic, consumer_group) pair — two
    -- rows describing the same logical subscription would leave "which one
    -- actually runs" unanswered.
    UNIQUE (topic, consumer_group)
);
