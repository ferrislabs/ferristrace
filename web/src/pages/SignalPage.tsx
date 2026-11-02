import { Search, TriangleAlert } from 'lucide-react'
import { useState } from 'react'
import { useParams } from '@/lib/router'
import { Card } from '@/components/ui/card'
import { InputGroup } from '@/components/ui/input'
import { Segmented } from '@/components/ui/segmented'
import { Kbd, Table, Td, Th, Tr } from '@/components/ui/misc'
import { EmptyState, Kpi, PageHeader, barBg, mapCls } from '@/components/ft/bits'
import { LineChart } from '@/components/ft/charts'
import { useSignalView } from '@/data/signals'
import { NotFound } from './NotFound'
import { cn } from '@/lib/utils'

const tone = (c: string) => (c.includes('warn') ? 'warn' : c.includes('ok') ? 'ok' : undefined) as 'warn' | 'ok' | undefined

export function SignalPage({ group }: { group: 'backend' | 'frontend' }) {
  const { view = '' } = useParams()
  const series = useSignalView(group, view)
  const [by, setBy] = useState('0')
  const [q, setQ] = useState('')
  if (!series) return <NotFound />
  const v = series.view
  const rows = v.rows.filter((r) => r.some((c) => c.v.toLowerCase().includes(q.toLowerCase())))
  return (
    <>
      <PageHeader title={v.title} meta={v.subtitle}>
        <Segmented label="Group by" value={by} onChange={setBy} options={v.group.map((g, i) => ({ value: String(i), label: `By ${g.toLowerCase()}` }))} />
      </PageHeader>
      <div className="grid grid-cols-[repeat(auto-fit,minmax(190px,1fr))] gap-3">
        {v.kpis.map(([label, value, sub, cls, subCls]) => <Kpi key={label} label={label} value={value} sub={sub} tone={tone(cls) ?? (subCls.includes('warn') ? 'warn' : undefined)} />)}
      </div>
      <LineChart title={v.chart[0]} legend={v.chart[1]} values={series.a} second={series.b} lo={v.lo} hi={v.hi} yLabels={[v.chart[2], v.chart[3], v.chart[4]]} aria={v.chart[5]} />
      <Card className="overflow-hidden">
        <div className="flex flex-wrap items-center gap-2 border-b px-4 py-2.5">
          <h2 className="text-[15px] font-bold">{v.table[0]}</h2><span className="text-caption text-muted-foreground">{v.table[1]}</span><span className="flex-1" />
          <InputGroup className="w-[260px] max-w-full"><Search aria-hidden /><input aria-label="Filter rows" placeholder={v.table[2]} value={q} onChange={(e) => setQ(e.target.value)} /><Kbd>/</Kbd></InputGroup>
        </div>
        {rows.length === 0 ? <div className="p-6"><EmptyState title={v.empty[0]} code={v.empty[2]}>{v.empty[1]}</EmptyState></div> : (
          <div className="overflow-x-auto">
            <Table aria-label={v.table[0]}>
              <thead><tr>{v.cols.map((c) => <Th key={c.label} aria-sort={c.sort} className={mapCls(c.cls)}>{c.label}{c.sort === 'descending' ? ' ↓' : c.sort === 'ascending' ? ' ↑' : ''}</Th>)}</tr></thead>
              <tbody>
                {rows.map((r, i) => (
                  <Tr key={i}>
                    {r.map((c, j) => (
                      <Td key={j} className={cn(mapCls(c.cls))} title={c.title || undefined}>
                        <span className="inline-flex max-w-full items-center gap-1.5">
                          {c.warn ? <TriangleAlert className="size-3.5 shrink-0" aria-hidden /> : null}
                          {c.bar !== undefined ? <span className="inline-flex h-1.5 w-14 shrink-0 overflow-hidden rounded bg-muted"><span className={cn('rounded', barBg(c.barColor))} style={{ width: `${Math.min(100, c.bar)}%` }} /></span> : null}
                          <span className="truncate">{c.v}</span>
                        </span>
                      </Td>
                    ))}
                  </Tr>
                ))}
              </tbody>
            </Table>
          </div>
        )}
        <p className="m-0 px-4 py-2.5 text-caption text-muted-foreground">{v.foot}</p>
      </Card>
    </>
  )
}
