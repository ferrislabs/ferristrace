import { useQuery } from '@tanstack/react-query'
import { get, json } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { useScope } from '@/lib/scope'
import { NOWHERE, placed, type Placement } from '@/data/placement'

export type MetricKind = 'gauge' | 'sum' | 'histogram'

export type Aggregation = 'sum' | 'avg' | 'min' | 'max' | 'rate' | 'p50' | 'p95' | 'p99'

export type MetricInfo = {
  name: string
  kind: MetricKind
  unit: string
  description: string
  last_seen_ns: number
}

export type MetricCatalogue = { metrics: MetricInfo[] }

export type SeriesPoint = [number, number | null]

export type MetricLine = { labels: Record<string, string>; points: SeriesPoint[] }

export type Exemplar = { time_ns: number; value: number; trace_id: string; span_id: string }

export type MetricSeriesResult = { step_ns: number; series: MetricLine[]; exemplars: Exemplar[] }

export type MetricQuery = {
  name: string
  rangeMs: number
  fromMs: number | null
  toMs: number | null
  groupBy: string[]
  agg: Aggregation
}

export const AGGREGATIONS: Record<MetricKind, Aggregation[]> = {
  gauge: ['sum', 'avg', 'min', 'max'],
  sum: ['sum', 'avg', 'min', 'max', 'rate'],
  histogram: ['p50', 'p95', 'p99', 'avg', 'rate'],
}

export const DEFAULT_AGG: Record<MetricKind, Aggregation> = { gauge: 'avg', sum: 'rate', histogram: 'p95' }

export const RANGES: [string, number][] = [
  ['15 min', 15 * 60 * 1000],
  ['1 hour', 60 * 60 * 1000],
  ['6 hours', 6 * 60 * 60 * 1000],
  ['24 hours', 24 * 60 * 60 * 1000],
  ['7 days', 7 * 24 * 60 * 60 * 1000],
]

export const DEFAULT_RANGE_MS = 60 * 60 * 1000

const FRESH_MS = 15_000

export const isPinned = (query: Pick<MetricQuery, 'fromMs' | 'toMs'>) => query.fromMs !== null && query.toMs !== null

export function useMetricCatalogue(q: string, projectOverride?: string) {
  const { active } = useOrganisations()
  const { project: scoped } = useScope()
  const project = projectOverride || scoped
  const found = useQuery({
    queryKey: ['metrics', active?.org, project, q.trim()],
    enabled: Boolean(active),
    staleTime: FRESH_MS,
    queryFn: async () => {
      const params = new URLSearchParams()
      if (q.trim()) params.set('q', q.trim())
      if (project) params.set('project', project)
      return json<MetricCatalogue>(await get(`/api/orgs/${active?.org}/metrics?${params.toString()}`))
    },
  })
  return { metrics: found.data?.metrics ?? [], loading: found.isFetching, pending: found.isPending, failed: found.error }
}

export function useMetricSeries(query: MetricQuery | null, placement: Placement = NOWHERE) {
  const { active } = useOrganisations()
  const { project } = useScope()
  const found = useQuery({
    queryKey: ['metric-series', active?.org, project, query, placement],
    enabled: Boolean(active) && query !== null,
    staleTime: FRESH_MS,
    retry: false,
    queryFn: async () => {
      const q = query as MetricQuery
      const to = q.toMs ?? Date.now()
      const from = q.fromMs ?? to - q.rangeMs
      const params = new URLSearchParams({ from: String(from), to: String(to), agg: q.agg })
      for (const key of q.groupBy) params.append('group_by', key)
      if (project) params.set('project', project)
      placed(params, placement)
      return json<MetricSeriesResult>(
        await get(`/api/orgs/${active?.org}/metrics/${encodeURIComponent(q.name)}/series?${params.toString()}`),
      )
    },
  })
  return {
    result: found.data ?? null,
    loading: found.isFetching,
    pending: found.isPending && query !== null,
    failed: found.error,
  }
}
