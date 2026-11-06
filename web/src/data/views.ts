import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { del, get, json, post, put } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { itemsText, normalise } from '@/lib/queryLang'
import { validChips } from '@/lib/traceQuery'
import type { Order } from '@/data/traces'

export type { Order }

export type Visibility = 'private' | 'org'

export type Tab = 'traces' | 'spans' | 'series' | 'table'

export const TABS: Tab[] = ['traces', 'spans', 'series', 'table']

export const TAB_LABELS: Record<Tab, string> = { traces: 'Traces', spans: 'Spans', series: 'Time series', table: 'Table' }

export type Measure = 'count' | 'errors' | 'error_rate' | 'average' | 'p50' | 'p95' | 'p99'

export const MEASURES: { value: Measure; label: string; unit: string }[] = [
  { value: 'count', label: 'Spans', unit: '' },
  { value: 'errors', label: 'Errors', unit: '' },
  { value: 'error_rate', label: 'Error rate', unit: '%' },
  { value: 'average', label: 'Average duration', unit: 'ms' },
  { value: 'p50', label: 'Duration p50', unit: 'ms' },
  { value: 'p95', label: 'Duration p95', unit: 'ms' },
  { value: 'p99', label: 'Duration p99', unit: 'ms' },
]

export const measureOf = (raw: unknown): Measure => (MEASURES.some((one) => one.value === raw) ? (raw as Measure) : 'count')

export const groupOf = (raw: unknown): string => (typeof raw === 'string' && raw.length <= 140 ? raw : '')

export const tabOf = (raw: unknown): Tab => (TABS.includes(raw as Tab) ? (raw as Tab) : 'traces')

export type Definition = { q: string; o: Order; t: Tab; m: Measure; g: string }

export type SavedView = {
  id: string
  name: string
  visibility: Visibility
  definition: Definition
  createdBy: string | null
  updatedAtMs: number
}

export type BuiltIn = { key: string; name: string; hint: string; definition: Definition }

export const BUILT_IN: BuiltIn[] = [
  { key: 'recent', name: 'Recent', hint: 'Newest first', definition: { q: '', o: 'started', t: 'traces', m: 'count', g: '' } },
  {
    key: 'errors',
    name: 'Errors',
    hint: 'Spans that failed',
    definition: { q: 'status = error', o: 'started', t: 'traces', m: 'count', g: '' },
  },
  { key: 'slow', name: 'Slow', hint: 'Slowest first', definition: { q: '', o: 'duration', t: 'traces', m: 'count', g: '' } },
]

const ORDERS: Order[] = ['started', 'duration', 'spans']

export const orderOf = (raw: unknown): Order => (ORDERS.includes(raw as Order) ? (raw as Order) : 'started')

export function definitionOf(raw: unknown): Definition {
  const body = typeof raw === 'object' && raw !== null ? (raw as Record<string, unknown>) : {}
  const q = typeof body.q === 'string' ? normalise(body.q) : itemsText(validChips(body.q))
  return { q, o: orderOf(body.o), t: tabOf(body.t), m: measureOf(body.m), g: groupOf(body.g) }
}

export const sameDefinition = (a: Definition, b: Definition) => a.o === b.o && a.t === b.t && a.m === b.m && a.g === b.g && normalise(a.q) === normalise(b.q)

type Wire = {
  id: string
  name: string
  visibility: Visibility
  definition: unknown
  created_by: string | null
  updated_at_ms: number
}

const viewed = (wire: Wire): SavedView => ({
  id: wire.id,
  name: wire.name,
  visibility: wire.visibility,
  definition: definitionOf(wire.definition),
  createdBy: wire.created_by,
  updatedAtMs: wire.updated_at_ms,
})

const key = (org: string | undefined) => ['org', org, 'views', 'traces']

export function useViews(): { views: SavedView[]; loading: boolean; failed: unknown } {
  const { active } = useOrganisations()
  const { data, isPending, error } = useQuery({
    queryKey: key(active?.org),
    enabled: Boolean(active),
    staleTime: 15_000,
    queryFn: async () => (await json<{ views: Wire[] }>(await get(`/api/orgs/${active?.org}/views?signal=traces`))).views.map(viewed),
  })
  return { views: data ?? [], loading: isPending && Boolean(active), failed: error }
}

export function useSaveView() {
  const { active } = useOrganisations()
  const client = useQueryClient()
  return useMutation({
    mutationFn: async (view: { name: string; visibility: Visibility; definition: Definition }) =>
      viewed(await json<Wire>(await post(`/api/orgs/${active?.org}/views`, { signal: 'traces', ...view }))),
    onSuccess: () => void client.invalidateQueries({ queryKey: key(active?.org) }),
  })
}

export function useChangeView() {
  const { active } = useOrganisations()
  const client = useQueryClient()
  return useMutation({
    mutationFn: async ({ id, ...change }: { id: string; name: string; definition: Definition; visibility?: Visibility }) =>
      viewed(await json<Wire>(await put(`/api/orgs/${active?.org}/views/${id}`, change))),
    onSuccess: () => void client.invalidateQueries({ queryKey: key(active?.org) }),
  })
}

export function useDeleteView() {
  const { active } = useOrganisations()
  const client = useQueryClient()
  return useMutation({
    mutationFn: async (id: string) => void (await del(`/api/orgs/${active?.org}/views/${id}`)),
    onSuccess: () => void client.invalidateQueries({ queryKey: key(active?.org) }),
  })
}
