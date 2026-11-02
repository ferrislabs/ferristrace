ALTER TABLE sessions ADD COLUMN id uuid NOT NULL DEFAULT gen_random_uuid();

ALTER TABLE sessions ADD COLUMN user_agent text;

CREATE UNIQUE INDEX sessions_by_id ON sessions (id);
