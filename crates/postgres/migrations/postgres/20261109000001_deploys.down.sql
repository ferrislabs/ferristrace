DROP TABLE deploy_watermark;

DELETE FROM alert_duties WHERE duty = 'deploys';
ALTER TABLE alert_duties DROP CONSTRAINT alert_duties_duty_check;
ALTER TABLE alert_duties ADD CONSTRAINT alert_duties_duty_check CHECK (duty IN ('evaluate', 'deliver', 'escalate', 'announce'));

DROP INDEX incident_events_one_deploy_each;

ALTER TABLE incident_events DROP CONSTRAINT incident_events_kind_check;
ALTER TABLE incident_events ADD CONSTRAINT incident_events_kind_check CHECK (
    kind IN ('opened', 'status', 'severity', 'owner', 'comment', 'alert', 'attachment', 'title', 'public', 'paged', 'acknowledged')
) NOT VALID;
