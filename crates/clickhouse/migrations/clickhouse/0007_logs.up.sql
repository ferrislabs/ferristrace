CREATE TABLE IF NOT EXISTS logs
(
    tenant_id       UUID,
    project_id      UUID,
    service_name    LowCardinality(String),
    time            DateTime64(9, 'UTC'),
    observed_time   DateTime64(9, 'UTC'),
    severity        Enum8('trace' = 1, 'debug' = 2, 'info' = 3, 'warn' = 4, 'error' = 5, 'fatal' = 6),
    severity_number UInt8,
    body            String,
    attributes      Map(String, String),
    resource        Map(String, String),
    trace_id        FixedString(16) DEFAULT toFixedString('', 16),
    span_id         FixedString(8) DEFAULT toFixedString('', 8),
    expire_at       DateTime('UTC'),
    INDEX logs_by_trace trace_id TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX logs_by_project project_id TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX logs_by_body body TYPE tokenbf_v1(32768, 3, 0) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(time)
ORDER BY (tenant_id, service_name, time)
TTL expire_at DELETE
