import { useQuery } from '@tanstack/react-query'
import { get, json } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { useScope } from '@/lib/scope'
import { boundsOf, useZoom, zoomKey } from '@/lib/bounds'
import { NOWHERE, placed, type Placement } from '@/data/placement'
import { dur } from '@/lib/format'
import { perSecond, since } from '@/data/services'

export type Grouping = 'route' | 'service' | 'method'

export const GROUPINGS = [
  { value: 'route', label: 'By route', noun: 'route', field: 'http.route' },
  { value: 'service', label: 'By service', noun: 'service', field: 'service name' },
  { value: 'method', label: 'By method', noun: 'method', field: 'http.request.method' },
] as const

export const groupingOf = (value: unknown): Grouping =>
  GROUPINGS.find((one) => one.value === value)?.value ?? 'route'

export const groupingNoun = (grouping: Grouping) =>
  GROUPINGS.find((one) => one.value === grouping) ?? GROUPINGS[0]

export type Route = {
  route: string
  service: string
  method: string
  requests: number
  answered: number
  refused: number
  broken: number
  refusedRate: number | null
  brokenRate: number | null
  perSecond: number
  rps: string
  p50: string
  p95: string
  p95Us: number
  seen: string
  totalNanos: number
}

export type Point = { atMs: number; requests: number; refused: number; broken: number; p95Ms: number | null }

export type Series = {
  stepMs: number
  requests: number
  answered: number
  refused: number
  broken: number
  refusedRate: number | null
  brokenRate: number | null
  perSecond: number
  p50Ms: number | null
  p95Ms: number | null
  p99Ms: number | null
  points: Point[]
}

type Taken = {
  route: string
  service: string
  method: string
  requests: number
  answered: number
  refused: number
  broken: number
  refused_rate: number | null
  broken_rate: number | null
  per_second: number
  p50_us: number
  p95_us: number
  last_seen_ms: number
  total_nanos: number
}

type Flowing = {
  step_ns: number
  requests: number
  answered: number
  refused: number
  broken: number
  refused_rate: number | null
  broken_rate: number | null
  p50_ns: number | null
  p95_ns: number | null
  p99_ns: number | null
  points: [number, number, number, number, number | null][]
}

const FRESH_MS = 15_000
const NS_PER_MS = 1_000_000

const percent = (rate: number | null) => (rate === null ? null : rate * 100)

const taken = (row: Taken): Route => ({
  route: row.route,
  service: row.service,
  method: row.method,
  requests: row.requests,
  answered: row.answered,
  refused: row.refused,
  broken: row.broken,
  refusedRate: percent(row.refused_rate),
  brokenRate: percent(row.broken_rate),
  perSecond: row.per_second,
  rps: perSecond(row.per_second),
  p50: dur(row.p50_us / 1000),
  p95: dur(row.p95_us / 1000),
  p95Us: row.p95_us,
  seen: since(row.last_seen_ms),
  totalNanos: row.total_nanos,
})

const flowing = (steps: Flowing, seconds: number): Series => ({
  stepMs: steps.step_ns / NS_PER_MS,
  requests: steps.requests,
  answered: steps.answered,
  refused: steps.refused,
  broken: steps.broken,
  refusedRate: percent(steps.refused_rate),
  brokenRate: percent(steps.broken_rate),
  perSecond: seconds === 0 ? 0 : steps.requests / seconds,
  p50Ms: steps.p50_ns === null ? null : steps.p50_ns / NS_PER_MS,
  p95Ms: steps.p95_ns === null ? null : steps.p95_ns / NS_PER_MS,
  p99Ms: steps.p99_ns === null ? null : steps.p99_ns / NS_PER_MS,
  points: steps.points.map(([at, requests, refused, broken, p95]) => ({
    atMs: Math.floor(at / NS_PER_MS),
    requests,
    refused,
    broken,
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

export function useRequestRoutes(
  grouping: Grouping,
  rangeMs: number,
  environment = '',
  placement: Placement = NOWHERE,
) {
  const { active } = useOrganisations()
  const { project } = useScope()
  const zoom = useZoom()
  const { data, isPending, error } = useQuery({
    queryKey: ['requests', active?.org, project, grouping, environment.trim(), rangeMs, zoomKey(zoom), placement],
    enabled: Boolean(active),
    staleTime: FRESH_MS,
    queryFn: async () => {
      const { from, to } = boundsOf(rangeMs, zoom)
      const params = spanning(from, to, project, environment, placement)
      params.set('group', grouping)
      const found = await json<{ routes: Taken[] }>(
        await get(`/api/orgs/${active?.org}/requests?${params.toString()}`),
      )
      return found.routes.map(taken)
    },
  })
  return { routes: data ?? [], pending: isPending, failed: error }
}

export function useRequestSeries(rangeMs: number, environment = '', placement: Placement = NOWHERE) {
  const { active } = useOrganisations()
  const { project } = useScope()
  const zoom = useZoom()
  const { data, isPending, error } = useQuery({
    queryKey: ['requests', active?.org, project, 'series', environment.trim(), rangeMs, zoomKey(zoom), placement],
    enabled: Boolean(active),
    staleTime: FRESH_MS,
    queryFn: async () => {
      const { from, to } = boundsOf(rangeMs, zoom)
      const params = spanning(from, to, project, environment, placement)
      const found = await json<Flowing>(await get(`/api/orgs/${active?.org}/requests/series?${params.toString()}`))
      return flowing(found, (to - from) / 1000)
    },
  })
  return { series: data ?? null, pending: isPending, failed: error }
}
