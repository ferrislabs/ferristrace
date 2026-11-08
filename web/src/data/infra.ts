import { useQuery } from '@tanstack/react-query'
import { get, json } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { useScope } from '@/lib/scope'

export type ClusterNode = {
  name: string
  ready: boolean | null
  cpu: number | null
  memory: number | null
  memory_available: number | null
  pods: number
}

export type ClusterWorkload = {
  cluster: string | null
  kind: string
  name: string
  namespace: string
  desired: number | null
  ready: number | null
  pods: number
}

export type ClusterPod = {
  name: string
  namespace: string
  cluster: string | null
  workload: { kind: string; name: string } | null
  node: string | null
  phase: string | null
  ready: boolean | null
  restarts: number
  restarts_total: number
  cpu: number | null
  memory: number | null
  services: string[]
  unhealthy: boolean
  reason: string | null
}

export type ClusterView = {
  clusters: string[]
  namespaces: string[]
  nodes: ClusterNode[]
  workloads: ClusterWorkload[]
  pods: ClusterPod[]
  seen_ms: number | null
}

const NOTHING: ClusterView = { clusters: [], namespaces: [], nodes: [], workloads: [], pods: [], seen_ms: null }

const FRESH_MS = 30_000

export function useCluster(cluster: string, namespace: string) {
  const { active } = useOrganisations()
  const { project } = useScope()
  const found = useQuery({
    queryKey: ['cluster', active?.org, project, cluster, namespace],
    enabled: Boolean(active),
    staleTime: FRESH_MS,
    refetchInterval: FRESH_MS,
    retry: false,
    queryFn: async () => {
      const params = new URLSearchParams()
      if (cluster) params.set('cluster', cluster)
      if (namespace) params.set('namespace', namespace)
      if (project) params.set('project', project)
      return json<ClusterView>(await get(`/api/orgs/${active?.org}/cluster?${params.toString()}`))
    },
  })
  return { view: found.data ?? NOTHING, pending: found.isPending, loading: found.isFetching, failed: found.error }
}

export const isReporting = (view: ClusterView) => view.clusters.length > 0 || view.nodes.length > 0 || view.pods.length > 0

export function cores(value: number | null): string {
  if (value === null) return '—'
  if (value < 1) return `${Math.round(value * 1000)}m`
  return `${value.toFixed(2)} cores`
}

export function bytes(value: number | null): string {
  if (value === null) return '—'
  const mib = value / (1024 * 1024)
  if (mib < 1024) return `${Math.round(mib)} Mi`
  return `${(mib / 1024).toFixed(1)} Gi`
}

export const collectorConfig = (endpoint: string) => `receivers:
  otlp:
    protocols:
      grpc: {}
      http: {}
  k8s_cluster:
    auth_type: serviceAccount
    collection_interval: 30s
    node_conditions_to_report: [Ready]
  kubeletstats:
    auth_type: serviceAccount
    endpoint: https://\${env:K8S_NODE_NAME}:10250
    insecure_skip_verify: true
    collection_interval: 30s
    metric_groups: [node, pod]

processors:
  k8sattributes:
    pod_association:
      - sources:
          - from: resource_attribute
            name: k8s.pod.uid
      - sources:
          - from: connection
    extract:
      metadata:
        - k8s.namespace.name
        - k8s.pod.name
        - k8s.pod.uid
        - k8s.node.name
        - k8s.deployment.name
        - k8s.statefulset.name
        - k8s.daemonset.name
        - k8s.cronjob.name
        - k8s.job.name
  resource:
    attributes:
      - key: k8s.cluster.name
        value: prod-eu-1
        action: upsert
  batch: {}

exporters:
  otlphttp:
    endpoint: ${endpoint}
    headers:
      authorization: Bearer \${env:FERRISTRACE_INGEST_KEY}

service:
  pipelines:
    metrics:
      receivers: [k8s_cluster, kubeletstats]
      processors: [k8sattributes, resource, batch]
      exporters: [otlphttp]
    traces:
      receivers: [otlp]
      processors: [k8sattributes, resource, batch]
      exporters: [otlphttp]`

export type ResourceKind = 'cluster' | 'host' | 'container'

export type InfraResource = {
  kind: ResourceKind
  id: string
  name: string
  cluster: string | null
  environment: string | null
  provider: string | null
  region: string | null
  zone: string | null
  host_type: string | null
  first_seen_at_ms: number
  last_seen_at_ms: number
  silent: boolean
  cpu_percent: number | null
  memory_bytes: number | null
  services: string[]
}

export function useInfrastructure() {
  const { active } = useOrganisations()
  const { project } = useScope()
  const found = useQuery({
    queryKey: ['infrastructure', active?.org, project],
    enabled: Boolean(active),
    staleTime: FRESH_MS,
    refetchInterval: FRESH_MS,
    retry: false,
    queryFn: async () => {
      const params = new URLSearchParams()
      if (project) params.set('project', project)
      const body = await json<{ resources: InfraResource[] }>(await get(`/api/orgs/${active?.org}/infrastructure?${params.toString()}`))
      return body.resources.map((resource) => ({ ...resource, services: resource.services ?? [] }))
    },
  })
  return { resources: found.data ?? [], pending: found.isPending, failed: found.error }
}

export const KIND_LABEL: Record<ResourceKind, string> = { cluster: 'Clusters', host: 'Hosts', container: 'Containers' }

export const RECEIVER: Record<ResourceKind, string> = {
  cluster: 'k8s_cluster and kubeletstats, with k8s.cluster.name set on the resource',
  host: 'hostmetrics, with the resourcedetection processor so host.name is set',
  container: 'docker_stats, for containers that do not run in Kubernetes',
}
