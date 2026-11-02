ALTER TABLE incident_events DROP CONSTRAINT incident_events_authors_are_named;
ALTER TABLE incident_events ADD CONSTRAINT incident_events_authors_are_named CHECK (
    (author_kind IN ('account', 'api_key') AND author_id IS NOT NULL)
    OR (author_kind = 'agent' AND author_id IS NULL AND author_label IS NOT NULL)
    OR (author_kind = 'system' AND author_id IS NULL)
) NOT VALID;
