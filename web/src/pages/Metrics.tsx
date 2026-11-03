import { useNavigate as useRouterNavigate } from '@tanstack/react-router'
import { Plus, Search, X } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { useSearchParam } from '@/lib/router'
import { Button } from '@/components/ui/button'
import { InputGroup, NativeSelect } from '@/components/ui/input'
import { EmptyState } from '@/components/ft/bits'
import { AttachToIncident } from '@/components/ft/AttachToIncident'
import { MetricChart, labelOf, type DeployMark } from '@/components/ft/MetricChart'
import { firstDeploys, releasePath, useDeploys } from '@/data/releases'
import {
  AGGREGATIONS,
  DEFAULT_AGG,
  DEFAULT_RANGE_MS,
  isPinned,
  useMetricCatalogue,
  useMetricSeries,
  type Aggregation,
  type MetricInfo,
  type MetricQuery,
} from '@/data/metrics'
import { ApiError, because } from '@/auth/client'
import { usePlacement } from '@/data/placement'
import { PlacementPicker } from '@/components/ft/Placement'
import { stamp } from '@/lib/format'
import { useSettled } from '@/lib/settle'
import { cn } from '@/lib/utils'
import { useWindow } from '@/lib/window'

type Picked = { name: string; agg: Aggregation | null; groupBy: string[]; rangeMs: number; fromMs: number | null; toMs: number | null; q: string }

const EVERY = Object.values(AGGREGATIONS).flat()

function decode(raw: string | undefined): Picked {
  const p = new URLSearchParams(raw ?? '')
  const number = (key: string) => {
    const value = Number(p.get(key))
    return p.get(key) && Number.isFinite(value) && value > 0 ? value : null
  }
  const from = number('from')
  const to = number('to')
  const pinned = from !== null && to !== null && from < to
  const agg = p.get('agg')
  return {
    name: p.get('metric') ?? '',
    agg: (EVERY as string[]).includes(agg ?? '') ? (agg as Aggregation) : null,
    groupBy: p.getAll('by'),
    rangeMs: DEFAULT_RANGE_MS,
    fromMs: pinned ? from : null,
    toMs: pinned ? to : null,
    q: p.get('q') ?? '',
  }
}

function encode(picked: Picked): string | undefined {
  const p = new URLSearchParams()
  if (picked.name) p.set('metric', picked.name)
  if (picked.agg) p.set('agg', picked.agg)
  for (const key of picked.groupBy) p.append('by', key)
  if (isPinned(picked)) {
    p.set('from', String(picked.fromMs))
    p.set('to', String(picked.toMs))
  }
  if (picked.q) p.set('q', picked.q)
  const out = p.toString()
  return out || undefined
}

function Chip({ children, onRemove, label }: { children: React.ReactNode; onRemove: () => void; label: string }) {
  return (
    <span className="inline-flex h-6 items-center gap-1 rounded-sm border bg-muted pl-2 pr-1 font-mono text-caption">
      {children}
      <button type="button" aria-label={label} onClick={onRemove} className="inline-flex size-4 cursor-pointer items-center justify-center rounded-sm hover:bg-border-subtle focus-visible:outline-2 focus-visible:outline-ring"><X className="size-3" aria-hidden /></button>
    </span>
  )
}

function Meta({ metric }: { metric: MetricInfo }) {
  return (
    <dl className="m-0 grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-0.5 text-caption" aria-label="Metric metadata">
      <dt className="text-muted-foreground">Kind</dt><dd className="m-0 font-mono">{metric.kind}</dd>
      <dt className="text-muted-foreground">Unit</dt><dd className="m-0 font-mono">{metric.unit || 'none'}</dd>
      <dt className="text-muted-foreground">Description</dt><dd className="m-0">{metric.description || 'No description.'}</dd>
      <dt className="text-muted-foreground">Last seen</dt><dd className="m-0 font-mono">{stamp(Math.floor(metric.last_seen_ns / 1_000_000))}</dd>
    </dl>
  )
}

export function Metrics() {
  const navigate = useRouterNavigate()
  const raw = useSearchParam('f')
  const span = useWindow()
  const picked = useMemo(() => ({ ...decode(raw), rangeMs: span.ms }), [raw, span.ms])
  const [typed, setTyped] = useState(picked.q)
  const [key, setKey] = useState('')
  const settled = useSettled(typed)

  const { placement, place } = usePlacement()
  const apply = (part: Partial<Picked>) => {
    const f = encode({ ...picked, ...part })
    void navigate({ to: '/metrics' as never, search: ((was: Record<string, unknown>) => ({ ...was, f })) as never, replace: true })
  }

  useEffect(() => {
    if (settled === typed && settled !== picked.q) apply({ q: settled })
  }, [settled])

  useEffect(() => {
    setTyped(picked.q)
  }, [picked.q])

  const catalogue = useMetricCatalogue(picked.q)
  const lookup = useMetricCatalogue(picked.name)
  const metric = lookup.metrics.find((m) => m.name === picked.name) ?? null
  const valid = metric ? AGGREGATIONS[metric.kind] : []
  const agg = metric ? (picked.agg && valid.includes(picked.agg) ? picked.agg : DEFAULT_AGG[metric.kind]) : null

  const query: MetricQuery | null = picked.name && agg
    ? { name: picked.name, agg, groupBy: picked.groupBy, rangeMs: picked.rangeMs, fromMs: picked.fromMs, toMs: picked.toMs }
    : null
  const series = useMetricSeries(query, placement)

  const addKey = () => {
    const value = key.trim()
    if (!value || picked.groupBy.includes(value)) return
    apply({ groupBy: [...picked.groupBy, value] })
    setKey('')
  }

  const status = series.failed instanceof ApiError ? series.failed.status : null
  const catalogueUnconfigured = catalogue.failed instanceof ApiError && catalogue.failed.status === 503
  const unconfigured = catalogueUnconfigured || status === 503
  const result = series.result
  const hasPoints = result ? result.series.some((s) => s.points.some((p) => p[1] !== null)) : false
  const times = result ? result.series.flatMap((s) => s.points.map((p) => p[0])) : []
  const deploys = firstDeploys(useDeploys(times.length ? Math.floor(Math.min(...times) / 1_000_000) : null, times.length ? Math.ceil(Math.max(...times) / 1_000_000) + 1 : null))
  const marks: DeployMark[] = deploys.map((d) => ({ atNs: d.atMs * 1_000_000, label: `${d.service} ${d.version}`, to: releasePath(d.service, d.version) }))

  return (
    <div className="-mx-4 -my-4 flex min-h-[calc(100vh-48px)] md:-mx-5">
      <section aria-label="Metric names" className="hidden w-[300px] shrink-0 flex-col gap-2 border-r bg-sidebar p-4 md:flex">
        <h1 className="text-title">Metrics</h1>
        <InputGroup><Search aria-hidden /><input aria-label="Search metric names" placeholder="Search metric names" value={typed} onChange={(e) => setTyped(e.target.value)} className="font-mono text-[12px]" /></InputGroup>
        <span className="text-caption text-muted-foreground">{catalogue.pending ? 'Reading the catalogue…' : `${catalogue.metrics.length} name${catalogue.metrics.length === 1 ? '' : 's'} match`}</span>
        <div role="listbox" aria-label="Matching metrics" className="flex flex-col gap-0.5">
          {catalogue.metrics.map((m) => (
            <button key={m.name} type="button" role="option" aria-selected={m.name === picked.name} onClick={() => apply({ name: m.name, agg: null })} className={cn('flex cursor-pointer flex-col items-start gap-0.5 rounded-sm px-2 py-1.5 text-left hover:bg-muted focus-visible:outline-2 focus-visible:outline-ring', m.name === picked.name && 'bg-muted')}>
              <span className="break-all font-mono text-[12.5px] font-medium">{m.name}</span>
              <span className="text-caption text-muted-foreground">{m.kind} · {m.unit || 'no unit'}</span>
            </button>
          ))}
        </div>
      </section>
      <section className="flex min-w-0 flex-1 flex-col gap-3 p-4 md:p-5" aria-label="Selected metric">
        <div className="md:hidden">
          <NativeSelect aria-label="Metric" value={picked.name} onChange={(e) => apply({ name: e.target.value, agg: null })}>
            <option value="">Pick a metric…</option>
            {catalogue.metrics.map((m) => <option key={m.name} value={m.name}>{m.name}</option>)}
          </NativeSelect>
        </div>
        {unconfigured ? (
          <EmptyState title="Metrics are not configured">This deployment has no span store, so there are no metrics to read. Configure one to see metrics here.</EmptyState>
        ) : catalogue.failed ? (
          <EmptyState title="The metrics could not be listed">{because(catalogue.failed, 'The metric store did not answer.')}</EmptyState>
        ) : !catalogue.pending && catalogue.metrics.length === 0 && !picked.q ? (
          <EmptyState title="No metrics yet">Point an OTLP exporter at /v1/metrics and the metric names will show up here.</EmptyState>
        ) : !catalogue.pending && catalogue.metrics.length === 0 && !picked.name ? (
          <EmptyState title="No metric matches" actions={<Button size="sm" onClick={() => { setTyped(''); apply({ q: '' }) }}>Clear search</Button>}>Try a shorter search.</EmptyState>
        ) : !picked.name ? (
          <EmptyState title="Pick a metric">Choose a metric name from the list to chart it.</EmptyState>
        ) : !lookup.pending && !metric ? (
          <EmptyState title="Unknown metric">No metric named <span className="font-mono">{picked.name}</span> is known to this project.</EmptyState>
        ) : metric && agg ? (
          <>
            <div className="flex min-w-0 flex-col gap-2">
              <div className="flex flex-wrap items-center gap-2">
                <span className="min-w-0 flex-1 break-all font-mono text-[13px] font-semibold">{metric.name}</span>
                <AttachToIncident attachment={{ kind: 'metric', reference: encode(isPinned(picked) ? { ...picked, agg } : { ...picked, agg, fromMs: Date.now() - picked.rangeMs, toMs: Date.now() }) ?? '', label: `${agg}(${metric.name})${picked.groupBy.length ? ` by ${picked.groupBy.join(', ')}` : ''}`.slice(0, 200) }} />
              </div>
              <Meta metric={metric} />
            </div>
            <div className="flex flex-wrap items-center gap-2" role="search">
              {isPinned(picked) ? <Button size="sm" onClick={() => apply({ fromMs: null, toMs: null })}><X aria-hidden />Back to the last {span.label}</Button> : null}
              <NativeSelect aria-label="Aggregation" value={agg} onChange={(e) => apply({ agg: e.target.value as Aggregation })} className="w-[130px]">
                {valid.map((a) => <option key={a} value={a}>{a}</option>)}
              </NativeSelect>
              <form className="flex items-center gap-1" onSubmit={(e) => { e.preventDefault(); addKey() }}>
                <InputGroup className="w-[240px] max-w-full"><input aria-label="Group by attribute key" placeholder="group by attribute key" className="font-mono" value={key} onChange={(e) => setKey(e.target.value)} /></InputGroup>
                <Button type="submit" variant="outline" size="icon" aria-label="Add group by key" disabled={!key.trim()}><Plus aria-hidden /></Button>
              </form>
              {picked.groupBy.map((k) => <Chip key={k} label={`Remove group by ${k}`} onRemove={() => apply({ groupBy: picked.groupBy.filter((x) => x !== k) })}>by {k}</Chip>)}
            </div>
            <PlacementPicker collapsible placement={placement} onChange={place} windowMs={isPinned(picked) ? (picked.toMs as number) - (picked.fromMs as number) : picked.rangeMs} />
            {series.failed ? (
              status === 404 ? (
                <EmptyState title="Unknown metric">No metric named <span className="font-mono">{picked.name}</span> is known to this project.</EmptyState>
              ) : status === 422 ? (
                <EmptyState title="That query is not valid">{because(series.failed, 'This aggregation or time range does not apply to this metric.')}</EmptyState>
              ) : (
                <EmptyState title="The series could not be read">{because(series.failed, 'The metric store did not answer.')}</EmptyState>
              )
            ) : series.pending || !result ? (
              <EmptyState title="Reading the series…">Asking the metric store.</EmptyState>
            ) : !hasPoints ? (
              <EmptyState title="No data in this window">Nothing was recorded for this metric in the window. Try a wider one.</EmptyState>
            ) : (
              <MetricChart series={result.series} exemplars={result.exemplars} deploys={marks} unit={metric.unit} aria={`Line chart of ${metric.name}, ${agg}, ${result.series.map((s) => labelOf(s.labels)).join('; ')}`} />
            )}
          </>
        ) : (
          <EmptyState title="Reading the catalogue…">Asking the metric store.</EmptyState>
        )}
      </section>
    </div>
  )
}
