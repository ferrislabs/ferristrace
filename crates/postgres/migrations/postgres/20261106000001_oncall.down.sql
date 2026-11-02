ALTER TABLE incident_events DROP CONSTRAINT incident_events_kind_check;
ALTER TABLE incident_events ADD CONSTRAINT incident_events_kind_check CHECK (
    kind IN ('opened', 'status', 'severity', 'owner', 'comment', 'alert', 'attachment', 'title', 'public')
) NOT VALID;

ALTER TABLE audit_entries DROP CONSTRAINT audit_entries_operator_is_anonymous;
ALTER TABLE audit_entries DROP CONSTRAINT audit_entries_actor_kind_check;
ALTER TABLE audit_entries ADD CONSTRAINT audit_entries_actor_kind_check
    CHECK (actor_kind IN ('account', 'api_key', 'operator')) NOT VALID;
ALTER TABLE audit_entries ADD CONSTRAINT audit_entries_operator_is_anonymous
    CHECK ((actor_kind = 'operator') = (actor_id IS NULL)) NOT VALID;

DELETE FROM alert_duties WHERE duty = 'escalate';
ALTER TABLE alert_duties DROP CONSTRAINT alert_duties_duty_check;
ALTER TABLE alert_duties ADD CONSTRAINT alert_duties_duty_check CHECK (duty IN ('evaluate', 'deliver'));

UPDATE alerts SET definition = definition - 'page' WHERE definition ? 'page';
ALTER TABLE incidents DROP COLUMN paged_schedule;
DROP TABLE page_outbox;
DROP TABLE pages;
DROP TABLE oncall_overrides;
DROP TABLE oncall_rotation;
DROP TABLE oncall_schedules;
