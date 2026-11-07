import { useQuery } from '@tanstack/react-query'
import { get, json } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { useScope } from '@/lib/scope'
import { boundsOf, useZoom, zoomKey } from '@/lib/bounds'
import { NOWHERE, placed, type Placement } from '@/data/placement'
import { dur } from '@/lib/format'
import { perSecond, since } from '@/data/services'

export type Grouping = 'job' | 'queue' | 'system'

export const GROUPINGS = [
  { value: 'job', label: 'By job', noun: 'job', field: 'span name' },
  { value: 'queue', label: 'By queue', noun: 'queue', field: 'messaging.destination.name' },
  { value: 'system', label: 'By system', noun: 'system', field: 'messaging.system' },
] as const

export const groupingOf = (value: unknown): Grouping =>
  GROUPINGS.find((one) => one.value === value)?.value ?? 'job'

export const groupingNoun = (grouping: Grouping) =>
  GROUPINGS.find((one) => one.value === grouping) ?? GROUPINGS[0]

export type Run = {
  group: string
  queue: string
  system: string
  processed: number
  failed: number
  failureRate: number | null
  perSecond: number
  rps: string
  p50: string
  p95: string
  totalMs: number
  share: number
  seen: string
}

export type Point = { atMs: number; processed: number; failed: number; failureRate: number | null; p95Ms: number | null }

export type Series = {
  stepMs: number
  processed: number
  failed: number
  failureRate: number | null
  perSecond: number
  p50Ms: number | null
  p95Ms: number | null
  p99Ms: number | null
  points: Point[]
}

type Done = {
  job: string
  queue: string
  system: string
  processed: number
  failed: number
  failure_rate: number | null
  per_second: number
  p50_us: number
  p95_us: number
  total_us: number
  last_seen_ms: number
}

type Working = {
  step_ns: number
  processed: number
  failed: number
  failure_rate: number | null
  p50_ns: number | null
  p95_ns: number | null
  p99_ns: number | null
  points: [number, number, number, number | null][]
}

const FRESH_MS = 15_000
const NS_PER_MS = 1_000_000

const percent = (rate: number | null) => (rate === null ? null : rate * 100)

const done = (row: Done, totalMs: number): Run => {
  const own = row.total_us / 1000
  return {
    group: row.job,
    queue: row.queue,
    system: row.system,
    processed: row.processed,
    failed: row.failed,
    failureRate: percent(row.failure_rate),
    perSecond: row.per_second,
    rps: perSecond(row.per_second),
    p50: dur(row.p50_us / 1000),
    p95: dur(row.p95_us / 1000),
    totalMs: own,
    share: totalMs === 0 ? 0 : (own / totalMs) * 100,
    seen: since(row.last_seen_ms),
  }
}

const working = (steps: Working, seconds: number): Series => ({
  stepMs: steps.step_ns / NS_PER_MS,
  processed: steps.processed,
  failed: steps.failed,
  failureRate: percent(steps.failure_rate),
  perSecond: seconds === 0 ? 0 : steps.processed / seconds,
  p50Ms: steps.p50_ns === null ? null : steps.p50_ns / NS_PER_MS,
  p95Ms: steps.p95_ns === null ? null : steps.p95_ns / NS_PER_MS,
  p99Ms: steps.p99_ns === null ? null : steps.p99_ns / NS_PER_MS,
  points: steps.points.map(([at, processed, failed, p95]) => ({
    atMs: Math.floor(at / NS_PER_MS),
    processed,
    failed,
    failureRate: processed === 0 ? null : (failed / processed) * 100,
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

export function useJobs(
  grouping: Grouping,
  rangeMs: number,
  environment = '',
  placement: Placement = NOWHERE,
) {
  const { active } = useOrganisations()
  const { project } = useScope()
  const zoom = useZoom()
  const { data, isPending, error } = useQuery({
    queryKey: ['jobs', active?.org, project, grouping, environment.trim(), rangeMs, zoomKey(zoom), placement],
    enabled: Boolean(active),
    staleTime: FRESH_MS,
    queryFn: async () => {
      const { from, to } = boundsOf(rangeMs, zoom)
      const params = spanning(from, to, project, environment, placement)
      params.set('group', grouping)
      const found = await json<{ jobs: Done[] }>(
        await get(`/api/orgs/${active?.org}/jobs?${params.toString()}`),
      )
      const totalMs = found.jobs.reduce((sum, row) => sum + row.total_us / 1000, 0)
      return found.jobs.map((row) => done(row, totalMs))
    },
  })
  return { jobs: data ?? [], pending: isPending, failed: error }
}

export function useJobSeries(rangeMs: number, environment = '', placement: Placement = NOWHERE) {
  const { active } = useOrganisations()
  const { project } = useScope()
  const zoom = useZoom()
  const { data, isPending, error } = useQuery({
    queryKey: ['jobs', active?.org, project, 'series', environment.trim(), rangeMs, zoomKey(zoom), placement],
    enabled: Boolean(active),
    staleTime: FRESH_MS,
    queryFn: async () => {
      const { from, to } = boundsOf(rangeMs, zoom)
      const params = spanning(from, to, project, environment, placement)
      const found = await json<Working>(await get(`/api/orgs/${active?.org}/jobs/series?${params.toString()}`))
      return working(found, (to - from) / 1000)
    },
  })
  return { series: data ?? null, pending: isPending, failed: error }
}
