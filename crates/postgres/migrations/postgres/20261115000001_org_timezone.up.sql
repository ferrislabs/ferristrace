ALTER TABLE orgs ADD COLUMN timezone text NOT NULL DEFAULT 'UTC'
    CHECK (timezone <> '');
