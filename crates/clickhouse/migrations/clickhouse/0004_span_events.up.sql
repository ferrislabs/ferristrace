-- Something that happened at a moment inside a span. An exception is one of these.
--
-- One row per event rather than arrays on the span row. Arrays would render a trace in one read
-- and nothing else: counting exceptions by type across a day would have to arrayJoin the whole
-- spans table to get at them. Rows pay one extra read on the trace view and are the shape every
-- question after this one is asked in. They also expire on their own `expire_at`, so events of a
-- span written under a shorter retention do not outlive it.
--
-- `name` is low cardinality because event names are a vocabulary, not data: `exception`, a log
-- level, whatever a service decided to record. The order by puts it before trace_id so that
-- counting one kind of event in a window reads a range rather than the partition.
CREATE TABLE IF NOT EXISTS span_events
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
ENGINE = MergeTree
PARTITION BY toDate(time)
ORDER BY (tenant_id, service_name, toStartOfHour(time), name, trace_id)
TTL expire_at DELETE
