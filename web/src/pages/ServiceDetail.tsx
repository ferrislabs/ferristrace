import { ScrollText, TriangleAlert, Waypoints } from 'lucide-react'
import { Link, useParams, useSearchParam } from '@/lib/router'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardHeader, CardTitle } from '@/components/ui/card'
import { Table, Td, Th, Tr } from '@/components/ui/misc'
import { EmptyState, Kpi, svcBg } from '@/components/ft/bits'
import { LineChart } from '@/components/ft/charts'
import { ceiling, timeLabels, useDeployMarkers } from '@/data/series'
import { ERROR_THRESHOLD, WINDOW_MS, charted, perSecond, useOperations, useServiceSeries, useServiceTable } from '@/data/services'
import { ANY as ANY_ISSUE, useIssues } from '@/data/issues'
import { because } from '@/auth/client'
import { dur, num } from '@/lib/format'
import { cn } from '@/lib/utils'

const axis = (hi: number, shown: (v: number) => string): [string, string, string] => [shown(hi), shown(hi / 2), '0']

export function ServiceDetail() {
  const { name = '' } = useParams()
  const environment = useSearchParam('env') ?? ''
  const { services, pending } = useServiceTable(environment)
  const { operations, failed: opsFailed } = useOperations(name, environment)
  const { series } = useServiceSeries(name, environment)
  const { issues } = useIssues({ ...ANY_ISSUE, rangeMs: WINDOW_MS, service: name, limit: 50 })
  const markers = useDeployMarkers()
  const s = services.find((x) => x.name === name)
  const bad = (s?.err ?? 0) >= ERROR_THRESHOLD
  const lines = charted(series)
  const to = series?.points.length ? series.points[series.points.length - 1].atMs + series.stepMs : Date.now()
  const labels = timeLabels(to - WINDOW_MS, to)
  const rateHi = ceiling(lines.perSecond)
  const errHi = Math.max(ceiling(lines.errorRate), ERROR_THRESHOLD * 2)
  const p95Hi = ceiling(lines.p95)
  const open = issues.filter((issue) => issue.status === 'unresolved').slice(0, 5)
  const kept = environment ? { env: environment } : undefined
  return (
    <>
      <nav aria-label="Breadcrumb" className="text-control"><Link to="/services" search={kept}>Services</Link><span className="text-faint-foreground"> / </span><span className="font-mono">{name}</span></nav>
      <div className="flex flex-wrap items-center gap-2.5">
        <span className={cn('size-3 rounded-[3px]', svcBg(name))} aria-hidden />
        <h1 className="font-mono text-title">{name}</h1>
        {pending ? null : !s ? <Badge variant="outline">No spans in the last hour</Badge> : bad ? <Badge variant="warning"><TriangleAlert />Error rate above {ERROR_THRESHOLD} %</Badge> : <Badge variant="success">Healthy</Badge>}
        <span className="flex-1" />
        <Button asChild size="sm"><Link to="/traces"><Waypoints />Traces</Link></Button>
        <Button asChild size="sm"><Link to="/logs" search={{ f: new URLSearchParams({ service: name }).toString() }}><ScrollText />Logs</Link></Button>
        <Button asChild size="sm" variant="ghost"><Link to="/metrics">Metrics</Link></Button>
      </div>
      <div className="flex flex-wrap gap-x-3.5 gap-y-1.5 text-caption text-muted-foreground">
        <span>Window <span className="font-mono text-foreground">last hour</span></span>
        {environment ? <span>Environment <span className="font-mono text-foreground">{environment}</span></span> : null}
        {s ? <span>Last span <span className="font-mono text-foreground">{s.seen}</span></span> : null}
      </div>
      <div className="grid grid-cols-[repeat(auto-fit,minmax(160px,1fr))] gap-3">
        <Kpi label="Requests" value={s?.rps ?? '—'} sub="server and consumer spans" />
        <Kpi label="Error rate" value={s?.err == null ? '—' : `${s.err.toFixed(1)} %`} sub={`amber from ${ERROR_THRESHOLD} %`} tone={bad ? 'warn' : undefined} />
        <Kpi label="p95 latency" value={s?.p95 ?? '—'} sub={s ? `p50 ${s.p50} · p99 ${s.p99}` : undefined} tone={bad ? 'warn' : undefined} />
        <Kpi label="Requests in range" value={s ? num(s.requests) : '—'} sub={s ? `${num(s.errors)} with errors` : undefined} />
      </div>
      <div className="grid grid-cols-[repeat(auto-fit,minmax(min(380px,100%),1fr))] gap-3">
        <LineChart title="Requests" legend="per second" values={lines.perSecond} hi={rateHi} yLabels={axis(rateHi, (v) => perSecond(v).replace('/s', ''))} xLabels={labels} aria={`Requests per second of ${name} over the last hour`} height={120} markers={markers} />
        <LineChart title="Error rate" legend={`dashed: ${ERROR_THRESHOLD} % status page threshold`} values={lines.errorRate} markers={markers} threshold={ERROR_THRESHOLD} hi={errHi} yLabels={axis(errHi, (v) => `${v.toFixed(v < 10 ? 1 : 0)} %`)} xLabels={labels} aria={`Error rate of ${name} over the last hour`} height={120} />
        <LineChart title="Latency" legend="p95" values={lines.p95} markers={markers} hi={p95Hi} yLabels={axis(p95Hi, dur)} xLabels={labels} aria={`p95 latency of ${name} over the last hour`} height={120} />
      </div>
      <Card className="overflow-hidden">
        <CardHeader><CardTitle>Operations</CardTitle><span className="text-caption text-muted-foreground">by span name, server and consumer spans</span></CardHeader>
        {opsFailed ? <p className="m-0 px-4 py-3 text-control text-muted-foreground">{because(opsFailed, 'The operations could not be read.')}</p> : null}
        {!opsFailed && operations.length === 0 ? <p className="m-0 px-4 py-3 text-control text-muted-foreground">No operation served in the last hour.</p> : null}
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
        <CardHeader><CardTitle className="flex-1">Open issues</CardTitle><span className="text-caption text-muted-foreground">seen in the last hour</span></CardHeader>
        {open.length === 0 ? <p className="m-0 px-4 py-3 text-control text-muted-foreground">No unresolved issue was seen on this service in the last hour.</p> : null}
        {open.map((issue) => (
          <div key={issue.fingerprint} className="flex flex-col gap-1 border-b border-border-subtle px-4 py-2.5">
            <div className="flex items-baseline gap-2"><Link to={`/issues/${issue.fingerprint}`} className="flex-1 truncate font-mono font-semibold text-destructive-text">{issue.exception_type}</Link><span className="font-mono text-control">{num(issue.events)}</span></div>
            <span className="truncate font-mono text-caption text-muted-foreground">{issue.message}</span>
          </div>
        ))}
      </Card>
      {!pending && !s ? <EmptyState title="Nothing measured yet">This service served no span in the last hour{environment ? ` in ${environment}` : ''}.</EmptyState> : null}
    </>
  )
}
