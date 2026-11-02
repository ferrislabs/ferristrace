CREATE TABLE IF NOT EXISTS spans_replacing
(
    tenant_id        UUID,
    project_id       UUID,
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
    expire_at        DateTime('UTC'),
    INDEX spans_by_trace trace_id TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX spans_by_project project_id TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = ReplacingMergeTree
PARTITION BY toDate(start_time)
ORDER BY (tenant_id, service_name, toStartOfHour(start_time), trace_id, span_id)
TTL expire_at DELETE;

INSERT INTO spans_replacing
(
    tenant_id, project_id, service_name, start_time, end_time, trace_id, span_id,
    parent_span_id, name, kind, status, http_status_code, attributes, expire_at
)
SELECT
    tenant_id, project_id, service_name, start_time, end_time, trace_id, span_id,
    parent_span_id, name, kind, status, http_status_code, attributes, expire_at
FROM spans;

RENAME TABLE spans TO spans_merge_tree, spans_replacing TO spans;

DROP TABLE spans_merge_tree;

CREATE TABLE IF NOT EXISTS span_events_replacing
(
    tenant_id    UUID,
    project_id   UUID,
    service_name LowCardinality(String),
    time         DateTime64(9, 'UTC'),
    trace_id     FixedString(16),
    span_id      FixedString(8),
    name         LowCardinality(String),
    attributes   Map(String, String),
    expire_at    DateTime('UTC'),
    INDEX span_events_by_trace trace_id TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX span_events_by_project project_id TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = ReplacingMergeTree
PARTITION BY toDate(time)
ORDER BY (tenant_id, service_name, toStartOfHour(time), name, trace_id, span_id, time)
TTL expire_at DELETE;

INSERT INTO span_events_replacing
(
    tenant_id, project_id, service_name, time, trace_id, span_id, name, attributes, expire_at
)
SELECT
    tenant_id, project_id, service_name, time, trace_id, span_id, name, attributes, expire_at
FROM span_events;

RENAME TABLE span_events TO span_events_merge_tree, span_events_replacing TO span_events;

DROP TABLE span_events_merge_tree
