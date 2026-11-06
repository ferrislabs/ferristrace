import { useQuery } from '@tanstack/react-query'
import { get, json } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { useScope } from '@/lib/scope'

export type SeenKey = { key: string; spans: number; numeric: boolean }
export type SeenValue = { value: string; spans: number }

const STALE_MS = 30_000

function window(rangeMs: number, project: string | undefined, prefix: string): string {
  const to = Date.now()
  const query = new URLSearchParams({ from: String(to - rangeMs), to: String(to) })
  if (project) query.set('project', project)
  if (prefix.trim()) query.set('prefix', prefix.trim())
  return query.toString()
}

export function useAttributeKeys(rangeMs: number, prefix: string, enabled: boolean): { keys: SeenKey[]; loading: boolean } {
  const { active } = useOrganisations()
  const { project } = useScope()
  const { data, isFetching } = useQuery({
    queryKey: ['attributes', active?.org, project, rangeMs, prefix],
    enabled: Boolean(active) && enabled,
    staleTime: STALE_MS,
    queryFn: async () => (await json<{ attributes: SeenKey[] }>(await get(`/api/orgs/${active?.org}/attributes?${window(rangeMs, project, prefix)}`))).attributes,
  })
  return { keys: data ?? [], loading: isFetching }
}

export function useAttributeValues(key: string | undefined, rangeMs: number, prefix: string): { values: SeenValue[]; loading: boolean } {
  const { active } = useOrganisations()
  const { project } = useScope()
  const { data, isFetching } = useQuery({
    queryKey: ['attribute-values', active?.org, project, rangeMs, key, prefix],
    enabled: Boolean(active && key),
    staleTime: STALE_MS,
    queryFn: async () =>
      (await json<{ values: SeenValue[] }>(await get(`/api/orgs/${active?.org}/attributes/${encodeURIComponent(key!)}/values?${window(rangeMs, project, prefix)}`))).values,
  })
  return { values: data ?? [], loading: isFetching }
}
