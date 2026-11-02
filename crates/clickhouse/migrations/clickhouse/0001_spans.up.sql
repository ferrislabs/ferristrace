CREATE TABLE IF NOT EXISTS spans
(
    tenant_id        UUID,
    service_name     LowCardinality(String),
    start_time       DateTime64(9, 'UTC'),
    end_time         DateTime64(9, 'UTC'),
    trace_id         FixedString(16),
    span_id          FixedString(8),
    parent_span_id   Nullable(FixedString(8)),
    name             LowCardinality(String),
    kind             Enum8('unspecified' = 0, 'internal' = 1, 'server' = 2, 'client' = 3, 'producer' = 4, 'consumer' = 5),
    status           Enum8('unset' = 0, 'ok' = 1, 'error' = 2),
    http_status_code Nullable(UInt16),
    attributes       Map(String, String),
    expire_at        DateTime('UTC')
)
ENGINE = MergeTree
PARTITION BY toDate(start_time)
ORDER BY (tenant_id, service_name, toStartOfHour(start_time), trace_id)
TTL expire_at DELETE
