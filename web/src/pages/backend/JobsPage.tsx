import { Boxes, Search } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useNavigate as useRouterNavigate } from '@tanstack/react-router'
import { useSearchParam } from '@/lib/router'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import { InputGroup } from '@/components/ui/input'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Segmented } from '@/components/ui/segmented'
import { Kbd, Table, Td, Th, Tr } from '@/components/ui/misc'
import { EmptyState, Kpi, PageHeader, barBg } from '@/components/ft/bits'
import { PlacementPicker, placedCount } from '@/components/ft/Placement'
import { usePlacement } from '@/data/placement'
import { GROUPINGS, groupingNoun, groupingOf, useJobs, useJobSeries, type Run, type Series } from '@/data/jobs'
import { useZoom } from '@/lib/bounds'
import { useWindow } from '@/lib/window'
import { because } from '@/auth/client'
import { useSettled } from '@/lib/settle'
import { dur } from '@/lib/format'
import { cn } from '@/lib/utils'

const HOT = 2

const SORTS = [
  { value: 'rate', label: 'Failure rate' },
  { value: 'processed', label: 'Processed' },
  { value: 'failed', label: 'Failed' },
  { value: 'name', label: 'Name' },
] as const

type Sort = (typeof SORTS)[number]['value']

const sortOf = (value: unknown): Sort => SORTS.find((one) => one.value === value)?.value ?? 'rate'

const percent = (rate: number | null) => (rate === null ? '—' : `${rate.toFixed(1)} %`)

function sorted(runs: Run[], by: Sort): Run[] {
  const order: Record<Sort, (a: Run, b: Run) => number> = {
    rate: (a, b) => (b.failureRate ?? -1) - (a.failureRate ?? -1),
    processed: (a, b) => b.perSecond - a.perSecond,
    failed: (a, b) => b.failed - a.failed,
    name: (a, b) => a.group.localeCompare(b.group),
  }
  return [...runs].sort(order[by])
}

function JobBars({ series, pending, onZoom }: { series: Series | null; pending: boolean; onZoom: (from: number, to: number) => void }) {
  const bars = series?.points ?? []
  const step = series?.stepMs ?? 0
  const peak = Math.max(1, ...bars.map((bar) => bar.processed))
  const total = bars.reduce((sum, bar) => sum + bar.processed, 0)
  const failed = bars.reduce((sum, bar) => sum + bar.failed, 0)

  if (bars.length === 0 || total === 0) {
    return (
      <div className="flex h-[84px] items-center justify-center rounded-lg border border-dashed text-caption text-muted-foreground" role="status">
        {pending ? 'Counting the jobs…' : 'No jobs match in this window.'}
      </div>
    )
  }

  return (
    <figure className="m-0 flex flex-col gap-2 rounded-lg border bg-card px-3 pb-2.5 pt-2.5">
      <div className="flex items-baseline gap-2 text-caption">
        <span className="font-semibold text-foreground">Jobs processed over time</span>
        <span className="text-muted-foreground">
          {total.toLocaleString()} in all{failed > 0 ? `, ${failed.toLocaleString()} failed` : ''}
        </span>
        <span className="ml-auto hidden text-faint-foreground sm:inline">Click a bar to zoom on that step</span>
      </div>
      <div className="flex h-[84px] items-end gap-[2px]" role="img" aria-label={`${total.toLocaleString()} jobs processed, ${failed.toLocaleString()} failed`}>
        {bars.map((bar) => {
          const tall = bar.processed === 0 ? 0 : Math.max(3, (bar.processed / peak) * 100)
          const share = bar.processed === 0 ? 0 : Math.min(1, bar.failed / bar.processed)
          return (
            <button
              key={bar.atMs}
              type="button"
              aria-label={`${bar.processed} processed, ${bar.failed} failed`}
              className="relative flex h-full min-w-0 flex-1 cursor-pointer items-end border-0 bg-transparent p-0"
              onClick={() => onZoom(bar.atMs, bar.atMs + step)}
            >
              <span className="relative w-full overflow-hidden rounded-t-[3px] bg-primary opacity-60 transition-opacity hover:opacity-100" style={{ height: `${tall}%` }}>
                {share > 0 ? <span className="absolute inset-x-0 bottom-0 bg-destructive" style={{ height: `${Math.max(share * 100, 12)}%` }} /> : null}
              </span>
            </button>
          )
        })}
      </div>
    </figure>
  )
}

export function JobsPage() {
  const goTo = useRouterNavigate()
  const { ms: windowMs, label } = useWindow()
  const group = groupingOf(useSearchParam('g'))
  const sort = sortOf(useSearchParam('s'))
  const environment = useSearchParam('env') ?? ''
  const [typedEnv, setTypedEnv] = useState(environment)
  const settledEnv = useSettled(typedEnv)
  const [q, setQ] = useState('')
  const zoom = useZoom()
  const { placement, place } = usePlacement()
  const { jobs, pending, failed } = useJobs(group, windowMs, settledEnv, placement)
  const { series, pending: seriesPending } = useJobSeries(windowMs, settledEnv, placement)

  const show = (part: Record<string, string | undefined>) =>
    void goTo({
      to: '.' as never,
      search: ((was: Record<string, unknown>) => ({ ...was, ...part })) as never,
      replace: true,
    })
  const zoomTo = (from: number, to: number | undefined) =>
    void goTo({
      to: '.' as never,
      search: ((was: Record<string, unknown>) => ({
        ...was,
        from: to === undefined ? undefined : Math.round(from),
        to: to === undefined ? undefined : Math.round(to),
      })) as never,
      replace: true,
    })

  const rows = useMemo(
    () =>
      sorted(
        jobs.filter((row) => `${row.group} ${row.queue} ${row.system}`.toLowerCase().includes(q.trim().toLowerCase())),
        sort,
      ),
    [jobs, sort, q],
  )
  const { noun, field } = groupingNoun(group)
  const processed = series?.processed ?? 0
  const seconds = zoom ? (zoom.to - zoom.from) / 1000 : windowMs / 1000
  const span = zoom ? `${Math.max(1, Math.round(seconds))} s` : label
  const placing = placedCount(placement)
  const sifting = placing > 0 || settledEnv.trim() !== '' || q.trim() !== ''
  const hot = series?.failureRate !== null && series?.failureRate !== undefined && series.failureRate > HOT

  return (
    <>
      <PageHeader
        title="Jobs"
        meta={
          <>
            Messaging consumer spans, grouped by <span className="font-mono">{field}</span> · last {span}
          </>
        }
      >
        <Segmented label="Group by" value={group} onChange={(next) => show({ g: next === 'job' ? undefined : next })} options={GROUPINGS.map((one) => ({ value: one.value, label: one.label }))} />
      </PageHeader>

      <div className="grid grid-cols-[repeat(auto-fit,minmax(190px,1fr))] gap-3">
        <Kpi label="Processed" value={processed === 0 ? '—' : `${(processed / seconds).toFixed(1)}/s`} sub={`${processed.toLocaleString()} in the last ${span}`} />
        <Kpi
          label="Failed"
          value={percent(series?.failureRate ?? null)}
          sub={processed === 0 ? 'nothing processed yet' : `${(series?.failed ?? 0).toLocaleString()} of ${processed.toLocaleString()}`}
          tone={hot ? 'warn' : undefined}
        />
        <Kpi
          label="p95"
          value={series?.p95Ms === null || series?.p95Ms === undefined ? '—' : dur(series.p95Ms)}
          sub={series?.p99Ms === null || series?.p99Ms === undefined ? 'no p99 yet' : `p99 ${dur(series.p99Ms)}`}
        />
        <Kpi
          label="p50"
          value={series?.p50Ms === null || series?.p50Ms === undefined ? '—' : dur(series.p50Ms)}
          sub="median processing time"
        />
      </div>

      <div className="flex flex-col gap-1.5">
        {zoom ? (
          <div className="flex items-center gap-2 text-caption text-muted-foreground">
            <span>Zoomed on a period of {Math.max(1, Math.round(seconds))} s.</span>
            <Button size="sm" variant="ghost" onClick={() => zoomTo(0, undefined)}>Back to the last {label}</Button>
          </div>
        ) : null}
        <JobBars series={series} pending={seriesPending} onZoom={(from, to) => zoomTo(from, to)} />
      </div>

      <Card className="overflow-hidden">
        <div className="flex flex-wrap items-center gap-2 border-b px-4 py-2.5">
          <h2 className="text-[15px] font-bold">{noun === 'queue' ? 'Queues' : noun === 'system' ? 'Systems' : 'Jobs'}</h2>
          <span className="text-caption text-muted-foreground">
            {rows.length} {noun}
            {rows.length === 1 ? '' : 's'}
          </span>
          <span className="flex-1" />
          <Segmented label="Sort by" value={sort} onChange={(next) => show({ s: next === 'rate' ? undefined : next })} options={SORTS.map((one) => ({ value: one.value, label: one.label }))} />
          <Popover>
            <PopoverTrigger asChild>
              <Button variant="outline" size="sm" aria-label="Kubernetes placement">
                <Boxes />Kubernetes
                {placing > 0 ? <span className="rounded-full bg-primary px-1.5 font-mono text-[11px] text-primary-foreground">{placing}</span> : null}
              </Button>
            </PopoverTrigger>
            <PopoverContent align="start" className="w-[min(560px,calc(100vw-32px))] p-3">
              <PlacementPicker placement={placement} onChange={place} windowMs={windowMs} />
            </PopoverContent>
          </Popover>
          <InputGroup className="w-[200px] max-w-full">
            <input aria-label="Environment" placeholder="Environment, e.g. production" value={typedEnv} onChange={(e) => setTypedEnv(e.target.value)} />
          </InputGroup>
          <InputGroup className="w-[240px] max-w-full">
            <Search aria-hidden />
            <input aria-label="Filter rows" placeholder={`Filter ${noun}s`} value={q} onChange={(e) => setQ(e.target.value)} />
            <Kbd>/</Kbd>
          </InputGroup>
        </div>

        {failed ? (
          <div className="p-6">
            <EmptyState title="The jobs could not be read">{because(failed, 'The span store did not answer')}</EmptyState>
          </div>
        ) : null}

        {!failed && !pending && jobs.length === 0 ? (
          <div className="p-6">
            <EmptyState
              title={sifting ? 'No jobs match' : `No jobs in the last ${span}`}
              code={sifting ? undefined : 'npm i @opentelemetry/instrumentation-kafkajs'}
              actions={sifting ? <Button size="sm" onClick={() => { setTypedEnv(''); setQ(''); place({ cluster: '', namespace: '', workload: '', pod: '', node: '' }) }}>Reset filters</Button> : null}
            >
              {sifting ? (
                'Try a wider window, no placement, or no environment.'
              ) : (
                <>
                  Jobs appear once your messaging consumer is instrumented. FerrisTrace reads a span of{' '}
                  <span className="font-mono">kind = consumer</span>, or one that carries{' '}
                  <span className="font-mono">messaging.operation.type = process</span> (the OpenTelemetry messaging semantic
                  convention for a processed message).
                </>
              )}
            </EmptyState>
          </div>
        ) : null}

        {rows.length > 0 ? (
          <div className="overflow-x-auto">
            <Table aria-label={`Jobs by ${noun}`}>
              <thead>
                <tr>
                  <Th>{noun === 'queue' ? 'Queue' : noun === 'system' ? 'System' : 'Job'}</Th>
                  {group === 'queue' ? null : <Th className="w-[130px]">Queue</Th>}
                  {group === 'system' ? null : <Th className="w-[110px]">System</Th>}
                  <Th numeric className="w-[100px]">Processed</Th>
                  <Th numeric className="w-[90px]">Failed</Th>
                  <Th className="w-[160px]">Share of time</Th>
                  <Th numeric className="w-[80px]">p50</Th>
                  <Th numeric className="w-[80px]">p95</Th>
                </tr>
              </thead>
              <tbody>
                {rows.map((row) => {
                  const rowHot = row.failureRate !== null && row.failureRate > HOT
                  return (
                    <Tr key={`${row.group} ${row.queue} ${row.system}`}>
                      <Td><span className="font-mono font-semibold">{row.group}</span></Td>
                      {group === 'queue' ? null : <Td className="text-control text-muted-foreground">{row.queue}</Td>}
                      {group === 'system' ? null : <Td className="text-control text-muted-foreground">{row.system}</Td>}
                      <Td numeric>{row.rps}</Td>
                      <Td numeric className={cn(rowHot && 'font-semibold text-warning-text')}>{row.failed.toLocaleString()}</Td>
                      <Td>
                        <span className="inline-flex max-w-full items-center gap-1.5">
                          <span className="inline-flex h-1.5 w-14 shrink-0 overflow-hidden rounded bg-muted">
                            <span className={cn('rounded', barBg('bar-o'))} style={{ width: `${Math.min(100, row.share)}%` }} />
                          </span>
                          <span className="font-mono text-caption">{row.share.toFixed(1)} %</span>
                        </span>
                      </Td>
                      <Td numeric>{row.p50}</Td>
                      <Td numeric>{row.p95}</Td>
                    </Tr>
                  )
                })}
              </tbody>
            </Table>
          </div>
        ) : null}

        <p className="m-0 px-4 py-2.5 text-caption text-muted-foreground">
          A job is a span carrying <span className="font-mono">kind = consumer</span>, or one marked{' '}
          <span className="font-mono">messaging.operation.type = process</span> (or the older{' '}
          <span className="font-mono">messaging.operation = process</span>); the job name is the span name, the queue is{' '}
          <span className="font-mono">messaging.destination.name</span> and the system is{' '}
          <span className="font-mono">messaging.system</span>, both falling back to <span className="font-mono">unknown</span>.{' '}
          FerrisTrace reports what the messaging instrumentation emits. OpenTelemetry defines no attribute for queue wait
          time, no retry counter and no next-run time, so none of those are shown here. Amber above {HOT} % failed.
        </p>
      </Card>
    </>
  )
}
