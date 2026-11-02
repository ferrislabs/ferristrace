CREATE TABLE identity_links (
    issuer text NOT NULL,
    subject text NOT NULL,
    account_id uuid NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    created_at bigint NOT NULL DEFAULT ((extract(epoch FROM clock_timestamp()) * 1000000000)::bigint),
    PRIMARY KEY (issuer, subject)
);

CREATE INDEX identity_links_by_account ON identity_links (account_id);

ALTER TABLE accounts DROP CONSTRAINT accounts_carry_a_way_in;

ALTER TABLE accounts DROP COLUMN oidc_subject;
