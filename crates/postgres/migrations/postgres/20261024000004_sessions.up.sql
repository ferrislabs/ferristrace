CREATE TABLE sessions (
    hash bytea PRIMARY KEY,
    account_id uuid NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    org_id uuid REFERENCES orgs (id) ON DELETE SET NULL,
    created_at bigint NOT NULL,
    last_seen_at bigint NOT NULL,
    expires_at bigint NOT NULL,
    revoked_at bigint
);

CREATE INDEX sessions_by_account ON sessions (account_id);

CREATE INDEX sessions_by_expiry ON sessions (expires_at);
