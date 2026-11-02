import { useInfiniteQuery, useQuery } from '@tanstack/react-query'
import { get, json } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { useScope } from '@/lib/scope'
import type { Sev } from '@/lib/otel'

export const SEVERITIES: Sev[] = ['trace', 'debug', 'info', 'warn', 'error', 'fatal']

export type LogRecord = {
  time_ns: number
  observed_ns: number
  severity: Sev
  service: string
  body: string
  trace_id: string | null
  span_id: string | null
  attributes: Record<string, string>
  resource: Record<string, string>
}

export type LogPage = { records: LogRecord[]; next: string | null }

export type Counts = Record<Sev, number>

export type Bucket = { start_ns: number; counts: Counts }

export type LogHistogram = { buckets: Bucket[] }

export type LogQuery = {
  rangeMs: number
  fromMs: number | null
  toMs: number | null
  services: string[]
  severities: Sev[]
  q: string
  attrs: string[]
  trace: string
  limit: number
}

export const RANGES: [string, number][] = [
  ['15 min', 15 * 60 * 1000],
  ['1 hour', 60 * 60 * 1000],
  ['6 hours', 6 * 60 * 60 * 1000],
  ['24 hours', 24 * 60 * 60 * 1000],
  ['7 days', 7 * 24 * 60 * 60 * 1000],
]

export const ANY: LogQuery = {
  rangeMs: 60 * 60 * 1000,
  fromMs: null,
  toMs: null,
  services: [],
  severities: [],
  q: '',
  attrs: [],
  trace: '',
  limit: 100,
}

export const LIVE_MS = 5000
const FRESH_MS = 15_000
const BUCKETS = 60

export const isPinned = (query: LogQuery) => query.fromMs !== null && query.toMs !== null

export const msOf = (ns: number) => Math.floor(ns / 1_000_000)

function filters(query: LogQuery, project: string | undefined): URLSearchParams {
  const to = query.toMs ?? Date.now()
  const from = query.fromMs ?? to - query.rangeMs
  const params = new URLSearchParams({ from: String(from), to: String(to) })
  if (project) params.set('project', project)
  for (const service of query.services) params.append('service', service)
  for (const severity of query.severities) params.append('severity', severity)
  if (query.q.trim()) params.set('q', query.q.trim())
  for (const attr of query.attrs) params.append('attr', attr)
  if (query.trace.trim()) params.set('trace', query.trace.trim())
  return params
}

export function useLogs(query: LogQuery, live = false) {
  const { active } = useOrganisations()
  const { project } = useScope()
  const found = useInfiniteQuery({
    queryKey: ['logs', active?.org, project, query, live],
    enabled: Boolean(active),
    staleTime: FRESH_MS,
    refetchInterval: live ? LIVE_MS : false,
    refetchIntervalInBackground: false,
    initialPageParam: null as string | null,
    getNextPageParam: (last: LogPage) => last.next,
    queryFn: async ({ pageParam }) => {
      const params = filters(query, project)
      params.set('limit', String(query.limit))
      if (pageParam) params.set('before', pageParam)
      return json<LogPage>(await get(`/api/orgs/${active?.org}/logs?${params.toString()}`))
    },
  })
  const records = found.data?.pages.flatMap((page) => page.records) ?? []
  return {
    records,
    loading: found.isFetching && !found.isFetchingNextPage,
    pending: found.isPending,
    failed: found.error,
    more: Boolean(found.hasNextPage) && !live,
    older: () => void found.fetchNextPage(),
    loadingOlder: found.isFetchingNextPage,
  }
}

export function useLogHistogram(query: LogQuery, live = false) {
  const { active } = useOrganisations()
  const { project } = useScope()
  const found = useQuery({
    queryKey: ['logs-histogram', active?.org, project, { ...query, limit: 0 }, live],
    enabled: Boolean(active),
    staleTime: FRESH_MS,
    refetchInterval: live ? LIVE_MS : false,
    refetchIntervalInBackground: false,
    queryFn: async () => {
      const params = filters(query, project)
      params.set('buckets', String(BUCKETS))
      return json<LogHistogram>(await get(`/api/orgs/${active?.org}/logs/histogram?${params.toString()}`))
    },
  })
  return { buckets: found.data?.buckets ?? [], loading: found.isFetching, failed: found.error }
}
