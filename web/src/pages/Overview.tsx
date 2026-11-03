import { CircleCheck, CircleX, Rocket, TriangleAlert } from 'lucide-react'
import { Link } from '@/lib/router'
import { Button } from '@/components/ui/button'
import { Card, CardHeader, CardTitle } from '@/components/ui/card'
import { EmptyState, PageHeader, ServiceDot, SeverityBadge } from '@/components/ft/bits'
import { LineChart, Sparkline } from '@/components/ft/charts'
import { ceiling, timeLabels, useDeployMarkers } from '@/data/series'
import { ERROR_THRESHOLD, charted, perSecond, relative, useServiceSeries, useServiceTable } from '@/data/services'
import { ANY as ANY_TRACE, useTraces } from '@/data/traces'
import { ANY as ANY_LOG, msOf, useLogs } from '@/data/logs'
import { ANY as ANY_ISSUE, useIssues } from '@/data/issues'
import { useProjects } from '@/data/projects'
import { otlpBase } from '@/data/onboarding'
import { useScope } from '@/lib/scope'
import { useWindow } from '@/lib/window'
import { isPlaced, placementSearch, usePlacement } from '@/data/placement'
import { PlacementPicker } from '@/components/ft/Placement'
import { dur, num } from '@/lib/format'
import { cn } from '@/lib/utils'

const pad = (n: number, width = 2) => String(n).padStart(width, '0')

function clock(ns: number): string {
  const at = new Date(msOf(ns))
  return `${pad(at.getHours())}:${pad(at.getMinutes())}:${pad(at.getSeconds())}`
}

const axis = (hi: number, shown: (v: number) => string): [string, string, string] => [shown(hi), shown(hi / 2), '0']

function Tile({ label, value, sub, spark, warn, to }: { label: string; value: string; sub?: string; spark?: number[]; warn?: boolean; to: string }) {
  return (
    <Link to={to} className="flex flex-col gap-0.5 rounded-lg border bg-card px-3.5 py-3 text-inherit no-underline hover:bg-row-hover">
      <span className="text-caption text-muted-foreground">{label}</span>
      <span className={cn('font-mono text-[22px] font-semibold leading-[30px]', warn && 'text-warning-text')}>{value}</span>
      {sub ? <span className={cn('text-caption', warn ? 'text-warning-text' : 'text-muted-foreground')}>{sub}</span> : null}
      {spark ? <span className="pt-1"><Sparkline values={relative(spark)} warn={warn} /></span> : null}
    </Link>
  )
}

export function Overview() {
  const { ms: windowMs, label } = useWindow()
  const { project, chooseProject } = useScope()
  const projects = useProjects()
  const { placement, place } = usePlacement()
  const { services: all, pending } = useServiceTable('', windowMs, placement)
  const { series } = useServiceSeries(null, '', windowMs, placement)
  const { issues } = useIssues({ ...ANY_ISSUE, rangeMs: windowMs, limit: 200 })
  const { records: errorLogs } = useLogs({ ...ANY_LOG, rangeMs: windowMs, severities: ['error', 'fatal'], limit: 10 }, false, placement)
  const { rows: slow } = useTraces({ ...ANY_TRACE, rangeMs: windowMs, order: 'duration', limit: 5 }, placement)
  const markers = useDeployMarkers(series)
  const scope = projects.find((p) => p.id === project)?.name ?? 'all projects'
  if (!pending && !all.length && !series?.requests && !isPlaced(placement)) return <><PageHeader title="Overview" meta="no signals yet" /><EmptyState icon={<Rocket />} title="No signals yet" code={`OTEL_SERVICE_NAME=checkout-api\nOTEL_EXPORTER_OTLP_ENDPOINT=${otlpBase()}\nOTEL_EXPORTER_OTLP_HEADERS="authorization=Bearer <ingest key>"`} actions={<Button asChild size="sm" variant="primary"><Link to="/onboarding">Connect your first service</Link></Button>}>This page fills in as soon as your first span, log record or metric point arrives. Set three environment variables in any OpenTelemetry SDK and restart your service.</EmptyState></>
  const lines = charted(series)
  const requests = series?.requests ?? 0
  const errorRate = requests ? ((series?.errors ?? 0) / requests) * 100 : null
  const hot = errorRate !== null && errorRate >= ERROR_THRESHOLD
  const open = issues.filter((issue) => issue.status === 'unresolved')
  const recent = [...open].sort((a, b) => b.last_seen_at_ms - a.last_seen_at_ms).slice(0, 5)
  const worst = [...all].filter((s) => s.err !== null).sort((a, b) => (b.err ?? 0) - (a.err ?? 0))
  const troubled = worst.find((s) => (s.err ?? 0) >= ERROR_THRESHOLD)
  const to = series?.points.length ? series.points[series.points.length - 1].atMs + series.stepMs : Date.now()
  const labels = timeLabels(to - windowMs, to)
  const rateHi = ceiling(lines.perSecond)
  const errHi = Math.max(ceiling(lines.errorRate), ERROR_THRESHOLD * 2)
  const p95Hi = ceiling(lines.p95)
  return (
    <>
      <PageHeader title="Overview" meta={`${scope} · last ${label}`} />
      <PlacementPicker collapsible placement={placement} onChange={place} windowMs={windowMs} />
      {troubled ? (
        <section className="flex flex-wrap items-center gap-3 rounded-lg border border-warning-border bg-card px-4 py-3" aria-label="Needs attention">
          <TriangleAlert className="size-5 text-warning-text" aria-hidden />
          <div className="flex min-w-0 flex-[1_1_320px] flex-col">
            <span className="font-semibold"><span className="font-mono">{troubled.name}</span> error rate {troubled.err?.toFixed(1)} % over the last {label}</span>
            <span className="text-control text-muted-foreground">Above the {ERROR_THRESHOLD} % threshold. {open.length ? `${open.length} unresolved issues were seen in the window.` : ''}</span>
          </div>
          <Button asChild variant="primary" size="sm"><Link to={`/services/${troubled.name}`}>Open service</Link></Button>
          {recent[0] ? <Button asChild size="sm"><Link to={`/issues/${recent[0].fingerprint}`}>Latest issue</Link></Button> : null}
        </section>
      ) : null}
      <div className="grid grid-cols-[repeat(auto-fit,minmax(170px,1fr))] gap-3">
        <Tile label="Request rate" value={perSecond(requests / (windowMs / 1000))} sub={`${num(requests)} requests`} spark={lines.perSecond} to="/services" />
        <Tile label="Error rate" value={errorRate === null ? '—' : `${errorRate.toFixed(1)} %`} sub={`${num(series?.errors ?? 0)} errors · amber from ${ERROR_THRESHOLD} %`} spark={lines.errorRate} warn={hot} to="/traces" />
        <Tile label="p95 latency" value={series?.p95Ms == null ? '—' : dur(series.p95Ms)} sub="server and consumer spans" spark={lines.p95} to="/services" />
        <Tile label="Open issues" value={num(open.length)} sub={`unresolved, seen in the last ${label}`} warn={open.length > 0} to="/issues" />
        <Tile label="Services" value={num(all.length)} sub={`${worst.filter((s) => (s.err ?? 0) >= ERROR_THRESHOLD).length} above ${ERROR_THRESHOLD} % errors`} to="/services" />
      </div>
      <div className="grid grid-cols-[repeat(auto-fit,minmax(min(420px,100%),1fr))] gap-3">
        <LineChart title="Requests" legend="per second" values={lines.perSecond} hi={rateHi} yLabels={axis(rateHi, (v) => perSecond(v).replace('/s', ''))} xLabels={labels} aria={`Requests per second over the last ${label}`} height={120} markers={markers} />
        <LineChart title="Error rate" legend={`dashed: ${ERROR_THRESHOLD} % threshold`} values={lines.errorRate} threshold={ERROR_THRESHOLD} hi={errHi} yLabels={axis(errHi, (v) => `${v.toFixed(v < 10 ? 1 : 0)} %`)} xLabels={labels} aria={`Error rate over the last ${label}`} height={120} markers={markers} />
        <LineChart title="Latency" legend="p95" values={lines.p95} hi={p95Hi} yLabels={axis(p95Hi, dur)} xLabels={labels} aria={`p95 latency over the last ${label}`} height={120} markers={markers} />
      </div>
      <div className="grid grid-cols-[repeat(auto-fit,minmax(min(420px,100%),1fr))] items-start gap-3">
        <Card className="overflow-hidden">
          <CardHeader><CardTitle className="flex-1">Recent issues</CardTitle><Link to="/issues" className="text-control">All issues</Link></CardHeader>
          {recent.length === 0 ? <p className="m-0 px-4 py-3 text-control text-muted-foreground">No unresolved issue in the last {label}.</p> : null}
          {recent.map((issue) => (
            <Link key={issue.fingerprint} to={`/issues/${issue.fingerprint}`} className="flex flex-col gap-0.5 border-b border-border-subtle px-4 py-2 text-foreground no-underline hover:bg-row-hover">
              <span className="flex items-baseline gap-2"><span className="flex-1 truncate font-mono text-control font-semibold text-destructive-text">{issue.exception_type}</span><span className="font-mono text-caption">{num(issue.events)}</span></span>
              <span className="flex gap-2 text-caption text-muted-foreground"><span className="truncate font-mono">{issue.message}</span><span className="ml-auto shrink-0">{issue.service}</span></span>
            </Link>
          ))}
        </Card>
        <Card className="overflow-hidden">
          <CardHeader><CardTitle className="flex-1">Services by error rate</CardTitle><Link to="/services" className="text-control">All {all.length} services</Link></CardHeader>
          {worst.slice(0, 5).map((s) => (
            <Link key={s.name} to={`/services/${s.name}`} className="flex items-center gap-2.5 border-b border-border-subtle px-4 py-2 text-control text-foreground no-underline hover:bg-row-hover">
              <ServiceDot name={s.name} /><span className="flex-1 truncate font-mono">{s.name}</span>
              <span className="w-[70px] text-right font-mono text-muted-foreground">{s.rps}</span>
              <span className="w-[70px] text-right font-mono text-muted-foreground">{s.p95}</span>
              <span className={cn('inline-flex w-[72px] items-center justify-end gap-1 font-mono', (s.err ?? 0) >= ERROR_THRESHOLD && 'font-semibold text-warning-text')}>
                {(s.err ?? 0) >= ERROR_THRESHOLD ? <TriangleAlert className="size-3.5" aria-hidden /> : null}{(s.err ?? 0).toFixed(1)} %
              </span>
            </Link>
          ))}
          <div className="flex justify-end gap-10 px-4 py-2 text-caption text-muted-foreground"><span>req/s</span><span>p95</span><span>errors</span></div>
        </Card>
        <Card className="overflow-hidden">
          <CardHeader><CardTitle className="flex-1">Recent error logs</CardTitle><Link to="/logs" search={{ ...placementSearch(placement), f: 'severity=error&severity=fatal' }} className="text-control">Open in Logs</Link></CardHeader>
          {errorLogs.length === 0 ? <p className="m-0 px-4 py-3 text-control text-muted-foreground">No error or fatal record in the last {label}.</p> : null}
          {errorLogs.map((l, at) => {
            const row = (
              <>
                <time className="font-mono text-caption text-muted-foreground">{clock(l.time_ns)}</time>
                <SeverityBadge level={l.severity} />
                <span className="flex min-w-0 flex-col"><span className="truncate font-mono text-caption">{l.body}</span><span className="truncate text-caption text-muted-foreground">{l.service}</span></span>
              </>
            )
            const cls = 'grid grid-cols-[70px_62px_minmax(0,1fr)] items-center gap-2.5 border-b border-border-subtle px-4 py-2 text-foreground no-underline hover:bg-row-hover'
            return l.trace_id
              ? <Link key={`${l.time_ns}-${at}`} to={`/traces/${l.trace_id}`} className={cls}>{row}</Link>
              : <Link key={`${l.time_ns}-${at}`} to="/logs" search={{ f: new URLSearchParams({ service: l.service, severity: l.severity }).toString() }} className={cls}>{row}</Link>
          })}
        </Card>
        <Card className="overflow-hidden">
          <CardHeader><CardTitle className="flex-1">Slowest traces</CardTitle><Link to="/traces" search={placementSearch(placement)} className="text-control">All traces</Link></CardHeader>
          {slow.map((t) => (
            <Link key={t.id} to={`/traces/${t.id}`} className="flex items-center gap-2.5 border-b border-border-subtle px-4 py-2 text-control text-foreground no-underline hover:bg-row-hover">
              {t.errors ? <CircleX className="size-3.5 text-destructive-text" aria-label="Has errors" /> : null}
              <span className="flex-1 truncate font-semibold">{t.root}</span>
              <span className="hidden items-center gap-1.5 text-caption text-muted-foreground sm:inline-flex"><ServiceDot name={t.svc} />{t.svc}</span>
              <span className="w-[64px] text-right font-mono">{t.dur}</span>
            </Link>
          ))}
        </Card>
        <Card className="overflow-hidden">
          <CardHeader><CardTitle className="flex-1">Projects</CardTitle><Link to="/settings/projects" className="text-control">Manage</Link></CardHeader>
          {[{ id: undefined as string | undefined, name: 'All projects' }, ...projects.map((p) => ({ id: p.id as string | undefined, name: p.name }))].map((p) => (
            <button key={p.id ?? 'all'} type="button" onClick={() => chooseProject(p.id)} aria-pressed={project === p.id} className={cn('flex w-full items-center gap-2 border-b border-border-subtle px-4 py-2.5 text-left text-control hover:bg-row-hover', project === p.id && 'bg-accent-subtle')}>
              <span className="flex-1 font-semibold">{p.name}</span>
              {project === p.id ? <span className="inline-flex items-center gap-1 text-caption font-semibold text-success-text"><CircleCheck className="size-3.5" aria-hidden />Shown</span> : null}
            </button>
          ))}
        </Card>
      </div>
    </>
  )
}
