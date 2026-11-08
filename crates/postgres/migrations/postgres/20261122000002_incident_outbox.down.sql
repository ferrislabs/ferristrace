DELETE FROM alert_duties WHERE duty = 'narrate';

ALTER TABLE alert_duties DROP CONSTRAINT alert_duties_duty_check;
ALTER TABLE alert_duties ADD CONSTRAINT alert_duties_duty_check CHECK (duty IN ('evaluate', 'deliver', 'escalate', 'announce', 'deploys'));

DROP TABLE incident_outbox;
