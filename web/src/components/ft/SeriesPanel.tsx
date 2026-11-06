import { because } from '@/auth/client'
import { NativeSelect } from '@/components/ui/input'
import { EmptyState } from '@/components/ft/bits'
import { MetricChart } from '@/components/ft/MetricChart'
import { GroupSelect } from '@/components/ft/GroupSelect'
import { groupLabel, useSpanSeries } from '@/data/spanSeries'
import { MEASURES, measureOf, type Measure } from '@/data/views'
import type { Node } from '@/lib/queryLang'

export function SeriesPanel({ items, rangeMs, measure, group, onChange }: {
  items: Node[]
  rangeMs: number
  measure: Measure
  group: string
  onChange: (part: { m?: Measure; g?: string }) => void
}) {
  const { lines, unit, loading, failed } = useSpanSeries(rangeMs, measure, group, items, true)
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
        <GroupSelect group={group} onChange={(next) => onChange({ g: next })} rangeMs={rangeMs} none />
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
