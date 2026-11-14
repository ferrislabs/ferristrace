ALTER TABLE usage_hourly
    ADD COLUMN spans_bytes bigint CHECK (spans_bytes >= 0),
    ADD COLUMN logs_bytes bigint CHECK (logs_bytes >= 0),
    ADD COLUMN metrics_points bigint CHECK (metrics_points >= 0);
