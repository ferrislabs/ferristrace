CREATE TABLE oncall_schedules (
    id uuid PRIMARY KEY,
    org_id uuid NOT NULL REFERENCES orgs (id) ON DELETE CASCADE,
    team_id uuid NOT NULL,
    name text NOT NULL,
    timezone text NOT NULL,
    handoff_weekday smallint NOT NULL CHECK (handoff_weekday BETWEEN 0 AND 6),
    handoff_minute smallint NOT NULL CHECK (handoff_minute BETWEEN 0 AND 1439),
    length_days smallint NOT NULL CHECK (length_days BETWEEN 1 AND 28),
    starts_on date NOT NULL,
    created_at bigint NOT NULL DEFAULT ((extract(epoch FROM clock_timestamp()) * 1000000000)::bigint),
    UNIQUE (org_id, team_id, name),
    UNIQUE (org_id, id),
    FOREIGN KEY (org_id, team_id) REFERENCES teams (org_id, id) ON DELETE CASCADE
);

CREATE TABLE oncall_rotation (
    schedule_id uuid NOT NULL REFERENCES oncall_schedules (id) ON DELETE CASCADE,
    position integer NOT NULL CHECK (position >= 0),
    account_id uuid NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    PRIMARY KEY (schedule_id, position),
    UNIQUE (schedule_id, account_id)
);

CREATE TABLE oncall_overrides (
    id uuid PRIMARY KEY,
    org_id uuid NOT NULL,
    schedule_id uuid NOT NULL,
    account_id uuid NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    from_ms bigint NOT NULL,
    to_ms bigint NOT NULL,
    CONSTRAINT oncall_overrides_end_after_start CHECK (from_ms < to_ms),
    FOREIGN KEY (org_id, schedule_id) REFERENCES oncall_schedules (org_id, id) ON DELETE CASCADE
);

CREATE INDEX oncall_overrides_by_schedule ON oncall_overrides (schedule_id, to_ms);

CREATE TABLE pages (
    id uuid PRIMARY KEY,
    org_id uuid NOT NULL,
    schedule_id uuid NOT NULL,
    target_id uuid NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    rule_id uuid REFERENCES alerts (id) ON DELETE CASCADE,
    event_id uuid,
    incident_id uuid REFERENCES incidents (id) ON DELETE CASCADE,
    headline text NOT NULL,
    state text NOT NULL CHECK (state IN ('triggered', 'acknowledged', 'escalated', 'resolved')),
    step integer NOT NULL DEFAULT 0 CHECK (step >= 0),
    created_ms bigint NOT NULL,
    escalate_at_ms bigint,
    acknowledged_by uuid REFERENCES accounts (id) ON DELETE SET NULL,
    acknowledged_ms bigint,
    resolved_ms bigint,
    FOREIGN KEY (org_id, schedule_id) REFERENCES oncall_schedules (org_id, id) ON DELETE CASCADE,
    CONSTRAINT pages_come_from_one_source CHECK (
        (rule_id IS NOT NULL AND event_id IS NOT NULL AND incident_id IS NULL)
        OR (rule_id IS NULL AND event_id IS NULL AND incident_id IS NOT NULL)
    ),
    CONSTRAINT pages_resolved_once_resolved CHECK ((state = 'resolved') = (resolved_ms IS NOT NULL))
);

CREATE UNIQUE INDEX pages_one_open_per_rule ON pages (rule_id) WHERE state <> 'resolved' AND rule_id IS NOT NULL;
CREATE UNIQUE INDEX pages_one_open_per_incident ON pages (incident_id) WHERE state <> 'resolved' AND incident_id IS NOT NULL;
CREATE INDEX pages_due ON pages (escalate_at_ms) WHERE state IN ('triggered', 'escalated') AND escalate_at_ms IS NOT NULL;
CREATE INDEX pages_newest_by_org ON pages (org_id, created_ms DESC);

CREATE TABLE page_outbox (
    id uuid PRIMARY KEY,
    org_id uuid NOT NULL REFERENCES orgs (id) ON DELETE CASCADE,
    page_id uuid NOT NULL REFERENCES pages (id) ON DELETE CASCADE,
    step integer NOT NULL CHECK (step >= 0),
    account_id uuid NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    attempt integer NOT NULL DEFAULT 0 CHECK (attempt >= 0),
    next_at_ms bigint NOT NULL,
    status text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'sent', 'abandoned')),
    last_error text,
    UNIQUE (page_id, step, account_id)
);

CREATE INDEX page_outbox_due ON page_outbox (next_at_ms) WHERE status = 'pending';

ALTER TABLE incidents ADD COLUMN paged_schedule uuid REFERENCES oncall_schedules (id) ON DELETE SET NULL;

ALTER TABLE alert_duties DROP CONSTRAINT alert_duties_duty_check;
ALTER TABLE alert_duties ADD CONSTRAINT alert_duties_duty_check CHECK (duty IN ('evaluate', 'deliver', 'escalate'));

ALTER TABLE audit_entries DROP CONSTRAINT audit_entries_actor_kind_check;
ALTER TABLE audit_entries DROP CONSTRAINT audit_entries_operator_is_anonymous;
ALTER TABLE audit_entries ADD CONSTRAINT audit_entries_actor_kind_check
    CHECK (actor_kind IN ('account', 'api_key', 'operator', 'system'));
ALTER TABLE audit_entries ADD CONSTRAINT audit_entries_operator_is_anonymous
    CHECK ((actor_kind IN ('operator', 'system')) = (actor_id IS NULL));

ALTER TABLE incident_events DROP CONSTRAINT incident_events_kind_check;
ALTER TABLE incident_events ADD CONSTRAINT incident_events_kind_check CHECK (
    kind IN ('opened', 'status', 'severity', 'owner', 'comment', 'alert', 'attachment', 'title', 'public', 'paged', 'acknowledged')
);
