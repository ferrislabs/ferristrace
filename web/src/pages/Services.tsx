import { Search, TriangleAlert } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { useNavigate as useRouterNavigate } from '@tanstack/react-router'
import { Link, useSearchParam } from '@/lib/router'
import { Badge } from '@/components/ui/badge'
import { InputGroup } from '@/components/ui/input'
import { Segmented } from '@/components/ui/segmented'
import { Switch } from '@/components/ui/switch'
import { Table, Td, Th, Tr, Kbd } from '@/components/ui/misc'
import { EmptyState, PageHeader, ServiceDot } from '@/components/ft/bits'
import { ERROR_THRESHOLD, useServiceTable } from '@/data/services'
import { because } from '@/auth/client'
import { useSettled } from '@/lib/settle'
import { cn } from '@/lib/utils'

export function Services() {
  const [q, setQ] = useState('')
  const [sort, setSort] = useState<'err' | 'rps' | 'p95' | 'name'>('err')
  const [hideEmpty, setHideEmpty] = useState(false)
  const route = useRouterNavigate()
  const environment = useSearchParam('env') ?? ''
  const [typedEnv, setTypedEnv] = useState(environment)
  const settledEnv = useSettled(typedEnv)
  const { services, pending, failed } = useServiceTable(settledEnv)
  const kept = settledEnv.trim() ? { env: settledEnv.trim() } : undefined
  useEffect(() => {
    if (settledEnv.trim() !== environment) void route({ to: '/services' as never, search: (kept ?? {}) as never, replace: true })
  }, [settledEnv])
  const rows = useMemo(() => {
    const r = services.filter((s) => s.name.includes(q.trim()) && (!hideEmpty || s.err !== null))
    return [...r].sort((a, b) => sort === 'name' ? a.name.localeCompare(b.name) : sort === 'rps' ? b.perSecond - a.perSecond : sort === 'p95' ? b.p95Us - a.p95Us : (b.err ?? -1) - (a.err ?? -1))
  }, [q, sort, hideEmpty, services])
  return (
    <>
      <PageHeader title="Services" meta={<>From the <span className="font-mono">service.name</span> resource attribute of your spans · last hour</>} />
      <div className="flex flex-wrap items-center gap-2" role="search">
        <InputGroup className="w-[280px] max-w-full"><Search aria-hidden /><input aria-label="Find service" placeholder="Find service" value={q} onChange={(e) => setQ(e.target.value)} /><Kbd>/</Kbd></InputGroup>
        <InputGroup className="w-[220px] max-w-full"><input aria-label="Environment" placeholder="Environment, e.g. production" value={typedEnv} onChange={(e) => setTypedEnv(e.target.value)} /></InputGroup>
        <Segmented label="Sort by" value={sort} onChange={setSort} options={[{ value: 'err', label: 'Error rate' }, { value: 'rps', label: 'Requests' }, { value: 'p95', label: 'p95' }, { value: 'name', label: 'Name' }]} />
        <Switch checked={hideEmpty} onCheckedChange={setHideEmpty} label="Hide services with no data" />
      </div>
      {failed ? <EmptyState title="The services could not be read">{because(failed, 'The span store did not answer')}</EmptyState> : null}
      {!failed && !pending && services.length === 0 ? <EmptyState title={settledEnv.trim() ? `No services in ${settledEnv.trim()} in the last hour` : 'No services in the last hour'} code="OTEL_SERVICE_NAME=checkout-api">A service appears as soon as one span it served arrives with a service.name. The environment filter reads the deployment.environment span attribute.</EmptyState> : null}
      <div className={cn('-mx-4 overflow-x-auto border-t md:-mx-5', services.length === 0 && 'hidden')}>
        <Table aria-label="Services">
          <thead><tr><Th>Service</Th><Th numeric className="w-[100px]">Requests</Th><Th numeric className="w-[110px]">Error rate</Th><Th numeric className="w-[80px]">p50</Th><Th numeric className="w-[80px]">p95</Th><Th numeric className="w-[80px]">p99</Th><Th className="w-[120px]">Last span</Th></tr></thead>
          <tbody>
            {rows.map((s) => {
              const bad = (s.err ?? 0) >= ERROR_THRESHOLD
              return (
                <Tr key={s.name} className="cursor-pointer" onClick={() => void route({ to: `/services/${s.name}` as never, search: (kept ?? {}) as never })}>
                  <Td><Link to={`/services/${s.name}`} search={kept} className="inline-flex items-center gap-2 text-foreground no-underline" onClick={(e) => e.stopPropagation()}><ServiceDot name={s.name} /><span className="font-mono font-semibold">{s.name}</span>{s.name.startsWith('unknown') ? <Badge variant="warning" size="xs">service.name not set</Badge> : null}</Link></Td>
                  <Td numeric>{s.rps}</Td>
                  <Td numeric className={cn(bad && 'font-semibold text-warning-text', s.err === null && 'text-muted-foreground')}><span className="inline-flex items-center gap-1">{bad ? <TriangleAlert className="size-3.5" aria-hidden /> : null}{s.err === null ? '—' : `${s.err.toFixed(1)} %`}</span></Td>
                  <Td numeric>{s.p50}</Td><Td numeric className={cn(bad && 'text-warning-text')}>{s.p95}</Td><Td numeric>{s.p99}</Td>
                  <Td className={cn('font-mono text-control', s.seen.includes('min') && 'text-muted-foreground')}>{s.seen}</Td>
                </Tr>
              )
            })}
          </tbody>
        </Table>
        <p className="m-0 px-5 py-3 text-caption text-muted-foreground">Requests and error rate count the server and consumer spans of each service: spans with status ERROR ÷ all of them. Amber from {ERROR_THRESHOLD} %, the status page threshold.</p>
      </div>
    </>
  )
}
