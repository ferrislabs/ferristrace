import { Boxes, CircleCheck, CircleX, Clock, ScrollText, Server, Waypoints } from 'lucide-react'
import { Fragment, useState } from 'react'
import { Link } from '@/lib/router'
import { Badge } from '@/components/ui/badge'
import { Card } from '@/components/ui/card'
import { NativeSelect } from '@/components/ui/input'
import { Segmented } from '@/components/ui/segmented'
import { Table, Td, Th } from '@/components/ui/misc'
import { EmptyState, Kpi, PageHeader } from '@/components/ft/bits'
import { bytes, collectorConfig, cores, isReporting, useCluster, type ClusterNode, type ClusterPod } from '@/data/infra'
import { placementSearch } from '@/data/placement'
import { ApiError, because } from '@/auth/client'
import { stamp } from '@/lib/format'
import { cn } from '@/lib/utils'

type Grouping = 'node' | 'namespace'

const PHASE: Record<string, 'success' | 'warning' | 'destructive' | 'outline'> = {
  Running: 'success',
  Succeeded: 'outline',
  Pending: 'warning',
  Failed: 'destructive',
  Unknown: 'warning',
}

function Ready({ ready }: { ready: boolean | null }) {
  if (ready === null) return <Badge variant="outline">no condition</Badge>
  return ready ? <Badge variant="success"><CircleCheck />Ready</Badge> : <Badge variant="destructive"><CircleX />Not ready</Badge>
}

function NodeCard({ node }: { node: ClusterNode }) {
  const total = node.memory !== null && node.memory_available !== null ? node.memory + node.memory_available : null
  const used = total ? Math.round(((node.memory ?? 0) / total) * 100) : null
  return (
    <Card className={cn('flex flex-col gap-2 p-3', node.ready === false && 'border-destructive-border')}>
      <div className="flex items-center gap-2">
        <Server className="size-4 text-muted-foreground" aria-hidden />
        <span className="min-w-0 flex-1 truncate font-mono font-semibold">{node.name}</span>
        <Ready ready={node.ready} />
      </div>
      <dl className="m-0 grid grid-cols-3 gap-2 text-caption">
        <div><dt className="text-muted-foreground">CPU</dt><dd className="m-0 font-mono">{cores(node.cpu)}</dd></div>
        <div><dt className="text-muted-foreground">Memory</dt><dd className="m-0 font-mono">{bytes(node.memory)}{used !== null ? ` · ${used} %` : ''}</dd></div>
        <div><dt className="text-muted-foreground">Pods</dt><dd className="m-0 font-mono">{node.pods}</dd></div>
      </dl>
    </Card>
  )
}

function PodRow({ pod }: { pod: ClusterPod }) {
  const where = placementSearch({ pod: pod.name })
  return (
    <tr className={cn('hover:[&>td]:bg-row-hover', pod.unhealthy && '[&>td]:bg-destructive-subtle')}>
      <Td>
        <span className="flex flex-col leading-5">
          <span className="inline-flex items-center gap-1.5 font-mono font-semibold">{pod.unhealthy ? <CircleX className="size-3.5 text-destructive-text" aria-label="Unhealthy" /> : null}{pod.name}</span>
          <span className="truncate text-caption text-muted-foreground">{pod.namespace}{pod.workload ? ` · ${pod.workload.kind}:${pod.workload.name}` : ''}{pod.node ? ` · ${pod.node}` : ''}</span>
        </span>
      </Td>
      <Td>
        <span className="flex flex-col gap-0.5">
          {pod.phase ? <Badge variant={PHASE[pod.phase] ?? 'outline'} className="w-fit">{pod.phase === 'Pending' ? <Clock /> : null}{pod.phase}</Badge> : <span className="text-faint-foreground">—</span>}
          {pod.reason ? <span className="text-caption font-semibold text-destructive-text">{pod.reason}</span> : null}
        </span>
      </Td>
      <Td numeric className={cn(pod.restarts > 0 && 'font-semibold text-destructive-text')}>{pod.restarts > 0 ? `+${pod.restarts} · ` : ''}{pod.restarts_total}</Td>
      <Td numeric>{cores(pod.cpu)}</Td>
      <Td numeric>{bytes(pod.memory)}</Td>
      <Td>
        {pod.services.length ? (
          <span className="flex flex-wrap gap-1.5">{pod.services.map((service) => <Link key={service} to={`/services/${encodeURIComponent(service)}`} className="font-mono text-control">{service}</Link>)}</span>
        ) : <span className="text-faint-foreground">—</span>}
      </Td>
      <Td>
        <span className="inline-flex gap-2">
          <Link to="/traces" search={where} className="inline-flex items-center gap-1 text-control" aria-label={`Traces from ${pod.name}`}><Waypoints className="size-3.5" aria-hidden />Traces</Link>
          <Link to="/logs" search={where} className="inline-flex items-center gap-1 text-control" aria-label={`Logs from ${pod.name}`}><ScrollText className="size-3.5" aria-hidden />Logs</Link>
        </span>
      </Td>
    </tr>
  )
}

function grouped(pods: ClusterPod[], by: Grouping): [string, ClusterPod[]][] {
  const groups = new Map<string, ClusterPod[]>()
  for (const pod of pods) {
    const key = (by === 'node' ? pod.node : pod.namespace) ?? ''
    groups.set(key, [...(groups.get(key) ?? []), pod])
  }
  return [...groups.entries()].sort(([a], [b]) => (a === '' ? 1 : b === '' ? -1 : a.localeCompare(b)))
}

function NotReporting() {
  return (
    <EmptyState icon={<Boxes />} title="Kubernetes is not reporting" code={collectorConfig(window.location.origin)}>
      No pod, node or workload metric arrived in the last 10 minutes. Add the k8s_cluster receiver (one replica) and the kubeletstats receiver (a DaemonSet) to your OpenTelemetry Collector, with the k8sattributes processor, and export to FerrisTrace over OTLP/HTTP with an ingest key. Set k8s.cluster.name to tell clusters apart.
    </EmptyState>
  )
}

export function Kubernetes() {
  const [cluster, setCluster] = useState('')
  const [namespace, setNamespace] = useState('')
  const [by, setBy] = useState<Grouping>('node')
  const { view, pending, failed } = useCluster(cluster, namespace)

  const header = (
    <PageHeader title="Kubernetes" meta={view.seen_ms ? `From the Collector's k8s_cluster and kubeletstats receivers · last report ${stamp(view.seen_ms)}` : 'From the Collector\'s k8s_cluster and kubeletstats receivers'}>
      <NativeSelect aria-label="Cluster" value={cluster} onChange={(e) => { setCluster(e.target.value); setNamespace('') }} className="w-[170px] font-mono">
        <option value="">All clusters</option>
        {view.clusters.map((name) => <option key={name} value={name}>{name}</option>)}
      </NativeSelect>
      <NativeSelect aria-label="Namespace" value={namespace} onChange={(e) => setNamespace(e.target.value)} className="w-[170px]">
        <option value="">All namespaces</option>
        {view.namespaces.map((name) => <option key={name} value={name}>{name}</option>)}
      </NativeSelect>
    </PageHeader>
  )

  if (failed) {
    const unconfigured = failed instanceof ApiError && failed.status === 503
    return (
      <>
        {header}
        <EmptyState icon={<Boxes />} title={unconfigured ? 'No metric store is configured' : 'The cluster could not be read'}>{because(failed, 'Try again in a moment.')}</EmptyState>
      </>
    )
  }
  if (pending) return header
  if (!isReporting(view) && !cluster && !namespace) {
    return (
      <>
        {header}
        <NotReporting />
      </>
    )
  }

  const unhealthy = view.pods.filter((pod) => pod.unhealthy)
  const notReady = view.nodes.filter((node) => node.ready === false)
  const restarting = view.pods.reduce((sum, pod) => sum + pod.restarts, 0)
  return (
    <>
      {header}
      <div className="grid grid-cols-[repeat(auto-fit,minmax(170px,1fr))] gap-3">
        <Kpi label="Nodes" value={`${view.nodes.length - notReady.length} / ${view.nodes.length}`} sub={notReady.length ? `${notReady.length} not ready` : 'all ready'} tone={notReady.length ? 'err' : 'ok'} />
        <Kpi label="Pods" value={String(view.pods.length)} sub={unhealthy.length ? `${unhealthy.length} unhealthy` : 'all healthy'} tone={unhealthy.length ? 'warn' : 'ok'} />
        <Kpi label="Workloads" value={String(view.workloads.length)} />
        <Kpi label="Restarts, 10 min" value={String(restarting)} tone={restarting ? 'warn' : undefined} />
      </div>

      <section aria-label="Nodes" className="flex flex-col gap-2">
        <h2 className="text-heading">Nodes</h2>
        {view.nodes.length ? (
          <div className="grid grid-cols-[repeat(auto-fill,minmax(260px,1fr))] gap-3">{view.nodes.map((node) => <NodeCard key={node.name} node={node} />)}</div>
        ) : <p className="m-0 text-caption text-muted-foreground">No node reported in the last 10 minutes.</p>}
      </section>

      <section aria-label="Workloads" className="flex flex-col gap-2">
        <h2 className="text-heading">Workloads</h2>
        <Card className="overflow-x-auto">
          <Table aria-label="Workloads">
            <thead><tr><Th>Workload</Th>{cluster ? null : <Th className="w-[140px]">Cluster</Th>}<Th className="w-[140px]">Namespace</Th><Th className="w-[120px]">Kind</Th><Th numeric className="w-[110px]">Ready / desired</Th><Th numeric className="w-[80px]">Pods</Th></tr></thead>
            <tbody>{view.workloads.map((workload) => {
              const short = workload.desired !== null && workload.ready !== null && workload.ready < workload.desired
              return (
                <tr key={`${workload.cluster ?? ''}/${workload.namespace}/${workload.kind}/${workload.name}`} className="hover:[&>td]:bg-row-hover">
                  <Td className="font-mono font-semibold">{workload.name}</Td>
                  {cluster ? null : <Td className="font-mono text-control">{workload.cluster ?? '—'}</Td>}
                  <Td className="text-control">{workload.namespace}</Td>
                  <Td className="text-control text-muted-foreground">{workload.kind}</Td>
                  <Td numeric className={cn(short && 'font-semibold text-warning-text')}>{workload.ready ?? '—'} / {workload.desired ?? '—'}</Td>
                  <Td numeric>{workload.pods}</Td>
                </tr>
              )
            })}</tbody>
          </Table>
        </Card>
      </section>

      <section aria-label="Pods" className="flex flex-col gap-2">
        <div className="flex items-center gap-3">
          <h2 className="text-heading flex-1">Pods</h2>
          <Segmented label="Group pods by" value={by} onChange={setBy} options={[{ value: 'node', label: 'By node' }, { value: 'namespace', label: 'By namespace' }]} />
        </div>
        <Card className="overflow-x-auto">
          <Table aria-label="Pods">
            <thead><tr><Th>Pod</Th><Th className="w-[150px]">Phase</Th><Th numeric className="w-[100px]">Restarts</Th><Th numeric className="w-[90px]">CPU</Th><Th numeric className="w-[90px]">Memory</Th><Th className="w-[180px]">Services</Th><Th className="w-[150px]">Open</Th></tr></thead>
            <tbody>
              {grouped(view.pods, by).map(([key, pods]) => (
                <Fragment key={key || 'none'}>
                  <tr><Td colSpan={7} className="bg-muted font-mono text-caption font-semibold">{key || (by === 'node' ? 'not scheduled' : 'no namespace')} · {pods.length} pod{pods.length === 1 ? '' : 's'}</Td></tr>
                  {pods.map((pod) => <PodRow key={`${pod.cluster ?? ''}/${pod.namespace}/${pod.name}`} pod={pod} />)}
                </Fragment>
              ))}
            </tbody>
          </Table>
        </Card>
      </section>
      <p className="m-0 text-caption text-muted-foreground">Restarts show the growth over the last 10 minutes and the total. A pod is unhealthy when it is not Running or Succeeded, restarted in the last 10 minutes, is not ready, or runs on a node that is not ready.</p>
    </>
  )
}
