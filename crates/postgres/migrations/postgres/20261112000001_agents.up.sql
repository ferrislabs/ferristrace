CREATE TABLE agents (
    id uuid PRIMARY KEY,
    org_id uuid NOT NULL REFERENCES orgs (id) ON DELETE CASCADE,
    name text NOT NULL CHECK (name <> ''),
    client_id text NOT NULL CHECK (client_id <> ''),
    scopes text[] NOT NULL
        CHECK (cardinality(scopes) > 0 AND scopes <@ ARRAY['telemetry:read', 'incidents:comment', 'incidents:resolve']::text[]),
    project_id uuid REFERENCES projects (id) ON DELETE CASCADE,
    created_by uuid REFERENCES accounts (id) ON DELETE SET NULL,
    created_at bigint NOT NULL,
    last_used_at bigint,
    revoked_at bigint
);

CREATE UNIQUE INDEX agents_live_client_per_org ON agents (org_id, client_id) WHERE revoked_at IS NULL;
CREATE INDEX agents_by_org ON agents (org_id, created_at DESC);

ALTER TABLE audit_entries DROP CONSTRAINT audit_entries_actor_kind_check;
ALTER TABLE audit_entries DROP CONSTRAINT audit_entries_operator_is_anonymous;
ALTER TABLE audit_entries ADD CONSTRAINT audit_entries_actor_kind_check
    CHECK (actor_kind IN ('account', 'api_key', 'agent', 'operator', 'system'));
ALTER TABLE audit_entries ADD CONSTRAINT audit_entries_operator_is_anonymous
    CHECK ((actor_kind IN ('operator', 'system')) = (actor_id IS NULL));
