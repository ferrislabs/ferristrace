CREATE TABLE IF NOT EXISTS metric_points
(
    tenant_id    UUID,
    project_id   UUID,
    service_name LowCardinality(String),
    name         LowCardinality(String),
    kind         Enum8('gauge' = 1, 'sum' = 2),
    temporality  Enum8('unspecified' = 0, 'delta' = 1, 'cumulative' = 2),
    monotonic    Bool,
    attributes   Map(String, String),
    time         DateTime64(9, 'UTC'),
    value        Float64,
    expire_at    DateTime('UTC'),
    INDEX metric_points_by_project project_id TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(time)
ORDER BY (tenant_id, name, time)
TTL expire_at DELETE;

CREATE TABLE IF NOT EXISTS metric_histograms
(
    tenant_id    UUID,
    project_id   UUID,
    service_name LowCardinality(String),
    name         LowCardinality(String),
    temporality  Enum8('unspecified' = 0, 'delta' = 1, 'cumulative' = 2),
    monotonic    Bool,
    attributes   Map(String, String),
    time         DateTime64(9, 'UTC'),
    count        UInt64,
    sum          Float64,
    min          Nullable(Float64),
    max          Nullable(Float64),
    bounds       Array(Float64),
    counts       Array(UInt64),
    expire_at    DateTime('UTC'),
    INDEX metric_histograms_by_project project_id TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(time)
ORDER BY (tenant_id, name, time)
TTL expire_at DELETE;

CREATE TABLE IF NOT EXISTS metric_exemplars
(
    tenant_id  UUID,
    project_id UUID,
    name       LowCardinality(String),
    time       DateTime64(9, 'UTC'),
    value      Float64,
    trace_id   FixedString(16),
    span_id    FixedString(8),
    attributes Map(String, String),
    expire_at  DateTime('UTC')
)
ENGINE = MergeTree
PARTITION BY toDate(time)
ORDER BY (tenant_id, name, time)
TTL expire_at DELETE;

CREATE TABLE IF NOT EXISTS metric_catalogue
(
    tenant_id   UUID,
    project_id  UUID,
    name        String,
    kind        Enum8('gauge' = 1, 'sum' = 2, 'histogram' = 3),
    unit        String,
    description String,
    last_seen   DateTime64(9, 'UTC'),
    expire_at   DateTime('UTC')
)
ENGINE = ReplacingMergeTree(last_seen)
ORDER BY (tenant_id, name, project_id)
TTL expire_at DELETE
