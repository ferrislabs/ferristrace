CREATE TABLE audit_entries (
    id uuid PRIMARY KEY,
    org_id uuid NOT NULL REFERENCES orgs (id) ON DELETE CASCADE,
    actor_kind text NOT NULL CHECK (actor_kind IN ('account', 'api_key', 'operator')),
    actor_id uuid,
    action text NOT NULL,
    target_kind text NOT NULL,
    target_id text,
    detail jsonb NOT NULL DEFAULT '{}',
    at bigint NOT NULL,
    CONSTRAINT audit_entries_operator_is_anonymous CHECK ((actor_kind = 'operator') = (actor_id IS NULL))
);

CREATE INDEX audit_entries_newest_by_org ON audit_entries (org_id, at DESC);
