ALTER TABLE accounts ADD COLUMN active_org_id uuid REFERENCES orgs (id) ON DELETE SET NULL;

UPDATE accounts SET active_org_id = latest.org_id
FROM (
    SELECT DISTINCT ON (account_id) account_id, org_id
    FROM sessions
    WHERE revoked_at IS NULL AND org_id IS NOT NULL
    ORDER BY account_id, last_seen_at DESC
) AS latest
WHERE accounts.id = latest.account_id;

DROP TABLE oidc_flows;

DROP TABLE sessions;
