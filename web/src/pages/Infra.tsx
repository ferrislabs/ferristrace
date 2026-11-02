import { useNavigate as useRouterNavigate } from '@tanstack/react-router'
import { ArrowDown, ArrowUp, Boxes, CircleAlert, CircleCheck, Clock, Layers, RotateCcw, Search, ScrollText, Server, Waypoints, X } from 'lucide-react'
import { Fragment, useEffect, useMemo, useState } from 'react'
import { Link, useSearchParam } from '@/lib/router'
import { Card } from '@/components/ui/card'
import { InputGroup, NativeSelect } from '@/components/ui/input'
import { Segmented } from '@/components/ui/segmented'
import { Table, Td, Th } from '@/components/ui/misc'
import { EmptyState, PageHeader } from '@/components/ft/bits'
import { bytes, collectorConfig, isReporting, useCluster, type ClusterNode, type ClusterPod, type ClusterWorkload } from '@/data/infra'
import { placementSearch } from '@/data/placement'
import { ApiError, because } from '@/auth/client'
import { cn } from '@/lib/utils'

const HERE = '/infra/kubernetes'

export type Grouping = 'node' | 'namespace' | 'workload'
export type SortKey = 'name' | 'restarts' | 'cpu' | 'memory'

export const millicores = (value: number | null) => (value === null ? '—' : `${Math.round(value * 1000)}m`)

const REASON: Record<string, string> = {
  restarting: 'Restarting',
  'not ready': 'Not ready',
  'node not ready': 'Node not ready',
}
export const said = (reason: string) => REASON[reason] ?? reason

export type Health = 'bad' | 'waiting' | 'done' | 'ok'
export function health(pod: ClusterPod): Health {
  if (pod.unhealthy) return pod.phase === 'Pending' && pod.restarts === 0 ? 'waiting' : 'bad'
  if (pod.phase === 'Succeeded') return 'done'
  return 'ok'
}

export const podKey = (pod: ClusterPod) => `${pod.cluster ?? ''}/${pod.namespace}/${pod.name}`
const short = (workload: ClusterWorkload) => workload.desired !== null && workload.ready !== null && workload.ready < workload.desired

function commonPrefix(names: string[]): string {
  if (names.length < 2) return ''
  let prefix = names[0]
  for (const name of names) while (!name.startsWith(prefix)) prefix = prefix.slice(0, -1)
  const cut = Math.max(prefix.lastIndexOf('-'), prefix.lastIndexOf('.'))
  return cut > 0 ? prefix.slice(0, cut + 1) : ''
}

function useKubernetesSearch() {
  const navigate = useRouterNavigate()
  const read = {
    cluster: useSearchParam('cluster') ?? '',
    namespace: useSearchParam('namespace') ?? '',
    by: (useSearchParam('by') as Grouping | undefined) ?? 'node',
    q: useSearchParam('q') ?? '',
    problems: useSearchParam('problems') === '1',
    sort: (useSearchParam('sort') as SortKey | undefined) ?? 'name',
    desc: useSearchParam('dir') === 'desc',
  }
  const set = (part: Record<string, string | undefined>) =>
    void navigate({
      to: HERE as never,
      replace: true,
      search: ((was: Record<string, unknown>) => {
        const next: Record<string, unknown> = { ...was, ...part }
        for (const key of Object.keys(next)) if (next[key] === undefined || next[key] === '') delete next[key]
        return next
      }) as never,
    })
  return { ...read, set }
}

export function Seen({ ms }: { ms: number | null }) {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    const tick = setInterval(() => setNow(Date.now()), 5_000)
    return () => clearInterval(tick)
  }, [])
  if (ms === null) return null
  const seconds = Math.max(0, Math.round((now - ms) / 1000))
  const ago = seconds < 60 ? `${seconds} s ago` : `${Math.round(seconds / 60)} min ago`
  return <span title={new Date(ms).toISOString()}>reported {ago}</span>
}

export function Opens({ pod, quiet }: { pod: ClusterPod; quiet?: boolean }) {
  const where = placementSearch({ pod: pod.name })
  const cls = cn('inline-flex items-center gap-1 text-control no-underline hover:underline', quiet ? 'text-muted-foreground hover:text-foreground' : 'text-accent-text')
  return (
    <span className="inline-flex gap-3">
      <Link to="/logs" search={where} className={cls} aria-label={`Logs from ${pod.name}`}><ScrollText className="size-3.5" aria-hidden />Logs</Link>
      <Link to="/traces" search={where} className={cls} aria-label={`Traces from ${pod.name}`}><Waypoints className="size-3.5" aria-hidden />Traces</Link>
    </span>
  )
}

function Restarts({ pod }: { pod: ClusterPod }) {
  if (pod.restarts_total === 0) return <span className="text-faint-foreground">0</span>
  return (
    <span title={`${pod.restarts_total} restarts in total, ${pod.restarts} in the last 10 minutes`}>
      {pod.restarts_total}
      {pod.restarts > 0 ? <span className="ml-1.5 font-semibold text-destructive-text">+{pod.restarts}</span> : null}
    </span>
  )
}

function StatusCell({ pod }: { pod: ClusterPod }) {
  const state = health(pod)
  if (pod.reason && state !== 'ok') {
    return (
      <span className={cn('inline-flex items-center gap-1.5 font-semibold', state === 'waiting' ? 'text-warning-text' : 'text-destructive-text')}>
        <CircleAlert className="size-3.5" aria-hidden />
        {said(pod.reason)}
        {pod.phase && pod.phase !== said(pod.reason) ? <span className="font-normal text-muted-foreground">· {pod.phase}</span> : null}
      </span>
    )
  }
  return (
    <span className={cn('inline-flex items-center gap-1.5', state === 'done' ? 'text-faint-foreground' : 'text-muted-foreground')}>
      <span aria-hidden className={cn('size-1.5 rounded-full', state === 'done' ? 'bg-status-none' : 'bg-status-ok')} />
      {pod.phase ?? 'Unknown'}
    </span>
  )
}

type Problem =
  | { kind: 'pod'; pod: ClusterPod }
  | { kind: 'workload'; workload: ClusterWorkload }
  | { kind: 'node'; node: ClusterNode }

type Tone = 'ok' | 'warn' | 'bad' | 'plain'
const DOT: Record<Tone, string> = { ok: 'bg-status-ok', warn: 'bg-status-deg', bad: 'bg-status-out', plain: 'bg-series' }
const FIGURE: Record<Tone, string> = { ok: 'text-foreground', warn: 'text-warning-text', bad: 'text-destructive-text', plain: 'text-foreground' }

export function Stat({ label, value, suffix, tone = 'plain' }: { label: string; value: string; suffix?: string; tone?: Tone }) {
  return (
    <div className="flex min-w-0 flex-col gap-2 rounded-lg border bg-card px-4 py-3.5">
      <span className="inline-flex items-center gap-2 font-mono text-[11px] uppercase tracking-[0.06em] text-muted-foreground">
        <span aria-hidden className={cn('size-1.5 rounded-full', DOT[tone])} />
        {label}
      </span>
      <span className="flex items-baseline gap-2">
        <span className={cn('tabular text-[26px] font-semibold leading-none tracking-[-0.01em]', FIGURE[tone])}>{value}</span>
        {suffix ? <span className="truncate text-control text-muted-foreground">{suffix}</span> : null}
      </span>
    </div>
  )
}

function Attention({ problems, prefix, onPick }: { problems: Problem[]; prefix: string; onPick: (q: string) => void }) {
  return (
    <section aria-labelledby="attention" className="flex min-w-0 flex-col rounded-lg border bg-card">
      <div className="flex items-center gap-2 px-4 pt-4 pb-2">
        <h2 id="attention" className="text-heading flex-1">Needs attention</h2>
        {problems.length ? <span className="tabular rounded-full bg-destructive-subtle px-2 py-0.5 text-caption font-semibold text-destructive-text">{problems.length}</span> : null}
      </div>
      {!problems.length ? (
        <div className="flex items-center gap-3 px-4 pt-2 pb-5">
          <span className="flex size-8 shrink-0 items-center justify-center rounded-md bg-success-subtle text-success-text"><CircleCheck className="size-4" aria-hidden /></span>
          <span className="text-control text-muted-foreground">Every node is ready and every pod is healthy.</span>
        </div>
      ) : (
        <ul className="m-0 flex list-none flex-col p-2 pt-0">
          {problems.map((problem) => {
            const row = 'flex items-start gap-3 rounded-md px-2 py-2.5'
            if (problem.kind === 'pod') {
              const { pod } = problem
              const waiting = health(pod) === 'waiting'
              return (
                <li key={`pod:${podKey(pod)}`} className={row}>
                  <span className={cn('mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-md', waiting ? 'bg-warning-subtle text-warning-text' : 'bg-destructive-subtle text-destructive-text')}>
                    {waiting ? <Clock className="size-4" aria-hidden /> : <RotateCcw className="size-4" aria-hidden />}
                  </span>
                  <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                    <button type="button" onClick={() => onPick(pod.name)} className="cursor-pointer truncate bg-transparent p-0 text-left text-control font-semibold text-foreground hover:underline">
                      {pod.workload?.name ?? pod.name} <span className={waiting ? 'text-warning-text' : 'text-destructive-text'}>{said(pod.reason ?? 'unhealthy').toLowerCase()}</span>
                    </button>
                    <span className="truncate text-caption text-muted-foreground">
                      {pod.namespace}{pod.node ? ` · ${pod.node.slice(prefix.length) || pod.node}` : ''}{pod.restarts ? ` · ${pod.restarts} restart${pod.restarts === 1 ? '' : 's'} in 10 min` : ''}
                    </span>
                    <span className="pt-1"><Opens pod={pod} /></span>
                  </span>
                </li>
              )
            }
            if (problem.kind === 'workload') {
              const { workload } = problem
              return (
                <li key={`workload:${workload.cluster ?? ''}/${workload.namespace}/${workload.kind}/${workload.name}`} className={row}>
                  <span className="mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-md bg-warning-subtle text-warning-text"><Layers className="size-4" aria-hidden /></span>
                  <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                    <button type="button" onClick={() => onPick(workload.name)} className="cursor-pointer truncate bg-transparent p-0 text-left text-control font-semibold text-foreground hover:underline">
                      {workload.name} <span className="text-warning-text">short of replicas</span>
                    </button>
                    <span className="truncate text-caption text-muted-foreground">{workload.kind} · {workload.namespace} · {workload.ready} of {workload.desired} ready</span>
                  </span>
                </li>
              )
            }
            const { node } = problem
            return (
              <li key={`node:${node.name}`} className={row}>
                <span className="mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-md bg-destructive-subtle text-destructive-text"><Server className="size-4" aria-hidden /></span>
                <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                  <span className="truncate text-control font-semibold">{node.name} <span className="text-destructive-text">not ready</span></span>
                  <span className="text-caption text-muted-foreground">{node.pods} pod{node.pods === 1 ? '' : 's'} scheduled on it</span>
                </span>
              </li>
            )
          })}
        </ul>
      )}
    </section>
  )
}

const CELL: Record<Health, string> = {
  ok: 'bg-status-ok/75 hover:bg-status-ok',
  bad: 'bg-status-out',
  waiting: 'bg-status-deg',
  done: 'bg-status-none',
}
const ORDER: Record<Health, number> = { bad: 0, waiting: 1, ok: 2, done: 3 }

export function Usage({ label, text, share }: { label: string; text: string; share: number | null }) {
  return (
    <span className="grid grid-cols-[36px_minmax(0,1fr)_auto] items-center gap-2 font-mono text-[11px]">
      <span className="uppercase tracking-[0.06em] text-muted-foreground">{label}</span>
      {share !== null ? (
        <span className="h-1 overflow-hidden rounded-full bg-muted" role="img" aria-label={`${label} ${Math.round(share * 100)} % used`}>
          <span className={cn('block h-full rounded-full', share > 0.9 ? 'bg-status-out' : share > 0.75 ? 'bg-status-deg' : 'bg-series')} style={{ width: `${Math.max(2, share * 100)}%` }} />
        </span>
      ) : <span />}
      <span className="tabular text-right text-foreground">{text}</span>
    </span>
  )
}

function NodeCard({ name, label, node, pods, picked, onPick }: { name: string; label: string; node: ClusterNode | null; pods: ClusterPod[]; picked: string; onPick: (q: string) => void }) {
  const ordered = [...pods].sort((a, b) => ORDER[health(a)] - ORDER[health(b)] || a.name.localeCompare(b.name))
  const ill = ordered.filter((pod) => health(pod) === 'bad').length
  const down = node?.ready === false
  const total = node && node.memory !== null && node.memory_available !== null ? node.memory + node.memory_available : null
  const share = total && node?.memory !== null && node?.memory !== undefined ? node.memory / total : null
  return (
    <div className={cn('flex min-w-0 flex-col gap-3.5 rounded-lg border bg-card p-4', (down || ill) && 'border-destructive-border')}>
      <div className="flex items-baseline gap-2">
        {name ? (
          <Link to={`${HERE}/nodes/${encodeURIComponent(name)}`} title={`Open ${name}`} className="min-w-0 flex-1 truncate text-control font-semibold text-foreground no-underline hover:underline">{label}</Link>
        ) : <span className="min-w-0 flex-1 truncate text-control font-semibold">{label}</span>}
        {down ? <span className="text-caption font-semibold text-destructive-text">not ready</span> : null}
        <span className="tabular shrink-0 font-mono text-[11px] text-muted-foreground">{ordered.length} pod{ordered.length === 1 ? '' : 's'}</span>
      </div>
      <ul aria-label={`Pods on ${label}`} className="m-0 flex min-h-[38px] list-none flex-wrap content-start gap-1 p-0">
        {ordered.map((pod) => {
          const on = picked === pod.name
          const status = pod.reason ? said(pod.reason) : (pod.phase ?? 'Unknown')
          return (
            <li key={podKey(pod)}>
              <button
                type="button"
                onClick={() => onPick(on ? '' : pod.name)}
                aria-pressed={on}
                aria-label={`${pod.name}, ${status}`}
                title={`${pod.name}\n${pod.namespace} · ${status}${pod.restarts ? ` · ${pod.restarts} restarts in 10 min` : ''}`}
                className={cn('block size-[15px] cursor-pointer rounded-[3px] transition-[background-color] focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-ring', CELL[health(pod)], on && 'outline-2 outline-offset-1 outline-ring')}
              />
            </li>
          )
        })}
      </ul>
      {node ? (
        <span className="flex flex-col gap-1.5">
          <Usage label="CPU" text={millicores(node.cpu)} share={null} />
          <Usage label="Mem" text={share !== null ? `${Math.round(share * 100)}%` : bytes(node.memory)} share={share} />
        </span>
      ) : null}
    </div>
  )
}

function NodePool({ nodes, pods, prefix, picked, onPick }: { nodes: ClusterNode[]; pods: ClusterPod[]; prefix: string; picked: string; onPick: (q: string) => void }) {
  const byNode = useMemo(() => {
    const map = new Map<string, ClusterPod[]>()
    for (const pod of pods) map.set(pod.node ?? '', [...(map.get(pod.node ?? '') ?? []), pod])
    return map
  }, [pods])
  const unscheduled = byNode.get('') ?? []
  const tiles: [string, ClusterNode | null, ClusterPod[]][] = [
    ...nodes.map((node) => [node.name, node, byNode.get(node.name) ?? []] as [string, ClusterNode, ClusterPod[]]),
    ...(unscheduled.length ? [['', null, unscheduled] as [string, null, ClusterPod[]]] : []),
  ]
  const counts = { ok: 0, bad: 0, waiting: 0, done: 0 }
  for (const pod of pods) counts[health(pod)]++
  return (
    <section aria-labelledby="nodes" className="flex min-w-0 flex-col gap-4 rounded-lg border bg-card p-4">
      <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
        <h2 id="nodes" className="text-heading">Nodes</h2>
        {prefix ? <span className="font-mono text-caption text-muted-foreground">{prefix}*</span> : null}
        <span className="flex-1" />
        <span className="text-caption text-muted-foreground">each square is a pod</span>
      </div>
      <div className="grid grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-3">
        {tiles.map(([name, node, here]) => (
          <NodeCard key={name || 'unscheduled'} name={name} label={name ? name.slice(prefix.length) || name : 'Not scheduled'} node={node} pods={here} picked={picked} onPick={onPick} />
        ))}
      </div>
      <ul className="m-0 flex list-none flex-wrap gap-x-5 gap-y-1 p-0 text-caption text-muted-foreground">
        {([['ok', 'Healthy'], ['bad', 'Unhealthy'], ['waiting', 'Pending'], ['done', 'Completed']] as [Health, string][]).map(([state, text]) => (
          <li key={state} className="inline-flex items-center gap-1.5">
            <span aria-hidden className={cn('size-2.5 rounded-[2px]', CELL[state])} />
            {text}<span className="tabular font-semibold text-foreground">{counts[state]}</span>
          </li>
        ))}
      </ul>
    </section>
  )
}

function SortHeader({ label, column, sort, desc, onSort, className }: { label: string; column: SortKey; sort: SortKey; desc: boolean; onSort: (column: SortKey) => void; className?: string }) {
  const active = sort === column
  return (
    <Th numeric={column !== 'name'} className={className} aria-sort={active ? (desc ? 'descending' : 'ascending') : 'none'}>
      <button type="button" onClick={() => onSort(column)} className={cn('inline-flex cursor-pointer items-center gap-1 bg-transparent p-0 font-semibold', active ? 'text-foreground' : 'text-muted-foreground hover:text-foreground')}>
        {label}
        {active ? (desc ? <ArrowDown className="size-3" aria-hidden /> : <ArrowUp className="size-3" aria-hidden />) : null}
      </button>
    </Th>
  )
}

function compare(sort: SortKey, desc: boolean) {
  const sign = desc ? -1 : 1
  return (a: ClusterPod, b: ClusterPod) => {
    const rank = (pod: ClusterPod) => (health(pod) === 'bad' ? 0 : health(pod) === 'waiting' ? 1 : 2)
    if (sort === 'name') return rank(a) - rank(b) || sign * a.name.localeCompare(b.name)
    const value = (pod: ClusterPod) => (sort === 'restarts' ? pod.restarts_total : sort === 'cpu' ? pod.cpu ?? -1 : pod.memory ?? -1)
    return sign * (value(a) - value(b)) || a.name.localeCompare(b.name)
  }
}

function groupOf(pod: ClusterPod, by: Grouping): string {
  if (by === 'node') return pod.node ?? ''
  if (by === 'namespace') return pod.namespace
  return pod.workload ? `${pod.namespace}/${pod.workload.kind}/${pod.workload.name}` : ''
}

export function PodTable({ pods, workloads, by, sort, desc, prefix, set }: { pods: ClusterPod[]; workloads: ClusterWorkload[]; by: Grouping; sort: SortKey; desc: boolean; prefix: string; set: (part: Record<string, string | undefined>) => void }) {
  const groups = useMemo(() => {
    const map = new Map<string, ClusterPod[]>()
    for (const pod of [...pods].sort(compare(sort, desc))) {
      const key = groupOf(pod, by)
      map.set(key, [...(map.get(key) ?? []), pod])
    }
    const ill = (list: ClusterPod[]) => list.some((pod) => health(pod) === 'bad')
    return [...map.entries()].sort(([a, x], [b, y]) => Number(ill(y)) - Number(ill(x)) || (a === '' ? 1 : b === '' ? -1 : a.localeCompare(b)))
  }, [pods, by, sort, desc])
  const workloadOf = (key: string) => workloads.find((w) => `${w.namespace}/${w.kind}/${w.name}` === key)
  const onSort = (column: SortKey) => set(column === sort ? { dir: desc ? undefined : 'desc' } : { sort: column === 'name' ? undefined : column, dir: column === 'name' ? undefined : 'desc' })

  return (
    <Card className="overflow-x-auto">
      <Table aria-label="Pods" className="min-w-[760px]">
        <thead>
          <tr>
            <SortHeader label="Pod" column="name" sort={sort} desc={desc} onSort={onSort} className="min-w-[280px]" />
            <Th className="w-[220px]">Status</Th>
            <SortHeader label="Restarts" column="restarts" sort={sort} desc={desc} onSort={onSort} className="w-[100px]" />
            <SortHeader label="CPU" column="cpu" sort={sort} desc={desc} onSort={onSort} className="w-[80px]" />
            <SortHeader label="Memory" column="memory" sort={sort} desc={desc} onSort={onSort} className="w-[90px]" />
            <Th className="w-[150px]"><span className="sr-only">Open</span></Th>
          </tr>
        </thead>
        {groups.map(([key, list]) => {
          const workload = by === 'workload' ? workloadOf(key) : undefined
          const title = !key ? (by === 'node' ? 'Not scheduled' : by === 'workload' ? 'No workload' : 'No namespace')
            : by === 'node' ? key.slice(prefix.length) || key
              : by === 'workload' && workload ? `${workload.name}` : key
          return (
            <tbody key={key || 'none'}>
              <tr>
                <th scope="rowgroup" colSpan={6} className="h-8 border-b bg-muted px-3 text-left text-caption font-semibold">
                  {by === 'node' && key ? <Link to={`${HERE}/nodes/${encodeURIComponent(key)}`} className="font-mono text-foreground no-underline hover:underline">{title}</Link> : <span className="font-mono">{title}</span>}
                  {workload ? <span className="ml-2 font-normal text-muted-foreground">{workload.kind} in {workload.namespace}</span> : null}
                  <span className={cn('ml-2 font-normal', workload && short(workload) ? 'font-semibold text-warning-text' : 'text-muted-foreground')}>
                    {workload && workload.desired !== null ? `${workload.ready ?? 0} of ${workload.desired} ready` : `${list.length} pod${list.length === 1 ? '' : 's'}`}
                  </span>
                </th>
              </tr>
              {list.map((pod) => {
                const state = health(pod)
                return (
                  <tr key={podKey(pod)} className={cn('group hover:[&>td]:bg-row-hover', state === 'bad' && '[&>td]:bg-destructive-subtle/60')}>
                    <Td className="max-w-0">
                      <span className="flex min-w-0 flex-col leading-5">
                        <span className="truncate font-mono font-semibold">{pod.name}</span>
                        <span className="truncate text-caption text-muted-foreground">
                          {[by !== 'namespace' ? pod.namespace : null, by !== 'workload' && pod.workload ? pod.workload.name : null, by !== 'node' && pod.node ? pod.node.slice(prefix.length) || pod.node : null].filter(Boolean).join(' · ')}
                          {pod.services.length ? <> · serves {pod.services.map((service, i) => <Fragment key={service}>{i ? ', ' : ''}<Link to={`/services/${encodeURIComponent(service)}`} className="text-accent-text">{service}</Link></Fragment>)}</> : null}
                        </span>
                      </span>
                    </Td>
                    <Td><StatusCell pod={pod} /></Td>
                    <Td numeric><Restarts pod={pod} /></Td>
                    <Td numeric>{millicores(pod.cpu)}</Td>
                    <Td numeric>{bytes(pod.memory)}</Td>
                    <Td><Opens pod={pod} quiet={state === 'ok' || state === 'done'} /></Td>
                  </tr>
                )
              })}
            </tbody>
          )
        })}
      </Table>
      {!groups.length ? <p className="m-0 px-3 py-6 text-center text-control text-muted-foreground">No pod matches.</p> : null}
    </Card>
  )
}

function NotReporting() {
  return (
    <EmptyState icon={<Boxes />} title="Kubernetes is not reporting" code={collectorConfig(window.location.origin)}>
      No pod, node or workload metric arrived in the last 10 minutes. Add the k8s_cluster receiver (one replica) and the kubeletstats receiver (a DaemonSet) to your OpenTelemetry Collector, with the k8sattributes processor, and export to FerrisTrace over OTLP/HTTP with an ingest key. Set k8s.cluster.name to tell clusters apart.
    </EmptyState>
  )
}

export function Kubernetes() {
  const search = useKubernetesSearch()
  const { cluster, namespace, by, q, problems: problemsOnly, sort, desc, set } = search
  const { view, pending, failed } = useCluster(cluster, namespace)
  const [typed, setTyped] = useState(q)
  useEffect(() => setTyped(q), [q])
  useEffect(() => {
    if (typed === q) return
    const wait = setTimeout(() => set({ q: typed || undefined }), 200)
    return () => clearTimeout(wait)
  }, [typed])

  const prefix = useMemo(() => commonPrefix(view.nodes.map((node) => node.name)), [view.nodes])
  const problems = useMemo<Problem[]>(() => {
    const ill = view.pods.filter((pod) => pod.unhealthy).sort((a, b) => b.restarts - a.restarts || a.name.localeCompare(b.name))
    const covered = new Set(ill.map((pod) => (pod.workload ? `${pod.namespace}/${pod.workload.name}` : '')))
    return [
      ...view.nodes.filter((node) => node.ready === false).map((node) => ({ kind: 'node' as const, node })),
      ...ill.map((pod) => ({ kind: 'pod' as const, pod })),
      ...view.workloads.filter((w) => short(w) && !covered.has(`${w.namespace}/${w.name}`) && !ill.some((pod) => pod.namespace === w.namespace && pod.name.startsWith(`${w.name}-`))).map((workload) => ({ kind: 'workload' as const, workload })),
    ]
  }, [view])
  const shown = useMemo(() => {
    const needle = q.trim().toLowerCase()
    return view.pods.filter((pod) => {
      if (problemsOnly && !pod.unhealthy) return false
      if (!needle) return true
      return [pod.name, pod.namespace, pod.node, pod.workload?.name, ...pod.services].some((part) => part?.toLowerCase().includes(needle))
    })
  }, [view.pods, q, problemsOnly])
  const ill = view.pods.filter((pod) => pod.unhealthy).length
  const downNodes = view.nodes.filter((node) => node.ready === false).length
  const restarts = view.pods.reduce((sum, pod) => sum + pod.restarts, 0)
  const memoryUsed = view.nodes.reduce((sum, node) => sum + (node.memory ?? 0), 0)
  const memoryTotal = view.nodes.reduce((sum, node) => sum + (node.memory !== null && node.memory_available !== null ? node.memory + node.memory_available : 0), 0)
  const memoryShare = memoryTotal ? memoryUsed / memoryTotal : null

  const header = (
    <PageHeader
      title="Kubernetes"
      meta={view.seen_ms ? (
        <span className="inline-flex flex-wrap items-center gap-x-1.5">
          <span className={cn(problems.length ? 'font-semibold text-destructive-text' : 'text-success-text')}>{problems.length ? `${problems.length} problem${problems.length === 1 ? '' : 's'}` : 'Healthy'}</span>
          <span aria-hidden>·</span><span>{view.pods.length} pods</span>
          <span aria-hidden>·</span><span>{view.nodes.length} nodes</span>
          <span aria-hidden>·</span><Seen ms={view.seen_ms} />
        </span>
      ) : undefined}
    >
      <NativeSelect aria-label="Cluster" value={cluster} onChange={(e) => set({ cluster: e.target.value || undefined, namespace: undefined })} className="w-[170px] font-mono">
        <option value="">All clusters</option>
        {view.clusters.map((name) => <option key={name} value={name}>{name}</option>)}
      </NativeSelect>
      <NativeSelect aria-label="Namespace" value={namespace} onChange={(e) => set({ namespace: e.target.value || undefined })} className="w-[170px]">
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

  return (
    <>
      {header}
      <p className="sr-only" aria-live="polite">{problems.length ? `${problems.length} problems in the cluster` : 'The cluster is healthy'}</p>
      <div className="grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-6">
        <Stat label="Nodes ready" value={`${view.nodes.length - downNodes}/${view.nodes.length}`} tone={downNodes ? 'bad' : 'ok'} />
        <Stat label="Pods" value={String(view.pods.length)} suffix={`${view.namespaces.length} namespaces`} />
        <Stat label="Healthy" value={String(view.pods.length - ill)} suffix="pods" tone="ok" />
        <Stat label="Unhealthy" value={String(ill)} suffix={ill ? 'pods' : 'none'} tone={ill ? 'bad' : 'ok'} />
        <Stat label="Restarts 10 min" value={String(restarts)} tone={restarts ? 'warn' : 'plain'} />
        <Stat label="Memory" value={memoryShare !== null ? `${Math.round(memoryShare * 100)}%` : '—'} suffix={memoryTotal ? `of ${bytes(memoryTotal)}` : undefined} tone={memoryShare !== null && memoryShare > 0.85 ? 'warn' : 'plain'} />
      </div>
      <div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_340px]">
        <NodePool nodes={view.nodes} pods={view.pods} prefix={prefix} picked={q} onPick={(name) => set({ q: name || undefined })} />
        <Attention problems={problems} prefix={prefix} onPick={(name) => set({ q: name })} />
      </div>

      <section aria-labelledby="pods" className="flex flex-col gap-3">
        <div className="flex flex-wrap items-center gap-2">
          <h2 id="pods" className="text-heading mr-2">Pods</h2>
          <InputGroup className="w-[260px]">
            <Search aria-hidden />
            <input aria-label="Filter pods" placeholder="Filter by pod, workload, node, service" value={typed} onChange={(e) => setTyped(e.target.value)} />
            {typed ? <button type="button" aria-label="Clear the filter" onClick={() => { setTyped(''); set({ q: undefined }) }} className="cursor-pointer bg-transparent p-0 text-muted-foreground hover:text-foreground"><X className="size-3.5" aria-hidden /></button> : null}
          </InputGroup>
          <button
            type="button"
            aria-pressed={problemsOnly}
            onClick={() => set({ problems: problemsOnly ? undefined : '1' })}
            className={cn('inline-flex h-8 cursor-pointer items-center gap-1.5 rounded-md border px-2.5 text-control font-semibold focus-visible:outline-2 focus-visible:outline-ring', problemsOnly ? 'border-destructive-border bg-destructive-subtle text-destructive-text' : 'bg-background text-muted-foreground hover:text-foreground')}
          >
            <CircleAlert className="size-3.5" aria-hidden />
            Unhealthy only{ill ? ` (${ill})` : ''}
          </button>
          <span className="flex-1" />
          <Segmented label="Group pods by" value={by} onChange={(value) => set({ by: value === 'node' ? undefined : value })} options={[{ value: 'node', label: 'Node' }, { value: 'namespace', label: 'Namespace' }, { value: 'workload', label: 'Workload' }]} />
        </div>
        <PodTable pods={shown} workloads={view.workloads} by={by} sort={sort} desc={desc} prefix={prefix} set={set} />
      </section>
    </>
  )
}
