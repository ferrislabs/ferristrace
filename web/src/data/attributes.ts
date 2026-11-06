import { useQuery } from '@tanstack/react-query'
import { get, json } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { boundsOf, useZoom, zoomKey, type Zoom } from '@/lib/bounds'
import { useScope } from '@/lib/scope'
import { useMetric, useSignal } from '@/lib/signal'

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

const pathOf = (signal: 'traces' | 'logs' | 'metrics', metric: string) =>
  signal === 'metrics' ? `metrics/${encodeURIComponent(metric)}/attributes` : signal === 'logs' ? 'log-attributes' : 'attributes'

export function useAttributeKeys(rangeMs: number, prefix: string, enabled: boolean): { keys: SeenKey[]; loading: boolean } {
  const signal = useSignal()
  const metric = useMetric()
  const { active } = useOrganisations()
  const { project } = useScope()
  const zoom = useZoom()
  const { data, isFetching } = useQuery({
    queryKey: ['attributes', signal, metric, active?.org, project, rangeMs, prefix, zoomKey(zoom)],
    enabled: Boolean(active) && enabled && (signal !== 'metrics' || metric !== ''),
    staleTime: STALE_MS,
    queryFn: async () => (await json<{ attributes: SeenKey[] }>(await get(`/api/orgs/${active?.org}/${pathOf(signal, metric)}?${window(rangeMs, project, prefix, zoom)}`))).attributes,
  })
  return { keys: data ?? [], loading: isFetching }
}

export function useAttributeValues(key: string | undefined, rangeMs: number, prefix: string): { values: SeenValue[]; loading: boolean } {
  const signal = useSignal()
  const metric = useMetric()
  const { active } = useOrganisations()
  const { project } = useScope()
  const zoom = useZoom()
  const { data, isFetching } = useQuery({
    queryKey: ['attribute-values', signal, metric, active?.org, project, rangeMs, key, prefix, zoomKey(zoom)],
    enabled: Boolean(active && key) && (signal !== 'metrics' || metric !== ''),
    staleTime: STALE_MS,
    queryFn: async () =>
      (await json<{ values: SeenValue[] }>(await get(`/api/orgs/${active?.org}/${pathOf(signal, metric)}/${encodeURIComponent(key!)}/values?${window(rangeMs, project, prefix, zoom)}`))).values,
  })
  return { values: data ?? [], loading: isFetching }
}
