CREATE TABLE incidents (
    id uuid PRIMARY KEY,
    org_id uuid NOT NULL REFERENCES orgs (id) ON DELETE CASCADE,
    project_id uuid REFERENCES projects (id) ON DELETE SET NULL,
    title text NOT NULL,
    severity text NOT NULL CHECK (severity IN ('sev1', 'sev2', 'sev3', 'sev4')),
    status text NOT NULL CHECK (status IN ('investigating', 'identified', 'monitoring', 'resolved')),
    owner_id uuid REFERENCES accounts (id) ON DELETE SET NULL,
    public boolean NOT NULL DEFAULT false,
    opened_ms bigint NOT NULL,
    resolved_ms bigint,
    opened_from_alert uuid REFERENCES alerts (id) ON DELETE SET NULL,
    CONSTRAINT incidents_resolved_once_resolved CHECK ((status = 'resolved') = (resolved_ms IS NOT NULL))
);

CREATE INDEX incidents_open_by_org ON incidents (org_id, opened_ms DESC) WHERE status <> 'resolved';
CREATE INDEX incidents_newest_by_org ON incidents (org_id, opened_ms DESC);
CREATE INDEX incidents_following_alert ON incidents (opened_from_alert) WHERE status <> 'resolved';

CREATE TABLE incident_events (
    seq bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    id uuid NOT NULL UNIQUE,
    incident_id uuid NOT NULL REFERENCES incidents (id) ON DELETE CASCADE,
    org_id uuid NOT NULL REFERENCES orgs (id) ON DELETE CASCADE,
    kind text NOT NULL CHECK (kind IN ('opened', 'status', 'severity', 'owner', 'comment', 'alert', 'attachment', 'title', 'public')),
    author_kind text NOT NULL CHECK (author_kind IN ('account', 'api_key', 'agent', 'system')),
    author_id uuid,
    author_label text,
    body text,
    detail jsonb NOT NULL DEFAULT '{}',
    at_ms bigint NOT NULL,
    CONSTRAINT incident_events_authors_are_named CHECK (
        (author_kind IN ('account', 'api_key') AND author_id IS NOT NULL)
        OR (author_kind = 'agent' AND author_id IS NULL AND author_label IS NOT NULL)
        OR (author_kind = 'system' AND author_id IS NULL)
    ),
    CONSTRAINT incident_events_comments_have_a_body CHECK ((kind = 'comment') = (body IS NOT NULL))
);

CREATE INDEX incident_events_timeline ON incident_events (incident_id, seq);

CREATE TABLE incident_attachments (
    id uuid PRIMARY KEY,
    incident_id uuid NOT NULL REFERENCES incidents (id) ON DELETE CASCADE,
    org_id uuid NOT NULL REFERENCES orgs (id) ON DELETE CASCADE,
    kind text NOT NULL CHECK (kind IN ('trace', 'logs', 'metric')),
    reference text NOT NULL,
    label text NOT NULL,
    at_ms bigint NOT NULL
);

CREATE INDEX incident_attachments_by_incident ON incident_attachments (incident_id, at_ms);
