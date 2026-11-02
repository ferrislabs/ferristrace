import { Sparkles } from 'lucide-react'
import { useMemo, useState } from 'react'
import { Link, useNavigate } from '@/lib/router'
import { Badge } from '@/components/ui/badge'
import { Card, CardHeader, CardTitle } from '@/components/ui/card'
import { InputGroup } from '@/components/ui/input'
import { Segmented } from '@/components/ui/segmented'
import { Table, Td, Th } from '@/components/ui/misc'
import { EmptyState, Kpi, PageHeader } from '@/components/ft/bits'
import { StackedChart, stackColour, type Marker } from '@/components/ft/charts'
import {
  GROUP_BYS,
  LLM_PRICES_PATH,
  NO_FILTERS,
  SPEND_WINDOWS,
  formatCost,
  formatMs,
  formatTokens,
  useGroupCalls,
  useSpend,
  type GroupBy,
  type Spend,
  type SpendFilters,
  type SpendGroup,
} from '@/data/llm'
import { timeLabels } from '@/data/series'
import { ApiError, because } from '@/auth/client'
import { num, stamp } from '@/lib/format'
import { useSettled } from '@/lib/settle'
import { cn } from '@/lib/utils'

const LAYERS = 7
const OTHER = 'other'
const FILTER_FIELDS: { name: keyof SpendFilters; placeholder: string }[] = [
  { name: 'feature', placeholder: 'Feature' },
  { name: 'model', placeholder: 'Model' },
  { name: 'customer', placeholder: 'Customer' },
  { name: 'user', placeholder: 'User' },
  { name: 'prompt', placeholder: 'Prompt version' },
]

const named = (key: string, groupBy: GroupBy) => (key === '' ? `(no ${groupBy})` : key)

function layersOf(groups: SpendGroup[]) {
  const shown = groups.slice(0, LAYERS)
  const rest = groups.slice(LAYERS)
  const layers = shown.map((group) => ({ key: group.key, values: group.points.map((point) => point[1]) }))
  if (rest.length > 0) {
    layers.push({ key: OTHER, values: rest[0].points.map((_, at) => rest.reduce((sum, group) => sum + group.points[at][1], 0)) })
  }
  return layers
}

function markersOf(spend: Spend): Marker[] {
  const first = spend.groups[0]?.points[0]?.[0]
  if (first === undefined) return []
  const last = spend.groups[0].points.length - 1
  return spend.markers
    .map((marker) => ({ at: (marker.timeMs - first) / spend.stepMs, label: `${marker.feature} ${marker.promptVersion}` }))
    .filter((marker) => marker.at >= 0 && marker.at <= last)
}

function Calls({ windowMs, groupBy, selected, filters }: { windowMs: number; groupBy: GroupBy; selected: string; filters: SpendFilters }) {
  const navigate = useNavigate()
  const { calls, pending, failed } = useGroupCalls(windowMs, groupBy, selected, filters)
  return (
    <Card className="overflow-hidden">
      <CardHeader><CardTitle>Calls of {named(selected, groupBy)}</CardTitle></CardHeader>
      {failed ? <p className="m-0 px-4 py-3 text-control text-muted-foreground">{because(failed, 'The calls could not be read.')}</p> : null}
      {!failed && !pending && calls.length === 0 ? <p className="m-0 px-4 py-3 text-control text-muted-foreground">No call in this window.</p> : null}
      {calls.length > 0 ? (
        <div className="overflow-x-auto">
          <Table aria-label={`Calls of ${named(selected, groupBy)}`}>
            <thead><tr><Th className="w-[170px]">Time</Th><Th>Model</Th><Th>Feature</Th><Th numeric className="w-[90px]">Tokens in</Th><Th numeric className="w-[90px]">Tokens out</Th><Th numeric className="w-[90px]">Duration</Th><Th numeric className="w-[90px]">Cost</Th></tr></thead>
            <tbody>
              {calls.map((call) => {
                const to = `/traces/${call.traceId}`
                return (
                  <tr key={`${call.traceId}-${call.spanId}`} onClick={() => navigate(to)} className="cursor-pointer hover:[&>td]:bg-row-hover">
                    <Td className="font-mono text-control"><Link to={to} onClick={(e) => e.stopPropagation()} className="text-foreground">{stamp(call.timeMs)}</Link></Td>
                    <Td className="font-mono text-control">{call.model}{call.error ? <Badge variant="destructive" size="xs" className="ml-2">error</Badge> : null}</Td>
                    <Td className="text-control">{call.feature}</Td>
                    <Td numeric>{formatTokens(call.inputTokens)}</Td>
                    <Td numeric>{formatTokens(call.outputTokens)}</Td>
                    <Td numeric>{formatMs(call.durationMs)}</Td>
                    <Td numeric className={cn(call.cost === null && 'text-warning-text')}>{formatCost(call.cost)}</Td>
                  </tr>
                )
              })}
            </tbody>
          </Table>
        </div>
      ) : null}
    </Card>
  )
}

export function Ai() {
  const [range, setRange] = useState('1d')
  const [groupBy, setGroupBy] = useState<GroupBy>('feature')
  const [typed, setTyped] = useState<SpendFilters>(NO_FILTERS)
  const [selected, setSelected] = useState<string | null>(null)
  const filters = useSettled(typed)
  const windowMs = SPEND_WINDOWS.find((w) => w.value === range)?.ms ?? SPEND_WINDOWS[1].ms
  const { spend, pending, failed } = useSpend(windowMs, groupBy, filters)
  const groups = spend?.groups ?? []
  const totals = useMemo(
    () => groups.reduce((sum, g) => ({ cost: sum.cost + g.cost, calls: sum.calls + g.calls, input: sum.input + g.inputTokens, output: sum.output + g.outputTokens, unpriced: sum.unpriced + g.unpriced }), { cost: 0, calls: 0, input: 0, output: 0, unpriced: 0 }),
    [groups],
  )
  const unconfigured = failed instanceof ApiError && failed.status === 503
  const layers = layersOf(groups)
  const filtered = Object.values(filters).some((value) => value.trim() !== '')
  const toggle = (key: string) => setSelected(key === selected ? null : key)
  return (
    <>
      <PageHeader title="AI" meta="What LLM calls cost, from the GenAI spans your services send">
        <Segmented label="Window" value={range} onChange={setRange} options={SPEND_WINDOWS.map((w) => ({ value: w.value, label: w.label }))} />
      </PageHeader>
      <div className="flex flex-wrap items-center gap-2" role="search">
        {FILTER_FIELDS.map((field) => (
          <InputGroup key={field.name} className="w-[160px] max-w-full">
            <input aria-label={field.placeholder} placeholder={field.placeholder} value={typed[field.name]} onChange={(e) => setTyped({ ...typed, [field.name]: e.target.value })} />
          </InputGroup>
        ))}
        <Segmented label="Group by" value={groupBy} onChange={(next) => { setGroupBy(next); setSelected(null) }} options={GROUP_BYS.map((g) => ({ value: g.value, label: g.label }))} />
      </div>
      {unconfigured ? (
        <EmptyState title="AI spend is not configured">This deployment has no span store, so there are no LLM calls to read. Configure one to see them here.</EmptyState>
      ) : failed ? (
        <EmptyState title="The spend could not be read">{because(failed, 'The span store did not answer.')}</EmptyState>
      ) : !pending && groups.length === 0 ? (
        <EmptyState
          icon={<Sparkles />}
          title={filtered ? 'No LLM call matches in this window' : 'No LLM call in this window'}
          code={'gen_ai.provider.name=openai\ngen_ai.request.model=gpt-4o\ngen_ai.usage.input_tokens=1200\ngen_ai.usage.output_tokens=300\nferristrace.feature=ticket-summary'}
        >
          {filtered
            ? 'Clear a filter or widen the window.'
            : 'A span is an LLM call when it carries gen_ai.provider.name (or gen_ai.system) or gen_ai.request.model, the OpenTelemetry GenAI conventions. Send the token counts as gen_ai.usage.input_tokens and gen_ai.usage.output_tokens, and say who the call is for with ferristrace.feature, ferristrace.customer and ferristrace.prompt.version. Then enter the prices of your models under Settings, LLM prices. Prompts and completions are never copied out of the span.'}
        </EmptyState>
      ) : (
        <>
          <div className="grid grid-cols-[repeat(auto-fit,minmax(160px,1fr))] gap-3">
            <Kpi label="Total spend" value={formatCost(totals.cost)} sub="priced calls in the window" />
            <Kpi label="Calls" value={num(totals.calls)} />
            <Kpi label="Tokens" value={`${formatTokens(totals.input)} in`} sub={`${formatTokens(totals.output)} out`} />
            <Kpi
              label="Unpriced calls"
              value={num(totals.unpriced)}
              sub={totals.unpriced > 0 ? 'add their prices under LLM prices' : 'every call has a price'}
              tone={totals.unpriced > 0 ? 'warn' : undefined}
              to={totals.unpriced > 0 ? LLM_PRICES_PATH : undefined}
            />
          </div>
          {spend && layers.length > 0 ? (
            <StackedChart
              title={`Spend by ${groupBy}`}
              layers={layers}
              markers={markersOf(spend)}
              yLabels={(top) => [formatCost(top), formatCost(top / 2), '$0.00']}
              xLabels={timeLabels(spend.fromMs, spend.toMs)}
              aria={`Stacked cost per ${groupBy} over the window`}
            />
          ) : null}
          <div className="flex flex-wrap gap-x-3 gap-y-1 text-caption text-muted-foreground">
            {layers.map((layer, at) => <span key={layer.key} className="inline-flex items-center gap-1"><span className="inline-block size-2 rounded-sm" style={{ background: stackColour(at) }} />{named(layer.key, groupBy)}</span>)}
          </div>
          <div className="-mx-4 overflow-x-auto border-t md:-mx-5">
            <Table aria-label="Spend by group">
              <thead><tr><Th>{GROUP_BYS.find((g) => g.value === groupBy)?.label}</Th><Th numeric className="w-[100px]">Cost</Th><Th numeric className="w-[80px]">Calls</Th><Th numeric className="w-[90px]">Tokens in</Th><Th numeric className="w-[90px]">Tokens out</Th><Th numeric className="w-[80px]">p50</Th><Th numeric className="w-[80px]">p95</Th><Th numeric className="w-[80px]">Errors</Th></tr></thead>
              <tbody>
                {groups.map((g) => (
                  <tr key={g.key} onClick={() => toggle(g.key)} className={cn('cursor-pointer hover:[&>td]:bg-row-hover', g.key === selected && '[&>td]:bg-accent-subtle')}>
                    <Td>
                      <button type="button" aria-pressed={g.key === selected} onClick={(e) => { e.stopPropagation(); toggle(g.key) }} className="cursor-pointer bg-transparent p-0 font-mono font-semibold text-foreground">{named(g.key, groupBy)}</button>
                      {g.unpriced > 0 ? <Badge variant="warning" size="xs" className="ml-2">{num(g.unpriced)} unpriced</Badge> : null}
                    </Td>
                    <Td numeric>{formatCost(g.cost)}</Td>
                    <Td numeric>{num(g.calls)}</Td>
                    <Td numeric>{formatTokens(g.inputTokens)}</Td>
                    <Td numeric>{formatTokens(g.outputTokens)}</Td>
                    <Td numeric>{formatMs(g.p50Ms)}</Td>
                    <Td numeric>{formatMs(g.p95Ms)}</Td>
                    <Td numeric className={cn(g.errors > 0 && 'font-semibold text-destructive-text')}>{num(g.errors)}</Td>
                  </tr>
                ))}
              </tbody>
            </Table>
          </div>
          {selected === '' ? <p className="m-0 text-control text-muted-foreground">Calls without a {groupBy} cannot be listed on their own; filter on another field.</p> : null}
          {selected ? <Calls windowMs={windowMs} groupBy={groupBy} selected={selected} filters={filters} /> : null}
        </>
      )}
    </>
  )
}
