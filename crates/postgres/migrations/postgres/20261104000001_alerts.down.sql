DROP TABLE alert_duties;
DROP TABLE alert_outbox;
DROP TABLE alert_events;
DROP TABLE alert_states;
DROP TABLE alert_channels;
DROP INDEX alerts_enabled;
ALTER TABLE alerts DROP COLUMN project_id;
