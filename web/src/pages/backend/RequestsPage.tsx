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
  useRequestRoutes,
  useRequestSeries,
  type Route,
} from '@/data/requests'
import { useZoom } from '@/lib/bounds'
import { useWindow } from '@/lib/window'
import { because } from '@/auth/client'
import { useSettled } from '@/lib/settle'
import { dur } from '@/lib/format'
import { cn } from '@/lib/utils'

const BROKEN_THRESHOLD = 1

const SORTS = [
  { value: 'broken', label: '5xx rate' },
  { value: 'refused', label: '4xx rate' },
  { value: 'traffic', label: 'Requests' },
  { value: 'p95', label: 'p95' },
  { value: 'name', label: 'Name' },
] as const

type Sort = (typeof SORTS)[number]['value']

const sortOf = (value: unknown): Sort => SORTS.find((one) => one.value === value)?.value ?? 'broken'

const percent = (rate: number | null) => (rate === null ? '—' : `${rate.toFixed(1)} %`)

function sorted(routes: Route[], by: Sort): Route[] {
  const order: Record<Sort, (a: Route, b: Route) => number> = {
    broken: (a, b) => (b.brokenRate ?? -1) - (a.brokenRate ?? -1),
    refused: (a, b) => (b.refusedRate ?? -1) - (a.refusedRate ?? -1),
    traffic: (a, b) => b.perSecond - a.perSecond,
    p95: (a, b) => b.p95Us - a.p95Us,
    name: (a, b) => a.route.localeCompare(b.route),
  }
  return [...routes].sort(order[by])
}


export function RequestsPage() {
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
  const { routes, pending, failed } = useRequestRoutes(group, windowMs, settledEnv, placement)
  const { series, pending: seriesPending } = useRequestSeries(windowMs, settledEnv, placement)

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
        routes.filter((row) => `${row.route} ${row.service} ${row.method}`.toLowerCase().includes(q.trim().toLowerCase())),
        sort,
      ),
    [routes, sort, q],
  )
  const totalUs = useMemo(() => rows.reduce((sum, row) => sum + row.totalUs, 0), [rows])
  const bars = useMemo<Bar[]>(
    () => (series?.points ?? []).map((point) => ({ at: point.atMs, count: point.requests, errors: point.broken })),
    [series],
  )
  const { noun, field } = groupingNoun(group)
  const requests = series?.requests ?? 0
  const seconds = zoom ? (zoom.to - zoom.from) / 1000 : windowMs / 1000
  const span = zoom ? `${Math.max(1, Math.round(seconds))} s` : label
  const placing = placedCount(placement)
  const sifting = placing > 0 || settledEnv.trim() !== '' || q.trim() !== ''

  return (
    <>
      <PageHeader
        title="Requests"
        meta={
          <>
            HTTP server spans, grouped by <span className="font-mono">{field}</span> · last {span}
          </>
        }
      >
        <Segmented label="Group by" value={group} onChange={(next) => show({ g: next === 'route' ? undefined : next })} options={GROUPINGS.map((one) => ({ value: one.value, label: one.label }))} />
      </PageHeader>

      <div className="grid grid-cols-[repeat(auto-fit,minmax(190px,1fr))] gap-3">
        <Kpi label="Requests" value={requests === 0 ? '—' : `${(requests / seconds).toFixed(1)}/s`} sub={`${requests.toLocaleString()} in the last ${span}`} />
        <Kpi
          label="5xx rate"
          value={percent(series?.brokenRate ?? null)}
          sub={requests === 0 ? 'no requests yet' : `${(series?.broken ?? 0).toLocaleString()} of ${requests.toLocaleString()} requests`}
          tone={series?.brokenRate !== null && series?.brokenRate !== undefined && series.brokenRate > BROKEN_THRESHOLD ? 'warn' : undefined}
        />
        <Kpi
          label="4xx rate"
          value={percent(series?.refusedRate ?? null)}
          sub={requests === 0 ? 'no requests yet' : `${(series?.refused ?? 0).toLocaleString()} of ${requests.toLocaleString()} requests`}
        />
        <Kpi
          label="p95"
          value={series?.p95Ms === null || series?.p95Ms === undefined ? '—' : dur(series.p95Ms)}
          sub={series?.p99Ms === null || series?.p99Ms === undefined ? 'no p99 yet' : `p99 ${dur(series.p99Ms)}`}
        />
      </div>

      <div className="flex flex-col gap-1.5">
        {zoom ? (
          <div className="flex items-center gap-2 text-caption text-muted-foreground">
            <span>Zoomed on a period of {Math.max(1, Math.round(seconds))} s.</span>
            <Button size="sm" variant="ghost" onClick={() => zoomTo(0, undefined)}>Back to the last {label}</Button>
          </div>
        ) : null}
        <Bars bars={bars} loading={seriesPending} onZoom={(from, to) => zoomTo(from, to)} noun="requests" title="Requests over time" />
      </div>

      <Card className="overflow-hidden">
        <div className="flex flex-wrap items-center gap-2 border-b px-4 py-2.5">
          <h2 className="text-[15px] font-bold">{noun === 'service' ? 'Services' : noun === 'method' ? 'Methods' : 'Routes'}</h2>
          <span className="text-caption text-muted-foreground">
            {rows.length} {noun}
            {rows.length === 1 ? '' : 's'}
          </span>
          <span className="flex-1" />
          <Segmented label="Sort by" value={sort} onChange={(next) => show({ s: next === 'broken' ? undefined : next })} options={SORTS.map((one) => ({ value: one.value, label: one.label }))} />
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
            <EmptyState title="The requests could not be read">{because(failed, 'The span store did not answer')}</EmptyState>
          </div>
        ) : null}

        {!failed && !pending && routes.length === 0 ? (
          <div className="p-6">
            <EmptyState
              title={sifting ? 'No requests match' : `No requests in the last ${span}`}
              code={sifting ? undefined : 'npm i @opentelemetry/instrumentation-http'}
              actions={sifting ? <Button size="sm" onClick={() => { setTypedEnv(''); setQ(''); place({ cluster: '', namespace: '', workload: '', pod: '', node: '' }) }}>Reset filters</Button> : null}
            >
              {sifting ? (
                'Try a wider window, no placement, or no environment.'
              ) : (
                <>
                  Requests appear once your HTTP server is instrumented. FerrisTrace reads a server span that carries{' '}
                  <span className="font-mono">http.request.method</span> (or the older <span className="font-mono">http.method</span>) or a status
                  code, as OpenTelemetry's HTTP auto-instrumentation records it.
                </>
              )}
            </EmptyState>
          </div>
        ) : null}

        {routes.length > 0 ? (
          <div className="overflow-x-auto">
            <Table aria-label={`Requests by ${noun}`}>
              <thead>
                <tr>
                  <Th>{noun === 'service' ? 'Service' : noun === 'method' ? 'Method' : 'Route'}</Th>
                  {group === 'service' ? null : <Th className="w-[130px]">Service</Th>}
                  {group === 'method' ? null : <Th className="w-[90px]">Method</Th>}
                  <Th numeric className="w-[100px]">Requests</Th>
                  <Th numeric className="w-[80px]">4xx</Th>
                  <Th numeric className="w-[80px]">5xx</Th>
                  <Th className="w-[160px]">Share of time</Th>
                  <Th numeric className="w-[80px]">p50</Th>
                  <Th numeric className="w-[80px]">p95</Th>
                </tr>
              </thead>
              <tbody>
                {rows.map((row) => {
                  const broken = row.brokenRate !== null && row.brokenRate > BROKEN_THRESHOLD
                  const share = totalUs === 0 ? 0 : (row.totalUs / totalUs) * 100
                  return (
                    <Tr key={`${row.route} ${row.service} ${row.method}`}>
                      <Td><span className="font-mono font-semibold">{row.route}</span></Td>
                      {group === 'service' ? null : <Td className="text-control text-muted-foreground">{row.service}</Td>}
                      {group === 'method' ? null : <Td className="text-control text-muted-foreground">{row.method}</Td>}
                      <Td numeric>{row.rps}</Td>
                      <Td numeric>{row.refused.toLocaleString()}</Td>
                      <Td numeric className={cn(broken && 'font-semibold text-destructive-text')}>{row.broken.toLocaleString()}</Td>
                      <Td>
                        <span className="inline-flex max-w-full items-center gap-1.5">
                          <span className="inline-flex h-1.5 w-14 shrink-0 overflow-hidden rounded bg-muted">
                            <span className={cn('rounded', barBg('bar-o'))} style={{ width: `${Math.min(100, share)}%` }} />
                          </span>
                          <span className="font-mono text-caption">{share.toFixed(1)} %</span>
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
          A request is an HTTP server span: <span className="font-mono">http.request.method</span>, the older{' '}
          <span className="font-mono">http.method</span>, or a recorded status code. 4xx and 5xx read that status code; a span carrying none counts as
          5xx only when its span status is an error, so a span with neither is in the total and in no class.{' '}
          {group === 'route' ? (
            <>
              The route is <span className="font-mono">http.route</span>, falling back to the span name when a router names none.
            </>
          ) : group === 'service' ? (
            'Every route and method of a service is folded into one row.'
          ) : (
            'Every route and service of a method is folded into one row.'
          )}{' '}
          Share of time is this {noun}'s share of the total time spent across the {noun}s shown. Amber above {BROKEN_THRESHOLD} % 5xx.
        </p>
      </Card>
    </>
  )
}
