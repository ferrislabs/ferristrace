import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { ApiError, del, get, json, patch, post } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import type { Aggregation } from './metrics'
import { nothingYet } from './orgs'

export type AlertStatus = 'ok' | 'pending' | 'firing' | 'no_data'

export type TraceMeasure = 'count' | 'error_rate' | 'p50' | 'p95' | 'p99'

export type LogSeverity = 'trace' | 'debug' | 'info' | 'warn' | 'error' | 'fatal'

export type LlmMeasure = 'cost' | 'tokens' | 'calls' | 'p95'

export type LlmGroup = 'none' | 'feature' | 'customer' | 'model'

export type LlmFilter = { feature?: string; customer?: string; model?: string; project?: string }

export type RuleSignal =
  | { kind: 'traces'; service: string | null; operation: string | null; environment: string | null; measure: TraceMeasure }
  | { kind: 'logs'; services: string[]; severities: LogSeverity[]; q: string | null; attrs: string[]; measure: 'count' }
  | { kind: 'metrics'; name: string; aggregation: Aggregation; group_by: string[]; filter: Record<string, string> }
  | { kind: 'llm'; measure: LlmMeasure; group_by: LlmGroup; filter: LlmFilter }

export type SignalKind = RuleSignal['kind']

export type Op = '>' | '>=' | '<' | '<='

export type RuleSeverity = 'critical' | 'warning' | 'info'

export type NoData = 'ok' | 'no_data' | 'firing'

export type SeriesState = { labels: Record<string, string>; status: AlertStatus; value: number | null }

export type RuleState = {
  status: AlertStatus
  value: number | null
  since_ms: number
  evaluated_ms: number | null
  series: SeriesState[]
}

export type RuleDefinition = {
  name: string
  project: string | null
  enabled: boolean
  signal: RuleSignal
  condition: { op: Op; threshold: number }
  window_s: number
  pending_s: number
  severity: RuleSeverity
  no_data: NoData
  channels: string[]
  page: { schedule: string } | null
}

export type Rule = RuleDefinition & { id: string; state: RuleState; silenced_until_ms: number | null }

export type AlertContext = {
  pods: { name: string; node: string | null; count: number }[]
  others: number
  nodes: string[]
  versions: string[]
  labels: Record<string, string>
}

export type AlertEvent = {
  at_ms: number
  from: AlertStatus
  to: AlertStatus
  value: number | null
  labels: Record<string, string>
  context?: AlertContext
}

export type ChannelKind = 'email' | 'webhook' | 'slack' | 'discord'

export type Channel = { id: string; name: string; kind: ChannelKind; target: string; has_secret: boolean }

export type IssuedChannel = Channel & { secret?: string }

export type ChannelDraft = { name: string; kind: ChannelKind; target: string }

export type Preview = { step_ns: number; points: [number, number | null][]; threshold: number; would_fire: boolean }

export type Delivery = { channel: string; status: 'sent' | 'failed'; error: string | null }

export type TestResult = { deliveries: Delivery[] }

export const TRACE_MEASURES: TraceMeasure[] = ['count', 'error_rate', 'p50', 'p95', 'p99']

export const LLM_MEASURES: LlmMeasure[] = ['cost', 'tokens', 'calls', 'p95']

export const LLM_GROUPS: LlmGroup[] = ['none', 'feature', 'customer', 'model']

export const OPS: Op[] = ['>', '>=', '<', '<=']

export const SEVERITIES: RuleSeverity[] = ['critical', 'warning', 'info']

export const NO_DATA: NoData[] = ['ok', 'no_data', 'firing']

export const STATUSES: AlertStatus[] = ['firing', 'pending', 'no_data', 'ok']

export const LOG_SEVERITIES: LogSeverity[] = ['trace', 'debug', 'info', 'warn', 'error', 'fatal']

export const HISTORY_DAYS = 30

export const WINDOW_MIN_S = 60
export const WINDOW_MAX_S = 86_400
export const PENDING_MAX_S = 3_600

const FRESH_MS = 15_000

const key = (org: string | undefined) => ['org', org, 'alerts']
const channelKey = (org: string | undefined) => ['org', org, 'channels']

const retryable = (count: number, error: unknown) => !(error instanceof ApiError) && count < 2

export const isNotConfigured = (error: unknown) => error instanceof ApiError && error.status === 503
export const isForbidden = (error: unknown) => error instanceof ApiError && error.status === 403

export function useAlerts() {
  const found = useOrganisations()
  const { active } = found
  const { data, isPending, error } = useQuery({
    queryKey: key(active?.org),
    enabled: Boolean(active),
    staleTime: FRESH_MS,
    refetchInterval: 30_000,
    retry: retryable,
    queryFn: async () => (await json<{ alerts: Rule[] }>(await get(`/api/orgs/${active?.org}/alerts`))).alerts,
  })
  return { alerts: data, pending: isPending, failed: error, nothing: nothingYet(found, isPending, error, 'alert rules') }
}

export function useAlert(id: string | null) {
  const { active } = useOrganisations()
  const { data, isPending, error } = useQuery({
    queryKey: [...key(active?.org), id],
    enabled: Boolean(active) && id !== null,
    staleTime: FRESH_MS,
    retry: retryable,
    queryFn: async () => json<Rule>(await get(`/api/orgs/${active?.org}/alerts/${id}`)),
  })
  return { alert: data, pending: isPending && id !== null, failed: error }
}

export function useAlertHistory(id: string | null, days = HISTORY_DAYS) {
  const { active } = useOrganisations()
  const { data, isPending, error } = useQuery({
    queryKey: [...key(active?.org), id, 'history', days],
    enabled: Boolean(active) && id !== null,
    staleTime: FRESH_MS,
    retry: retryable,
    queryFn: async () => {
      const to = Date.now()
      const from = to - days * 86_400_000
      const asked = new URLSearchParams({ from: String(from), to: String(to) })
      return (await json<{ events: AlertEvent[] }>(await get(`/api/orgs/${active?.org}/alerts/${id}/history?${asked.toString()}`))).events
    },
  })
  return { events: data, pending: isPending && id !== null, failed: error }
}

export function useChannels() {
  const found = useOrganisations()
  const { active } = found
  const { data, isPending, error } = useQuery({
    queryKey: channelKey(active?.org),
    enabled: Boolean(active),
    staleTime: 30_000,
    retry: retryable,
    queryFn: async () => (await json<{ channels: Channel[] }>(await get(`/api/orgs/${active?.org}/channels`))).channels,
  })
  return { channels: data, pending: isPending, failed: error, nothing: nothingYet(found, isPending, error, 'alert channels') }
}

function useRefreshing<T, R>(root: (org: string | undefined) => unknown[], send: (org: string, asked: T) => Promise<R>) {
  const { active } = useOrganisations()
  const client = useQueryClient()
  return useMutation({
    mutationFn: async (asked: T) => send(active?.org ?? '', asked),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: root(active?.org) })
    },
  })
}

export const useCreateRule = () =>
  useRefreshing<RuleDefinition, Rule>(key, async (org, asked) => json(await post(`/api/orgs/${org}/alerts`, asked)))

export const useUpdateRule = () =>
  useRefreshing<{ id: string; definition: RuleDefinition }, Rule>(key, async (org, asked) =>
    json(await patch(`/api/orgs/${org}/alerts/${asked.id}`, asked.definition)),
  )

export const useToggleRule = () =>
  useRefreshing<{ id: string; enabled: boolean }, Rule>(key, async (org, asked) =>
    json(await patch(`/api/orgs/${org}/alerts/${asked.id}`, { enabled: asked.enabled })),
  )

export const SILENCE_MAX_DAYS = 30

export const useSilenceRule = () =>
  useRefreshing<{ id: string; until_ms: number }, Rule>(key, async (org, asked) =>
    json(await post(`/api/orgs/${org}/alerts/${asked.id}/silence`, { until_ms: asked.until_ms })),
  )

export const useUnsilenceRule = () =>
  useRefreshing<string, Rule>(key, async (org, id) => json(await del(`/api/orgs/${org}/alerts/${id}/silence`)))

export const useDeleteRule = () =>
  useRefreshing<string, void>(key, async (org, id) => {
    await del(`/api/orgs/${org}/alerts/${id}`)
  })

export const useCreateChannel = () =>
  useRefreshing<ChannelDraft, IssuedChannel>(channelKey, async (org, asked) =>
    json(await post(`/api/orgs/${org}/channels`, asked)),
  )

export const useUpdateChannel = () =>
  useRefreshing<{ id: string; name: string; target: string }, Channel>(channelKey, async (org, asked) =>
    json(await patch(`/api/orgs/${org}/channels/${asked.id}`, { name: asked.name, target: asked.target })),
  )

export const useDeleteChannel = () =>
  useRefreshing<string, void>(channelKey, async (org, id) => {
    await del(`/api/orgs/${org}/channels/${id}`)
  })

export async function previewRule(org: string, definition: RuleDefinition): Promise<Preview> {
  return json<Preview>(await post(`/api/orgs/${org}/alerts/preview`, definition))
}

export async function testRule(org: string, id: string): Promise<TestResult> {
  return json<TestResult>(await post(`/api/orgs/${org}/alerts/${id}/test`))
}

export const usePreviewRule = () => {
  const { active } = useOrganisations()
  return useMutation({ mutationFn: async (definition: RuleDefinition) => previewRule(active?.org ?? '', definition) })
}

export const useTestRule = () => {
  const { active } = useOrganisations()
  return useMutation({ mutationFn: async (id: string) => testRule(active?.org ?? '', id) })
}

export const definitionOf = (rule: Rule): RuleDefinition => ({
  name: rule.name,
  project: rule.project,
  enabled: rule.enabled,
  signal: rule.signal,
  condition: rule.condition,
  window_s: rule.window_s,
  pending_s: rule.pending_s,
  severity: rule.severity,
  no_data: rule.no_data,
  channels: rule.channels,
  page: rule.page,
})

export function duration(seconds: number): string {
  if (seconds % 3600 === 0 && seconds >= 3600) return `${seconds / 3600} h`
  if (seconds % 60 === 0) return `${seconds / 60} min`
  return `${seconds} s`
}

const quoted = (values: string[]) => values.join(', ')

export function scopeOf(signal: RuleSignal): string {
  if (signal.kind === 'traces') {
    const parts = [
      signal.service ? `service = ${signal.service}` : null,
      signal.operation ? `operation = ${signal.operation}` : null,
      signal.environment ? `environment = ${signal.environment}` : null,
    ].filter((part): part is string => part !== null)
    return parts.length ? parts.join(', ') : 'all spans'
  }
  if (signal.kind === 'logs') {
    const parts = [
      signal.services.length ? `service in ${quoted(signal.services)}` : null,
      signal.severities.length ? `severity in ${quoted(signal.severities)}` : null,
      signal.q ? `text "${signal.q}"` : null,
      ...signal.attrs,
    ].filter((part): part is string => part !== null)
    return parts.length ? parts.join(', ') : 'all logs'
  }
  if (signal.kind === 'llm') {
    const filters = Object.entries(signal.filter).map(([k, v]) => `${k} = ${v}`)
    const grouped = signal.group_by === 'none' ? null : `by ${signal.group_by}`
    const parts = [...filters, grouped].filter((part): part is string => part !== null)
    return parts.length ? parts.join(', ') : 'all calls'
  }
  const filters = Object.entries(signal.filter).map(([k, v]) => `${k} = ${v}`)
  const grouped = signal.group_by.length ? `by ${quoted(signal.group_by)}` : null
  const parts = [...filters, grouped].filter((part): part is string => part !== null)
  return parts.length ? parts.join(', ') : 'all series'
}

export function conditionOf(rule: RuleDefinition, metricUnit = ''): string {
  const { signal, condition } = rule
  const what =
    signal.kind === 'traces' ? `span ${signal.measure}` : signal.kind === 'logs' ? 'log count' : signal.kind === 'llm' ? `llm ${signal.measure}` : `${signal.aggregation}(${signal.name})`
  return `${what} ${condition.op} ${withUnit(condition.threshold, unitOf(signal, metricUnit))} over ${duration(rule.window_s)}${rule.pending_s > 0 ? `, pending ${duration(rule.pending_s)}` : ''}`
}

function logsFilter(signal: Extract<RuleSignal, { kind: 'logs' }>): string | undefined {
  const p = new URLSearchParams()
  for (const service of signal.services) p.append('service', service)
  for (const severity of signal.severities) p.append('severity', severity)
  if (signal.q) p.set('q', signal.q)
  for (const attr of signal.attrs) p.append('attr', attr)
  return p.toString() || undefined
}

export type SignalLink = { to: string; search?: Record<string, string>; label: string }

export function signalLink(signal: RuleSignal): SignalLink {
  if (signal.kind === 'traces') {
    return signal.service
      ? { to: `/services/${encodeURIComponent(signal.service)}`, label: `Open ${signal.service}` }
      : { to: '/traces', label: 'Open traces' }
  }
  if (signal.kind === 'logs') {
    const f = logsFilter(signal)
    return { to: '/logs', search: f ? { f } : undefined, label: 'Open these logs' }
  }
  if (signal.kind === 'llm') return { to: '/ai', label: 'Open AI spend' }
  const p = new URLSearchParams({ metric: signal.name, agg: signal.aggregation })
  for (const by of signal.group_by) p.append('by', by)
  return { to: '/metrics', search: { f: p.toString() }, label: 'Open this metric' }
}

const LLM_UNITS: Record<LlmMeasure, string> = { cost: '$', tokens: 'tokens', calls: 'calls', p95: 'ms' }

export function unitOf(signal: RuleSignal, metricUnit = ''): string {
  if (signal.kind === 'logs') return 'events'
  if (signal.kind === 'metrics') return metricUnit
  if (signal.kind === 'llm') return LLM_UNITS[signal.measure]
  if (signal.measure === 'count') return 'events'
  return signal.measure === 'error_rate' ? '%' : 'ms'
}

export const withUnit = (value: number | null, unit: string): string => {
  if (value === null) return 'no data'
  const shown = value.toLocaleString('en-US', { maximumFractionDigits: 6 })
  if (unit === '$') return `$${shown}`
  return `${shown}${unit ? ` ${unit}` : ''}`
}
