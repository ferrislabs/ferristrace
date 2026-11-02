-- A factor is a row per method rather than a column per method, so the second one costs an insert
-- and not a migration. The kind is text for the same reason, and the primary key says an account
-- keeps at most one of each.
--
-- The secret is the one thing in this schema that has to be read back rather than compared: a code
-- is recomputed from it, so it cannot be hashed. It is sealed by the adapter before it gets here,
-- nonce first, and ADR 0020 says why and what it costs.
--
-- confirmed_at is null until the owner reads a code off the thing and sends it back, and only a
-- confirmed row is offered at sign in, so an abandoned enrolment locks nobody out. last_step is the
-- step the last answer spent, so the same code cannot answer twice.
CREATE TABLE auth_factors (
    account_id uuid NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    kind text NOT NULL,
    secret bytea NOT NULL,
    created_at bigint NOT NULL,
    confirmed_at bigint,
    last_step bigint,
    PRIMARY KEY (account_id, kind)
);

-- A recovery code is spent by deleting its row, like a password reset, so what is left is what is
-- left: the count is the answer the screen shows.
CREATE TABLE recovery_codes (
    digest bytea PRIMARY KEY,
    account_id uuid NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    created_at bigint NOT NULL
);

CREATE INDEX recovery_codes_by_account ON recovery_codes (account_id);

-- The challenge is what stands between a correct password and a session. It carries its own window
-- and its own answer count, because five answers close it whatever they carry.
CREATE TABLE second_step_challenges (
    digest bytea PRIMARY KEY,
    account_id uuid NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    created_at bigint NOT NULL,
    expires_at bigint NOT NULL,
    answers integer NOT NULL DEFAULT 0
);

CREATE INDEX second_step_challenges_by_expiry ON second_step_challenges (expires_at);
