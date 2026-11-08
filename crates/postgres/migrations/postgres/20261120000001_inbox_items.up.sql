CREATE TABLE inbox_items (
    id uuid PRIMARY KEY,
    account_id uuid NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    org_id uuid NOT NULL REFERENCES orgs (id) ON DELETE CASCADE,
    kind text NOT NULL,
    title text NOT NULL,
    body text NOT NULL,
    link text,
    created_at bigint NOT NULL,
    read_at bigint
);

CREATE INDEX inbox_items_by_account ON inbox_items (account_id, org_id, created_at DESC);
CREATE INDEX inbox_items_by_age ON inbox_items (created_at);
