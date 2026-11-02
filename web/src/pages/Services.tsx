import { Search, TriangleAlert } from 'lucide-react'
import { useMemo, useState } from 'react'
import { Link, useNavigate } from '@/lib/router'
import { Badge } from '@/components/ui/badge'
import { InputGroup } from '@/components/ui/input'
import { Segmented } from '@/components/ui/segmented'
import { Switch } from '@/components/ui/switch'
import { Table, Td, Th, Tr, Kbd } from '@/components/ui/misc'
import { PageHeader, ServiceDot } from '@/components/ft/bits'
import { Sparkline } from '@/components/ft/charts'
import { useServices } from '@/data/services'
import { EmptyState } from '@/components/ft/bits'
import { cn } from '@/lib/utils'

const ms = (s: string) => (s === '—' ? -1 : parseFloat(s) * (s.endsWith(' s') ? 1000 : s.endsWith('µs') ? 0.001 : 1))

export function Services() {
  const [q, setQ] = useState('')
  const [sort, setSort] = useState<'err' | 'rps' | 'p95' | 'name'>('err')
  const [hideEmpty, setHideEmpty] = useState(false)
  const navigate = useNavigate()
  const services = useServices()
  const rows = useMemo(() => {
    let r = services.filter((s) => s.name.includes(q.trim()) && (!hideEmpty || s.err !== null))
    r = [...r].sort((a, b) => sort === 'name' ? a.name.localeCompare(b.name) : sort === 'rps' ? parseFloat(b.rps) - parseFloat(a.rps) || 0 : sort === 'p95' ? ms(b.p95) - ms(a.p95) : (b.err ?? -1) - (a.err ?? -1))
    return r
  }, [q, sort, hideEmpty, services])
  return (
    <>
      <PageHeader title="Services" meta={<>From the <span className="font-mono">service.name</span> resource attribute of your spans</>} />
      <div className="flex flex-wrap items-center gap-2" role="search">
        <InputGroup className="w-[280px] max-w-full"><Search aria-hidden /><input aria-label="Find service" placeholder="Find service" value={q} onChange={(e) => setQ(e.target.value)} /><Kbd>/</Kbd></InputGroup>
        <Segmented label="Sort by" value={sort} onChange={setSort} options={[{ value: 'err', label: 'Error rate' }, { value: 'rps', label: 'Requests' }, { value: 'p95', label: 'p95' }, { value: 'name', label: 'Name' }]} />
        <Switch checked={hideEmpty} onCheckedChange={setHideEmpty} label="Hide services with no data" />
      </div>
      {services.length === 0 ? <EmptyState title="No services in the last hour" code="OTEL_SERVICE_NAME=checkout-api">A service appears as soon as one span arrives with a service.name.</EmptyState> : null}
      <div className={cn('-mx-4 overflow-x-auto border-t md:-mx-5', services.length === 0 && 'hidden')}>
        <Table aria-label="Services">
          <thead><tr><Th>Service</Th><Th className="w-[130px]">Project</Th><Th numeric className="w-[100px]">Requests</Th><Th className="w-[120px]"><span className="sr-only">Trend</span></Th><Th numeric className="w-[110px]">Error rate</Th><Th numeric className="w-[80px]">p50</Th><Th numeric className="w-[80px]">p95</Th><Th numeric className="w-[80px]">p99</Th><Th className="w-[120px]">Last span</Th></tr></thead>
          <tbody>
            {rows.map((s) => {
              const bad = (s.err ?? 0) >= 2
              return (
                <Tr key={s.name} className="cursor-pointer" onClick={() => navigate(`/services/${s.name}`)}>
                  <Td><Link to={`/services/${s.name}`} className="inline-flex items-center gap-2 text-foreground no-underline" onClick={(e) => e.stopPropagation()}><ServiceDot name={s.name} /><span className="font-mono font-semibold">{s.name}</span>{s.name.startsWith('unknown') ? <Badge variant="warning" size="xs">service.name not set</Badge> : null}</Link></Td>
                  <Td className="text-control text-muted-foreground">{s.project}</Td>
                  <Td numeric>{s.rps}</Td>
                  <Td><Sparkline values={s.spark} /></Td>
                  <Td numeric className={cn(bad && 'font-semibold text-warning-text', s.err === null && 'text-muted-foreground')}><span className="inline-flex items-center gap-1">{bad ? <TriangleAlert className="size-3.5" aria-hidden /> : null}{s.err === null ? '—' : `${s.err.toFixed(1)} %`}</span></Td>
                  <Td numeric>{s.p50}</Td><Td numeric className={cn(bad && 'text-warning-text')}>{s.p95}</Td><Td numeric>{s.p99}</Td>
                  <Td className={cn('font-mono text-control', s.seen.includes('min') && 'text-muted-foreground')}>{s.seen}</Td>
                </Tr>
              )
            })}
          </tbody>
        </Table>
        <p className="m-0 px-5 py-3 text-caption text-muted-foreground">Error rate = spans with status ERROR ÷ all spans of the service. Amber from 2 %, the status page threshold.</p>
      </div>
    </>
  )
}
