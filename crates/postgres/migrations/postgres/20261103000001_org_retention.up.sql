ALTER TABLE orgs ADD COLUMN retention_days integer
    CHECK (retention_days IS NULL OR retention_days > 0);
