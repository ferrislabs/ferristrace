ALTER TABLE metric_histograms
    DROP INDEX IF EXISTS metric_histograms_by_workload,
    DROP INDEX IF EXISTS metric_histograms_by_node,
    DROP INDEX IF EXISTS metric_histograms_by_pod,
    DROP COLUMN IF EXISTS k8s_workload,
    DROP COLUMN IF EXISTS k8s_node,
    DROP COLUMN IF EXISTS k8s_pod,
    DROP COLUMN IF EXISTS k8s_namespace,
    DROP COLUMN IF EXISTS k8s_cluster;

ALTER TABLE metric_points
    DROP INDEX IF EXISTS metric_points_by_workload,
    DROP INDEX IF EXISTS metric_points_by_node,
    DROP INDEX IF EXISTS metric_points_by_pod,
    DROP COLUMN IF EXISTS k8s_workload,
    DROP COLUMN IF EXISTS k8s_node,
    DROP COLUMN IF EXISTS k8s_pod,
    DROP COLUMN IF EXISTS k8s_namespace,
    DROP COLUMN IF EXISTS k8s_cluster;

ALTER TABLE logs
    DROP INDEX IF EXISTS logs_by_workload,
    DROP INDEX IF EXISTS logs_by_node,
    DROP INDEX IF EXISTS logs_by_pod,
    DROP COLUMN IF EXISTS k8s_workload,
    DROP COLUMN IF EXISTS k8s_node,
    DROP COLUMN IF EXISTS k8s_pod,
    DROP COLUMN IF EXISTS k8s_namespace,
    DROP COLUMN IF EXISTS k8s_cluster;

ALTER TABLE spans
    DROP INDEX IF EXISTS spans_by_workload,
    DROP INDEX IF EXISTS spans_by_node,
    DROP INDEX IF EXISTS spans_by_pod,
    DROP COLUMN IF EXISTS k8s_workload,
    DROP COLUMN IF EXISTS k8s_node,
    DROP COLUMN IF EXISTS k8s_pod,
    DROP COLUMN IF EXISTS k8s_namespace,
    DROP COLUMN IF EXISTS k8s_cluster
