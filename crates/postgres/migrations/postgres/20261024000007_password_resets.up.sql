CREATE TABLE password_resets (
    digest bytea PRIMARY KEY,
    account_id uuid NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    created_at bigint NOT NULL,
    expires_at bigint NOT NULL
);

CREATE INDEX password_resets_by_expiry ON password_resets (expires_at);
