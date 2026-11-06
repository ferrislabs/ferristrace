import { useMemo } from 'react'
import { because } from '@/auth/client'
import { NativeSelect } from '@/components/ui/input'
import { EmptyState } from '@/components/ft/bits'
import { MetricChart } from '@/components/ft/MetricChart'
import { useAttributeKeys } from '@/data/attributes'
import { groupLabel, useSpanSeries } from '@/data/spanSeries'
import { MEASURES, measureOf, type Measure } from '@/data/views'
import type { Node } from '@/lib/queryLang'

const BUILT_GROUPS = [
  { value: 'service', label: 'Service' },
  { value: 'status', label: 'Status' },
  { value: 'kind', label: 'Kind' },
  { value: 'name', label: 'Span name' },
  { value: 'http_status_code', label: 'HTTP status code' },
]

export function SeriesPanel({ items, rangeMs, measure, group, onChange }: {
  items: Node[]
  rangeMs: number
  measure: Measure
  group: string
  onChange: (part: { m?: Measure; g?: string }) => void
}) {
  const { lines, unit, loading, failed } = useSpanSeries(rangeMs, measure, group, items, true)
  const { keys } = useAttributeKeys(rangeMs, '', true)
  const attributes = useMemo(() => keys.slice(0, 40).map((key) => key.key), [keys])
  const empty = lines.length === 0 || lines.every((line) => line.points.every((point) => point[1] === null || point[1] === 0))
  const label = MEASURES.find((one) => one.value === measure)?.label ?? 'Spans'
  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-2">
        <label className="flex items-center gap-2 text-caption font-semibold text-muted-foreground">
          Show
          <NativeSelect aria-label="Measure" value={measure} onChange={(event) => onChange({ m: measureOf(event.target.value) })} className="h-8 w-[180px]">
            {MEASURES.map((one) => <option key={one.value} value={one.value}>{one.label}</option>)}
          </NativeSelect>
        </label>
        <label className="flex items-center gap-2 text-caption font-semibold text-muted-foreground">
          Group by
          <NativeSelect aria-label="Group by" value={group} onChange={(event) => onChange({ g: event.target.value })} className="h-8 w-[220px]">
            <option value="">Nothing</option>
            <optgroup label="Fields">
              {BUILT_GROUPS.map((one) => <option key={one.value} value={one.value}>{one.label}</option>)}
            </optgroup>
            <optgroup label="Attributes">
              {group.startsWith('attr:') && !attributes.includes(group.slice(5)) ? <option value={group}>{group.slice(5)}</option> : null}
              {attributes.map((key) => <option key={key} value={`attr:${key}`}>{key}</option>)}
            </optgroup>
          </NativeSelect>
        </label>
        {group !== '' ? <span className="text-caption text-muted-foreground">The 8 most used values of {groupLabel(group)}.</span> : null}
      </div>
      {failed ? (
        <EmptyState title="The series could not be read">{because(failed, 'The span store did not answer')}</EmptyState>
      ) : empty ? (
        <EmptyState title={loading ? 'Reading the spans…' : 'Nothing to plot'}>
          {loading ? 'Asking the span store.' : 'No span matches in this window. Try fewer filters or a wider window.'}
        </EmptyState>
      ) : (
        <MetricChart series={lines} exemplars={[]} unit={unit} aria={`${label} over time${group ? `, by ${groupLabel(group)}` : ''}`} />
      )}
    </div>
  )
}
