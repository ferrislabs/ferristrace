import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { del, get, json, post } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { useScope } from '@/lib/scope'
import { nothingYet } from './orgs'

export type LlmCall = {
  timeMs: number
  traceId: string
  spanId: string
  service: string
  provider: string
  operation: string
  model: string
  inputTokens: number
  outputTokens: number
  cachedInputTokens: number
  cost: number | null
  durationMs: number
  error: boolean
  feature: string
  customer: string
  user: string
  promptVersion: string
  environment: string
}

export type ModelPrice = {
  id: string
  provider: string
  model: string
  input: number
  output: number
  cachedInput: number | null
  effectiveFromMs: number
}

export type PriceDraft = {
  provider: string
  model: string
  input: number
  output: number
  cachedInput: number | null
  effectiveFromMs: number
}

export type UnpricedModel = { provider: string; model: string; calls: number; lastSeenMs: number }

type WireCall = {
  time_ms: number
  trace_id: string
  span_id: string
  service: string
  provider: string
  operation: string
  model: string
  input_tokens: number
  output_tokens: number
  cached_input_tokens: number
  cost: number | null
  duration_ms: number
  error: boolean
  feature: string
  customer: string
  user: string
  prompt_version: string
  environment: string
}

type WirePrice = {
  id: string
  provider: string
  model: string
  input: number
  output: number
  cached_input: number | null
  effective_from_ms: number
}

type WireUnpriced = { provider: string; model: string; calls: number; last_seen_ms: number }

const FRESH_MS = 30_000
const MICROS_PER_DOLLAR = 1_000_000
const TRACE_CALLS = 500
const AROUND_TRACE_MS = 5 * 60 * 1000
export const UNPRICED_WINDOW_MS = 30 * 24 * 60 * 60 * 1000
export const MAX_RATE_MICROS = 1_000_000_000_000

const called = (call: WireCall): LlmCall => ({
  timeMs: call.time_ms,
  traceId: call.trace_id,
  spanId: call.span_id,
  service: call.service,
  provider: call.provider,
  operation: call.operation,
  model: call.model,
  inputTokens: call.input_tokens,
  outputTokens: call.output_tokens,
  cachedInputTokens: call.cached_input_tokens,
  cost: call.cost,
  durationMs: call.duration_ms,
  error: call.error,
  feature: call.feature,
  customer: call.customer,
  user: call.user,
  promptVersion: call.prompt_version,
  environment: call.environment,
})

const priced = (price: WirePrice): ModelPrice => ({
  id: price.id,
  provider: price.provider,
  model: price.model,
  input: price.input,
  output: price.output,
  cachedInput: price.cached_input,
  effectiveFromMs: price.effective_from_ms,
})

export const dollarsToMicros = (dollars: number) => Math.round(dollars * MICROS_PER_DOLLAR)

export const microsToDollars = (micros: number) => micros / MICROS_PER_DOLLAR

export function rateProblem(spelling: string, optional = false): string | null {
  const kept = spelling.trim()
  if (kept === '') return optional ? null : 'a price is needed'
  const dollars = Number(kept)
  if (!Number.isFinite(dollars) || dollars < 0) return 'a price is a number of dollars, zero or more'
  if (dollarsToMicros(dollars) > MAX_RATE_MICROS) return 'that price is beyond one million dollars per million tokens'
  return null
}

export function modelProblem(spelling: string): string | null {
  const kept = spelling.trim()
  if (kept === '') return 'a model is needed'
  if (kept.length > 128) return 'a model name is at most 128 bytes'
  const body = kept.endsWith('*') ? kept.slice(0, -1) : kept
  if (/[*\s]/.test(body)) return 'a model name has no blank, and `*` only as its last character'
  return null
}

export function providerProblem(spelling: string): string | null {
  const kept = spelling.trim()
  if (kept === '' || kept.length > 64 || /\s/.test(kept)) return 'a provider is one word of at most 64 bytes, like openai'
  return null
}

export function formatCost(cost: number | null): string {
  if (cost === null) return 'unpriced'
  if (cost === 0) return '$0.00'
  if (cost < 0.01) return `$${cost.toFixed(4)}`
  return `$${cost.toFixed(2)}`
}

export function formatRate(micros: number | null): string {
  if (micros === null) return '—'
  return `$${microsToDollars(micros).toFixed(microsToDollars(micros) < 1 ? 3 : 2)}`
}

export function formatTokens(tokens: number): string {
  if (tokens < 1000) return String(tokens)
  if (tokens < 1_000_000) return `${(tokens / 1000).toFixed(tokens < 10_000 ? 1 : 0)}k`
  return `${(tokens / 1_000_000).toFixed(1)}M`
}

export function modelTimeMs(calls: LlmCall[]): number {
  const spans = calls.map((call) => [call.timeMs, call.timeMs + call.durationMs] as const).sort((a, b) => a[0] - b[0])
  let total = 0
  let reach = -Infinity
  for (const [start, end] of spans) {
    const from = Math.max(start, reach)
    if (end > from) total += end - from
    reach = Math.max(reach, end)
  }
  return total
}

export function useTraceCalls(trace: string | undefined, startedAtMs: number, durationMs: number) {
  const { active } = useOrganisations()
  const { project } = useScope()
  const found = useQuery({
    queryKey: ['llm', active?.org, project, 'trace', trace, startedAtMs, durationMs],
    enabled: Boolean(active && trace),
    staleTime: FRESH_MS,
    retry: false,
    queryFn: async () => {
      const params = new URLSearchParams({
        from: String(startedAtMs - AROUND_TRACE_MS),
        to: String(Math.ceil(startedAtMs + durationMs) + AROUND_TRACE_MS),
        trace: trace ?? '',
        limit: String(TRACE_CALLS),
      })
      if (project) params.set('project', project)
      const body = await json<{ calls: WireCall[] }>(await get(`/api/orgs/${active?.org}/llm/calls?${params.toString()}`))
      return body.calls.map(called)
    },
  })
  return { calls: found.data ?? [], pending: found.isPending, failed: found.error }
}

const pricesKey = (org: string | undefined) => ['org', org, 'llm-prices']

export function usePrices() {
  const found = useOrganisations()
  const { active } = found
  const { data, isPending, error } = useQuery({
    queryKey: pricesKey(active?.org),
    enabled: Boolean(active),
    staleTime: FRESH_MS,
    queryFn: async () => (await json<{ prices: WirePrice[] }>(await get(`/api/orgs/${active?.org}/llm/prices`))).prices.map(priced),
  })
  return { prices: data, nothing: nothingYet(found, isPending, error, 'model prices') }
}

export function useUnpriced() {
  const { active } = useOrganisations()
  const { data } = useQuery({
    queryKey: ['org', active?.org, 'llm-unpriced'],
    enabled: Boolean(active),
    staleTime: FRESH_MS,
    retry: false,
    queryFn: async () => {
      const to = Date.now()
      const params = new URLSearchParams({ from: String(to - UNPRICED_WINDOW_MS), to: String(to) })
      const body = await json<{ models: WireUnpriced[] }>(await get(`/api/orgs/${active?.org}/llm/unpriced?${params.toString()}`))
      return body.models.map((model): UnpricedModel => ({ provider: model.provider, model: model.model, calls: model.calls, lastSeenMs: model.last_seen_ms }))
    },
  })
  return data
}

function useRefreshing<T, R>(send: (org: string, asked: T) => Promise<R>) {
  const { active } = useOrganisations()
  const client = useQueryClient()
  return useMutation({
    mutationFn: async (asked: T) => send(active?.org ?? '', asked),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: pricesKey(active?.org) })
      void client.invalidateQueries({ queryKey: ['org', active?.org, 'llm-unpriced'] })
      void client.invalidateQueries({ queryKey: ['llm', active?.org] })
    },
  })
}

export const useAddPrice = () =>
  useRefreshing<PriceDraft, ModelPrice>(async (org, asked) =>
    priced(
      await json<WirePrice>(
        await post(`/api/orgs/${org}/llm/prices`, {
          provider: asked.provider,
          model: asked.model,
          input: asked.input,
          output: asked.output,
          cached_input: asked.cachedInput,
          effective_from_ms: asked.effectiveFromMs,
        }),
      ),
    ),
  )

export const useDeletePrice = () =>
  useRefreshing<string, void>(async (org, id) => {
    await del(`/api/orgs/${org}/llm/prices/${id}`)
  })

export type GroupBy = 'feature' | 'model' | 'customer' | 'environment' | 'provider'

export const GROUP_BYS: { value: GroupBy; label: string }[] = [
  { value: 'feature', label: 'Feature' },
  { value: 'model', label: 'Model' },
  { value: 'customer', label: 'Customer' },
  { value: 'environment', label: 'Environment' },
  { value: 'provider', label: 'Provider' },
]

export const SPEND_WINDOWS: { value: string; label: string; ms: number }[] = [
  { value: '1h', label: '1 hour', ms: 60 * 60 * 1000 },
  { value: '1d', label: '24 hours', ms: 24 * 60 * 60 * 1000 },
  { value: '7d', label: '7 days', ms: 7 * 24 * 60 * 60 * 1000 },
  { value: '30d', label: '30 days', ms: 30 * 24 * 60 * 60 * 1000 },
]

export type SpendFilters = { feature: string; model: string; customer: string; user: string; prompt: string }

export const NO_FILTERS: SpendFilters = { feature: '', model: '', customer: '', user: '', prompt: '' }

export type SpendGroup = {
  key: string
  cost: number
  unpriced: number
  calls: number
  inputTokens: number
  outputTokens: number
  p50Ms: number
  p95Ms: number
  errors: number
  points: [number, number][]
}

export type PromptMarker = { timeMs: number; feature: string; promptVersion: string }

export type Spend = { fromMs: number; toMs: number; stepMs: number; groups: SpendGroup[]; markers: PromptMarker[] }

type WireSpendGroup = {
  key: string
  cost: number
  unpriced: number
  calls: number
  input_tokens: number
  output_tokens: number
  p50_ms: number
  p95_ms: number
  errors: number
  points: [number, number][]
}

type WireSpend = {
  step_ms: number
  groups: WireSpendGroup[]
  markers: { time_ms: number; feature: string; prompt_version: string }[]
}

const MINUTE_MS = 60_000
const GROUP_CALLS = 50

export const LLM_PRICES_PATH = '/settings/llm-prices'

export function useSpend(windowMs: number, groupBy: GroupBy, filters: SpendFilters) {
  const { active } = useOrganisations()
  const { project } = useScope()
  const minute = Math.floor(Date.now() / MINUTE_MS)
  const found = useQuery({
    queryKey: ['llm', active?.org, project, 'spend', windowMs, groupBy, filters, minute],
    enabled: Boolean(active),
    staleTime: FRESH_MS,
    retry: false,
    queryFn: async (): Promise<Spend> => {
      const to = Date.now()
      const from = to - windowMs
      const params = new URLSearchParams({ from: String(from), to: String(to), group_by: groupBy })
      for (const [name, value] of Object.entries(filters)) if (value.trim()) params.set(name, value.trim())
      if (project) params.set('project', project)
      const body = await json<WireSpend>(await get(`/api/orgs/${active?.org}/llm/spend?${params.toString()}`))
      return {
        fromMs: from,
        toMs: to,
        stepMs: body.step_ms,
        groups: body.groups.map((group) => ({
          key: group.key,
          cost: group.cost,
          unpriced: group.unpriced,
          calls: group.calls,
          inputTokens: group.input_tokens,
          outputTokens: group.output_tokens,
          p50Ms: group.p50_ms,
          p95Ms: group.p95_ms,
          errors: group.errors,
          points: group.points,
        })),
        markers: body.markers.map((marker) => ({ timeMs: marker.time_ms, feature: marker.feature, promptVersion: marker.prompt_version })),
      }
    },
  })
  return { spend: found.data ?? null, pending: found.isPending, failed: found.error }
}

export function useGroupCalls(windowMs: number, groupBy: GroupBy, key: string | null, filters: SpendFilters) {
  const { active } = useOrganisations()
  const { project } = useScope()
  const minute = Math.floor(Date.now() / MINUTE_MS)
  const found = useQuery({
    queryKey: ['llm', active?.org, project, 'group-calls', windowMs, groupBy, key, filters, minute],
    enabled: Boolean(active) && key !== null && key !== '',
    staleTime: FRESH_MS,
    retry: false,
    queryFn: async () => {
      const to = Date.now()
      const params = new URLSearchParams({ from: String(to - windowMs), to: String(to), limit: String(GROUP_CALLS) })
      for (const [name, value] of Object.entries(filters)) if (value.trim()) params.set(name, value.trim())
      params.set(groupBy, key ?? '')
      if (project) params.set('project', project)
      const body = await json<{ calls: WireCall[] }>(await get(`/api/orgs/${active?.org}/llm/calls?${params.toString()}`))
      return body.calls.map(called)
    },
  })
  return { calls: found.data ?? [], pending: found.isPending, failed: found.error }
}

export function formatMs(ms: number): string {
  if (ms === 0) return '—'
  if (ms < 1000) return `${ms.toFixed(0)} ms`
  return `${(ms / 1000).toFixed(2)} s`
}
