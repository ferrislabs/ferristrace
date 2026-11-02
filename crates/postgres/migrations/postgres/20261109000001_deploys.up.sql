ALTER TABLE incident_events DROP CONSTRAINT incident_events_kind_check;
ALTER TABLE incident_events ADD CONSTRAINT incident_events_kind_check CHECK (
    kind IN ('opened', 'status', 'severity', 'owner', 'comment', 'alert', 'attachment', 'title', 'public', 'paged', 'acknowledged', 'deploy')
);

CREATE UNIQUE INDEX incident_events_one_deploy_each ON incident_events (
    incident_id,
    (detail ->> 'service'),
    (detail ->> 'version'),
    (detail ->> 'environment')
) WHERE kind = 'deploy';

ALTER TABLE alert_duties DROP CONSTRAINT alert_duties_duty_check;
ALTER TABLE alert_duties ADD CONSTRAINT alert_duties_duty_check CHECK (duty IN ('evaluate', 'deliver', 'escalate', 'announce', 'deploys'));

CREATE TABLE deploy_watermark (
    singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
    at_ms bigint NOT NULL CHECK (at_ms >= 0)
);
