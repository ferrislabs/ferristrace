DROP INDEX api_keys_by_prefix_live;

ALTER TABLE api_keys DROP COLUMN secret_digest;
