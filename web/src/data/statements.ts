import { useQuery } from '@tanstack/react-query'
import { get, json } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { useScope } from '@/lib/scope'
import { boundsOf, useZoom, zoomKey } from '@/lib/bounds'
import { NOWHERE, placed, type Placement } from '@/data/placement'
import { dur } from '@/lib/format'
import { perSecond, since } from '@/data/services'

export type Grouping = 'statement' | 'database' | 'operation'

export const GROUPINGS = [
  { value: 'statement', label: 'By statement', noun: 'statement', field: 'db.query.text' },
  { value: 'database', label: 'By database', noun: 'database', field: 'db.namespace' },
  { value: 'operation', label: 'By operation', noun: 'operation', field: 'db.operation.name' },
] as const

export const groupingOf = (value: unknown): Grouping =>
  GROUPINGS.find((one) => one.value === value)?.value ?? 'statement'

export const groupingNoun = (grouping: Grouping) =>
  GROUPINGS.find((one) => one.value === grouping) ?? GROUPINGS[0]

export type Statement = {
  group: string
  database: string
  system: string
  calls: number
  errors: number
  errorRate: number | null
  perSecond: number
  rps: string
  totalUs: number
  p50: string
  p95: string
  seen: string
}

export type Point = { atMs: number; calls: number; errors: number; p95Ms: number | null }

export type Series = {
  stepMs: number
  calls: number
  errors: number
  errorRate: number | null
  perSecond: number
  p50Ms: number | null
  p95Ms: number | null
  p99Ms: number | null
  points: Point[]
}

type Said = {
  statement: string
  database: string
  system: string
  calls: number
  errors: number
  error_rate: number | null
  per_second: number
  total_us: number
  p50_us: number
  p95_us: number
  last_seen_ms: number
}

type Telling = {
  step_ns: number
  calls: number
  errors: number
  error_rate: number | null
  p50_ns: number | null
  p95_ns: number | null
  p99_ns: number | null
  points: [number, number, number, number | null][]
}

const FRESH_MS = 15_000
const NS_PER_MS = 1_000_000

const percent = (rate: number | null) => (rate === null ? null : rate * 100)

const said = (row: Said): Statement => ({
  group: row.statement,
  database: row.database,
  system: row.system,
  calls: row.calls,
  errors: row.errors,
  errorRate: percent(row.error_rate),
  perSecond: row.per_second,
  rps: perSecond(row.per_second),
  totalUs: row.total_us,
  p50: dur(row.p50_us / 1000),
  p95: dur(row.p95_us / 1000),
  seen: since(row.last_seen_ms),
})

const told = (steps: Telling, seconds: number): Series => ({
  stepMs: steps.step_ns / NS_PER_MS,
  calls: steps.calls,
  errors: steps.errors,
  errorRate: percent(steps.error_rate),
  perSecond: seconds === 0 ? 0 : steps.calls / seconds,
  p50Ms: steps.p50_ns === null ? null : steps.p50_ns / NS_PER_MS,
  p95Ms: steps.p95_ns === null ? null : steps.p95_ns / NS_PER_MS,
  p99Ms: steps.p99_ns === null ? null : steps.p99_ns / NS_PER_MS,
  points: steps.points.map(([at, calls, errors, p95]) => ({
    atMs: Math.floor(at / NS_PER_MS),
    calls,
    errors,
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

export function useStatements(
  grouping: Grouping,
  rangeMs: number,
  environment = '',
  placement: Placement = NOWHERE,
) {
  const { active } = useOrganisations()
  const { project } = useScope()
  const zoom = useZoom()
  const { data, isPending, error } = useQuery({
    queryKey: ['statements', active?.org, project, grouping, environment.trim(), rangeMs, zoomKey(zoom), placement],
    enabled: Boolean(active),
    staleTime: FRESH_MS,
    queryFn: async () => {
      const { from, to } = boundsOf(rangeMs, zoom)
      const params = spanning(from, to, project, environment, placement)
      params.set('group', grouping)
      const found = await json<{ statements: Said[] }>(
        await get(`/api/orgs/${active?.org}/statements?${params.toString()}`),
      )
      return found.statements.map(said)
    },
  })
  return { statements: data ?? [], pending: isPending, failed: error }
}

export function useStatementSeries(rangeMs: number, environment = '', placement: Placement = NOWHERE) {
  const { active } = useOrganisations()
  const { project } = useScope()
  const zoom = useZoom()
  const { data, isPending, error } = useQuery({
    queryKey: ['statements', active?.org, project, 'series', environment.trim(), rangeMs, zoomKey(zoom), placement],
    enabled: Boolean(active),
    staleTime: FRESH_MS,
    queryFn: async () => {
      const { from, to } = boundsOf(rangeMs, zoom)
      const params = spanning(from, to, project, environment, placement)
      const found = await json<Telling>(await get(`/api/orgs/${active?.org}/statements/series?${params.toString()}`))
      return told(found, (to - from) / 1000)
    },
  })
  return { series: data ?? null, pending: isPending, failed: error }
}
