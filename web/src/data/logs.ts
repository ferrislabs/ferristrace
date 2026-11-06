import { useInfiniteQuery, useQuery } from '@tanstack/react-query'
import { get, json, post } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { useScope } from '@/lib/scope'
import type { Sev } from '@/lib/otel'
import { NOWHERE, PLACES, placed, type Placement } from '@/data/placement'
import { bodyOf, type Node } from '@/lib/queryLang'
import { boundsOf, useZoom, zoomKey } from '@/lib/bounds'

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

function filters(query: LogQuery, project: string | undefined, placement: Placement): URLSearchParams {
  const to = query.toMs ?? Date.now()
  const from = query.fromMs ?? to - query.rangeMs
  const params = new URLSearchParams({ from: String(from), to: String(to) })
  if (project) params.set('project', project)
  for (const service of query.services) params.append('service', service)
  for (const severity of query.severities) params.append('severity', severity)
  if (query.q.trim()) params.set('q', query.q.trim())
  for (const attr of query.attrs) params.append('attr', attr)
  if (query.trace.trim()) params.set('trace', query.trace.trim())
  return placed(params, placement)
}

export function useLogs(query: LogQuery, live = false, placement: Placement = NOWHERE) {
  const { active } = useOrganisations()
  const { project } = useScope()
  const found = useInfiniteQuery({
    queryKey: ['logs', active?.org, project, query, live, placement],
    enabled: Boolean(active),
    staleTime: FRESH_MS,
    refetchInterval: live ? LIVE_MS : false,
    refetchIntervalInBackground: false,
    initialPageParam: null as string | null,
    getNextPageParam: (last: LogPage) => last.next,
    queryFn: async ({ pageParam }) => {
      const params = filters(query, project, placement)
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

export function useLogHistogram(query: LogQuery, live = false, placement: Placement = NOWHERE) {
  const { active } = useOrganisations()
  const { project } = useScope()
  const found = useQuery({
    queryKey: ['logs-histogram', active?.org, project, { ...query, limit: 0 }, live, placement],
    enabled: Boolean(active),
    staleTime: FRESH_MS,
    refetchInterval: live ? LIVE_MS : false,
    refetchIntervalInBackground: false,
    queryFn: async () => {
      const params = filters(query, project, placement)
      params.set('buckets', String(BUCKETS))
      return json<LogHistogram>(await get(`/api/orgs/${active?.org}/logs/histogram?${params.toString()}`))
    },
  })
  return { buckets: found.data?.buckets ?? [], loading: found.isFetching, failed: found.error }
}

export const AROUND_TRACE_MS = 5 * 60 * 1000
const TRACE_LOGS = 200

export function traceWindow(startedAtMs: number, durationMs: number): { fromMs: number; toMs: number } {
  return { fromMs: startedAtMs - AROUND_TRACE_MS, toMs: Math.ceil(startedAtMs + durationMs) + AROUND_TRACE_MS }
}

export function useTraceLogs(trace: string | undefined, startedAtMs: number, durationMs: number) {
  const { active } = useOrganisations()
  const { project } = useScope()
  const found = useQuery({
    queryKey: ['logs', active?.org, project, 'trace', trace, startedAtMs, durationMs],
    enabled: Boolean(active && trace),
    staleTime: FRESH_MS,
    queryFn: async () => {
      const { fromMs, toMs } = traceWindow(startedAtMs, durationMs)
      const params = new URLSearchParams({ from: String(fromMs), to: String(toMs), trace: trace ?? '', limit: String(TRACE_LOGS) })
      if (project) params.set('project', project)
      return json<LogPage>(await get(`/api/orgs/${active?.org}/logs?${params.toString()}`))
    },
  })
  return {
    records: found.data?.records ?? [],
    more: Boolean(found.data?.next),
    pending: found.isPending,
    failed: found.error,
  }
}

function explored(items: Node[], rangeMs: number, zoom: ReturnType<typeof useZoom>, project: string | undefined, placement: Placement): Record<string, unknown> {
  const bounds = boundsOf(rangeMs, zoom)
  const body: Record<string, unknown> = { from: bounds.from, to: bounds.to }
  if (project) body.project = project
  const query = bodyOf(items)
  if (query !== undefined) body.query = query
  for (const place of PLACES) {
    const value = placement[place].trim()
    if (value) body[place] = value
  }
  return body
}

export function useLogExplorer(items: Node[], rangeMs: number, limit: number, live: boolean, placement: Placement = NOWHERE) {
  const { active } = useOrganisations()
  const { project } = useScope()
  const zoom = useZoom()
  const found = useInfiniteQuery({
    queryKey: ['logs-explorer', active?.org, project, rangeMs, items, limit, live, placement, zoomKey(zoom)],
    enabled: Boolean(active),
    staleTime: FRESH_MS,
    refetchInterval: live ? LIVE_MS : false,
    refetchIntervalInBackground: false,
    initialPageParam: null as string | null,
    getNextPageParam: (last: LogPage) => last.next,
    queryFn: async ({ pageParam }) => {
      const body = explored(items, rangeMs, zoom, project, placement)
      body.limit = limit
      if (pageParam) body.before = pageParam
      return json<LogPage>(await post(`/api/orgs/${active?.org}/logs/search`, body))
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

export function useLogSeries(items: Node[], rangeMs: number, placement: Placement, live: boolean) {
  const { active } = useOrganisations()
  const { project } = useScope()
  const zoom = useZoom()
  const found = useQuery({
    queryKey: ['logs-series', active?.org, project, rangeMs, items, placement, live, zoomKey(zoom)],
    enabled: Boolean(active),
    staleTime: FRESH_MS,
    refetchInterval: live ? LIVE_MS : false,
    refetchIntervalInBackground: false,
    queryFn: async () => {
      const body = explored(items, rangeMs, zoom, project, placement)
      body.buckets = BUCKETS
      return json<LogHistogram>(await post(`/api/orgs/${active?.org}/logs/series`, body))
    },
  })
  const bars = (found.data?.buckets ?? []).map((bucket) => ({
    at: msOf(bucket.start_ns),
    count: SEVERITIES.reduce((sum, level) => sum + bucket.counts[level], 0),
    errors: bucket.counts.error + bucket.counts.fatal,
  }))
  return { bars, loading: found.isFetching, failed: found.error }
}

export function legacyText(raw: string | undefined): string {
  if (!raw) return ''
  const p = new URLSearchParams(raw)
  const typed = p.get('query')
  if (typed !== null) return typed
  const quoted = (value: string) => `"${value.replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"`
  const parts: string[] = []
  const services = p.getAll('service')
  if (services.length === 1) parts.push(`service = ${quoted(services[0])}`)
  else if (services.length > 1) parts.push(`service IN (${services.map(quoted).join(', ')})`)
  const levels = p.getAll('severity').filter((level) => (SEVERITIES as string[]).includes(level))
  if (levels.length === 1) parts.push(`severity = ${levels[0]}`)
  else if (levels.length > 1) parts.push(`severity IN (${levels.join(', ')})`)
  const text = p.get('q')?.trim()
  if (text) parts.push(`body CONTAINS ${quoted(text)}`)
  for (const pair of p.getAll('attr')) {
    const at = pair.indexOf('=')
    if (at > 0) parts.push(`${quoted(pair.slice(0, at))} = ${quoted(pair.slice(at + 1))}`)
  }
  const trace = p.get('trace')?.trim()
  if (trace) parts.push(`trace_id = ${quoted(trace)}`)
  return parts.join(' AND ')
}

export function legacyZoom(raw: string | undefined): { from: number; to: number } | null {
  if (!raw) return null
  const p = new URLSearchParams(raw)
  const from = Number(p.get('from'))
  const to = Number(p.get('to'))
  return Number.isFinite(from) && Number.isFinite(to) && from > 0 && to > from ? { from, to } : null
}
