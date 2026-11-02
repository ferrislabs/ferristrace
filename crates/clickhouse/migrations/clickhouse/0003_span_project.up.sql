ALTER TABLE spans ADD COLUMN IF NOT EXISTS project_id UUID AFTER tenant_id, ADD INDEX IF NOT EXISTS spans_by_project project_id TYPE bloom_filter(0.01) GRANULARITY 4
