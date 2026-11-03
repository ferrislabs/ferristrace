ALTER TABLE agents DROP CONSTRAINT agents_scopes_check;
ALTER TABLE agents ADD CONSTRAINT agents_scopes_check
    CHECK (cardinality(scopes) > 0 AND scopes <@ ARRAY['telemetry:read', 'incidents:comment', 'incidents:resolve', 'oncall:acknowledge']::text[]);
