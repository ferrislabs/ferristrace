import { useNavigate as useRouterNavigate } from '@tanstack/react-router'
import { ChevronRight, ScrollText, Server, Waypoints } from 'lucide-react'
import { useMemo } from 'react'
import { Link, useParams, useSearchParam } from '@/lib/router'
import { Segmented } from '@/components/ui/segmented'
import { Skeleton } from '@/components/ui/misc'
import { EmptyState } from '@/components/ft/bits'
import { MetricChart, labelOf } from '@/components/ft/MetricChart'
import { bytes, useCluster, type ClusterNode } from '@/data/infra'
import { useMetricSeries, type MetricLine, type MetricQuery } from '@/data/metrics'
import { NOWHERE, placementSearch } from '@/data/placement'
import { ApiError, because } from '@/auth/client'
import { cn } from '@/lib/utils'
import { PodTable, Seen, Stat, health, millicores, type SortKey } from './Infra'

const RANGES = [['15 min', 15 * 60_000], ['1 h', 60 * 60_000], ['6 h', 6 * 60 * 60_000], ['24 h', 24 * 60 * 60_000]] as const
type RangeLabel = (typeof RANGES)[number][0]

type Chart = { title: string; metric: string; unit: string; scale: number; groupBy?: string[] }

const CHARTS: Chart[] = [
  { title: 'CPU', metric: 'k8s.node.cpu.usage', unit: 'm', scale: 1000 },
  { title: 'Memory working set', metric: 'k8s.node.memory.working_set', unit: 'MiB', scale: 1 / (1024 * 1024) },
  { title: 'CPU by pod', metric: 'k8s.pod.cpu.usage', unit: 'm', scale: 1000, groupBy: ['k8s.pod.name'] },
  { title: 'Memory by pod', metric: 'k8s.pod.memory.working_set', unit: 'MiB', scale: 1 / (1024 * 1024), groupBy: ['k8s.pod.name'] },
]

const scaled = (lines: MetricLine[], by: number): MetricLine[] =>
  lines.map((line) => ({
    labels: line.labels['k8s.pod.name'] ? { pod: line.labels['k8s.pod.name'] } : line.labels,
    points: line.points.map(([t, v]) => [t, v === null ? null : Math.round(v * by * 10) / 10]),
  }))

function NodeChart({ chart, node, cluster, rangeMs }: { chart: Chart; node: string; cluster: string; rangeMs: number }) {
  const query = useMemo<MetricQuery>(() => ({ name: chart.metric, rangeMs, fromMs: null, toMs: null, groupBy: chart.groupBy ?? [], agg: 'avg' }), [chart, rangeMs])
  const placement = useMemo(() => ({ ...NOWHERE, node, cluster }), [node, cluster])
  const { result, pending, failed } = useMetricSeries(query, placement)
  const series = useMemo(() => scaled(result?.series ?? [], chart.scale).sort((a, b) => labelOf(a.labels).localeCompare(labelOf(b.labels))), [result, chart.scale])
  const empty = !pending && !failed && !series.some((line) => line.points.some(([, v]) => v !== null))
  return (
    <section className="flex min-w-0 flex-col gap-2">
      <div className="flex items-baseline gap-2">
        <h3 className="m-0 text-control font-semibold">{chart.title}</h3>
        <span className="font-mono text-[11px] text-muted-foreground">{chart.metric}</span>
      </div>
      {pending ? (
        <div className="flex h-[220px] flex-col justify-end gap-2"><Skeleton className="w-full" /><Skeleton className="w-2/3" /></div>
      ) : failed ? (
        <p className="m-0 flex h-[120px] items-center text-control text-muted-foreground">{because(failed, 'The series could not be read.')}</p>
      ) : empty ? (
        <p className="m-0 flex h-[120px] items-center text-control text-muted-foreground">No {chart.metric} for this node in the window. The kubeletstats receiver sends it.</p>
      ) : (
        <MetricChart series={series} exemplars={[]} unit={chart.unit} aria={`${chart.title} of ${node} over the window`} />
      )}
    </section>
  )
}

function Verdict({ node, ill }: { node: ClusterNode; ill: number }) {
  const down = node.ready === false
  const tone = down || ill ? 'border-destructive-border bg-destructive-subtle text-destructive-text' : node.ready ? 'border-success-border bg-success-subtle text-success-text' : 'bg-muted text-muted-foreground'
  const text = down ? 'Not ready' : ill ? `Ready · ${ill} unhealthy pod${ill === 1 ? '' : 's'}` : node.ready ? 'Ready' : 'No condition reported'
  return <span className={cn('inline-flex h-6 items-center gap-1.5 rounded-full border px-2.5 text-caption font-semibold', tone)}><span aria-hidden className={cn('size-1.5 rounded-full', down || ill ? 'bg-status-out' : node.ready ? 'bg-status-ok' : 'bg-status-none')} />{text}</span>
}

export function KubernetesNode() {
  const { name = '' } = useParams()
  const node = decodeURIComponent(name)
  const navigate = useRouterNavigate()
  const cluster = useSearchParam('cluster') ?? ''
  const range = (useSearchParam('range') as RangeLabel | undefined) ?? '1 h'
  const sort = (useSearchParam('sort') as SortKey | undefined) ?? 'name'
  const desc = useSearchParam('dir') === 'desc'
  const rangeMs = RANGES.find(([label]) => label === range)?.[1] ?? 60 * 60_000
  const set = (part: Record<string, string | undefined>) =>
    void navigate({
      to: `/infra/kubernetes/nodes/${encodeURIComponent(node)}` as never,
      replace: true,
      search: ((was: Record<string, unknown>) => {
        const next: Record<string, unknown> = { ...was, ...part }
        for (const key of Object.keys(next)) if (next[key] === undefined || next[key] === '') delete next[key]
        return next
      }) as never,
    })

  const { view, pending, failed } = useCluster(cluster, '')
  const found = view.nodes.find((candidate) => candidate.name === node) ?? null
  const pods = useMemo(() => view.pods.filter((pod) => pod.node === node), [view.pods, node])
  const ill = pods.filter((pod) => health(pod) === 'bad').length
  const restarts = pods.reduce((sum, pod) => sum + pod.restarts, 0)
  const namespaces = new Set(pods.map((pod) => pod.namespace)).size
  const total = found && found.memory !== null && found.memory_available !== null ? found.memory + found.memory_available : null
  const share = total && found?.memory != null ? found.memory / total : null
  const where = placementSearch({ node })

  const crumbs = (
    <nav aria-label="Breadcrumb" className="flex items-center gap-1.5 text-control text-muted-foreground">
      <Link to="/infra/kubernetes" className="text-muted-foreground no-underline hover:text-foreground hover:underline">Kubernetes</Link>
      <ChevronRight className="size-3.5" aria-hidden />
      <span>Nodes</span>
    </nav>
  )

  if (failed) {
    return (
      <>
        {crumbs}
        <EmptyState icon={<Server />} title={failed instanceof ApiError && failed.status === 503 ? 'No metric store is configured' : 'The node could not be read'}>{because(failed, 'Try again in a moment.')}</EmptyState>
      </>
    )
  }
  if (pending) return crumbs
  if (!found) {
    return (
      <>
        {crumbs}
        <EmptyState icon={<Server />} title={`${node} is not reporting`} actions={<Link to="/infra/kubernetes" className="text-control text-accent-text">Back to Kubernetes</Link>}>
          No metric from this node arrived in the last 10 minutes. It may have been removed from the cluster, or its kubeletstats collector stopped.
        </EmptyState>
      </>
    )
  }

  return (
    <>
      <div className="flex flex-col gap-2">
        {crumbs}
        <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
          <h1 className="text-title m-0 min-w-0 truncate font-mono">{node}</h1>
          <Verdict node={found} ill={ill} />
          <span className="text-control text-muted-foreground">
            {cluster || view.clusters.length === 1 ? `${cluster || view.clusters[0]} · ` : ''}<Seen ms={view.seen_ms} />
          </span>
          <span className="flex-1" />
          <Link to="/logs" search={where} className="inline-flex h-8 items-center gap-1.5 rounded-md border bg-background px-2.5 text-control font-semibold text-foreground no-underline hover:bg-row-hover"><ScrollText className="size-3.5" aria-hidden />Logs on this node</Link>
          <Link to="/traces" search={where} className="inline-flex h-8 items-center gap-1.5 rounded-md border bg-background px-2.5 text-control font-semibold text-foreground no-underline hover:bg-row-hover"><Waypoints className="size-3.5" aria-hidden />Traces on this node</Link>
        </div>
      </div>

      <div className="grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-5">
        <Stat label="Pods" value={String(pods.length)} suffix={`${namespaces} namespace${namespaces === 1 ? '' : 's'}`} />
        <Stat label="Unhealthy" value={String(ill)} suffix={ill ? 'pods' : 'none'} tone={ill ? 'bad' : 'ok'} />
        <Stat label="Restarts 10 min" value={String(restarts)} tone={restarts ? 'warn' : 'plain'} />
        <Stat label="CPU" value={millicores(found.cpu)} suffix="in use" />
        <Stat label="Memory" value={share !== null ? `${Math.round(share * 100)}%` : bytes(found.memory)} suffix={total ? `${bytes(found.memory)} of ${bytes(total)}` : undefined} tone={share !== null && share > 0.85 ? 'warn' : 'plain'} />
      </div>

      <section aria-labelledby="usage" className="flex flex-col gap-3">
        <div className="flex items-center gap-3">
          <h2 id="usage" className="text-heading flex-1">Usage</h2>
          <Segmented label="Window" value={range} onChange={(value) => set({ range: value === '1 h' ? undefined : value })} options={RANGES.map(([label]) => ({ value: label, label }))} />
        </div>
        <div className="grid gap-3 lg:grid-cols-2">
          {CHARTS.map((chart) => <NodeChart key={chart.metric} chart={chart} node={node} cluster={cluster} rangeMs={rangeMs} />)}
        </div>
      </section>

      <section aria-labelledby="pods-here" className="flex flex-col gap-3">
        <h2 id="pods-here" className="text-heading">Pods on this node</h2>
        <PodTable pods={pods} workloads={view.workloads} by="namespace" sort={sort} desc={desc} prefix="" set={set} />
      </section>
    </>
  )
}
