CREATE TABLE incident_outbox (
    id          uuid PRIMARY KEY,
    org_id      uuid    NOT NULL REFERENCES orgs (id) ON DELETE CASCADE,
    incident_id uuid    NOT NULL REFERENCES incidents (id) ON DELETE CASCADE,
    event_id    uuid    NOT NULL REFERENCES incident_events (id) ON DELETE CASCADE,
    channel_id  uuid    NOT NULL REFERENCES alert_channels (id) ON DELETE CASCADE,
    attempt     integer NOT NULL DEFAULT 0,
    next_at_ms  bigint  NOT NULL,
    status      text    NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'sent', 'abandoned')),
    last_error  text,
    UNIQUE (event_id, channel_id)
);

CREATE INDEX incident_outbox_due ON incident_outbox (next_at_ms) WHERE status = 'pending';

ALTER TABLE alert_duties DROP CONSTRAINT alert_duties_duty_check;
ALTER TABLE alert_duties ADD CONSTRAINT alert_duties_duty_check CHECK (duty IN ('evaluate', 'deliver', 'escalate', 'announce', 'deploys', 'narrate'));
