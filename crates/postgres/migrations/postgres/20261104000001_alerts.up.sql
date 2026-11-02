ALTER TABLE alerts ADD COLUMN project_id uuid REFERENCES projects (id) ON DELETE CASCADE;

CREATE INDEX alerts_enabled ON alerts (org_id) WHERE enabled;

CREATE TABLE alert_channels (
    id uuid PRIMARY KEY,
    org_id uuid NOT NULL REFERENCES orgs (id) ON DELETE CASCADE,
    name text NOT NULL,
    kind text NOT NULL CHECK (kind IN ('email', 'webhook')),
    target text NOT NULL,
    secret_sealed bytea,
    created_by uuid REFERENCES accounts (id) ON DELETE SET NULL,
    created_at bigint NOT NULL DEFAULT ((extract(epoch FROM clock_timestamp()) * 1000000000)::bigint),
    UNIQUE (org_id, name),
    CONSTRAINT alert_channels_only_webhooks_are_signed CHECK ((kind = 'webhook') = (secret_sealed IS NOT NULL))
);

CREATE TABLE alert_states (
    rule_id uuid NOT NULL REFERENCES alerts (id) ON DELETE CASCADE,
    labels_key text NOT NULL,
    labels jsonb NOT NULL DEFAULT '{}',
    status text NOT NULL CHECK (status IN ('ok', 'pending', 'firing', 'no_data')),
    value double precision,
    since_ms bigint NOT NULL,
    evaluated_ms bigint NOT NULL,
    PRIMARY KEY (rule_id, labels_key)
);

CREATE TABLE alert_events (
    id uuid PRIMARY KEY,
    rule_id uuid NOT NULL REFERENCES alerts (id) ON DELETE CASCADE,
    org_id uuid NOT NULL REFERENCES orgs (id) ON DELETE CASCADE,
    labels jsonb NOT NULL DEFAULT '{}',
    from_status text NOT NULL CHECK (from_status IN ('ok', 'pending', 'firing', 'no_data')),
    to_status text NOT NULL CHECK (to_status IN ('ok', 'pending', 'firing', 'no_data')),
    value double precision,
    at_ms bigint NOT NULL
);

CREATE INDEX alert_events_newest_by_rule ON alert_events (rule_id, at_ms DESC);
CREATE INDEX alert_events_by_age ON alert_events (at_ms);

CREATE TABLE alert_outbox (
    id uuid PRIMARY KEY,
    org_id uuid NOT NULL REFERENCES orgs (id) ON DELETE CASCADE,
    rule_id uuid NOT NULL REFERENCES alerts (id) ON DELETE CASCADE,
    event_id uuid NOT NULL REFERENCES alert_events (id) ON DELETE CASCADE,
    channel_id uuid NOT NULL REFERENCES alert_channels (id) ON DELETE CASCADE,
    attempt integer NOT NULL DEFAULT 0 CHECK (attempt >= 0),
    next_at_ms bigint NOT NULL,
    status text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'sent', 'abandoned')),
    last_error text,
    UNIQUE (event_id, channel_id)
);

CREATE INDEX alert_outbox_due ON alert_outbox (next_at_ms) WHERE status = 'pending';

CREATE TABLE alert_duties (
    duty text PRIMARY KEY CHECK (duty IN ('evaluate', 'deliver')),
    slot bigint NOT NULL
);
