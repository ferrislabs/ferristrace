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
import { Bars, type Bar } from '@/components/ft/Histogram'
import { PlacementPicker, placedCount } from '@/components/ft/Placement'
import { usePlacement } from '@/data/placement'
import {
  GROUPINGS,
  groupingNoun,
  groupingOf,
  useStatementFanouts,
  useStatements,
  useStatementSeries,
  type Statement,
} from '@/data/statements'
import { ERROR_THRESHOLD } from '@/data/services'
import { useZoom } from '@/lib/bounds'
import { useWindow } from '@/lib/window'
import { because } from '@/auth/client'
import { useSettled } from '@/lib/settle'
import { dur } from '@/lib/format'
import { cn } from '@/lib/utils'

const SORTS = [
  { value: 'time', label: 'Total time' },
  { value: 'calls', label: 'Calls' },
  { value: 'errors', label: 'Error rate' },
  { value: 'name', label: 'Name' },
] as const

type Sort = (typeof SORTS)[number]['value']

const sortOf = (value: unknown): Sort => SORTS.find((one) => one.value === value)?.value ?? 'time'

const percent = (rate: number | null) => (rate === null ? '—' : `${rate.toFixed(1)} %`)

function sorted(statements: Statement[], by: Sort): Statement[] {
  const order: Record<Sort, (a: Statement, b: Statement) => number> = {
    time: (a, b) => b.totalUs - a.totalUs,
    calls: (a, b) => b.perSecond - a.perSecond,
    errors: (a, b) => (b.errorRate ?? -1) - (a.errorRate ?? -1),
    name: (a, b) => a.group.localeCompare(b.group),
  }
  return [...statements].sort(order[by])
}

export function QueriesPage() {
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
  const { statements, pending, failed } = useStatements(group, windowMs, settledEnv, placement)
  const { series } = useStatementSeries(windowMs, settledEnv, placement)
  const { fanouts, least } = useStatementFanouts(windowMs, settledEnv, placement)

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
        statements.filter((row) => `${row.group} ${row.database} ${row.system}`.toLowerCase().includes(q.trim().toLowerCase())),
        sort,
      ),
    [statements, sort, q],
  )
  const bars = useMemo<Bar[]>(
    () => (series?.points ?? []).map((point) => ({ at: point.atMs, count: point.calls, errors: point.errors })),
    [series],
  )
  const { noun, field } = groupingNoun(group)
  const calls = series?.calls ?? 0
  const seconds = zoom ? (zoom.to - zoom.from) / 1000 : windowMs / 1000
  const span = zoom ? `${Math.max(1, Math.round(seconds))} s` : label
  const placing = placedCount(placement)
  const sifting = placing > 0 || settledEnv.trim() !== '' || q.trim() !== ''
  const totalUs = rows.reduce((sum, row) => sum + row.totalUs, 0)
  const bad = series?.errorRate !== null && series?.errorRate !== undefined && series.errorRate >= ERROR_THRESHOLD

  return (
    <>
      <PageHeader
        title="Queries"
        meta={
          <>
            Database client spans, grouped by <span className="font-mono">{field}</span> · last {span}
          </>
        }
      >
        <Segmented label="Group by" value={group} onChange={(next) => show({ g: next === 'statement' ? undefined : next })} options={GROUPINGS.map((one) => ({ value: one.value, label: one.label }))} />
      </PageHeader>

      <div className="grid grid-cols-[repeat(auto-fit,minmax(190px,1fr))] gap-3">
        <Kpi label="Calls" value={calls === 0 ? '—' : `${(calls / seconds).toFixed(1)}/s`} sub={`${calls.toLocaleString()} in the last ${span}`} />
        <Kpi
          label="Errors"
          value={percent(series?.errorRate ?? null)}
          sub={`${(series?.errors ?? 0).toLocaleString()} of ${calls.toLocaleString()} calls`}
          tone={bad ? 'warn' : undefined}
        />
        <Kpi
          label="p95"
          value={series?.p95Ms === null || series?.p95Ms === undefined ? '—' : dur(series.p95Ms)}
          sub={series?.p99Ms === null || series?.p99Ms === undefined ? 'no p99 yet' : `p99 ${dur(series.p99Ms)}`}
        />
        <Kpi
          label="Possible N+1"
          value={fanouts.length === 0 ? '—' : String(fanouts.length)}
          sub={
            fanouts.length === 0
              ? `no statement ran ${least || 20}+ times in one trace`
              : `worst: ${fanouts[0].worst.toLocaleString()} calls in one trace`
          }
          tone={fanouts.length > 0 ? 'warn' : undefined}
        />
      </div>

      <div className="flex flex-col gap-1.5">
        {zoom ? (
          <div className="flex items-center gap-2 text-caption text-muted-foreground">
            <span>Zoomed on a period of {Math.max(1, Math.round(seconds))} s.</span>
            <Button size="sm" variant="ghost" onClick={() => zoomTo(0, undefined)}>Back to the last {label}</Button>
          </div>
        ) : null}
        <Bars bars={bars} loading={pending} onZoom={(from, to) => zoomTo(from, to)} noun="calls" title="Database calls over time" />
      </div>

      {fanouts.length > 0 ? (
        <Card className="overflow-hidden">
          <div className="flex flex-wrap items-center gap-2 border-b px-4 py-2.5">
            <h2 className="text-[15px] font-bold">Possible N+1</h2>
            <span className="text-caption text-muted-foreground">
              {fanouts.length} statement{fanouts.length === 1 ? '' : 's'} ran {least}+ times inside one trace
            </span>
          </div>
          <div className="overflow-x-auto">
            <Table aria-label="Statements that fan out inside a trace">
              <thead>
                <tr>
                  <Th>Statement</Th>
                  <Th numeric className="w-[110px]">Worst trace</Th>
                  <Th numeric className="w-[90px]">Traces</Th>
                  <Th numeric className="w-[90px]">Calls</Th>
                </tr>
              </thead>
              <tbody>
                {fanouts.map((fanout) => (
                  <Tr key={fanout.statement}>
                    <Td>
                      <span className="truncate font-mono text-control" title={fanout.statement}>
                        {fanout.statement}
                      </span>
                    </Td>
                    <Td numeric className="font-semibold text-warning-text">{fanout.worst.toLocaleString()}</Td>
                    <Td numeric>{fanout.traces.toLocaleString()}</Td>
                    <Td numeric>{fanout.calls.toLocaleString()}</Td>
                  </Tr>
                ))}
              </tbody>
            </Table>
          </div>
          <p className="m-0 px-4 py-2.5 text-caption text-muted-foreground">
            The same normalised statement ran {least} or more times inside a single trace, which is what an N+1 looks
            like. Counted over the spans in this window, so a trace that started before it is counted only in part.
          </p>
        </Card>
      ) : null}

      <Card className="overflow-hidden">
        <div className="flex flex-wrap items-center gap-2 border-b px-4 py-2.5">
          <h2 className="text-[15px] font-bold">{noun === 'database' ? 'Databases' : noun === 'operation' ? 'Operations' : 'Statements'}</h2>
          <span className="text-caption text-muted-foreground">
            {rows.length} {noun}
            {rows.length === 1 ? '' : 's'}
          </span>
          <span className="flex-1" />
          <Segmented label="Sort by" value={sort} onChange={(next) => show({ s: next === 'time' ? undefined : next })} options={SORTS.map((one) => ({ value: one.value, label: one.label }))} />
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
            <EmptyState title="The queries could not be read">{because(failed, 'The span store did not answer')}</EmptyState>
          </div>
        ) : null}

        {!failed && !pending && statements.length === 0 ? (
          <div className="p-6">
            <EmptyState
              title={sifting ? 'No database statements match' : `No database statements in the last ${span}`}
              code={sifting ? undefined : 'npm i @opentelemetry/instrumentation-pg'}
              actions={sifting ? <Button size="sm" onClick={() => { setTypedEnv(''); setQ(''); place({ cluster: '', namespace: '', workload: '', pod: '', node: '' }) }}>Reset filters</Button> : null}
            >
              {sifting ? (
                'Try a wider window, no placement, or no environment.'
              ) : (
                <>
                  Statements appear once your database client is instrumented with an OpenTelemetry database instrumentation such as{' '}
                  <span className="font-mono">@opentelemetry/instrumentation-pg</span>. FerrisTrace reads a client span naming its system in{' '}
                  <span className="font-mono">db.system.name</span>, and the statement text in <span className="font-mono">db.query.text</span>. Redis, Memcached and Valkey calls are on the{' '}
                  Cache page instead.
                </>
              )}
            </EmptyState>
          </div>
        ) : null}

        {rows.length > 0 ? (
          <div className="overflow-x-auto">
            <Table aria-label={`Database ${noun}s`}>
              <thead>
                <tr>
                  <Th>{noun === 'database' ? 'Database' : noun === 'operation' ? 'Operation' : 'Statement'}</Th>
                  {group === 'database' ? null : <Th className="w-[130px]">Database</Th>}
                  <Th className="w-[90px]">System</Th>
                  <Th numeric className="w-[100px]">Calls</Th>
                  <Th numeric className="w-[90px]">Errors</Th>
                  <Th numeric className="w-[80px]">p50</Th>
                  <Th numeric className="w-[80px]">p95</Th>
                  <Th className="w-[160px]">Share of time</Th>
                </tr>
              </thead>
              <tbody>
                {rows.map((row) => {
                  const cold = row.errorRate !== null && row.errorRate >= ERROR_THRESHOLD
                  const share = totalUs === 0 ? 0 : (row.totalUs / totalUs) * 100
                  return (
                    <Tr key={`${row.group} ${row.database} ${row.system}`}>
                      <Td className="max-w-[420px] truncate" title={row.group}>
                        <span className="font-mono font-semibold">{row.group}</span>
                      </Td>
                      {group === 'database' ? null : <Td className="text-control text-muted-foreground">{row.database}</Td>}
                      <Td className="text-control text-muted-foreground">{row.system}</Td>
                      <Td numeric>{row.rps}</Td>
                      <Td numeric className={cn(cold && 'font-semibold text-warning-text')}>
                        {row.errorRate === null ? '—' : percent(row.errorRate)}
                      </Td>
                      <Td numeric>{row.p50}</Td>
                      <Td numeric>{row.p95}</Td>
                      <Td>
                        <span className="inline-flex max-w-full items-center gap-1.5">
                          <span className="inline-flex h-1.5 w-14 shrink-0 overflow-hidden rounded bg-muted">
                            <span className={cn('rounded', barBg())} style={{ width: `${Math.min(100, share)}%` }} />
                          </span>
                          <span className="font-mono text-caption">{share.toFixed(1)} %</span>
                        </span>
                      </Td>
                    </Tr>
                  )
                })}
              </tbody>
            </Table>
          </div>
        ) : null}

        <p className="m-0 px-4 py-2.5 text-caption text-muted-foreground">
          A statement is <span className="font-mono">db.query.text</span>, falling back to <span className="font-mono">db.statement</span> and then the span name, with its numbers and
          single-quoted strings replaced by <span className="font-mono">?</span> so <span className="font-mono">id = 42</span> reads <span className="font-mono">id = ?</span>. Redis, Memcached
          and Valkey calls are excluded, they have their own Cache page. Share of time is each row's share of the total time spent across the {rows.length} {noun}
          {rows.length === 1 ? '' : 's'} shown. Amber from {ERROR_THRESHOLD} %, the status page threshold.
        </p>
      </Card>
    </>
  )
}
