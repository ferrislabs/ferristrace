DELETE FROM alert_duties WHERE duty = 'expire_keys';
ALTER TABLE alert_duties DROP CONSTRAINT alert_duties_duty_check;
ALTER TABLE alert_duties ADD CONSTRAINT alert_duties_duty_check
    CHECK (duty IN ('evaluate', 'deliver', 'escalate', 'announce', 'deploys', 'narrate'));

ALTER TABLE api_keys DROP COLUMN notified_7d;
ALTER TABLE api_keys DROP COLUMN notified_1d;
