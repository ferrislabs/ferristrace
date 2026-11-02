ALTER TABLE accounts ADD COLUMN password_hash text;

CREATE TABLE password_resets (
    digest bytea PRIMARY KEY,
    account_id uuid NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    created_at bigint NOT NULL,
    expires_at bigint NOT NULL
);

CREATE INDEX password_resets_by_expiry ON password_resets (expires_at);

CREATE TABLE auth_factors (
    account_id uuid NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    kind text NOT NULL,
    secret bytea NOT NULL,
    created_at bigint NOT NULL,
    confirmed_at bigint,
    last_step bigint,
    PRIMARY KEY (account_id, kind)
);

CREATE TABLE recovery_codes (
    digest bytea PRIMARY KEY,
    account_id uuid NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    created_at bigint NOT NULL
);

CREATE INDEX recovery_codes_by_account ON recovery_codes (account_id);

CREATE TABLE second_step_challenges (
    digest bytea PRIMARY KEY,
    account_id uuid NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    created_at bigint NOT NULL,
    expires_at bigint NOT NULL,
    answers integer NOT NULL DEFAULT 0
);

CREATE INDEX second_step_challenges_by_expiry ON second_step_challenges (expires_at);
