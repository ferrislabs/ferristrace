CREATE TABLE explorer_views (
    id uuid PRIMARY KEY,
    org_id uuid NOT NULL REFERENCES orgs (id) ON DELETE CASCADE,
    signal text NOT NULL CHECK (signal IN ('traces')),
    name text NOT NULL,
    visibility text NOT NULL CHECK (visibility IN ('private', 'org')),
    definition jsonb NOT NULL,
    created_by uuid REFERENCES accounts (id) ON DELETE SET NULL,
    created_at bigint NOT NULL DEFAULT ((extract(epoch FROM clock_timestamp()) * 1000000000)::bigint),
    updated_at bigint NOT NULL DEFAULT ((extract(epoch FROM clock_timestamp()) * 1000000000)::bigint)
);

CREATE UNIQUE INDEX explorer_views_org_name
    ON explorer_views (org_id, signal, name) WHERE visibility = 'org';

CREATE UNIQUE INDEX explorer_views_private_name
    ON explorer_views (org_id, signal, created_by, name) WHERE visibility = 'private';

CREATE INDEX explorer_views_by_org ON explorer_views (org_id, signal);
