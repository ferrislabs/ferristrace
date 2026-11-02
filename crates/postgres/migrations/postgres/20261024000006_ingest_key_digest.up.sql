ALTER TABLE api_keys ADD COLUMN secret_digest bytea;

CREATE INDEX api_keys_by_prefix_live ON api_keys (prefix) WHERE revoked_at IS NULL;
