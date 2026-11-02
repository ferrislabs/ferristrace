ALTER TABLE audit_entries DROP CONSTRAINT audit_entries_operator_is_anonymous;
ALTER TABLE audit_entries DROP CONSTRAINT audit_entries_actor_kind_check;
ALTER TABLE audit_entries ADD CONSTRAINT audit_entries_actor_kind_check
    CHECK (actor_kind IN ('account', 'api_key', 'operator', 'system')) NOT VALID;
ALTER TABLE audit_entries ADD CONSTRAINT audit_entries_operator_is_anonymous
    CHECK ((actor_kind IN ('operator', 'system')) = (actor_id IS NULL)) NOT VALID;

DROP TABLE agents;
