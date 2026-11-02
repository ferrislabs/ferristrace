ALTER TABLE spans
    ADD COLUMN IF NOT EXISTS k8s_cluster LowCardinality(String) MATERIALIZED attributes['k8s.cluster.name'],
    ADD COLUMN IF NOT EXISTS k8s_namespace LowCardinality(String) MATERIALIZED attributes['k8s.namespace.name'],
    ADD COLUMN IF NOT EXISTS k8s_pod String MATERIALIZED attributes['k8s.pod.name'],
    ADD COLUMN IF NOT EXISTS k8s_node LowCardinality(String) MATERIALIZED attributes['k8s.node.name'],
    ADD COLUMN IF NOT EXISTS k8s_workload LowCardinality(String) MATERIALIZED coalesce(nullIf(attributes['k8s.deployment.name'], ''), nullIf(attributes['k8s.statefulset.name'], ''), nullIf(attributes['k8s.daemonset.name'], ''), nullIf(attributes['k8s.cronjob.name'], ''), nullIf(attributes['k8s.job.name'], ''), ''),
    ADD INDEX IF NOT EXISTS spans_by_pod k8s_pod TYPE bloom_filter(0.01) GRANULARITY 4,
    ADD INDEX IF NOT EXISTS spans_by_node k8s_node TYPE bloom_filter(0.01) GRANULARITY 4,
    ADD INDEX IF NOT EXISTS spans_by_workload k8s_workload TYPE bloom_filter(0.01) GRANULARITY 4;

ALTER TABLE logs
    ADD COLUMN IF NOT EXISTS k8s_cluster LowCardinality(String) MATERIALIZED resource['k8s.cluster.name'],
    ADD COLUMN IF NOT EXISTS k8s_namespace LowCardinality(String) MATERIALIZED resource['k8s.namespace.name'],
    ADD COLUMN IF NOT EXISTS k8s_pod String MATERIALIZED resource['k8s.pod.name'],
    ADD COLUMN IF NOT EXISTS k8s_node LowCardinality(String) MATERIALIZED resource['k8s.node.name'],
    ADD COLUMN IF NOT EXISTS k8s_workload LowCardinality(String) MATERIALIZED coalesce(nullIf(resource['k8s.deployment.name'], ''), nullIf(resource['k8s.statefulset.name'], ''), nullIf(resource['k8s.daemonset.name'], ''), nullIf(resource['k8s.cronjob.name'], ''), nullIf(resource['k8s.job.name'], ''), ''),
    ADD INDEX IF NOT EXISTS logs_by_pod k8s_pod TYPE bloom_filter(0.01) GRANULARITY 4,
    ADD INDEX IF NOT EXISTS logs_by_node k8s_node TYPE bloom_filter(0.01) GRANULARITY 4,
    ADD INDEX IF NOT EXISTS logs_by_workload k8s_workload TYPE bloom_filter(0.01) GRANULARITY 4;

ALTER TABLE metric_points
    ADD COLUMN IF NOT EXISTS k8s_cluster LowCardinality(String) MATERIALIZED attributes['k8s.cluster.name'],
    ADD COLUMN IF NOT EXISTS k8s_namespace LowCardinality(String) MATERIALIZED attributes['k8s.namespace.name'],
    ADD COLUMN IF NOT EXISTS k8s_pod String MATERIALIZED attributes['k8s.pod.name'],
    ADD COLUMN IF NOT EXISTS k8s_node LowCardinality(String) MATERIALIZED attributes['k8s.node.name'],
    ADD COLUMN IF NOT EXISTS k8s_workload LowCardinality(String) MATERIALIZED coalesce(nullIf(attributes['k8s.deployment.name'], ''), nullIf(attributes['k8s.statefulset.name'], ''), nullIf(attributes['k8s.daemonset.name'], ''), nullIf(attributes['k8s.cronjob.name'], ''), nullIf(attributes['k8s.job.name'], ''), ''),
    ADD INDEX IF NOT EXISTS metric_points_by_pod k8s_pod TYPE bloom_filter(0.01) GRANULARITY 4,
    ADD INDEX IF NOT EXISTS metric_points_by_node k8s_node TYPE bloom_filter(0.01) GRANULARITY 4,
    ADD INDEX IF NOT EXISTS metric_points_by_workload k8s_workload TYPE bloom_filter(0.01) GRANULARITY 4;

ALTER TABLE metric_histograms
    ADD COLUMN IF NOT EXISTS k8s_cluster LowCardinality(String) MATERIALIZED attributes['k8s.cluster.name'],
    ADD COLUMN IF NOT EXISTS k8s_namespace LowCardinality(String) MATERIALIZED attributes['k8s.namespace.name'],
    ADD COLUMN IF NOT EXISTS k8s_pod String MATERIALIZED attributes['k8s.pod.name'],
    ADD COLUMN IF NOT EXISTS k8s_node LowCardinality(String) MATERIALIZED attributes['k8s.node.name'],
    ADD COLUMN IF NOT EXISTS k8s_workload LowCardinality(String) MATERIALIZED coalesce(nullIf(attributes['k8s.deployment.name'], ''), nullIf(attributes['k8s.statefulset.name'], ''), nullIf(attributes['k8s.daemonset.name'], ''), nullIf(attributes['k8s.cronjob.name'], ''), nullIf(attributes['k8s.job.name'], ''), ''),
    ADD INDEX IF NOT EXISTS metric_histograms_by_pod k8s_pod TYPE bloom_filter(0.01) GRANULARITY 4,
    ADD INDEX IF NOT EXISTS metric_histograms_by_node k8s_node TYPE bloom_filter(0.01) GRANULARITY 4,
    ADD INDEX IF NOT EXISTS metric_histograms_by_workload k8s_workload TYPE bloom_filter(0.01) GRANULARITY 4
