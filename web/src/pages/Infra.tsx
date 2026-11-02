import { Box, Boxes, CircleCheck, CircleX, Clock, ScrollText, Server, TriangleAlert, Waypoints, X } from 'lucide-react'
import { useEffect, useState } from 'react'
import { Link } from '@/lib/router'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import { NativeSelect } from '@/components/ui/input'
import { Segmented } from '@/components/ui/segmented'
import { Table, Td, Th } from '@/components/ui/misc'
import { EmptyState, Kpi, PageHeader } from '@/components/ft/bits'
import { LineChart } from '@/components/ft/charts'
import { useDocker, useKubernetes, type CStatus, type PodStatus } from '@/data/infra'
import { cn } from '@/lib/utils'

function Bar({ pct, warn = 80 }: { pct: number; warn?: number }) {
  return (
    <span className="inline-flex items-center gap-2">
      <span className="inline-flex h-1.5 w-16 overflow-hidden rounded bg-muted"><span className={cn('rounded', pct >= 95 ? 'bg-status-out' : pct >= warn ? 'bg-status-deg' : 'bg-chart-line')} style={{ width: `${Math.min(100, pct)}%` }} /></span>
      <span className={cn('w-9 font-mono text-caption', pct >= warn && 'font-semibold text-warning-text', pct >= 95 && 'text-destructive-text')}>{pct} %</span>
    </span>
  )
}
const POD: Record<PodStatus, { v: 'success' | 'warning' | 'destructive' | 'outline'; icon: React.ReactNode }> = {
  Running: { v: 'success', icon: <CircleCheck /> }, Pending: { v: 'warning', icon: <Clock /> }, CrashLoopBackOff: { v: 'destructive', icon: <CircleX /> }, OOMKilled: { v: 'destructive', icon: <CircleX /> }, Completed: { v: 'outline', icon: <CircleCheck /> },
}
const CS: Record<CStatus, 'success' | 'warning' | 'destructive' | 'outline'> = { running: 'success', restarting: 'warning', exited: 'outline', unhealthy: 'destructive' }

function SidePanel({ title, sub, onClose, children }: { title: string; sub?: React.ReactNode; onClose: () => void; children: React.ReactNode }) {
  useEffect(() => {
    const k = (e: KeyboardEvent) => e.key === 'Escape' && onClose()
    window.addEventListener('keydown', k)
    return () => window.removeEventListener('keydown', k)
  }, [onClose])
  return (
    <aside className="fixed inset-y-0 right-0 z-40 flex w-[440px] max-w-full flex-col border-l bg-card shadow-lg" aria-label={title}>
      <div className="flex items-start gap-2 border-b px-4 py-3">
        <div className="flex min-w-0 flex-1 flex-col"><span className="truncate font-mono font-semibold">{title}</span>{sub ? <span className="text-caption text-muted-foreground">{sub}</span> : null}</div>
        <Button variant="ghost" size="icon-sm" aria-label="Close" onClick={onClose}><X /></Button>
      </div>
      <div className="flex flex-col gap-4 overflow-y-auto p-4">{children}</div>
    </aside>
  )
}
const KV = ({ rows }: { rows: [string, React.ReactNode][] }) => (
  <dl className="m-0 grid grid-cols-[130px_1fr] gap-x-3 text-caption">{rows.map(([k, v]) => <div key={k} className="contents"><dt className="border-b border-border-subtle py-1.5 font-mono text-muted-foreground">{k}</dt><dd className="m-0 break-all border-b border-border-subtle py-1.5 font-mono">{v}</dd></div>)}</dl>
)

export function Kubernetes() {
  const { clusters, namespaces, workloads, pods, nodes, events, cpu, requested: req, memory: mem } = useKubernetes()
  const [cluster, setCluster] = useState(clusters[0])
  const [ns, setNs] = useState('all')
  const [tab, setTab] = useState<'workloads' | 'pods' | 'nodes' | 'events'>('workloads')
  const [pod, setPod] = useState<(typeof pods)[number] | null>(null)
  if (!clusters.length) return (
    <>
      <PageHeader title="Kubernetes" />
      <EmptyState icon={<Boxes />} title="No cluster connected" code={'helm repo add ferristrace https://charts.ferristrace.example.com\nhelm install ferristrace-agent ferristrace/agent \\\n  --set ingestKey=ft_ik_… --set clusterName=prod-eu-1'}>Install the agent with Helm. It runs the OpenTelemetry Collector as a DaemonSet and sends nodes, pods, workloads, events and container logs.</EmptyState>
    </>
  )
  const inNs = <T extends { ns: string }>(l: T[]) => l.filter((x) => ns === 'all' || x.ns === ns)
  return (
    <>
      <PageHeader title="Kubernetes" meta="From the FerrisTrace agent: kubelet stats, cluster state and events">
        <NativeSelect aria-label="Cluster" value={cluster} onChange={(e) => setCluster(e.target.value)} className="w-[170px] font-mono">{clusters.map((c) => <option key={c}>{c}</option>)}</NativeSelect>
        <NativeSelect aria-label="Namespace" value={ns} onChange={(e) => setNs(e.target.value)} className="w-[170px]"><option value="all">All namespaces</option>{namespaces.map((n) => <option key={n}>{n}</option>)}</NativeSelect>
      </PageHeader>
      <div className="grid grid-cols-[repeat(auto-fit,minmax(170px,1fr))] gap-3">
        <Kpi label="Nodes" value="6 / 6" sub="all Ready · 3 zones" tone="ok" />
        <Kpi label="Pods" value="142" sub="138 running · 1 pending · 1 crash looping" tone="warn" />
        <Kpi label="CPU used / requested" value="58 %" sub="of 48 cores · requests 64 %" />
        <Kpi label="Memory used" value="71 %" sub="of 192 GiB · node-pool-b-2 at 81 %" />
        <Kpi label="Restarts, 1 h" value="15" sub="14 from notifier (OOMKilled)" tone="warn" />
      </div>
      <div className="grid grid-cols-[repeat(auto-fit,minmax(min(420px,100%),1fr))] gap-3">
        <LineChart title="Cluster CPU" legend="solid: used · dashed: requested" values={cpu} second={req} hi={100} yLabels={['100 %', '50 %', '0']} aria="Cluster CPU used near 55 %, requests at 64 %" height={110} markers={[{ at: 32, label: '2.14.1', to: '/releases/2.14.1' }]} />
        <LineChart title="Cluster memory" legend="used, % of allocatable" values={mem} hi={100} yLabels={['100 %', '50 %', '0']} aria="Memory slowly rising to 71 %" height={110} />
      </div>
      <div role="tablist" aria-label="Kubernetes" className="flex gap-1 border-b">
        {([['workloads', 'Workloads', workloads.length], ['pods', 'Pods', pods.length], ['nodes', 'Nodes', nodes.length], ['events', 'Events', events.filter((e) => e[1] === 'Warning').length]] as const).map(([v, l, n]) => (
          <button key={v} role="tab" type="button" aria-selected={tab === v} onClick={() => setTab(v)} className={cn('-mb-px inline-flex h-9 cursor-pointer items-center gap-1.5 border-b-2 border-transparent px-2.5 text-[13.5px] font-semibold text-muted-foreground hover:text-foreground', tab === v && 'border-primary text-foreground')}>
            {l}<span className={cn('rounded-sm px-1 font-mono text-[11px] font-medium leading-4', v === 'events' ? 'bg-warning-subtle text-warning-text' : 'bg-muted')}>{n}{v === 'events' ? ' warnings' : ''}</span>
          </button>
        ))}
      </div>
      {tab === 'workloads' && (
        <Card className="overflow-x-auto">
          <Table aria-label="Workloads"><thead><tr><Th>Workload</Th><Th className="w-[120px]">Namespace</Th><Th className="w-[110px]">Kind</Th><Th className="w-[80px]">Ready</Th><Th numeric className="w-[80px]">Restarts</Th><Th className="w-[130px]">CPU / limit</Th><Th className="w-[130px]">Memory / limit</Th><Th className="w-[130px]">Service</Th></tr></thead>
            <tbody>{inNs(workloads).map((w) => (
              <tr key={w.name} className="hover:[&>td]:bg-row-hover">
                <Td><span className="flex flex-col leading-5"><span className="inline-flex items-center gap-1.5 font-mono font-semibold">{w.status === 'bad' ? <CircleX className="size-3.5 text-destructive-text" aria-label="Unhealthy" /> : w.status === 'pending' ? <Clock className="size-3.5 text-warning-text" aria-label="Pending" /> : null}{w.name}</span><span className="truncate font-mono text-caption text-muted-foreground">{w.image}</span></span></Td>
                <Td className="text-control">{w.ns}</Td><Td className="text-control text-muted-foreground">{w.kind}</Td>
                <Td className={cn('font-mono', w.status !== 'ok' && 'font-semibold text-warning-text')}>{w.ready}</Td>
                <Td numeric className={cn(w.restarts > 5 && 'font-semibold text-destructive-text')}>{w.restarts}</Td>
                <Td><Bar pct={w.cpu} /></Td><Td><Bar pct={w.mem} warn={85} /></Td>
                <Td>{w.svc ? <Link to={['checkout-api', 'api-gateway', 'inventory', 'payments'].includes(w.svc) ? `/services/${w.svc}` : '/services'} className="font-mono text-control">{w.svc}</Link> : <span className="text-faint-foreground">—</span>}</Td>
              </tr>
            ))}</tbody></Table>
        </Card>
      )}
      {tab === 'pods' && (
        <Card className="overflow-x-auto">
          <Table aria-label="Pods"><thead><tr><Th>Pod</Th><Th className="w-[160px]">Status</Th><Th className="w-[140px]">Node</Th><Th numeric className="w-[80px]">Restarts</Th><Th className="w-[80px]">Age</Th><Th numeric className="w-[80px]">CPU</Th><Th className="w-[140px]">Memory</Th></tr></thead>
            <tbody>{inNs(pods).map((p) => (
              <tr key={p.name} onClick={() => setPod(p)} className="cursor-pointer hover:[&>td]:bg-row-hover">
                <Td><span className="flex flex-col leading-5"><span className="font-mono font-semibold">{p.name}</span><span className="text-caption text-muted-foreground">{p.ns}</span></span></Td>
                <Td><Badge variant={POD[p.status].v}>{POD[p.status].icon}{p.status}</Badge></Td>
                <Td className="font-mono text-control">{p.node}</Td><Td numeric className={cn(p.restarts > 5 && 'font-semibold text-destructive-text')}>{p.restarts}</Td>
                <Td className="font-mono text-control">{p.age}</Td><Td numeric>{p.cpu}</Td><Td className={cn('font-mono text-control', p.mem.startsWith('508') && 'font-semibold text-destructive-text')}>{p.mem}</Td>
              </tr>
            ))}</tbody></Table>
        </Card>
      )}
      {tab === 'nodes' && (
        <Card className="overflow-x-auto">
          <Table aria-label="Nodes"><thead><tr><Th>Node</Th><Th className="w-[140px]">Type</Th><Th className="w-[150px]">Zone</Th><Th numeric className="w-[70px]">Pods</Th><Th className="w-[130px]">CPU</Th><Th className="w-[130px]">Memory</Th><Th className="w-[90px]">Status</Th></tr></thead>
            <tbody>{nodes.map((n) => (
              <tr key={n.name} className="hover:[&>td]:bg-row-hover"><Td className="font-mono font-semibold">{n.name}</Td><Td className="font-mono text-control">{n.type}</Td><Td className="font-mono text-control text-muted-foreground">{n.zone}</Td><Td numeric>{n.pods}</Td><Td><Bar pct={n.cpu} /></Td><Td><Bar pct={n.mem} /></Td><Td><Badge variant="success"><CircleCheck />Ready</Badge></Td></tr>
            ))}</tbody></Table>
        </Card>
      )}
      {tab === 'events' && (
        <Card className="overflow-hidden">
          {events.map((e) => (
            <div key={e[0] + e[2]} className="grid grid-cols-[80px_24px_150px_minmax(0,1fr)] items-start gap-3 border-b border-border-subtle px-4 py-2.5 text-control last:border-b-0">
              <time className="font-mono text-muted-foreground">{e[0]}</time>
              {e[1] === 'Warning' ? <TriangleAlert className="mt-0.5 size-3.5 text-warning-text" aria-label="Warning" /> : <CircleCheck className="mt-0.5 size-3.5 text-muted-foreground" aria-label="Normal" />}
              <span className={cn('font-semibold', e[1] === 'Warning' && 'text-warning-text')}>{e[2]}</span>
              <span className="flex min-w-0 flex-col"><span className="font-mono text-caption text-muted-foreground">{e[3]}</span><span>{e[4]}</span></span>
            </div>
          ))}
        </Card>
      )}
      <p className="m-0 text-caption text-muted-foreground">Pods carry <span className="font-mono">k8s.pod.name</span> and <span className="font-mono">k8s.deployment.name</span> on their spans and logs, so every pod opens its own traces and logs.</p>
      {pod ? (
        <SidePanel title={pod.name} sub={`${pod.ns} · ${pod.workload}`} onClose={() => setPod(null)}>
          <div className="flex flex-wrap gap-2"><Badge variant={POD[pod.status].v}>{POD[pod.status].icon}{pod.status}</Badge><Badge variant="outline" mono>restarts {pod.restarts}</Badge></div>
          {pod.reason ? <div className="rounded-md border border-destructive-border bg-destructive-subtle px-3 py-2 text-control text-destructive-text">{pod.reason}</div> : null}
          {pod.status === 'CrashLoopBackOff' ? <div className="rounded-md bg-accent-subtle px-3 py-2 text-control text-accent-text">Memory reached its 512Mi limit 14 times in 3 h. The same minute, the service logged “smtp pool exhausted”. Raise the limit or cap the pool size.</div> : null}
          <KV rows={[['node', pod.node], ['cpu', pod.cpu], ['memory', pod.mem], ['age', pod.age], ['k8s.namespace.name', pod.ns], ['k8s.deployment.name', pod.workload], ['container', `${pod.workload} · acme/${pod.workload}`]]} />
          <div className="flex flex-wrap gap-2">
            <Button asChild size="sm"><Link to="/logs"><ScrollText />Container logs</Link></Button>
            <Button asChild size="sm"><Link to="/traces"><Waypoints />Traces from this pod</Link></Button>
            {pod.workload === 'notifier' ? <Button asChild size="sm" variant="ghost"><Link to="/issues/iss_120">Issue DATA-20</Link></Button> : null}
          </div>
        </SidePanel>
      ) : null}
    </>
  )
}

export function Docker() {
  const { hosts, containers, cpu, pressure: pdf } = useDocker()
  const [host, setHost] = useState(hosts[0])
  const [filter, setFilter] = useState<'all' | 'problems'>('all')
  const [c, setC] = useState<(typeof containers)[number] | null>(null)
  if (!hosts.length) return (
    <>
      <PageHeader title="Docker" />
      <EmptyState icon={<Box />} title="No Docker host connected" code={'docker run -d --name ferristrace-agent \\\n  -v /var/run/docker.sock:/var/run/docker.sock:ro \\\n  -e FT_INGEST_KEY=ft_ik_… ferristrace/agent'}>Run the agent next to your containers. It reads container stats and logs from the Docker socket, read-only.</EmptyState>
    </>
  )
  const rows = containers.filter((x) => filter === 'all' || x.status !== 'running')
  return (
    <>
      <PageHeader title="Docker" meta="Containers on hosts without Kubernetes, from the Docker stats and logs of the agent">
        <NativeSelect aria-label="Host" value={host} onChange={(e) => setHost(e.target.value)} className="w-[220px]">{hosts.map((h) => <option key={h}>{h}</option>)}</NativeSelect>
      </PageHeader>
      <div className="grid grid-cols-[repeat(auto-fit,minmax(170px,1fr))] gap-3">
        <Kpi label="Containers" value="8" sub="5 running · 1 restarting · 1 exited · 1 unhealthy" tone="warn" />
        <Kpi label="Host CPU" value="41 %" sub="8 cores" />
        <Kpi label="Host memory" value="68 %" sub="10.9 of 16 GiB" />
        <Kpi label="Disk" value="72 %" sub="/var/lib/docker · images 18 GiB" />
      </div>
      <div className="grid grid-cols-[repeat(auto-fit,minmax(min(420px,100%),1fr))] gap-3">
        <LineChart title="Host CPU" legend="all containers" values={cpu} hi={100} yLabels={['100 %', '50 %', '0']} aria="Host CPU near 40 %" height={110} />
        <LineChart title="pdf-renderer CPU" legend="busiest container · dashed: 80 % warning" values={pdf} threshold={80} hi={100} yLabels={['100 %', '50 %', '0']} aria="pdf-renderer CPU rose above 85 % at 14:13" height={110} />
      </div>
      <div className="flex items-center gap-2"><Segmented label="Show" value={filter} onChange={setFilter} options={[{ value: 'all', label: 'All containers' }, { value: 'problems', label: 'Problems only' }]} /></div>
      <Card className="overflow-x-auto">
        <Table aria-label="Containers"><thead><tr><Th>Container</Th><Th className="w-[120px]">Compose</Th><Th className="w-[200px]">Status</Th><Th className="w-[110px]">Up</Th><Th className="w-[110px]">CPU</Th><Th className="w-[200px]">Memory</Th><Th numeric className="w-[80px]">Restarts</Th></tr></thead>
          <tbody>{rows.map((x) => (
            <tr key={x.name} onClick={() => setC(x)} className="cursor-pointer hover:[&>td]:bg-row-hover">
              <Td><span className="flex flex-col leading-5"><span className="font-mono font-semibold">{x.name}</span><span className="truncate font-mono text-caption text-muted-foreground">{x.image}</span></span></Td>
              <Td className="font-mono text-control">{x.compose}</Td>
              <Td><span className="flex flex-col gap-0.5"><Badge variant={CS[x.status]} className="w-fit">{x.status}</Badge><span className="text-caption text-muted-foreground">{x.health}</span></span></Td>
              <Td className="font-mono text-control">{x.up}</Td>
              <Td>{x.status === 'running' || x.status === 'unhealthy' ? <Bar pct={x.cpu} /> : <span className="text-faint-foreground">—</span>}</Td>
              <Td>{x.memPct ? <span className="flex flex-col gap-0.5"><Bar pct={x.memPct} warn={85} /><span className="font-mono text-caption text-muted-foreground">{x.mem}</span></span> : <span className="text-faint-foreground">—</span>}</Td>
              <Td numeric className={cn(x.restarts > 3 && 'font-semibold text-destructive-text')}>{x.restarts}</Td>
            </tr>
          ))}</tbody></Table>
      </Card>
      <p className="m-0 text-caption text-muted-foreground">Environment variables are never collected. Labels like <span className="font-mono">com.docker.compose.service</span> become resource attributes, so traces and logs link back here.</p>
      {c ? (
        <SidePanel title={c.name} sub={`${c.compose} · ${host}`} onClose={() => setC(null)}>
          <div className="flex flex-wrap gap-2"><Badge variant={CS[c.status]}>{c.status}</Badge><Badge variant="outline">{c.health}</Badge></div>
          {c.status === 'restarting' ? <div className="rounded-md border border-destructive-border bg-destructive-subtle px-3 py-2 text-control text-destructive-text">Exits with code 1 a few seconds after start. Last log line: “smtp pool exhausted, worker exiting with code 1”.</div> : null}
          {c.status === 'unhealthy' ? <div className="rounded-md border border-destructive-border bg-destructive-subtle px-3 py-2 text-control text-destructive-text">Health check GET /health returns 503 since 14:13, memory at 95 % of its 2 GiB limit.</div> : null}
          <KV rows={[['image', c.image], ['uptime', c.up], ['cpu', `${c.cpu} %`], ['memory', c.mem], ['network', c.net], ['ports', c.ports], ['restarts', String(c.restarts)], ['compose project', c.compose]]} />
          <pre className="m-0 max-h-48 overflow-auto rounded-md border bg-muted px-3 py-2 font-mono text-[11.5px] leading-[18px]">{c.status === 'restarting' ? '14:32:03 starting mailer worker pool=50\n14:32:04 connecting to smtp.mailgun.org:587\n14:32:04 FATAL smtp pool exhausted, worker exiting with code 1' : '14:32:01 GET /health 200 2ms\n14:32:06 render job 8841 done in 1.2s\n14:32:11 GET /health 200 3ms'}</pre>
          <div className="flex flex-wrap gap-2"><Button asChild size="sm"><Link to="/logs"><ScrollText />All logs</Link></Button><Button asChild size="sm" variant="ghost"><Link to="/backend/system"><Server />Host metrics</Link></Button></div>
        </SidePanel>
      ) : null}
    </>
  )
}
