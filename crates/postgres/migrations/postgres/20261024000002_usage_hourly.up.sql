CREATE TABLE usage_hourly (
    org_id uuid NOT NULL REFERENCES orgs (id) ON DELETE CASCADE,
    hour_start bigint NOT NULL,
    ingested_bytes bigint NOT NULL CHECK (ingested_bytes >= 0),
    PRIMARY KEY (org_id, hour_start)
);

CREATE INDEX usage_hourly_by_hour ON usage_hourly (hour_start);
