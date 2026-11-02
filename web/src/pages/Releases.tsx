import { useState } from 'react'
import { Rocket, TriangleAlert } from 'lucide-react'
import { Link, useNavigate, useParams } from '@/lib/router'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardHeader, CardTitle } from '@/components/ui/card'
import { Segmented } from '@/components/ui/segmented'
import { Table, Td, Th } from '@/components/ui/misc'
import { EmptyState, Kpi, PageHeader, ServiceDot } from '@/components/ft/bits'
import { RELEASE_WINDOWS, errorRateOf, releasePath, useRelease, useReleases, type Release, type Traffic } from '@/data/releases'
import { ApiError, because } from '@/auth/client'
import { num, pct, stamp } from '@/lib/format'
import { cn } from '@/lib/utils'
import { NotFound } from './NotFound'

const HOT_RATE_POINTS = 1

const rateOf = (release: Release) => errorRateOf(release.requests, release.errors)

const shown = (rate: number | null) => (rate === null ? '—' : pct(rate))

const envs = (release: Release) => (release.environments.length ? release.environments.join(', ') : '—')

export function Releases() {
  const navigate = useNavigate()
  const [range, setRange] = useState('7d')
  const windowMs = RELEASE_WINDOWS.find((w) => w.value === range)?.ms ?? RELEASE_WINDOWS[1].ms
  const { releases, pending, failed } = useReleases(windowMs)
  const unconfigured = failed instanceof ApiError && failed.status === 503
  return (
    <>
      <PageHeader title="Releases" meta="From service.version on your spans and the deploy events your CI sends">
        <Segmented label="Window" value={range} onChange={setRange} options={RELEASE_WINDOWS.map((w) => ({ value: w.value, label: w.label }))} />
      </PageHeader>
      {unconfigured ? (
        <EmptyState title="Releases are not configured">This deployment has no span store, so there are no releases to read. Configure one to see them here.</EmptyState>
      ) : failed ? (
        <EmptyState title="The releases could not be read">{because(failed, 'The span store did not answer.')}</EmptyState>
      ) : !pending && releases.length === 0 ? (
        <EmptyState
          icon={<Rocket />}
          title="No release in this window"
          code={'OTEL_RESOURCE_ATTRIBUTES=service.version=1.4.2,deployment.environment.name=production'}
        >
          A release appears the first time a span carries a service.version. Set it on your service, or send a log record with event.name set to deploy, the service name and service.version from your CI.
        </EmptyState>
      ) : (
        <div className="-mx-4 overflow-x-auto border-t md:-mx-5">
          <Table aria-label="Releases">
            <thead><tr><Th>Release</Th><Th className="w-[160px]">Environments</Th><Th className="w-[170px]">First seen</Th><Th className="w-[170px]">Last seen</Th><Th numeric className="w-[100px]">Requests</Th><Th numeric className="w-[100px]">Error rate</Th><Th numeric className="w-[100px]">New issues</Th></tr></thead>
            <tbody>
              {releases.map((r) => {
                const to = releasePath(r.service, r.version)
                const rate = rateOf(r)
                return (
                  <tr key={`${r.service}@${r.version}`} onClick={() => navigate(to)} className="cursor-pointer hover:[&>td]:bg-row-hover">
                    <Td>
                      <span className="flex flex-col leading-5">
                        <Link to={to} onClick={(e) => e.stopPropagation()} className="font-mono font-semibold text-foreground">{r.version}</Link>
                        <span className="inline-flex items-center gap-1.5 text-caption text-muted-foreground"><ServiceDot name={r.service} />{r.service}</span>
                      </span>
                    </Td>
                    <Td className="text-control">{envs(r)}</Td>
                    <Td className="font-mono text-control">{stamp(r.firstSeenMs)}</Td>
                    <Td className="font-mono text-control">{stamp(r.lastSeenMs)}</Td>
                    <Td numeric>{num(r.requests)}</Td>
                    <Td numeric>{shown(rate)}</Td>
                    <Td numeric className={cn(r.newIssues > 0 && 'font-semibold text-destructive-text')}>{r.newIssues}</Td>
                  </tr>
                )
              })}
            </tbody>
          </Table>
          <p className="m-0 px-5 py-3 text-caption text-muted-foreground">Counts are server and consumer spans since the release was first seen. New issues are exceptions first seen while the release was the newest of its service.</p>
        </div>
      )}
    </>
  )
}

function Around({ label, traffic }: { label: string; traffic: Traffic }) {
  const rate = traffic.errorRate === null ? null : traffic.errorRate * 100
  return <Kpi label={label} value={shown(rate)} sub={`${num(traffic.errors)} errors in ${num(traffic.requests)} requests`} />
}

export function ReleaseDetail() {
  const { service, version } = useParams()
  const { release: r, pending, failed } = useRelease(service, version)
  if (failed instanceof ApiError && failed.status === 404) return <NotFound />
  if (failed) return <EmptyState title="The release could not be read">{because(failed, 'The span store did not answer.')}</EmptyState>
  if (pending || !r) return <EmptyState title="Reading the release…">Asking the span store.</EmptyState>
  const before = r.before.errorRate === null ? null : r.before.errorRate * 100
  const after = r.after.errorRate === null ? null : r.after.errorRate * 100
  const regressed = before !== null && after !== null && after - before >= HOT_RATE_POINTS
  const change = before === null || after === null ? '—' : `${after - before >= 0 ? '+' : '−'}${Math.abs(after - before).toFixed(1)} pts`
  return (
    <>
      <nav aria-label="Breadcrumb" className="text-control"><Link to="/releases">Releases</Link><span className="text-faint-foreground"> / </span><span className="font-mono">{r.service} {r.version}</span></nav>
      <div className="flex flex-wrap items-center gap-3">
        <Rocket className="size-5 text-muted-foreground" aria-hidden />
        <h1 className="font-mono text-title">{r.version}</h1>
        {regressed ? <Badge variant="warning"><TriangleAlert />Error rate rose after the deploy</Badge> : null}
        <span className="flex-1" />
        <Button asChild size="sm"><Link to={`/services/${encodeURIComponent(r.service)}`}>Open {r.service}</Link></Button>
      </div>
      <div className="flex flex-wrap gap-x-4 gap-y-1 text-control text-muted-foreground">
        <span>Service <span className="font-mono text-foreground">{r.service}</span></span>
        <span>Environments <span className="font-mono text-foreground">{envs(r)}</span></span>
        <span>First seen <span className="font-mono text-foreground">{stamp(r.firstSeenMs)}</span></span>
        <span>Last seen <span className="font-mono text-foreground">{stamp(r.lastSeenMs)}</span></span>
      </div>
      <div className="grid grid-cols-[repeat(auto-fit,minmax(160px,1fr))] gap-3">
        <Around label="Error rate, hour before" traffic={r.before} />
        <Around label="Error rate, hour after" traffic={r.after} />
        <Kpi label="Change" value={change} sub="hour after vs hour before, whole service" tone={regressed ? 'warn' : undefined} />
        <Kpi label="New issues" value={String(r.issues.length)} sub="first seen with this release" tone={r.issues.length ? 'err' : undefined} />
        <Kpi label="Requests" value={num(r.requests)} sub={`${num(r.errors)} with errors`} />
      </div>
      <Card className="overflow-hidden">
        <CardHeader><CardTitle>Issues introduced</CardTitle></CardHeader>
        {r.issues.length === 0 ? <p className="m-0 px-4 py-3 text-control text-muted-foreground">No exception was first seen while this release was the newest of {r.service}.</p> : null}
        {r.issues.map((issue) => (
          <Link key={issue.fingerprint} to={`/issues/${issue.fingerprint}`} className="flex flex-col gap-0.5 border-b border-border-subtle px-4 py-2 text-foreground no-underline last:border-b-0 hover:bg-row-hover">
            <span className="flex items-baseline gap-2"><span className="flex-1 truncate font-mono text-control font-semibold text-destructive-text">{issue.exceptionType}</span><span className="font-mono text-caption">{num(issue.events)}</span></span>
            <span className="flex gap-2 text-caption text-muted-foreground"><span className="truncate font-mono">{issue.message}</span><span className="ml-auto shrink-0 font-mono">{stamp(issue.firstSeenMs)}</span></span>
          </Link>
        ))}
      </Card>
    </>
  )
}
