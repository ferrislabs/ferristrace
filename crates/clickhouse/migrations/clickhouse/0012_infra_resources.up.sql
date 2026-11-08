CREATE TABLE IF NOT EXISTS infra_resources
(
    tenant_id    UUID,
    project_id   UUID,
    kind         LowCardinality(String),
    id           String,
    label        SimpleAggregateFunction(max, String),
    cluster      SimpleAggregateFunction(max, String),
    environment  SimpleAggregateFunction(max, String),
    provider     SimpleAggregateFunction(max, String),
    region       SimpleAggregateFunction(max, String),
    zone         SimpleAggregateFunction(max, String),
    host_type    SimpleAggregateFunction(max, String),
    first_seen   SimpleAggregateFunction(min, DateTime64(9, 'UTC')),
    last_seen    SimpleAggregateFunction(max, DateTime64(9, 'UTC')),
    expire_at    SimpleAggregateFunction(max, DateTime('UTC'))
)
ENGINE = AggregatingMergeTree
ORDER BY (tenant_id, project_id, kind, id)
TTL expire_at DELETE;

INSERT INTO infra_resources
SELECT
    tenant_id,
    project_id,
    'cluster' AS kind,
    attributes['k8s.cluster.name'] AS id,
    max(attributes['k8s.cluster.name']) AS label,
    max(attributes['k8s.cluster.name']) AS cluster,
    max(coalesce(nullIf(attributes['deployment.environment.name'], ''), nullIf(attributes['deployment.environment'], ''), '')) AS environment,
    max(attributes['cloud.provider']) AS provider,
    max(attributes['cloud.region']) AS region,
    max(attributes['cloud.availability_zone']) AS zone,
    max(attributes['host.type']) AS host_type,
    min(time) AS first_seen,
    max(time) AS last_seen,
    max(expire_at) AS expire_at
FROM metric_points
WHERE attributes['k8s.cluster.name'] != ''
GROUP BY tenant_id, project_id, kind, id;

INSERT INTO infra_resources
SELECT
    tenant_id,
    project_id,
    'host' AS kind,
    coalesce(nullIf(attributes['host.id'], ''), attributes['host.name']) AS id,
    max(coalesce(nullIf(attributes['host.name'], ''), attributes['host.id'])) AS label,
    max(attributes['k8s.cluster.name']) AS cluster,
    max(coalesce(nullIf(attributes['deployment.environment.name'], ''), nullIf(attributes['deployment.environment'], ''), '')) AS environment,
    max(attributes['cloud.provider']) AS provider,
    max(attributes['cloud.region']) AS region,
    max(attributes['cloud.availability_zone']) AS zone,
    max(attributes['host.type']) AS host_type,
    min(time) AS first_seen,
    max(time) AS last_seen,
    max(expire_at) AS expire_at
FROM metric_points
WHERE name LIKE 'system.%' AND coalesce(nullIf(attributes['host.id'], ''), attributes['host.name']) != ''
GROUP BY tenant_id, project_id, kind, id;

INSERT INTO infra_resources
SELECT
    tenant_id,
    project_id,
    'container' AS kind,
    coalesce(nullIf(attributes['container.name'], ''), attributes['container.id']) AS id,
    max(coalesce(nullIf(attributes['container.name'], ''), attributes['container.id'])) AS label,
    max(attributes['k8s.cluster.name']) AS cluster,
    max(coalesce(nullIf(attributes['deployment.environment.name'], ''), nullIf(attributes['deployment.environment'], ''), '')) AS environment,
    max(attributes['cloud.provider']) AS provider,
    max(attributes['cloud.region']) AS region,
    max(attributes['cloud.availability_zone']) AS zone,
    max(attributes['host.type']) AS host_type,
    min(time) AS first_seen,
    max(time) AS last_seen,
    max(expire_at) AS expire_at
FROM metric_points
WHERE name LIKE 'container.%' AND attributes['k8s.pod.name'] = '' AND coalesce(nullIf(attributes['container.name'], ''), attributes['container.id']) != ''
GROUP BY tenant_id, project_id, kind, id;

CREATE MATERIALIZED VIEW IF NOT EXISTS infra_clusters_from_metrics TO infra_resources AS
SELECT
    tenant_id,
    project_id,
    'cluster' AS kind,
    attributes['k8s.cluster.name'] AS id,
    max(attributes['k8s.cluster.name']) AS label,
    max(attributes['k8s.cluster.name']) AS cluster,
    max(coalesce(nullIf(attributes['deployment.environment.name'], ''), nullIf(attributes['deployment.environment'], ''), '')) AS environment,
    max(attributes['cloud.provider']) AS provider,
    max(attributes['cloud.region']) AS region,
    max(attributes['cloud.availability_zone']) AS zone,
    max(attributes['host.type']) AS host_type,
    min(time) AS first_seen,
    max(time) AS last_seen,
    max(expire_at) AS expire_at
FROM metric_points
WHERE attributes['k8s.cluster.name'] != ''
GROUP BY tenant_id, project_id, kind, id;

CREATE MATERIALIZED VIEW IF NOT EXISTS infra_hosts_from_metrics TO infra_resources AS
SELECT
    tenant_id,
    project_id,
    'host' AS kind,
    coalesce(nullIf(attributes['host.id'], ''), attributes['host.name']) AS id,
    max(coalesce(nullIf(attributes['host.name'], ''), attributes['host.id'])) AS label,
    max(attributes['k8s.cluster.name']) AS cluster,
    max(coalesce(nullIf(attributes['deployment.environment.name'], ''), nullIf(attributes['deployment.environment'], ''), '')) AS environment,
    max(attributes['cloud.provider']) AS provider,
    max(attributes['cloud.region']) AS region,
    max(attributes['cloud.availability_zone']) AS zone,
    max(attributes['host.type']) AS host_type,
    min(time) AS first_seen,
    max(time) AS last_seen,
    max(expire_at) AS expire_at
FROM metric_points
WHERE name LIKE 'system.%' AND coalesce(nullIf(attributes['host.id'], ''), attributes['host.name']) != ''
GROUP BY tenant_id, project_id, kind, id;

CREATE MATERIALIZED VIEW IF NOT EXISTS infra_containers_from_metrics TO infra_resources AS
SELECT
    tenant_id,
    project_id,
    'container' AS kind,
    coalesce(nullIf(attributes['container.name'], ''), attributes['container.id']) AS id,
    max(coalesce(nullIf(attributes['container.name'], ''), attributes['container.id'])) AS label,
    max(attributes['k8s.cluster.name']) AS cluster,
    max(coalesce(nullIf(attributes['deployment.environment.name'], ''), nullIf(attributes['deployment.environment'], ''), '')) AS environment,
    max(attributes['cloud.provider']) AS provider,
    max(attributes['cloud.region']) AS region,
    max(attributes['cloud.availability_zone']) AS zone,
    max(attributes['host.type']) AS host_type,
    min(time) AS first_seen,
    max(time) AS last_seen,
    max(expire_at) AS expire_at
FROM metric_points
WHERE name LIKE 'container.%' AND attributes['k8s.pod.name'] = '' AND coalesce(nullIf(attributes['container.name'], ''), attributes['container.id']) != ''
GROUP BY tenant_id, project_id, kind, id
