import { Search } from 'lucide-react'
import { useState } from 'react'
import { InputGroup } from '@/components/ui/input'
import { Segmented } from '@/components/ui/segmented'
import { Table, Td, Th, Tr } from '@/components/ui/misc'
import { LineChart } from '@/components/ft/charts'
import { useMetricSeries, useMetrics } from '@/data/metrics'
import { cn } from '@/lib/utils'

export function Metrics() {
  const metrics = useMetrics()
  const [q, setQ] = useState('http.server')
  const [sel, setSel] = useState(metrics[0].name)
  const [view, setView] = useState<'charts' | 'points'>('charts')
  const list = metrics.filter((m) => m.name.includes(q))
  const metric = metrics.find((m) => m.name === sel) ?? metrics[0]
  const series = useMetricSeries(sel)
  return (
    <div className="-mx-4 -my-4 flex min-h-[calc(100vh-48px)] md:-mx-5">
      <section aria-label="Metric names" className="hidden w-[300px] shrink-0 flex-col gap-2 border-r bg-sidebar p-4 md:flex">
        <h1 className="text-title">Metrics</h1>
        <InputGroup><Search aria-hidden /><input aria-label="Search metric names" value={q} onChange={(e) => setQ(e.target.value)} className="font-mono text-[12px]" /></InputGroup>
        <span className="text-caption text-muted-foreground">{list.length} names match</span>
        <div role="listbox" aria-label="Matching metrics" className="flex flex-col gap-0.5">
          {list.map((m) => (
            <button key={m.name} type="button" role="option" aria-selected={m.name === sel} onClick={() => setSel(m.name)} className={cn('flex cursor-pointer flex-col items-start gap-0.5 rounded-sm px-2 py-1.5 text-left hover:bg-muted', m.name === sel && 'bg-muted')}>
              <span className="font-mono text-[12.5px] font-medium">{m.name}</span>
              <span className="text-caption text-muted-foreground">{m.kind} · {m.unit}</span>
            </button>
          ))}
        </div>
      </section>
      <section className="flex min-w-0 flex-1 flex-col gap-3 p-4 md:p-5" aria-label="Selected metric">
        <div className="flex flex-wrap items-start gap-3">
          <div className="flex min-w-0 flex-1 flex-col gap-1">
            <span className="font-mono text-[13px] font-semibold">{metric.name}</span>
            <span className="text-control text-muted-foreground">{metric.desc} {metric.kind} · unit <span className="font-mono">{metric.unit}</span> · {series.length} attribute sets</span>
          </div>
          <Segmented label="View" value={view} onChange={setView} options={[{ value: 'charts', label: 'Charts' }, { value: 'points', label: 'Raw points' }]} />
        </div>
        {view === 'charts' ? (
          <div className="grid grid-cols-[repeat(auto-fit,minmax(min(420px,100%),1fr))] gap-3">
            {series.map((s) => <LineChart key={s.label} title={s.label} legend={`last ${s.vals[59]}`} values={s.vals} hi={24} yLabels={['24', '12', '0']} aria={`Line chart of ${s.label}`} />)}
          </div>
        ) : (
          <div className="overflow-x-auto rounded-lg border">
            <Table aria-label="Raw metric points, newest first">
              <thead><tr><Th className="w-[210px]">Time ↓</Th><Th>Attribute set</Th><Th numeric className="w-[120px]">Value</Th></tr></thead>
              <tbody>
                {Array.from({ length: 20 }, (_, k) => {
                  const i = 59 - Math.floor(k / 4), s = series[k % 4]
                  const m = 32 + i
                  return <Tr key={k}><Td className="font-mono text-control">2026-10-20 {String(13 + Math.floor(m / 60)).padStart(2, '0')}:{String(m % 60).padStart(2, '0')}:00.000</Td><Td className="max-w-0 truncate font-mono text-control">{s.label}</Td><Td numeric>{s.vals[i]}</Td></Tr>
                })}
              </tbody>
            </Table>
          </div>
        )}
      </section>
    </div>
  )
}
