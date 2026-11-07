import { Search } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useSearchParam } from '@/lib/router'
import { Card } from '@/components/ui/card'
import { InputGroup } from '@/components/ui/input'
import { Segmented } from '@/components/ui/segmented'
import { Kbd, Table, Td, Th, Tr } from '@/components/ui/misc'
import { EmptyState, Kpi, PageHeader, barBg } from '@/components/ft/bits'
import { LineChart } from '@/components/ft/charts'
import { useCacheOperations, useCacheSeries, type Grouping, type Operation } from '@/data/caches'
import { useWindow } from '@/lib/window'
import { because } from '@/auth/client'
import { useSettled } from '@/lib/settle'
import { dur } from '@/lib/format'
import { cn } from '@/lib/utils'

const COLD = 80

type Sort = 'rate' | 'calls' | 'misses' | 'p95' | 'name'

const percent = (rate: number | null) => (rate === null ? '—' : `${rate.toFixed(1)} %`)

function sorted(operations: Operation[], by: Sort): Operation[] {
  const order: Record<Sort, (a: Operation, b: Operation) => number> = {
    rate: (a, b) => (a.hitRate ?? 101) - (b.hitRate ?? 101),
    calls: (a, b) => b.perSecond - a.perSecond,
    misses: (a, b) => b.misses - a.misses,
    p95: (a, b) => b.calls - a.calls,
    name: (a, b) => a.operation.localeCompare(b.operation),
  }
  return [...operations].sort(order[by])
}

export function CachePage() {
  const { ms: windowMs, label } = useWindow()
  const [group, setGroup] = useState<Grouping>('operation')
  const [sort, setSort] = useState<Sort>('rate')
  const [q, setQ] = useState('')
  const environment = useSearchParam('env') ?? ''
  const [typedEnv, setTypedEnv] = useState(environment)
  const settledEnv = useSettled(typedEnv)
  const { operations, pending, failed } = useCacheOperations(group, settledEnv, windowMs)
  const { series } = useCacheSeries(settledEnv, windowMs)
  const rows = useMemo(
    () =>
      sorted(
        operations.filter((row) => `${row.operation} ${row.store}`.toLowerCase().includes(q.trim().toLowerCase())),
        sort,
      ),
    [operations, sort, q],
  )
  const calls = series?.calls ?? 0
  const judged = (series?.hits ?? 0) + (series?.misses ?? 0)
  const perStep = series?.points.map((point) => point.calls) ?? []
  const missed = series?.points.map((point) => point.misses) ?? []
  const ceiling = Math.max(1, ...perStep)
  const noun = group === 'operation' ? 'operation' : 'store'

  return (
    <>
      <PageHeader
        title="Cache"
        meta={
          <>
            Cache client spans, grouped by <span className="font-mono">{group === 'operation' ? 'span name' : 'db.system.name'}</span> · last {label}
          </>
        }
      >
        <Segmented
          label="Group by"
          value={group}
          onChange={setGroup}
          options={[
            { value: 'operation', label: 'By operation' },
            { value: 'store', label: 'By store' },
          ]}
        />
      </PageHeader>

      <div className="grid grid-cols-[repeat(auto-fit,minmax(190px,1fr))] gap-3">
        <Kpi
          label="Hit rate"
          value={percent(series?.hitRate ?? null)}
          sub={judged === 0 ? 'no cache.hit in range' : `over ${judged.toLocaleString()} judged ${judged === 1 ? 'call' : 'calls'}`}
          tone={series?.hitRate !== null && series?.hitRate !== undefined && series.hitRate < COLD ? 'warn' : undefined}
        />
        <Kpi label="Operations" value={calls === 0 ? '—' : `${(calls / (windowMs / 1000)).toFixed(1)}/s`} sub={`${calls.toLocaleString()} in the last ${label}`} />
        <Kpi label="Misses" value={(series?.misses ?? 0).toLocaleString()} sub={judged === 0 ? 'nothing judged yet' : `of ${judged.toLocaleString()} judged`} />
        <Kpi
          label="p95"
          value={series?.p95Ms === null || series?.p95Ms === undefined ? '—' : dur(series.p95Ms)}
          sub={series?.p99Ms === null || series?.p99Ms === undefined ? 'no p99 yet' : `p99 ${dur(series.p99Ms)}`}
        />
      </div>

      <LineChart
        title="Cache operations per second"
        legend="solid: all · dashed: misses"
        values={perStep}
        second={missed}
        lo={0}
        hi={ceiling}
        yLabels={[String(ceiling), String(Math.round(ceiling / 2)), '0']}
        aria={`Cache operations over the last ${label}, with misses beneath`}
      />

      <Card className="overflow-hidden">
        <div className="flex flex-wrap items-center gap-2 border-b px-4 py-2.5">
          <h2 className="text-[15px] font-bold">{group === 'operation' ? 'Operations' : 'Stores'}</h2>
          <span className="text-caption text-muted-foreground">
            {rows.length} {noun}
            {rows.length === 1 ? '' : 's'}
          </span>
          <span className="flex-1" />
          <Segmented
            label="Sort by"
            value={sort}
            onChange={setSort}
            options={[
              { value: 'rate', label: 'Hit rate' },
              { value: 'calls', label: 'Calls' },
              { value: 'misses', label: 'Misses' },
              { value: 'name', label: 'Name' },
            ]}
          />
          <InputGroup className="w-[220px] max-w-full">
            <input aria-label="Environment" placeholder="Environment, e.g. production" value={typedEnv} onChange={(e) => setTypedEnv(e.target.value)} />
          </InputGroup>
          <InputGroup className="w-[260px] max-w-full">
            <Search aria-hidden />
            <input aria-label="Filter rows" placeholder={group === 'operation' ? 'Filter operations, e.g. GET' : 'Filter stores'} value={q} onChange={(e) => setQ(e.target.value)} />
            <Kbd>/</Kbd>
          </InputGroup>
        </div>

        {failed ? (
          <div className="p-6">
            <EmptyState title="The cache could not be read">{because(failed, 'The span store did not answer')}</EmptyState>
          </div>
        ) : null}

        {!failed && !pending && operations.length === 0 ? (
          <div className="p-6">
            <EmptyState title={`No cache operations in the last ${label}`} code="npm i @opentelemetry/instrumentation-ioredis">
              Operations appear once your Redis or Memcached client is instrumented. FerrisTrace reads a client span that names{' '}
              <span className="font-mono">redis</span>, <span className="font-mono">memcached</span> or <span className="font-mono">valkey</span> in{' '}
              <span className="font-mono">db.system.name</span>, or that carries <span className="font-mono">cache.hit</span>.
            </EmptyState>
          </div>
        ) : null}

        {operations.length > 0 ? (
          <div className="overflow-x-auto">
            <Table aria-label={group === 'operation' ? 'Cache operations' : 'Cache stores'}>
              <thead>
                <tr>
                  <Th>{group === 'operation' ? 'Operation' : 'Store'}</Th>
                  {group === 'operation' ? <Th className="w-[130px]">Store</Th> : null}
                  <Th numeric className="w-[100px]">Calls</Th>
                  <Th numeric className="w-[90px]">Hits</Th>
                  <Th numeric className="w-[90px]">Misses</Th>
                  <Th className="w-[160px]">Hit rate</Th>
                  <Th numeric className="w-[80px]">p50</Th>
                  <Th numeric className="w-[80px]">p95</Th>
                </tr>
              </thead>
              <tbody>
                {rows.map((row) => {
                  const cold = row.hitRate !== null && row.hitRate < COLD
                  return (
                    <Tr key={`${row.operation} ${row.store}`}>
                      <Td><span className="font-mono font-semibold">{row.operation}</span></Td>
                      {group === 'operation' ? <Td className="text-control text-muted-foreground">{row.store}</Td> : null}
                      <Td numeric>{row.rps}</Td>
                      <Td numeric>{row.hits.toLocaleString()}</Td>
                      <Td numeric className={cn(cold && 'font-semibold text-warning-text')}>{row.misses.toLocaleString()}</Td>
                      <Td>
                        {row.hitRate === null ? (
                          <span className="text-caption text-muted-foreground">no verdict</span>
                        ) : (
                          <span className="inline-flex max-w-full items-center gap-1.5">
                            <span className="inline-flex h-1.5 w-14 shrink-0 overflow-hidden rounded bg-muted">
                              <span className={cn('rounded', barBg(cold ? 'bar-w' : 'bar-o'))} style={{ width: `${Math.min(100, row.hitRate)}%` }} />
                            </span>
                            <span className={cn('font-mono text-caption', cold && 'font-semibold text-warning-text')}>{percent(row.hitRate)}</span>
                          </span>
                        )}
                      </Td>
                      <Td numeric>{row.p50}</Td>
                      <Td numeric>{row.p95}</Td>
                    </Tr>
                  )
                })}
              </tbody>
            </Table>
          </div>
        ) : null}

        <p className="m-0 px-4 py-2.5 text-caption text-muted-foreground">
          A hit is <span className="font-mono">cache.hit = true</span> and a miss <span className="font-mono">false</span>; a span carrying neither counts in the calls and in no hit rate, which then reads{' '}
          <span className="font-mono">no verdict</span> rather than nought per cent. The operation is the span name, so a write such as{' '}
          <span className="font-mono">SET</span> is a row of its own. <span className="font-mono">cache.key</span> is never read. Amber under {COLD} %.
        </p>
      </Card>
    </>
  )
}
