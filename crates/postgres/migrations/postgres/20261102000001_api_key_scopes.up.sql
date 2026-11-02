ALTER TABLE api_keys
    ADD COLUMN name text NOT NULL DEFAULT '',
    ADD COLUMN scopes text[] NOT NULL DEFAULT '{ingest}',
    ADD COLUMN created_by uuid REFERENCES accounts (id) ON DELETE SET NULL;

ALTER TABLE api_keys
    ADD CONSTRAINT api_keys_scopes_known
    CHECK (cardinality(scopes) > 0 AND scopes <@ ARRAY['ingest', 'read']::text[]);
