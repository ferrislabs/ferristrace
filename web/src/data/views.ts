import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { del, get, json, post, put } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { validChips, type Chip } from '@/lib/traceQuery'
import type { Order } from '@/data/traces'

export type { Order }

export type Visibility = 'private' | 'org'

export type Definition = { q: Chip[]; o: Order }

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
  { key: 'recent', name: 'Recent', hint: 'Newest first', definition: { q: [], o: 'started' } },
  {
    key: 'errors',
    name: 'Errors',
    hint: 'Spans that failed',
    definition: { q: [{ field: { built: 'status' }, op: 'eq', value: 'error' }], o: 'started' },
  },
  { key: 'slow', name: 'Slow', hint: 'Slowest first', definition: { q: [], o: 'duration' } },
]

const ORDERS: Order[] = ['started', 'duration', 'spans']

export const orderOf = (raw: unknown): Order => (ORDERS.includes(raw as Order) ? (raw as Order) : 'started')

export function definitionOf(raw: unknown): Definition {
  const body = typeof raw === 'object' && raw !== null ? (raw as Record<string, unknown>) : {}
  return { q: validChips(body.q), o: orderOf(body.o) }
}

export const sameDefinition = (a: Definition, b: Definition) => a.o === b.o && JSON.stringify(a.q) === JSON.stringify(b.q)

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
