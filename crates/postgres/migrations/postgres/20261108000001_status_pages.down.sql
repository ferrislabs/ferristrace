DELETE FROM alert_duties WHERE duty = 'announce';
ALTER TABLE alert_duties DROP CONSTRAINT alert_duties_duty_check;
ALTER TABLE alert_duties ADD CONSTRAINT alert_duties_duty_check CHECK (duty IN ('evaluate', 'deliver', 'escalate'));

DROP TABLE status_outbox;
DROP TABLE status_subscribers;
DROP TABLE status_components;
DROP TABLE status_pages;
