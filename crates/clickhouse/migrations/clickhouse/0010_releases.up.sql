CREATE TABLE IF NOT EXISTS releases
(
    tenant_id    UUID,
    project_id   UUID,
    service_name LowCardinality(String),
    version      String,
    environment  LowCardinality(String),
    first_seen   SimpleAggregateFunction(min, DateTime64(9, 'UTC')),
    last_seen    SimpleAggregateFunction(max, DateTime64(9, 'UTC')),
    requests     SimpleAggregateFunction(sum, UInt64),
    errors       SimpleAggregateFunction(sum, UInt64),
    expire_at    SimpleAggregateFunction(max, DateTime('UTC'))
)
ENGINE = AggregatingMergeTree
ORDER BY (tenant_id, project_id, service_name, version, environment)
TTL expire_at DELETE;

INSERT INTO releases
SELECT
    tenant_id,
    project_id,
    service_name,
    attributes['service.version'] AS version,
    coalesce(nullIf(attributes['deployment.environment.name'], ''), nullIf(attributes['deployment.environment'], ''), '') AS environment,
    min(start_time) AS first_seen,
    max(start_time) AS last_seen,
    countIf(kind IN ('server', 'consumer')) AS requests,
    countIf(kind IN ('server', 'consumer') AND status = 'error') AS errors,
    max(expire_at) AS expire_at
FROM spans
WHERE attributes['service.version'] != ''
GROUP BY tenant_id, project_id, service_name, version, environment;

CREATE MATERIALIZED VIEW IF NOT EXISTS releases_from_spans TO releases AS
SELECT
    tenant_id,
    project_id,
    service_name,
    attributes['service.version'] AS version,
    coalesce(nullIf(attributes['deployment.environment.name'], ''), nullIf(attributes['deployment.environment'], ''), '') AS environment,
    min(start_time) AS first_seen,
    max(start_time) AS last_seen,
    countIf(kind IN ('server', 'consumer')) AS requests,
    countIf(kind IN ('server', 'consumer') AND status = 'error') AS errors,
    max(expire_at) AS expire_at
FROM spans
WHERE attributes['service.version'] != ''
GROUP BY tenant_id, project_id, service_name, version, environment;

CREATE MATERIALIZED VIEW IF NOT EXISTS releases_from_deploys TO releases AS
SELECT
    tenant_id,
    project_id,
    service_name,
    coalesce(nullIf(attributes['service.version'], ''), nullIf(resource['service.version'], ''), '') AS version,
    coalesce(nullIf(attributes['deployment.environment.name'], ''), nullIf(attributes['deployment.environment'], ''), nullIf(resource['deployment.environment.name'], ''), nullIf(resource['deployment.environment'], ''), '') AS environment,
    min(time) AS first_seen,
    max(time) AS last_seen,
    toUInt64(0) AS requests,
    toUInt64(0) AS errors,
    max(expire_at) AS expire_at
FROM logs
WHERE attributes['event.name'] IN ('deploy', 'deployment.event')
  AND coalesce(nullIf(attributes['service.version'], ''), nullIf(resource['service.version'], ''), '') != ''
GROUP BY tenant_id, project_id, service_name, version, environment
