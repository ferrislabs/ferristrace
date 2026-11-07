import { useQuery } from '@tanstack/react-query'
import { get, json } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { useScope } from '@/lib/scope'
import { boundsOf, useZoom, zoomKey } from '@/lib/bounds'
import { NOWHERE, placed, type Placement } from '@/data/placement'
import { dur } from '@/lib/format'
import { perSecond, since } from '@/data/services'

export type Grouping = 'operation' | 'store' | 'key'

export const GROUPINGS = [
  { value: 'operation', label: 'By operation', noun: 'operation', field: 'span name' },
  { value: 'store', label: 'By store', noun: 'store', field: 'db.system.name' },
  { value: 'key', label: 'By key', noun: 'key pattern', field: 'cache.key' },
] as const

export const groupingOf = (value: unknown): Grouping =>
  GROUPINGS.find((one) => one.value === value)?.value ?? 'operation'

export const groupingNoun = (grouping: Grouping) =>
  GROUPINGS.find((one) => one.value === grouping) ?? GROUPINGS[0]

export type Operation = {
  group: string
  store: string
  calls: number
  hits: number
  misses: number
  hitRate: number | null
  perSecond: number
  rps: string
  p50: string
  p95: string
  seen: string
}

export type Point = { atMs: number; calls: number; hits: number; misses: number; hitRate: number | null; p95Ms: number | null }

export type Series = {
  stepMs: number
  calls: number
  hits: number
  misses: number
  hitRate: number | null
  perSecond: number
  p50Ms: number | null
  p95Ms: number | null
  p99Ms: number | null
  points: Point[]
}

type Looked = {
  operation: string
  store: string
  calls: number
  hits: number
  misses: number
  hit_rate: number | null
  per_second: number
  p50_us: number
  p95_us: number
  last_seen_ms: number
}

type Hitting = {
  step_ns: number
  calls: number
  hits: number
  misses: number
  hit_rate: number | null
  p50_ns: number | null
  p95_ns: number | null
  p99_ns: number | null
  points: [number, number, number, number, number | null, number | null][]
}

const FRESH_MS = 15_000
const NS_PER_MS = 1_000_000

const percent = (rate: number | null) => (rate === null ? null : rate * 100)

const looked = (row: Looked): Operation => ({
  group: row.operation,
  store: row.store,
  calls: row.calls,
  hits: row.hits,
  misses: row.misses,
  hitRate: percent(row.hit_rate),
  perSecond: row.per_second,
  rps: perSecond(row.per_second),
  p50: dur(row.p50_us / 1000),
  p95: dur(row.p95_us / 1000),
  seen: since(row.last_seen_ms),
})

const hitting = (steps: Hitting, seconds: number): Series => ({
  stepMs: steps.step_ns / NS_PER_MS,
  calls: steps.calls,
  hits: steps.hits,
  misses: steps.misses,
  hitRate: percent(steps.hit_rate),
  perSecond: seconds === 0 ? 0 : steps.calls / seconds,
  p50Ms: steps.p50_ns === null ? null : steps.p50_ns / NS_PER_MS,
  p95Ms: steps.p95_ns === null ? null : steps.p95_ns / NS_PER_MS,
  p99Ms: steps.p99_ns === null ? null : steps.p99_ns / NS_PER_MS,
  points: steps.points.map(([at, calls, hits, misses, rate, p95]) => ({
    atMs: Math.floor(at / NS_PER_MS),
    calls,
    hits,
    misses,
    hitRate: percent(rate),
    p95Ms: p95 === null ? null : p95 / NS_PER_MS,
  })),
})

function spanning(
  from: number,
  to: number,
  project: string | undefined,
  environment: string,
  placement: Placement,
): URLSearchParams {
  const params = new URLSearchParams({ from: String(Math.round(from)), to: String(Math.round(to)) })
  if (project) params.set('project', project)
  if (environment.trim()) params.set('environment', environment.trim())
  return placed(params, placement)
}

export function useCacheOperations(
  grouping: Grouping,
  rangeMs: number,
  environment = '',
  placement: Placement = NOWHERE,
) {
  const { active } = useOrganisations()
  const { project } = useScope()
  const zoom = useZoom()
  const { data, isPending, error } = useQuery({
    queryKey: ['caches', active?.org, project, grouping, environment.trim(), rangeMs, zoomKey(zoom), placement],
    enabled: Boolean(active),
    staleTime: FRESH_MS,
    queryFn: async () => {
      const { from, to } = boundsOf(rangeMs, zoom)
      const params = spanning(from, to, project, environment, placement)
      params.set('group', grouping)
      const found = await json<{ operations: Looked[] }>(
        await get(`/api/orgs/${active?.org}/caches?${params.toString()}`),
      )
      return found.operations.map(looked)
    },
  })
  return { operations: data ?? [], pending: isPending, failed: error }
}

export function useCacheSeries(rangeMs: number, environment = '', placement: Placement = NOWHERE) {
  const { active } = useOrganisations()
  const { project } = useScope()
  const zoom = useZoom()
  const { data, isPending, error } = useQuery({
    queryKey: ['caches', active?.org, project, 'series', environment.trim(), rangeMs, zoomKey(zoom), placement],
    enabled: Boolean(active),
    staleTime: FRESH_MS,
    queryFn: async () => {
      const { from, to } = boundsOf(rangeMs, zoom)
      const params = spanning(from, to, project, environment, placement)
      const found = await json<Hitting>(await get(`/api/orgs/${active?.org}/caches/series?${params.toString()}`))
      return hitting(found, (to - from) / 1000)
    },
  })
  return { series: data ?? null, pending: isPending, failed: error }
}
