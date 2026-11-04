CREATE TABLE sessions (
    hash bytea PRIMARY KEY,
    account_id uuid NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    org_id uuid REFERENCES orgs (id) ON DELETE SET NULL,
    created_at bigint NOT NULL,
    last_seen_at bigint NOT NULL,
    expires_at bigint NOT NULL,
    revoked_at bigint,
    id uuid NOT NULL DEFAULT gen_random_uuid(),
    user_agent text
);

CREATE INDEX sessions_by_account ON sessions (account_id);

CREATE INDEX sessions_by_expiry ON sessions (expires_at);

CREATE UNIQUE INDEX sessions_by_id ON sessions (id);

CREATE TABLE oidc_flows (
    state text PRIMARY KEY,
    nonce text NOT NULL,
    code_verifier text NOT NULL,
    redirect_uri text NOT NULL,
    created_at bigint NOT NULL,
    expires_at bigint NOT NULL
);

CREATE INDEX oidc_flows_by_expiry ON oidc_flows (expires_at);

ALTER TABLE accounts DROP COLUMN active_org_id;
