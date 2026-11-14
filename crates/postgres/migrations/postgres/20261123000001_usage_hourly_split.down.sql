ALTER TABLE usage_hourly
    DROP COLUMN spans_bytes,
    DROP COLUMN logs_bytes,
    DROP COLUMN metrics_points;
