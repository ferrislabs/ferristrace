import { keepPreviousData, useQuery } from '@tanstack/react-query'
import { json, post } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { useScope } from '@/lib/scope'
import { bodyOf, type Node } from '@/lib/queryLang'
import type { FieldRef } from '@/lib/traceQuery'

export type FacetValue = { value: string; spans: number }

export type Spread = { minUs: number; maxUs: number }

type Wire = { duration: { min_us: number; max_us: number } | null }

type WireFacet = ({ field: string } | { attr: string }) & { values: FacetValue[] }

const fieldOf = (wire: WireFacet): FieldRef =>
  'attr' in wire ? { attr: wire.attr } : { built: wire.field as never }

const STALE_MS = 15_000

const key = (field: FieldRef) => ('built' in field ? `built:${field.built}` : `attr:${field.attr}`)

export const facetKey = key

export function useFacets(
  items: Node[],
  rangeMs: number,
  wanted: FieldRef[],
  duration: boolean,
): { facets: Map<string, FacetValue[]>; spread: Spread | null; loading: boolean; failed: unknown } {
  const { active } = useOrganisations()
  const { project } = useScope()
  const asked = wanted.map(key).join('|')
  const { data, isFetching, error } = useQuery({
    queryKey: ['facets', active?.org, project, rangeMs, items, asked, duration],
    enabled: Boolean(active) && (wanted.length > 0 || duration),
    staleTime: STALE_MS,
    placeholderData: keepPreviousData,
    queryFn: async () => {
      const to = Date.now()
      const body: Record<string, unknown> = {
        from: to - rangeMs,
        to,
        facets: wanted.map((field) => ('built' in field ? { field: field.built } : { attr: field.attr })),
        duration,
      }
      if (project) body.project = project
      const query = bodyOf(items)
      if (query !== undefined) body.query = query
      const found = await json<{ facets: WireFacet[]; duration: Wire['duration'] }>(await post(`/api/orgs/${active?.org}/traces/facets`, body))
      return {
        facets: new Map(found.facets.map((facet) => [key(fieldOf(facet)), facet.values])),
        spread: found.duration ? { minUs: found.duration.min_us, maxUs: found.duration.max_us } : null,
      }
    },
  })
  return { facets: data?.facets ?? new Map(), spread: data?.spread ?? null, loading: isFetching, failed: error }
}
