CREATE TABLE status_pages (
    id uuid PRIMARY KEY,
    org_id uuid NOT NULL REFERENCES orgs (id) ON DELETE CASCADE,
    name text NOT NULL,
    slug text NOT NULL UNIQUE CHECK (slug ~ '^[a-z0-9-]{3,48}$'),
    public boolean NOT NULL DEFAULT false,
    hide_from_search boolean NOT NULL DEFAULT false,
    accent text NOT NULL CHECK (accent IN ('indigo', 'teal', 'orange', 'rose', 'slate')),
    mode text NOT NULL CHECK (mode IN ('light', 'dark', 'auto')),
    intro text,
    created_ms bigint NOT NULL,
    updated_ms bigint NOT NULL,
    UNIQUE (org_id, id)
);

CREATE INDEX status_pages_by_org ON status_pages (org_id, created_ms);

CREATE TABLE status_components (
    id uuid PRIMARY KEY,
    org_id uuid NOT NULL,
    page_id uuid NOT NULL,
    name text NOT NULL,
    link_kind text NOT NULL CHECK (link_kind IN ('service', 'alert', 'manual')),
    service text,
    project_id uuid REFERENCES projects (id) ON DELETE SET NULL,
    rule_id uuid REFERENCES alerts (id) ON DELETE SET NULL,
    manual_status text CHECK (manual_status IN ('operational', 'degraded', 'down')),
    position integer NOT NULL CHECK (position >= 0),
    public boolean NOT NULL DEFAULT true,
    FOREIGN KEY (org_id, page_id) REFERENCES status_pages (org_id, id) ON DELETE CASCADE,
    CONSTRAINT status_components_links_are_whole CHECK (
        (link_kind = 'service' AND service IS NOT NULL AND rule_id IS NULL)
        OR (link_kind = 'alert' AND service IS NULL AND project_id IS NULL)
        OR (link_kind = 'manual' AND service IS NULL AND project_id IS NULL AND rule_id IS NULL)
    )
);

CREATE INDEX status_components_by_page ON status_components (page_id, position);

CREATE TABLE status_subscribers (
    id uuid PRIMARY KEY,
    page_id uuid NOT NULL REFERENCES status_pages (id) ON DELETE CASCADE,
    email text NOT NULL,
    confirmed boolean NOT NULL DEFAULT false,
    invited_ms bigint NOT NULL,
    UNIQUE (page_id, email)
);

CREATE INDEX status_subscribers_confirmed ON status_subscribers (page_id) WHERE confirmed;

CREATE TABLE status_outbox (
    id uuid PRIMARY KEY,
    page_id uuid NOT NULL REFERENCES status_pages (id) ON DELETE CASCADE,
    subscriber_id uuid NOT NULL REFERENCES status_subscribers (id) ON DELETE CASCADE,
    kind text NOT NULL CHECK (kind IN ('confirm', 'opened', 'status', 'comment')),
    incident_id uuid REFERENCES incidents (id) ON DELETE CASCADE,
    title text,
    incident_status text CHECK (incident_status IN ('investigating', 'identified', 'monitoring', 'resolved')),
    body text,
    at_ms bigint NOT NULL,
    attempt integer NOT NULL DEFAULT 0 CHECK (attempt >= 0),
    next_at_ms bigint NOT NULL,
    status text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'sent', 'abandoned')),
    last_error text,
    CONSTRAINT status_outbox_updates_are_whole CHECK (
        (kind = 'confirm' AND incident_id IS NULL AND title IS NULL AND incident_status IS NULL AND body IS NULL)
        OR (kind <> 'confirm' AND incident_id IS NOT NULL AND title IS NOT NULL AND incident_status IS NOT NULL AND body IS NOT NULL)
    )
);

CREATE INDEX status_outbox_due ON status_outbox (next_at_ms) WHERE status = 'pending';

ALTER TABLE alert_duties DROP CONSTRAINT alert_duties_duty_check;
ALTER TABLE alert_duties ADD CONSTRAINT alert_duties_duty_check CHECK (duty IN ('evaluate', 'deliver', 'escalate', 'announce'));
