ALTER TABLE accounts ADD COLUMN oidc_subject text UNIQUE;

ALTER TABLE accounts
    ADD CONSTRAINT accounts_carry_a_way_in
    CHECK (password_hash IS NOT NULL OR oidc_subject IS NOT NULL) NOT VALID;

DROP INDEX identity_links_by_account;

DROP TABLE identity_links;
