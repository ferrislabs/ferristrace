import { ScrollText, TriangleAlert, Waypoints } from 'lucide-react'
import { Link, useParams, useSearchParam } from '@/lib/router'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardHeader, CardTitle } from '@/components/ui/card'
import { Table, Td, Th, Tr } from '@/components/ui/misc'
import { EmptyState, Kpi, svcBg } from '@/components/ft/bits'
import { LineChart } from '@/components/ft/charts'
import { ceiling, timeLabels, useDeployMarkers } from '@/data/series'
import { ERROR_THRESHOLD, charted, perSecond, useOperations, useServiceSeries, useServiceTable } from '@/data/services'
import { useWindow } from '@/lib/window'
import { ANY as ANY_ISSUE, useIssues } from '@/data/issues'
import { placementSearch, usePlacement } from '@/data/placement'
import { PlacementPicker } from '@/components/ft/Placement'
import { because } from '@/auth/client'
import { dur, num } from '@/lib/format'
import { cn } from '@/lib/utils'

const axis = (hi: number, shown: (v: number) => string): [string, string, string] => [shown(hi), shown(hi / 2), '0']

export function ServiceDetail() {
  const { ms: windowMs, label } = useWindow()
  const { name = '' } = useParams()
  const environment = useSearchParam('env') ?? ''
  const { placement, place } = usePlacement()
  const { services, pending } = useServiceTable(environment, windowMs, placement)
  const { operations, failed: opsFailed } = useOperations(name, environment, windowMs, placement)
  const { series } = useServiceSeries(name, environment, windowMs, placement)
  const { issues } = useIssues({ ...ANY_ISSUE, rangeMs: windowMs, service: name, limit: 50 })
  const markers = useDeployMarkers(series, name)
  const s = services.find((x) => x.name === name)
  const bad = (s?.err ?? 0) >= ERROR_THRESHOLD
  const lines = charted(series)
  const to = series?.points.length ? series.points[series.points.length - 1].atMs + series.stepMs : Date.now()
  const labels = timeLabels(to - windowMs, to)
  const rateHi = ceiling(lines.perSecond)
  const errHi = Math.max(ceiling(lines.errorRate), ERROR_THRESHOLD * 2)
  const p95Hi = ceiling(lines.p95)
  const open = issues.filter((issue) => issue.status === 'unresolved').slice(0, 5)
  const where = placementSearch(placement)
  const kept = { ...(environment ? { env: environment } : {}), ...where }
  return (
    <>
      <nav aria-label="Breadcrumb" className="text-control"><Link to="/services" search={kept}>Services</Link><span className="text-faint-foreground"> / </span><span className="font-mono">{name}</span></nav>
      <div className="flex flex-wrap items-center gap-2.5">
        <span className={cn('size-3 rounded-[3px]', svcBg(name))} aria-hidden />
        <h1 className="font-mono text-title">{name}</h1>
        {pending ? null : !s ? <Badge variant="outline">No spans in the last {label}</Badge> : bad ? <Badge variant="warning"><TriangleAlert />Error rate above {ERROR_THRESHOLD} %</Badge> : <Badge variant="success">Healthy</Badge>}
        <span className="flex-1" />
        <Button asChild size="sm"><Link to="/traces" search={where}><Waypoints />Traces</Link></Button>
        <Button asChild size="sm"><Link to="/logs" search={{ ...where, f: new URLSearchParams({ service: name }).toString() }}><ScrollText />Logs</Link></Button>
        <Button asChild size="sm" variant="ghost"><Link to="/metrics" search={where}>Metrics</Link></Button>
      </div>
      <div className="flex flex-wrap gap-x-3.5 gap-y-1.5 text-caption text-muted-foreground">
        <span>Window <span className="text-foreground">last {label}</span></span>
        {environment ? <span>Environment <span className="font-mono text-foreground">{environment}</span></span> : null}
        {s ? <span>Last span <span className="font-mono text-foreground">{s.seen}</span></span> : null}
      </div>
      <PlacementPicker placement={placement} onChange={place} windowMs={windowMs} />
      <div className="grid grid-cols-[repeat(auto-fit,minmax(160px,1fr))] gap-3">
        <Kpi label="Requests" value={s?.rps ?? '—'} sub="server and consumer spans" />
        <Kpi label="Error rate" value={s?.err == null ? '—' : `${s.err.toFixed(1)} %`} sub={`amber from ${ERROR_THRESHOLD} %`} tone={bad ? 'warn' : undefined} />
        <Kpi label="p95 latency" value={s?.p95 ?? '—'} sub={s ? `p50 ${s.p50} · p99 ${s.p99}` : undefined} tone={bad ? 'warn' : undefined} />
        <Kpi label="Requests in range" value={s ? num(s.requests) : '—'} sub={s ? `${num(s.errors)} with errors` : undefined} />
      </div>
      <div className="grid grid-cols-[repeat(auto-fit,minmax(min(380px,100%),1fr))] gap-3">
        <LineChart title="Requests" legend="per second" values={lines.perSecond} hi={rateHi} yLabels={axis(rateHi, (v) => perSecond(v).replace('/s', ''))} xLabels={labels} aria={`Requests per second of ${name} over the last ${label}`} height={120} markers={markers} />
        <LineChart title="Error rate" legend={`dashed: ${ERROR_THRESHOLD} % status page threshold`} values={lines.errorRate} markers={markers} threshold={ERROR_THRESHOLD} hi={errHi} yLabels={axis(errHi, (v) => `${v.toFixed(v < 10 ? 1 : 0)} %`)} xLabels={labels} aria={`Error rate of ${name} over the last ${label}`} height={120} />
        <LineChart title="Latency" legend="p95" values={lines.p95} markers={markers} hi={p95Hi} yLabels={axis(p95Hi, dur)} xLabels={labels} aria={`p95 latency of ${name} over the last ${label}`} height={120} />
      </div>
      <Card className="overflow-hidden">
        <CardHeader><CardTitle>Operations</CardTitle><span className="text-caption text-muted-foreground">by span name, server and consumer spans</span></CardHeader>
        {opsFailed ? <p className="m-0 px-4 py-3 text-control text-muted-foreground">{because(opsFailed, 'The operations could not be read.')}</p> : null}
        {!opsFailed && operations.length === 0 ? <p className="m-0 px-4 py-3 text-control text-muted-foreground">No operation served in the last {label}.</p> : null}
        <div className={cn('overflow-x-auto', operations.length === 0 && 'hidden')}>
          <Table aria-label="Operations">
            <thead><tr><Th>Span name</Th><Th numeric className="w-[90px]">Requests</Th><Th numeric className="w-[90px]">Error rate</Th><Th numeric className="w-[80px]">p50</Th><Th numeric className="w-[80px]">p95</Th><Th numeric className="w-[80px]">p99</Th><Th className="w-[110px]">Last seen</Th></tr></thead>
            <tbody>
              {operations.map((o) => (
                <Tr key={o.name}>
                  <Td><span className="font-mono">{o.name}</span></Td><Td numeric>{o.rps}</Td>
                  <Td numeric className={cn((o.err ?? 0) >= ERROR_THRESHOLD && 'font-semibold text-warning-text')}>{o.err === null ? '—' : `${o.err.toFixed(1)} %`}</Td><Td numeric>{o.p50}</Td><Td numeric>{o.p95}</Td><Td numeric>{o.p99}</Td>
                  <Td className="font-mono text-control">{o.seen}</Td>
                </Tr>
              ))}
            </tbody>
          </Table>
        </div>
      </Card>
      <Card className="overflow-hidden">
        <CardHeader><CardTitle className="flex-1">Open issues</CardTitle><span className="text-caption text-muted-foreground">seen in the last {label}</span></CardHeader>
        {open.length === 0 ? <p className="m-0 px-4 py-3 text-control text-muted-foreground">No unresolved issue was seen on this service in the last {label}.</p> : null}
        {open.map((issue) => (
          <div key={issue.fingerprint} className="flex flex-col gap-1 border-b border-border-subtle px-4 py-2.5">
            <div className="flex items-baseline gap-2"><Link to={`/issues/${issue.fingerprint}`} className="flex-1 truncate font-mono font-semibold text-destructive-text">{issue.exception_type}</Link><span className="font-mono text-control">{num(issue.events)}</span></div>
            <span className="truncate font-mono text-caption text-muted-foreground">{issue.message}</span>
          </div>
        ))}
      </Card>
      {!pending && !s ? <EmptyState title="Nothing measured yet">This service served no span in the last {label}{environment ? ` in ${environment}` : ''}.</EmptyState> : null}
    </>
  )
}
