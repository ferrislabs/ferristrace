import { ScrollText, TriangleAlert, Waypoints } from 'lucide-react'
import { useState } from 'react'
import { Link, useParams } from '@/lib/router'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardHeader, CardTitle } from '@/components/ui/card'
import { Segmented } from '@/components/ui/segmented'
import { Table, Td, Th, Tr } from '@/components/ui/misc'
import { Kpi, ServiceDot, svcBg } from '@/components/ft/bits'
import { LineChart } from '@/components/ft/charts'
import { DEPLOY_MARKERS, useServiceSeries } from '@/data/series'
import { useServices } from '@/data/services'
import { cn } from '@/lib/utils'
import { NotFound } from './NotFound'

const OPS = [
  ['POST /api/checkout', 'SERVER', '18.2/s', 7.9, '210 ms', '1.9 s', '3.4 s', 62],
  ['psp.authorize', 'CLIENT', '9.7/s', 8.3, '380 ms', '1.6 s', '3.0 s', 21],
  ['GET /api/cart', 'SERVER', '14.1/s', 0.2, '12 ms', '48 ms', '110 ms', 7],
  ['SELECT orders', 'CLIENT', '52.0/s', 0, '1.8 ms', '9 ms', '31 ms', 5],
  ['cart.price', 'INTERNAL', '18.2/s', 0, '4 ms', '15 ms', '40 ms', 3],
  ['checkout.expire_carts', 'INTERNAL', '0.02/s', 0, '4.1 s', '4.9 s', '5.2 s', 2],
] as const
const ERRORS = [
  { type: 'PspTimeoutError', msg: 'upstream psp-gateway timed out after 3000 ms', count: '412', first: '14:05:12', last: '14:32:01' },
  { type: 'HttpError 502', msg: 'POST payments:8080/authorize returned 502 Bad Gateway', count: '96', first: '14:06:40', last: '14:31:48' },
  { type: 'ValidationError', msg: 'cart is empty', count: '18', first: '13:34:02', last: '14:29:15' },
]
export function ServiceDetail() {
  const { name = 'checkout-api' } = useParams()
  const services = useServices()
  const [kind, setKind] = useState<'all' | 'SERVER' | 'CLIENT' | 'INTERNAL'>('all')
  const s = services.find((x) => x.name === name) ?? services[0]
  const bad = (s?.err ?? 0) >= 2
  const series = useServiceSeries(s?.name ?? name, bad)
  const ops = OPS.filter((o) => kind === 'all' || o[1] === kind)
  if (!s) return <NotFound />
  return (
    <>
      <nav aria-label="Breadcrumb" className="text-control"><Link to="/services">Services</Link><span className="text-faint-foreground"> / </span><span className="font-mono">{s.name}</span></nav>
      <div className="flex flex-wrap items-center gap-2.5">
        <span className={cn('size-3 rounded-[3px]', svcBg(s.name))} aria-hidden />
        <h1 className="font-mono text-title">{s.name}</h1>
        {bad ? <Badge variant="warning"><TriangleAlert />Error rate above 2 % since 14:05</Badge> : <Badge variant="success">Healthy</Badge>}
        <span className="flex-1" />
        <Button asChild size="sm"><Link to="/traces"><Waypoints />Traces</Link></Button>
        <Button asChild size="sm"><Link to="/logs"><ScrollText />Logs</Link></Button>
        <Button asChild size="sm" variant="ghost"><Link to="/metrics">Metrics</Link></Button>
      </div>
      <div className="flex flex-wrap gap-x-3.5 gap-y-1.5 text-caption text-muted-foreground">
        <span>Project <span className="font-semibold text-foreground">{s.project}</span></span><span>Version <span className="font-mono text-foreground">2.14.1</span></span><span>Environment <span className="font-mono text-foreground">production</span></span><span>Instances <span className="font-mono text-foreground">4</span></span><span>SDK <span className="font-mono text-foreground">opentelemetry-js 1.27.0</span></span><span>Last span <span className="font-mono text-foreground">{s.seen}</span></span>
      </div>
      <div className="grid grid-cols-[repeat(auto-fit,minmax(160px,1fr))] gap-3">
        <Kpi label="Requests" value={s.rps} sub="−2 % vs previous hour" />
        <Kpi label="Error rate" value={s.err === null ? '—' : `${s.err.toFixed(1)} %`} sub={bad ? '+3.7 pts vs previous hour' : 'stable'} tone={bad ? 'warn' : undefined} />
        <Kpi label="p95 latency" value={s.p95} sub={bad ? '×3.1 vs previous hour' : 'stable'} tone={bad ? 'warn' : undefined} />
        <Kpi label="Spans in range" value="612,480" sub="18,230 with errors" />
      </div>
      <div className="grid grid-cols-[repeat(auto-fit,minmax(min(380px,100%),1fr))] gap-3">
        <LineChart title="Requests" legend="per second" values={series.requests} hi={60} yLabels={['60', '30', '0']} aria="Requests steady around 40 per second" height={120} markers={DEPLOY_MARKERS} />
        <LineChart title="Error rate" legend="dashed: 2 % status page threshold" values={series.errorRate} markers={DEPLOY_MARKERS} threshold={2} hi={6} yLabels={['6 %', '3 %', '0']} aria="Error rate under 1 % until 14:05, then 3 to 5 %" height={120} />
        <LineChart title="Latency" legend="solid p95 · dashed p50" values={series.p95} markers={DEPLOY_MARKERS} second={series.p50} hi={2000} yLabels={['2 s', '1 s', '0']} aria="p95 latency rises from 400 ms to 1.3 s at 14:05" height={120} />
      </div>
      <Card className="overflow-hidden">
        <CardHeader><CardTitle>Operations</CardTitle><span className="text-caption text-muted-foreground">by span name</span><span className="flex-1" /><Segmented label="Span kind" value={kind} onChange={setKind} options={[{ value: 'all', label: 'All' }, { value: 'SERVER', label: 'Server' }, { value: 'CLIENT', label: 'Client' }, { value: 'INTERNAL', label: 'Internal' }]} /></CardHeader>
        <div className="overflow-x-auto">
          <Table>
            <thead><tr><Th>Span name</Th><Th className="w-[90px]">Kind</Th><Th numeric className="w-[90px]">Requests</Th><Th numeric className="w-[90px]">Error rate</Th><Th numeric className="w-[80px]">p50</Th><Th numeric className="w-[80px]">p95</Th><Th numeric className="w-[80px]">p99</Th><Th className="w-[180px]">Share of time</Th></tr></thead>
            <tbody>
              {ops.map((o) => (
                <Tr key={o[0]}>
                  <Td><Link to="/traces" className="font-mono text-foreground">{o[0]}</Link></Td><Td><Badge variant="outline" mono>{o[1]}</Badge></Td><Td numeric>{o[2]}</Td>
                  <Td numeric className={cn(o[3] >= 2 && 'font-semibold text-warning-text')}>{o[3].toFixed(1)} %</Td><Td numeric>{o[4]}</Td><Td numeric>{o[5]}</Td><Td numeric>{o[6]}</Td>
                  <Td><span className="flex items-center gap-2"><span className="flex h-1.5 flex-1 overflow-hidden rounded bg-muted"><span className="rounded bg-chart-line" style={{ width: `${o[7]}%` }} /></span><span className="w-[34px] text-right font-mono text-caption">{o[7]} %</span></span></Td>
                </Tr>
              ))}
            </tbody>
          </Table>
        </div>
      </Card>
      <div className="grid grid-cols-[repeat(auto-fit,minmax(min(380px,100%),1fr))] gap-3">
        <Card className="overflow-hidden">
          <CardHeader><CardTitle>Dependencies</CardTitle></CardHeader>
          <div className="px-4 pb-1 pt-2 text-caption font-semibold text-muted-foreground">Called by</div>
          <DepRow name="api-gateway" rps="32.3/s" err="3.9 %" warn />
          <div className="px-4 pb-1 pt-2.5 text-caption font-semibold text-muted-foreground">Calls</div>
          <DepRow name="payments" rps="9.7/s" err="8.3 %" warn strong />
          <DepRow name="inventory" rps="12.0/s" err="0.1 %" />
          <DepRow name="postgresql" note="db" rps="52.0/s" err="0.0 %" />
          <DepRow name="redis" note="cache" rps="80.4/s" err="0.0 %" />
          <p className="m-0 px-4 py-2.5 text-caption text-muted-foreground">From parent and child spans. Databases and queues come from <span className="font-mono">db.system</span> and <span className="font-mono">messaging.system</span>.</p>
        </Card>
        <Card className="overflow-hidden">
          <CardHeader><CardTitle className="flex-1">Top errors</CardTitle><span className="text-caption text-muted-foreground">by exception.type</span></CardHeader>
          {ERRORS.map((e) => (
            <div key={e.type} className="flex flex-col gap-1 border-b border-border-subtle px-4 py-2.5">
              <div className="flex items-baseline gap-2"><Link to="/issues/iss_142" className="flex-1 truncate font-mono font-semibold text-destructive-text">{e.type}</Link><span className="font-mono text-control">{e.count}</span></div>
              <span className="truncate font-mono text-caption text-muted-foreground">{e.msg}</span>
              <div className="flex items-center gap-2.5 text-caption"><span className="flex-1 text-muted-foreground">first {e.first} · last {e.last}</span><Link to="/traces/4bf92f3577b34da6a3ce929d0e0e4736">Example trace</Link><Link to="/logs">Logs</Link></div>
            </div>
          ))}
        </Card>
      </div>
    </>
  )
}

function DepRow({ name, note, rps, err, warn, strong }: { name: string; note?: string; rps: string; err: string; warn?: boolean; strong?: boolean }) {
  return (
    <Link to={['payments', 'inventory', 'api-gateway'].includes(name) ? `/services/${name}` : '/backend/queries'} className="flex items-center gap-2 border-b border-border-subtle px-4 py-2 text-control text-foreground no-underline hover:bg-row-hover">
      <ServiceDot name={name} /><span className="flex-1 font-mono">{name}</span>{note ? <span className="text-caption text-muted-foreground">{note}</span> : null}
      <span className="font-mono text-muted-foreground">{rps}</span><span className={cn('w-14 text-right font-mono', warn && 'text-warning-text', strong && 'font-semibold')}>{err}</span>
    </Link>
  )
}
