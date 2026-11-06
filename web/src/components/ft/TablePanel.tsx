import { ArrowDown } from 'lucide-react'
import { because } from '@/auth/client'
import { EmptyState } from '@/components/ft/bits'
import { GroupSelect } from '@/components/ft/GroupSelect'
import { Table, Td, Th } from '@/components/ui/misc'
import { groupLabel } from '@/data/spanSeries'
import { useSpanTable } from '@/data/spanTable'
import type { Measure } from '@/data/views'
import { dur } from '@/lib/format'
import type { Node } from '@/lib/queryLang'
import { cn } from '@/lib/utils'

export const TABLE_DEFAULT_GROUP = 'service'

const COLUMNS: { measure: Measure; label: string; show: (row: ReturnType<typeof useSpanTable>['rows'][number]) => string }[] = [
  { measure: 'count', label: 'Spans', show: (row) => row.spans.toLocaleString('en-US') },
  { measure: 'errors', label: 'Errors', show: (row) => row.errors.toLocaleString('en-US') },
  { measure: 'error_rate', label: 'Error rate', show: (row) => `${row.errorRate.toLocaleString('en-US', { maximumFractionDigits: 1 })}%` },
  { measure: 'average', label: 'Average', show: (row) => dur(row.averageUs / 1000) },
  { measure: 'p50', label: 'p50', show: (row) => dur(row.p50Us / 1000) },
  { measure: 'p95', label: 'p95', show: (row) => dur(row.p95Us / 1000) },
  { measure: 'p99', label: 'p99', show: (row) => dur(row.p99Us / 1000) },
]

export function TablePanel({ items, rangeMs, order, group, onChange, onPick }: {
  items: Node[]
  rangeMs: number
  order: Measure
  group: string
  onChange: (part: { m?: Measure; g?: string }) => void
  onPick: (value: string) => void
}) {
  const by = group === '' ? TABLE_DEFAULT_GROUP : group
  const { rows, loading, failed } = useSpanTable(rangeMs, order, by, items, true)
  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-2">
        <GroupSelect group={by} onChange={(next) => onChange({ g: next })} rangeMs={rangeMs} none={false} />
        <span className="text-caption text-muted-foreground">Click a header to sort, a row to filter on it.</span>
      </div>
      {failed ? (
        <EmptyState title="The table could not be read">{because(failed, 'The span store did not answer')}</EmptyState>
      ) : rows.length === 0 ? (
        <EmptyState title={loading ? 'Reading the spans…' : 'Nothing to list'}>
          {loading ? 'Asking the span store.' : 'No span with this field matches in this window. Try fewer filters or another group.'}
        </EmptyState>
      ) : (
        <div className="overflow-x-auto rounded-lg border">
          <Table aria-label={`Spans by ${groupLabel(by)}`}>
            <thead>
              <tr>
                <Th>{groupLabel(by)}</Th>
                {COLUMNS.map((column) => (
                  <Th key={column.measure} numeric className="w-[100px]" aria-sort={order === column.measure ? 'descending' : 'none'}>
                    <button type="button" onClick={() => onChange({ m: column.measure })} className={cn('inline-flex cursor-pointer items-center gap-1 bg-transparent p-0 font-semibold hover:text-foreground focus-visible:outline-2 focus-visible:outline-ring', order === column.measure ? 'text-foreground' : 'text-muted-foreground')}>
                      {order === column.measure ? <ArrowDown className="size-3" aria-hidden /> : null}{column.label}
                    </button>
                  </Th>
                ))}
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => (
                <tr key={row.group} onClick={() => onPick(row.group)} className="cursor-pointer hover:[&>td]:bg-row-hover">
                  <Td className="max-w-[360px]">
                    <button type="button" onClick={(event) => { event.stopPropagation(); onPick(row.group) }} className="max-w-full cursor-pointer truncate bg-transparent p-0 text-left font-semibold text-foreground hover:underline focus-visible:outline-2 focus-visible:outline-ring" title={`Filter on ${row.group}`}>
                      {row.group}
                    </button>
                  </Td>
                  {COLUMNS.map((column) => <Td key={column.measure} numeric className={cn(row.errors > 0 && (column.measure === 'errors' || column.measure === 'error_rate') && 'text-destructive-text')}>{column.show(row)}</Td>)}
                </tr>
              ))}
            </tbody>
          </Table>
        </div>
      )}
    </div>
  )
}
