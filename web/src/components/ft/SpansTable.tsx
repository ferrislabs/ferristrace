import { CircleX, Copy } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Table, Td, Th } from '@/components/ui/misc'
import { EmptyState, ServiceDot } from '@/components/ft/bits'
import { because } from '@/auth/client'
import type { SpanRow } from '@/data/spans'
import { shortId } from '@/lib/format'
import { cn } from '@/lib/utils'

const codeTone = (code: number, failed: boolean) =>
  code >= 500 || failed ? 'text-destructive-text' : code >= 400 ? 'text-warning-text' : 'text-muted-foreground'

export function SpansTable({ rows, loading, failed, sifting, limit, cursor, density, onOpen, onCopy, onClear }: {
  rows: SpanRow[]
  loading: boolean
  failed: unknown
  sifting: boolean
  limit: number
  cursor: number
  density: 'compact' | 'comfy'
  onOpen: (row: SpanRow) => void
  onCopy: (id: string) => void
  onClear: () => void
}) {
  if (failed) return <EmptyState title="The spans could not be read">{because(failed, 'The span store did not answer')}</EmptyState>
  if (rows.length === 0) {
    return (
      <EmptyState title={loading ? 'Reading the spans…' : sifting ? 'No spans match' : 'No spans in this window'} actions={sifting ? <Button size="sm" onClick={onClear}>Reset filters</Button> : null}>
        {loading ? 'Asking the span store.' : sifting ? 'Try fewer filters, a wider window or no placement.' : 'Nothing arrived in the window you are looking at. Try a wider one.'}
      </EmptyState>
    )
  }
  return (
    <div className="overflow-x-auto rounded-lg border">
      <Table aria-label="Spans">
        <thead>
          <tr>
            <Th className="w-[130px] max-md:w-[96px]">Started</Th>
            <Th className="w-[150px] max-md:hidden">Service</Th>
            <Th>Span</Th>
            <Th className="w-[90px] max-md:hidden">Kind</Th>
            <Th numeric className="w-[80px] max-md:w-[64px]">Code</Th>
            <Th numeric className="w-[110px] max-md:w-[84px]">Duration</Th>
            <Th className="w-[150px] max-md:hidden">Trace id</Th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row, index) => (
            <tr key={`${row.traceId}-${row.spanId}`} onClick={() => onOpen(row)} className={cn('cursor-pointer hover:[&>td]:bg-row-hover', index === cursor && '[&>td]:bg-accent-subtle')}>
              <Td className={cn(density === 'comfy' && 'h-10', 'font-mono text-control max-md:h-[52px] max-md:text-caption')}>{row.start}</Td>
              <Td className="max-md:hidden"><span className="inline-flex items-center gap-1.5 text-control"><ServiceDot name={row.service} />{row.service}</span></Td>
              <Td className="max-md:max-w-0">
                <span className="flex min-w-0 flex-col">
                  <span className="truncate font-semibold text-foreground">{row.name}</span>
                  <span className="inline-flex items-center gap-1.5 truncate text-caption text-muted-foreground md:hidden"><ServiceDot name={row.service} />{row.service}</span>
                </span>
              </Td>
              <Td className="max-md:hidden"><span className="text-control text-muted-foreground">{row.kind}</span></Td>
              <Td numeric>
                {row.code === null
                  ? row.status === 'error'
                    ? <span className="inline-flex text-destructive-text" title="The span failed"><CircleX className="size-3.5" aria-label="The span failed" /></span>
                    : <span className="text-muted-foreground">—</span>
                  : <span className={codeTone(row.code, row.status === 'error')}>{row.code}</span>}
              </Td>
              <Td numeric>{row.dur}</Td>
              <Td className="max-md:hidden">
                <button type="button" onClick={(event) => { event.stopPropagation(); onCopy(row.traceId) }} className="inline-flex cursor-pointer items-center gap-1.5 rounded-sm font-mono text-caption text-muted-foreground hover:text-foreground focus-visible:outline-2 focus-visible:outline-ring" aria-label={`Copy trace id ${shortId(row.traceId)}`}>
                  {shortId(row.traceId)}<Copy className="size-3" aria-hidden />
                </button>
              </Td>
            </tr>
          ))}
        </tbody>
      </Table>
      <div className="flex flex-wrap items-center gap-3 px-3 py-2.5">
        <span className="text-caption text-muted-foreground">
          {rows.length === limit ? `The newest ${limit} spans of this window.` : `${rows.length} span${rows.length === 1 ? '' : 's'} in this window.`}
        </span>
      </div>
    </div>
  )
}
