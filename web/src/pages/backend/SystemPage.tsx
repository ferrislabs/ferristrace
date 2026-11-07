import { Boxes, Search } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useNavigate as useRouterNavigate } from '@tanstack/react-router'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import { InputGroup } from '@/components/ui/input'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Segmented } from '@/components/ui/segmented'
import { Kbd, Table, Td, Th, Tr } from '@/components/ui/misc'
import { EmptyState, Kpi, PageHeader, barBg } from '@/components/ft/bits'
import { MetricChart } from '@/components/ft/MetricChart'
import { PlacementPicker, placedCount } from '@/components/ft/Placement'
import { usePlacement } from '@/data/placement'
import { HOST_KEY, useHosts, type Host } from '@/data/hosts'
import { useSearchParam } from '@/lib/router'
import { useWindow } from '@/lib/window'
import { because } from '@/auth/client'
import { cn } from '@/lib/utils'

const HOT_CPU = 75
const FULL_DISK = 85

const SORTS = [
  { value: 'cpu', label: 'CPU' },
  { value: 'memory', label: 'Memory' },
  { value: 'disk', label: 'Disk' },
  { value: 'name', label: 'Name' },
] as const

type Sort = (typeof SORTS)[number]['value']

const sortOf = (value: unknown): Sort => SORTS.find((one) => one.value === value)?.value ?? 'cpu'

const pct = (value: number | null) => (value === null ? '—' : `${value.toFixed(0)} %`)

const num = (value: number | null) => (value === null ? '—' : value.toFixed(2))

const known = (values: (number | null)[]) => values.filter((one): one is number => one !== null)

const mean = (values: (number | null)[]) => {
  const seen = known(values)
  return seen.length === 0 ? null : seen.reduce((a, b) => a + b, 0) / seen.length
}

const peak = (values: (number | null)[]) => {
  const seen = known(values)
  return seen.length === 0 ? null : Math.max(...seen)
}

function sorted(hosts: Host[], by: Sort): Host[] {
  const low = (value: number | null) => value ?? -1
  const order: Record<Sort, (a: Host, b: Host) => number> = {
    cpu: (a, b) => low(b.cpu) - low(a.cpu),
    memory: (a, b) => low(b.memory) - low(a.memory),
    disk: (a, b) => low(b.disk) - low(a.disk),
    name: (a, b) => a.name.localeCompare(b.name),
  }
  return [...hosts].sort(order[by])
}

function Meter({ value, hot }: { value: number | null; hot: boolean }) {
  if (value === null) return <span className="text-caption text-muted-foreground">not reported</span>
  return (
    <span className="inline-flex max-w-full items-center gap-1.5">
      <span className="inline-flex h-1.5 w-14 shrink-0 overflow-hidden rounded bg-muted">
        <span
          className={cn('rounded', barBg(hot ? 'bar-w' : 'bar-o'))}
          style={{ width: `${Math.min(100, Math.max(0, value))}%` }}
        />
      </span>
      <span className={cn('font-mono text-caption', hot && 'font-semibold text-warning-text')}>{pct(value)}</span>
    </span>
  )
}

export function SystemPage() {
  const goTo = useRouterNavigate()
  const { ms: windowMs, label } = useWindow()
  const sort = sortOf(useSearchParam('s'))
  const [q, setQ] = useState('')
  const { placement, place } = usePlacement()
  const { hosts, busy, pending, failed, instrumented } = useHosts(windowMs, placement)

  const show = (part: Record<string, string | undefined>) =>
    void goTo({
      to: '.' as never,
      search: ((was: Record<string, unknown>) => ({ ...was, ...part })) as never,
      replace: true,
    })

  const rows = useMemo(
    () => sorted(hosts.filter((host) => host.name.toLowerCase().includes(q.trim().toLowerCase())), sort),
    [hosts, sort, q],
  )
  const placing = placedCount(placement)
  const sifting = placing > 0 || q.trim() !== ''
  const cpus = hosts.map((host) => host.cpu)
  const disks = hosts.map((host) => host.disk)
  const hottest = peak(cpus)
  const fullest = peak(disks)
  const busiest = hosts.find((host) => host.cpu !== null && host.cpu === hottest)
  const tightest = hosts.find((host) => host.disk !== null && host.disk === fullest)

  return (
    <>
      <PageHeader
        title="System metrics"
        meta={
          <>
            Hosts and containers, from <span className="font-mono">system.*</span> metrics · last {label}
          </>
        }
      />

      <div className="grid grid-cols-[repeat(auto-fit,minmax(190px,1fr))] gap-3">
        <Kpi
          label="Hosts"
          value={hosts.length === 0 ? '—' : String(hosts.length)}
          sub={`reporting ${HOST_KEY} in the last ${label}`}
        />
        <Kpi
          label="CPU"
          value={pct(mean(cpus))}
          sub={busiest === undefined ? 'no host reports cpu' : `average · peak ${pct(busiest.cpu)} on ${busiest.name}`}
          tone={(mean(cpus) ?? 0) >= HOT_CPU ? 'warn' : undefined}
        />
        <Kpi label="Memory" value={pct(mean(hosts.map((host) => host.memory)))} sub="average of the used state" />
        <Kpi
          label="Disk"
          value={pct(fullest)}
          sub={tightest === undefined ? 'no host reports a filesystem' : `fullest · ${tightest.name}`}
          tone={(fullest ?? 0) >= FULL_DISK ? 'warn' : undefined}
        />
      </div>

      {busy.length > 0 ? (
        <Card className="p-3">
          <MetricChart
            series={busy}
            exemplars={[]}
            unit="%"
            threshold={HOT_CPU}
            aria={`Line chart of the busy share of cpu per host over the last ${label}`}
          />
        </Card>
      ) : null}

      <Card className="overflow-hidden">
        <div className="flex flex-wrap items-center gap-2 border-b px-4 py-2.5">
          <h2 className="text-[15px] font-bold">Hosts</h2>
          <span className="text-caption text-muted-foreground">
            {rows.length} host{rows.length === 1 ? '' : 's'}
          </span>
          <span className="flex-1" />
          <Segmented
            label="Sort by"
            value={sort}
            onChange={(next) => show({ s: next === 'cpu' ? undefined : next })}
            options={SORTS.map((one) => ({ value: one.value, label: one.label }))}
          />
          <Popover>
            <PopoverTrigger asChild>
              <Button variant="outline" size="sm" aria-label="Kubernetes placement">
                <Boxes />
                Kubernetes
                {placing > 0 ? (
                  <span className="rounded-full bg-primary px-1.5 font-mono text-[11px] text-primary-foreground">
                    {placing}
                  </span>
                ) : null}
              </Button>
            </PopoverTrigger>
            <PopoverContent align="start" className="w-[min(560px,calc(100vw-32px))] p-3">
              <PlacementPicker placement={placement} onChange={place} windowMs={windowMs} />
            </PopoverContent>
          </Popover>
          <InputGroup className="w-[240px] max-w-full">
            <Search aria-hidden />
            <input
              aria-label="Filter hosts"
              placeholder="Filter hosts"
              value={q}
              onChange={(e) => setQ(e.target.value)}
            />
            <Kbd>/</Kbd>
          </InputGroup>
        </div>

        {failed ? (
          <div className="p-6">
            <EmptyState title="The host metrics could not be read">
              {because(failed, 'The metric store did not answer')}
            </EmptyState>
          </div>
        ) : null}

        {!failed && !pending && hosts.length === 0 ? (
          <div className="p-6">
            <EmptyState
              title={
                sifting ? 'No hosts match' : instrumented ? `No host reported in the last ${label}` : 'No system metrics yet'
              }
              code={sifting || instrumented ? undefined : 'receivers:\n  hostmetrics:\n    scrapers: [cpu, memory, filesystem, load]'}
              actions={
                sifting ? (
                  <Button
                    size="sm"
                    onClick={() => {
                      setQ('')
                      place({ cluster: '', namespace: '', workload: '', pod: '', node: '' })
                    }}
                  >
                    Reset filters
                  </Button>
                ) : null
              }
            >
              {sifting ? (
                'Try a wider window or no placement.'
              ) : instrumented ? (
                <>
                  A <span className="font-mono">system.*</span> metric arrived but none carried{' '}
                  <span className="font-mono">{HOST_KEY}</span>, so no host can be named. Set it as a resource attribute
                  on the collector.
                </>
              ) : (
                <>
                  Add the <span className="font-mono">hostmetrics</span> receiver to your OpenTelemetry Collector and
                  point its exporter at FerrisTrace.
                </>
              )}
            </EmptyState>
          </div>
        ) : null}

        {hosts.length > 0 ? (
          <div className="overflow-x-auto">
            <Table aria-label="Hosts">
              <thead>
                <tr>
                  <Th>Host</Th>
                  <Th className="w-[160px]">CPU</Th>
                  <Th className="w-[160px]">Memory</Th>
                  <Th className="w-[160px]">Disk</Th>
                  <Th numeric className="w-[90px]">Load 1m</Th>
                </tr>
              </thead>
              <tbody>
                {rows.map((host) => (
                  <Tr key={host.name}>
                    <Td>
                      <span className="font-mono font-semibold">{host.name}</span>
                    </Td>
                    <Td>
                      <Meter value={host.cpu} hot={host.cpu !== null && host.cpu >= HOT_CPU} />
                    </Td>
                    <Td>
                      <Meter value={host.memory} hot={false} />
                    </Td>
                    <Td>
                      <Meter value={host.disk} hot={host.disk !== null && host.disk >= FULL_DISK} />
                    </Td>
                    <Td numeric>{num(host.load)}</Td>
                  </Tr>
                ))}
              </tbody>
            </Table>
          </div>
        ) : null}

        <p className="m-0 px-4 py-2.5 text-caption text-muted-foreground">
          A host is a value of <span className="font-mono">{HOST_KEY}</span>. CPU is{' '}
          <span className="font-mono">1 − avg(system.cpu.utilization)</span> over the{' '}
          <span className="font-mono">idle</span> state, averaged across cores, so it is the busy share of one core on
          average and not the sum of them. Memory is the <span className="font-mono">used</span> state of{' '}
          <span className="font-mono">system.memory.utilization</span>, and disk the fullest mounted filesystem. A
          column reads <span className="font-mono">not reported</span> when the scraper for it is off. Amber from{' '}
          {HOT_CPU} % CPU and {FULL_DISK} % disk.
        </p>
      </Card>
    </>
  )
}
