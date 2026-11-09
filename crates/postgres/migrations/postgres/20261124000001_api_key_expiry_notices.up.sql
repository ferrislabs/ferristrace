ALTER TABLE api_keys ADD COLUMN notified_7d boolean NOT NULL DEFAULT false;
ALTER TABLE api_keys ADD COLUMN notified_1d boolean NOT NULL DEFAULT false;

ALTER TABLE alert_duties DROP CONSTRAINT alert_duties_duty_check;
ALTER TABLE alert_duties ADD CONSTRAINT alert_duties_duty_check
    CHECK (duty IN ('evaluate', 'deliver', 'escalate', 'announce', 'deploys', 'narrate', 'expire_keys'));
