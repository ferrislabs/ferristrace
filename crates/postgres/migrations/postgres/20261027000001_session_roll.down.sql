DROP INDEX sessions_by_id;

ALTER TABLE sessions DROP COLUMN user_agent;

ALTER TABLE sessions DROP COLUMN id;
