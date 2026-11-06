import { useQuery } from '@tanstack/react-query'
import { get, json } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { boundsOf, useZoom, zoomKey, type Zoom } from '@/lib/bounds'
import { useScope } from '@/lib/scope'
import { useSignal } from '@/lib/signal'

export type SeenKey = { key: string; spans: number; numeric: boolean }
export type SeenValue = { value: string; spans: number }

const STALE_MS = 30_000

function window(rangeMs: number, project: string | undefined, prefix: string, zoom: Zoom | null): string {
  const bounds = boundsOf(rangeMs, zoom)
  const query = new URLSearchParams({ from: String(bounds.from), to: String(bounds.to) })
  if (project) query.set('project', project)
  if (prefix.trim()) query.set('prefix', prefix.trim())
  return query.toString()
}

const PATH = { traces: 'attributes', logs: 'log-attributes' } as const

export function useAttributeKeys(rangeMs: number, prefix: string, enabled: boolean): { keys: SeenKey[]; loading: boolean } {
  const signal = useSignal()
  const { active } = useOrganisations()
  const { project } = useScope()
  const zoom = useZoom()
  const { data, isFetching } = useQuery({
    queryKey: ['attributes', signal, active?.org, project, rangeMs, prefix, zoomKey(zoom)],
    enabled: Boolean(active) && enabled,
    staleTime: STALE_MS,
    queryFn: async () => (await json<{ attributes: SeenKey[] }>(await get(`/api/orgs/${active?.org}/${PATH[signal]}?${window(rangeMs, project, prefix, zoom)}`))).attributes,
  })
  return { keys: data ?? [], loading: isFetching }
}

export function useAttributeValues(key: string | undefined, rangeMs: number, prefix: string): { values: SeenValue[]; loading: boolean } {
  const signal = useSignal()
  const { active } = useOrganisations()
  const { project } = useScope()
  const zoom = useZoom()
  const { data, isFetching } = useQuery({
    queryKey: ['attribute-values', signal, active?.org, project, rangeMs, key, prefix, zoomKey(zoom)],
    enabled: Boolean(active && key),
    staleTime: STALE_MS,
    queryFn: async () =>
      (await json<{ values: SeenValue[] }>(await get(`/api/orgs/${active?.org}/${PATH[signal]}/${encodeURIComponent(key!)}/values?${window(rangeMs, project, prefix, zoom)}`))).values,
  })
  return { values: data ?? [], loading: isFetching }
}
