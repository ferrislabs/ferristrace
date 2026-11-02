ALTER TABLE api_keys DROP CONSTRAINT api_keys_scopes_known;

ALTER TABLE api_keys
    DROP COLUMN created_by,
    DROP COLUMN scopes,
    DROP COLUMN name;
