import { Boxes, Search } from 'lucide-react'
import { useId, useMemo, useRef, useState } from 'react'
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
import {
  GROUPINGS,
  groupingNoun,
  groupingOf,
  useRequestRoutes,
  useRequestSeries,
  type Point,
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

const HEIGHT = 84
const compact = new Intl.NumberFormat('en', { notation: 'compact', maximumFractionDigits: 1 })
const whole = new Intl.NumberFormat('en')
const clock = (ms: number) => new Date(ms).toTimeString().slice(0, 8)
const day = (ms: number) => new Date(ms).toISOString().slice(5, 10)
const stamp = (ms: number) => `${day(ms)} ${clock(ms)}`

function RequestBars({ points, loading, onZoom }: { points: Point[]; loading: boolean; onZoom: (from: number, to: number) => void }) {
  const id = useId()
  const box = useRef<HTMLDivElement>(null)
  const [drag, setDrag] = useState<{ from: number; to: number } | null>(null)
  const [hover, setHover] = useState<number | null>(null)
  const [cursor, setCursor] = useState<number | null>(null)
  const [anchor, setAnchor] = useState<number | null>(null)

  const step = points.length > 1 ? points[1].atMs - points[0].atMs : 0
  const start = points[0]?.atMs ?? 0
  const end = points.length > 0 ? points[points.length - 1].atMs + step : 0
  const peak = Math.max(1, ...points.map((point) => point.requests))
  const requests = points.reduce((sum, point) => sum + point.requests, 0)
  const broken = points.reduce((sum, point) => sum + point.broken, 0)

  const indexAt = (clientX: number): number => {
    const rect = box.current?.getBoundingClientRect()
    if (!rect || rect.width === 0 || points.length === 0) return 0
    const fraction = Math.min(1, Math.max(0, (clientX - rect.left) / rect.width))
    return Math.min(points.length - 1, Math.floor(fraction * points.length))
  }

  const apply = (a: number, b: number) => {
    if (points.length === 0) return
    const low = Math.min(a, b)
    const high = Math.max(a, b)
    onZoom(points[low].atMs, points[high].atMs + step)
  }

  const selecting = drag ?? (cursor !== null && anchor !== null ? { from: anchor, to: cursor } : null)
  const low = selecting ? Math.min(selecting.from, selecting.to) : 0
  const high = selecting ? Math.max(selecting.from, selecting.to) : 0
  const pointed = hover ?? cursor
  const bar = pointed !== null ? points[pointed] : undefined
  const left = pointed !== null && points.length > 0 ? ((pointed + 0.5) / points.length) * 100 : 0

  if (points.length === 0 || requests === 0) {
    return (
      <div className="flex h-[84px] items-center justify-center rounded-lg border border-dashed text-caption text-muted-foreground" role="status">
        {loading ? 'Counting the requests…' : 'No request matches in this window.'}
      </div>
    )
  }

  return (
    <figure className="m-0 flex flex-col gap-2 rounded-lg border bg-card px-3 pb-2.5 pt-2.5">
      <div className="flex items-baseline gap-2 text-caption">
        <span className="font-semibold text-foreground">Requests over time</span>
        <span className="text-muted-foreground">{whole.format(requests)} in all{broken > 0 ? `, ${whole.format(broken)} broken` : ''}</span>
        <span className="ml-auto hidden text-faint-foreground sm:inline">Drag to zoom on a period</span>
      </div>
      <div className="grid grid-cols-[34px_minmax(0,1fr)] gap-2">
        <div className="flex flex-col justify-between text-right font-mono text-[10px] leading-3 text-faint-foreground" style={{ height: HEIGHT }} aria-hidden>
          <span>{compact.format(peak)}</span>
          <span>0</span>
        </div>
        <div className="relative">
          <div
            ref={box}
            role="application"
            aria-roledescription="histogram with a time selection"
            aria-label={`Requests over time, from ${stamp(start)} to ${stamp(end)}, ${whole.format(requests)} in all. Drag to zoom on a period, or use the arrow keys, Shift to select, Enter to zoom, Escape to cancel.`}
            tabIndex={0}
            className="relative flex cursor-crosshair touch-none select-none items-end gap-[2px] border-b border-border focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-ring"
            style={{ height: HEIGHT, backgroundImage: 'linear-gradient(to bottom, var(--chart-grid) 1px, transparent 1px)', backgroundSize: '100% 50%' }}
            onPointerDown={(event) => {
              event.currentTarget.setPointerCapture(event.pointerId)
              const at = indexAt(event.clientX)
              setDrag({ from: at, to: at })
              setCursor(null)
              setAnchor(null)
            }}
            onPointerMove={(event) => {
              const at = indexAt(event.clientX)
              setHover(at)
              if (drag) setDrag({ from: drag.from, to: at })
            }}
            onPointerUp={() => {
              if (drag) apply(drag.from, drag.to)
              setDrag(null)
            }}
            onPointerLeave={() => setHover(null)}
            onPointerCancel={() => setDrag(null)}
            onKeyDown={(event) => {
              const last = points.length - 1
              if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') {
                event.preventDefault()
                const next = Math.min(last, Math.max(0, (cursor ?? (event.key === 'ArrowLeft' ? last : 0)) + (event.key === 'ArrowLeft' ? -1 : 1)))
                if (event.shiftKey) setAnchor((was) => was ?? cursor ?? next)
                else setAnchor(null)
                setCursor(next)
              } else if (event.key === 'Enter' && cursor !== null && anchor !== null) {
                event.preventDefault()
                apply(anchor, cursor)
                setAnchor(null)
                setCursor(null)
              } else if (event.key === 'Escape') {
                setAnchor(null)
                setCursor(null)
                setDrag(null)
              }
            }}
            onBlur={() => { setAnchor(null); setCursor(null) }}
          >
            {points.map((one, index) => {
              const tall = one.requests === 0 ? 0 : Math.max(3, (one.requests / peak) * 100)
              const share = one.requests === 0 ? 0 : Math.min(1, one.broken / one.requests)
              const dim = selecting !== null && (index < low || index > high)
              return (
                <div key={one.atMs} className="relative flex h-full min-w-0 flex-1 items-end" aria-hidden>
                  <div
                    className={cn('relative w-full overflow-hidden rounded-t-[3px] bg-primary transition-opacity', index === pointed ? 'opacity-100' : dim ? 'opacity-30' : 'opacity-60')}
                    style={{ height: `${tall}%` }}
                  >
                    {share > 0 ? <div className="absolute inset-x-0 bottom-0 bg-destructive" style={{ height: `${Math.max(share * 100, 12)}%` }} /> : null}
                  </div>
                </div>
              )
            })}
            {selecting ? (
              <div
                aria-hidden
                className="pointer-events-none absolute inset-y-0 rounded-sm border border-primary bg-primary/10"
                style={{ left: `${(low / points.length) * 100}%`, width: `${((high - low + 1) / points.length) * 100}%` }}
              />
            ) : null}
          </div>
          {bar ? (
            <div
              id={`${id}-tip`}
              role="tooltip"
              className="pointer-events-none absolute bottom-full z-20 mb-2 flex w-max max-w-[260px] -translate-x-1/2 flex-col gap-0.5 rounded-md border bg-popover px-2.5 py-1.5 text-caption shadow-md"
              style={{ left: `clamp(70px, ${left}%, calc(100% - 70px))` }}
            >
              <span className="font-mono text-[11px] text-muted-foreground">{clock(bar.atMs)} to {clock(bar.atMs + step)}</span>
              <span className="flex items-center gap-1.5"><span className="size-2 rounded-sm bg-primary" aria-hidden /><span className="font-semibold">{whole.format(bar.requests)}</span> requests</span>
              {bar.broken > 0 ? (
                <span className="flex items-center gap-1.5 text-destructive-text"><span className="size-2 rounded-sm bg-destructive" aria-hidden /><span className="font-semibold">{whole.format(bar.broken)}</span> broken ({((bar.broken / Math.max(1, bar.requests)) * 100).toFixed(1)}%)</span>
              ) : null}
            </div>
          ) : null}
        </div>
      </div>
      <div className="grid grid-cols-[34px_minmax(0,1fr)] gap-2 font-mono text-[10px] text-faint-foreground" aria-hidden>
        <span />
        <span className="flex justify-between">
          <span>{stamp(start)}</span>
          <span className="max-sm:hidden">{clock((start + end) / 2)}</span>
          <span>{stamp(end)}</span>
        </span>
      </div>
    </figure>
  )
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
  const totalNanos = useMemo(() => rows.reduce((sum, row) => sum + row.totalNanos, 0), [rows])
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
        <RequestBars points={series?.points ?? []} loading={seriesPending} onZoom={(from, to) => zoomTo(from, to)} />
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
                  const share = totalNanos === 0 ? 0 : (row.totalNanos / totalNanos) * 100
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
