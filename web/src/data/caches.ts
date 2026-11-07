import { useQuery } from '@tanstack/react-query'
import { get, json } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { useScope } from '@/lib/scope'
import { dur } from '@/lib/format'
import { perSecond, since } from '@/data/services'

export type Grouping = 'operation' | 'store'

export type Operation = {
  operation: string
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

export const WINDOW_MS = 60 * 60 * 1000
const FRESH_MS = 15_000
const NS_PER_MS = 1_000_000

const percent = (rate: number | null) => (rate === null ? null : rate * 100)

const looked = (row: Looked): Operation => ({
  operation: row.operation,
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

const hitting = (steps: Hitting, windowMs: number): Series => ({
  stepMs: steps.step_ns / NS_PER_MS,
  calls: steps.calls,
  hits: steps.hits,
  misses: steps.misses,
  hitRate: percent(steps.hit_rate),
  perSecond: steps.calls / (windowMs / 1000),
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

function spanning(windowMs: number, project: string | undefined, environment: string): URLSearchParams {
  const to = Date.now()
  const params = new URLSearchParams({ from: String(to - windowMs), to: String(to) })
  if (project) params.set('project', project)
  if (environment.trim()) params.set('environment', environment.trim())
  return params
}

export function useCacheOperations(grouping: Grouping, environment = '', windowMs = WINDOW_MS) {
  const { active } = useOrganisations()
  const { project } = useScope()
  const { data, isPending, error } = useQuery({
    queryKey: ['caches', active?.org, project, grouping, environment.trim(), windowMs],
    enabled: Boolean(active),
    staleTime: FRESH_MS,
    queryFn: async () => {
      const params = spanning(windowMs, project, environment)
      params.set('group', grouping)
      const found = await json<{ operations: Looked[] }>(
        await get(`/api/orgs/${active?.org}/caches?${params.toString()}`),
      )
      return found.operations.map(looked)
    },
  })
  return { operations: data ?? [], pending: isPending, failed: error }
}

export function useCacheSeries(environment = '', windowMs = WINDOW_MS) {
  const { active } = useOrganisations()
  const { project } = useScope()
  const { data, isPending, error } = useQuery({
    queryKey: ['caches', active?.org, project, 'series', environment.trim(), windowMs],
    enabled: Boolean(active),
    staleTime: FRESH_MS,
    queryFn: async () => {
      const params = spanning(windowMs, project, environment)
      return hitting(await json<Hitting>(await get(`/api/orgs/${active?.org}/caches/series?${params.toString()}`)), windowMs)
    },
  })
  return { series: data ?? null, pending: isPending, failed: error }
}
