import { CircleCheck, GitCommitHorizontal, GitPullRequest, Rocket, TriangleAlert } from 'lucide-react'
import { Link, useNavigate, useParams } from '@/lib/router'
import { toast } from 'sonner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardHeader, CardTitle } from '@/components/ui/card'
import { Avatar, Table, Td, Th } from '@/components/ui/misc'
import { Kpi, PageHeader, ServiceDot } from '@/components/ft/bits'
import { LineChart } from '@/components/ft/charts'
import { useRelease, useReleaseSeries, useReleases } from '@/data/releases'
import { NotFound } from './NotFound'
import { initialsOf } from '@/data/profile'
import { cn } from '@/lib/utils'

export function Releases() {
  const navigate = useNavigate()
  const releases = useReleases()
  return (
    <>
      <PageHeader title="Releases" meta="From service.version and the deploy events your CI sends" />
      <div className="-mx-4 overflow-x-auto border-t md:-mx-5">
        <Table aria-label="Releases">
          <thead><tr><Th>Release</Th><Th className="w-[200px]">Services</Th><Th className="w-[140px]">Deployed</Th><Th className="w-[150px]">By</Th><Th numeric className="w-[100px]">New issues</Th><Th numeric className="w-[110px]">Error rate</Th><Th numeric className="w-[80px]">p95</Th><Th className="w-[120px]">Health</Th></tr></thead>
          <tbody>
            {releases.map((r) => {
              const bad = r.status === 'regressed'
              return (
                <tr key={r.version} onClick={() => navigate(`/releases/${r.version}`)} className="cursor-pointer hover:[&>td]:bg-row-hover">
                  <Td><span className="flex flex-col leading-5"><Link to={`/releases/${r.version}`} onClick={(e) => e.stopPropagation()} className="font-mono font-semibold text-foreground">{r.version}</Link><span className="text-caption text-muted-foreground">{r.project} · {r.commits} commits · <span className="font-mono">{r.commit}</span></span></span></Td>
                  <Td><span className="flex flex-wrap gap-x-2.5">{r.services.map((s) => <span key={s} className="inline-flex items-center gap-1.5 text-control"><ServiceDot name={s} />{s}</span>)}</span></Td>
                  <Td className="font-mono text-control">{r.deployed}</Td>
                  <Td><span className="inline-flex items-center gap-2 text-control"><Avatar initials={initialsOf(r.by)} className="size-6 text-[10px]" />{r.by}</span></Td>
                  <Td numeric className={cn(bad && r.newIssues && 'font-semibold text-destructive-text')}>{bad ? r.newIssues : r.version === '2.14.1' ? 0 : r.newIssues}</Td>
                  <Td numeric className={cn(bad && 'font-semibold text-warning-text')}>{bad ? r.errDelta : r.version === '2.14.1' ? '+0.0 pts' : r.errDelta}</Td>
                  <Td numeric className={cn(bad && 'text-warning-text')}>{bad ? r.p95Delta : r.version === '2.14.1' ? '−2 %' : r.p95Delta}</Td>
                  <Td>{bad ? <Badge variant="warning"><TriangleAlert />Regressed</Badge> : <Badge variant="success"><CircleCheck />Healthy</Badge>}</Td>
                </tr>
              )
            })}
          </tbody>
        </Table>
        <p className="m-0 px-5 py-3 text-caption text-muted-foreground">Deltas compare the hour after the deploy with the hour before. Send a deploy event from CI: <span className="font-mono">npx ferristrace deploy --release 2.14.1</span></p>
      </div>
    </>
  )
}

export function ReleaseDetail() {
  const { version } = useParams()
  const r = useRelease(version)
  const bad = r?.status === 'regressed'
  const { at, errorRate, p95 } = useReleaseSeries(r, bad)
  if (!r) return <NotFound />
  return (
    <>
      <nav aria-label="Breadcrumb" className="text-control"><Link to="/releases">Releases</Link><span className="text-faint-foreground"> / </span><span className="font-mono">{r.version}</span></nav>
      <div className="flex flex-wrap items-center gap-3">
        <Rocket className="size-5 text-muted-foreground" aria-hidden />
        <h1 className="font-mono text-title">{r.version}</h1>
        {bad ? <Badge variant="warning"><TriangleAlert />Regressed after deploy</Badge> : <Badge variant="success"><CircleCheck />Healthy</Badge>}
        <span className="flex-1" />
        {bad ? <Button size="sm" onClick={() => toast('A rollback is triggered from your CI', { description: 'Ferristrace posts the link to the previous release 2.14.0.' })}>Compare with 2.14.0</Button> : null}
      </div>
      <div className="flex flex-wrap gap-x-4 gap-y-1 text-control text-muted-foreground">
        <span>Project <span className="font-semibold text-foreground">{r.project}</span></span><span>Deployed <span className="font-mono text-foreground">{r.deployed}</span></span><span>By <span className="text-foreground">{r.by}</span></span><span className="inline-flex items-center gap-1"><GitCommitHorizontal className="size-3.5" aria-hidden /><span className="font-mono text-foreground">{r.commit}</span></span><span>Adoption <span className="font-mono text-foreground">{r.adoption} %</span></span>
      </div>
      <div className="grid grid-cols-[repeat(auto-fit,minmax(160px,1fr))] gap-3">
        <Kpi label="Error rate change" value={bad ? r.errDelta : '+0.0 pts'} sub="hour after vs hour before" tone={bad ? 'warn' : undefined} />
        <Kpi label="p95 change" value={bad ? r.p95Delta : '−2 %'} sub={r.services.join(', ')} tone={bad ? 'warn' : undefined} />
        <Kpi label="Commits" value={String(r.commits)} sub={`${r.prs.length} pull requests`} />
      </div>
      <div className="grid grid-cols-[repeat(auto-fit,minmax(min(420px,100%),1fr))] gap-3">
        <LineChart title="Error rate around the deploy" legend={r.services[0]} values={errorRate} threshold={2} hi={6} yLabels={['6 %', '3 %', '0']} aria="Error rate before and after the deploy" height={120} markers={[{ at, label: r.version }]} />
        <LineChart title="p95 latency around the deploy" legend={r.services[0]} values={p95} hi={2000} yLabels={['2 s', '1 s', '0']} aria="p95 before and after the deploy" height={120} markers={[{ at, label: r.version }]} />
      </div>
      <div className="grid grid-cols-[repeat(auto-fit,minmax(min(420px,100%),1fr))] items-start gap-3">
        <Card className="overflow-hidden">
          <CardHeader><CardTitle>What changed</CardTitle></CardHeader>
          {r.prs.map(([n, t]) => (
            <div key={n} className="flex items-center gap-2.5 border-b border-border-subtle px-4 py-2.5 text-control last:border-b-0"><GitPullRequest className="size-3.5 text-muted-foreground" aria-hidden /><span className="font-mono text-muted-foreground">{n}</span><span className="flex-1">{t}</span></div>
          ))}
        </Card>
      </div>
    </>
  )
}
