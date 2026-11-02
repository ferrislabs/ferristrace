import { CircleCheck, CircleMinus, CircleX, Rocket, TriangleAlert } from 'lucide-react'
import { Link } from '@/lib/router'
import { Button } from '@/components/ui/button'
import { Card, CardHeader, CardTitle } from '@/components/ui/card'
import { EmptyState, Kpi, PageHeader, ServiceDot, SeverityBadge } from '@/components/ft/bits'
import { LineChart } from '@/components/ft/charts'
import { useLogs } from '@/data/logs'
import { DEPLOY_MARKERS, useOverviewSeries } from '@/data/series'
import { useMock } from '@/data/seam'
import { useServices } from '@/data/services'
import { AN_HOUR, ANY, useTraces } from '@/data/traces'
import { otlpEndpoint } from '@/data/onboarding'
import { PROJECTS } from '@/mocks/mock'
import { cn } from '@/lib/utils'

export function Overview() {
  const all = useServices()
  const logs = useLogs()
  const { rows: slow } = useTraces({ ...ANY, rangeMs: AN_HOUR, order: 'duration', limit: 5 })
  const projects = useMock(['organisation', 'projects'], () => PROJECTS)
  const series = useOverviewSeries()
  if (!all.length) return <><PageHeader title="Overview" meta="no signals yet" /><EmptyState icon={<Rocket />} title="No signals yet" code={`OTEL_SERVICE_NAME=checkout-api\nOTEL_EXPORTER_OTLP_ENDPOINT=${otlpEndpoint()}\nOTEL_EXPORTER_OTLP_HEADERS="authorization=Bearer <ingest key>"`} actions={<Button asChild size="sm" variant="primary"><Link to="/onboarding">Connect your first service</Link></Button>}>This page fills in as soon as your first span, log record or metric point arrives. Set three environment variables in any OpenTelemetry SDK and restart your service.</EmptyState></>
  const services = [...all].filter((s) => s.err !== null).sort((a, b) => (b.err ?? 0) - (a.err ?? 0)).slice(0, 5)
  const errorLogs = logs.filter((l) => l.sev === 'error' || l.sev === 'fatal').slice(0, 5)
  return (
    <>
      <PageHeader title="Overview" meta="all projects · last hour" />
      <section className="flex flex-wrap items-center gap-3 rounded-lg border border-warning-border bg-card px-4 py-3" aria-label="Needs attention">
        <TriangleAlert className="size-5 text-warning-text" aria-hidden />
        <div className="flex min-w-0 flex-[1_1_320px] flex-col">
          <span className="font-semibold"><span className="font-mono">checkout-api</span> error rate 4.1 % since 14:05</span>
          <span className="text-control text-muted-foreground">Most errors come from its calls to <span className="font-mono">payments</span>. Your status page shows Degraded on Checkout.</span>
        </div>
        <Button asChild variant="primary" size="sm"><Link to="/services/checkout-api">Open service</Link></Button>
        <Button asChild size="sm"><Link to="/issues/iss_142">Issue STOREFRONT-142</Link></Button>
        <Button asChild size="sm"><Link to="/status-page">Status page</Link></Button>
      </section>
      <div className="grid grid-cols-[repeat(auto-fit,minmax(170px,1fr))] gap-3">
        <Kpi label="Services" value={String(all.length)} sub="1 degraded · 1 no data" tone="warn" to="/services" />
        <Kpi label="Spans" value="1,820/s" sub="6.5 M in range" to="/traces" />
        <Kpi label="Span error rate" value="0.9 %" sub="+0.6 pts vs previous hour" tone="warn" to="/traces" />
        <Kpi label="Log records" value="4,310/s" sub="38/s error or fatal" to="/logs" />
        <Kpi label="Metric points" value="2,200/s" sub="312 series" to="/metrics" />
      </div>
      <div className="grid grid-cols-[repeat(auto-fit,minmax(min(420px,100%),1fr))] gap-3">
        <LineChart title="Spans received" legend="per second, all projects" values={series.spans} hi={2400} yLabels={['2.4k', '1.2k', '0']} aria="Spans per second, steady around 1,800" height={120} markers={DEPLOY_MARKERS} />
        <LineChart title="Errors" legend="solid: error spans/s · dashed: error logs/s" values={series.errorSpans} second={series.errorLogs} markers={DEPLOY_MARKERS} hi={50} yLabels={['50', '25', '0']} aria="Error spans rise from 3 to 17 per second at 14:05" height={120} />
      </div>
      <div className="grid grid-cols-[repeat(auto-fit,minmax(min(420px,100%),1fr))] items-start gap-3">
        <Card className="overflow-hidden">
          <CardHeader><CardTitle className="flex-1">Services by error rate</CardTitle><Link to="/services" className="text-control">All {all.length} services</Link></CardHeader>
          {services.map((s) => (
            <Link key={s.name} to={`/services/${s.name}`} className="flex items-center gap-2.5 border-b border-border-subtle px-4 py-2 text-control text-foreground no-underline hover:bg-row-hover">
              <ServiceDot name={s.name} /><span className="flex-1 truncate font-mono">{s.name}</span>
              <span className="w-[70px] text-right font-mono text-muted-foreground">{s.rps}</span>
              <span className="w-[70px] text-right font-mono text-muted-foreground">{s.p95}</span>
              <span className={cn('inline-flex w-[72px] items-center justify-end gap-1 font-mono', (s.err ?? 0) >= 2 && 'font-semibold text-warning-text')}>
                {(s.err ?? 0) >= 2 ? <TriangleAlert className="size-3.5" aria-hidden /> : null}{s.err!.toFixed(1)} %
              </span>
            </Link>
          ))}
          <div className="flex justify-end gap-10 px-4 py-2 text-caption text-muted-foreground"><span>req/s</span><span>p95</span><span>errors</span></div>
        </Card>
        <Card className="overflow-hidden">
          <CardHeader><CardTitle className="flex-1">Recent error logs</CardTitle><Link to="/logs" className="text-control">Open in Logs</Link></CardHeader>
          {errorLogs.map((l) => (
            <Link key={l.time + l.body} to="/logs" className="grid grid-cols-[86px_62px_minmax(0,1fr)] items-center gap-2.5 border-b border-border-subtle px-4 py-2 text-foreground no-underline hover:bg-row-hover">
              <time className="font-mono text-caption text-muted-foreground">{l.time}</time>
              <SeverityBadge level={l.sev} />
              <span className="flex min-w-0 flex-col"><span className="truncate font-mono text-caption">{l.body}</span><span className="truncate text-caption text-muted-foreground">{l.svc}</span></span>
            </Link>
          ))}
        </Card>
        <Card className="overflow-hidden">
          <CardHeader><CardTitle className="flex-1">Slowest traces</CardTitle><Link to="/traces" className="text-control">All traces</Link></CardHeader>
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
          <CardHeader><CardTitle className="flex-1">Ingest by project</CardTitle><Link to="/settings/projects" className="text-control">Projects</Link></CardHeader>
          {projects.map((p, i) => (
            <div key={p.name} className="flex flex-col gap-1 border-b border-border-subtle px-4 py-2.5">
              <div className="flex items-center gap-2">
                <span className="flex-1 text-control font-semibold">{p.name}</span>
                {i < 2 ? <span className="inline-flex items-center gap-1 text-caption font-semibold text-success-text"><CircleCheck className="size-3.5" aria-hidden />Receiving</span> : <span className="inline-flex items-center gap-1 text-caption font-semibold text-warning-text"><CircleMinus className="size-3.5" aria-hidden />1 service silent 23 min</span>}
              </div>
              <div className="flex flex-wrap gap-x-3.5 font-mono text-caption text-muted-foreground"><span>spans {['1,410/s', '340/s', '70/s'][i]}</span><span>logs {['3,120/s', '980/s', '210/s'][i]}</span><span>last {['now', '1 s ago', '4 s ago'][i]}</span></div>
            </div>
          ))}
          <div className="flex flex-col gap-1.5 px-4 py-3">
            <div className="flex items-baseline gap-2 text-control"><span className="flex-1 font-semibold">Usage this month</span><span className="font-mono">54.6 %</span><span className="text-caption text-muted-foreground">of included</span></div>
            <div className="relative h-2 rounded bg-muted"><div className="absolute inset-y-0 left-0 w-[54.6%] rounded bg-primary" /></div>
          </div>
        </Card>
      </div>
    </>
  )
}
