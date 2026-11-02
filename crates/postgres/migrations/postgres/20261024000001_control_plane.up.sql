CREATE TABLE plans (
    id uuid PRIMARY KEY,
    slug text NOT NULL UNIQUE,
    name text NOT NULL,
    ingest_gb integer CHECK (ingest_gb IS NULL OR ingest_gb > 0),
    retention_days integer CHECK (retention_days IS NULL OR retention_days > 0),
    max_members integer CHECK (max_members IS NULL OR max_members > 0),
    features text[] NOT NULL DEFAULT '{}',
    created_at bigint NOT NULL DEFAULT ((extract(epoch FROM clock_timestamp()) * 1000000000)::bigint)
);

INSERT INTO plans (id, slug, name, ingest_gb, retention_days, max_members, features)
VALUES (
    '00000000-0000-0000-0000-000000000001',
    'self-hosted',
    'Self-hosted',
    NULL,
    NULL,
    NULL,
    '{}'
);

CREATE TABLE accounts (
    id uuid PRIMARY KEY,
    email text NOT NULL UNIQUE,
    password_hash text,
    oidc_subject text UNIQUE,
    created_at bigint NOT NULL DEFAULT ((extract(epoch FROM clock_timestamp()) * 1000000000)::bigint),
    CONSTRAINT accounts_carry_a_way_in CHECK (password_hash IS NOT NULL OR oidc_subject IS NOT NULL)
);

CREATE TABLE orgs (
    id uuid PRIMARY KEY,
    name text NOT NULL,
    plan_id uuid NOT NULL REFERENCES plans (id),
    created_at bigint NOT NULL DEFAULT ((extract(epoch FROM clock_timestamp()) * 1000000000)::bigint)
);

CREATE TYPE org_role AS ENUM ('owner', 'admin', 'member');

CREATE TABLE org_members (
    org_id uuid NOT NULL REFERENCES orgs (id) ON DELETE CASCADE,
    account_id uuid NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    role org_role NOT NULL,
    created_at bigint NOT NULL DEFAULT ((extract(epoch FROM clock_timestamp()) * 1000000000)::bigint),
    PRIMARY KEY (org_id, account_id)
);

CREATE TABLE projects (
    id uuid PRIMARY KEY,
    org_id uuid NOT NULL REFERENCES orgs (id) ON DELETE CASCADE,
    name text NOT NULL,
    created_at bigint NOT NULL DEFAULT ((extract(epoch FROM clock_timestamp()) * 1000000000)::bigint),
    UNIQUE (org_id, name)
);

CREATE TABLE api_keys (
    id uuid PRIMARY KEY,
    org_id uuid NOT NULL REFERENCES orgs (id) ON DELETE CASCADE,
    project_id uuid REFERENCES projects (id) ON DELETE CASCADE,
    prefix text NOT NULL UNIQUE,
    hash text NOT NULL,
    created_at bigint NOT NULL DEFAULT ((extract(epoch FROM clock_timestamp()) * 1000000000)::bigint),
    revoked_at bigint,
    last_used_at bigint
);

CREATE INDEX api_keys_live_by_org ON api_keys (org_id) WHERE revoked_at IS NULL;

CREATE TABLE dashboards (
    id uuid PRIMARY KEY,
    org_id uuid NOT NULL REFERENCES orgs (id) ON DELETE CASCADE,
    name text NOT NULL,
    definition jsonb NOT NULL,
    created_by uuid REFERENCES accounts (id) ON DELETE SET NULL,
    created_at bigint NOT NULL DEFAULT ((extract(epoch FROM clock_timestamp()) * 1000000000)::bigint),
    updated_at bigint NOT NULL DEFAULT ((extract(epoch FROM clock_timestamp()) * 1000000000)::bigint),
    UNIQUE (org_id, name)
);

CREATE TABLE alerts (
    id uuid PRIMARY KEY,
    org_id uuid NOT NULL REFERENCES orgs (id) ON DELETE CASCADE,
    name text NOT NULL,
    definition jsonb NOT NULL,
    enabled boolean NOT NULL DEFAULT true,
    created_by uuid REFERENCES accounts (id) ON DELETE SET NULL,
    created_at bigint NOT NULL DEFAULT ((extract(epoch FROM clock_timestamp()) * 1000000000)::bigint),
    updated_at bigint NOT NULL DEFAULT ((extract(epoch FROM clock_timestamp()) * 1000000000)::bigint),
    UNIQUE (org_id, name)
);
