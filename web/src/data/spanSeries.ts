import { useQuery } from '@tanstack/react-query'
import { json, post } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { bodyOf, type Node } from '@/lib/queryLang'
import { boundsOf, useZoom, zoomKey } from '@/lib/bounds'
import { useScope } from '@/lib/scope'
import { MEASURES, type Measure } from '@/data/views'
import type { MetricLine } from '@/data/metrics'

type Wire = { step_ms: number; series: { group: string | null; points: [number, number | null][] }[] }

const FRESH_MS = 15_000

const NS_PER_MS = 1_000_000

export const groupBody = (group: string): Record<string, string> | undefined => {
  if (group === '') return undefined
  return group.startsWith('attr:') ? { attr: group.slice(5) } : { field: group }
}

export const groupLabel = (group: string): string => (group.startsWith('attr:') ? group.slice(5) : group)

export function useSpanSeries(rangeMs: number, measure: Measure, group: string, items: Node[], enabled: boolean): { lines: MetricLine[]; unit: string; loading: boolean; failed: unknown } {
  const { active } = useOrganisations()
  const { project } = useScope()
  const zoom = useZoom()
  const unit = MEASURES.find((one) => one.value === measure)?.unit ?? ''
  const { data, isFetching, error } = useQuery({
    queryKey: ['span-series', active?.org, project, rangeMs, measure, group, items, zoomKey(zoom)],
    enabled: Boolean(active) && enabled,
    staleTime: FRESH_MS,
    queryFn: async () => {
      const bounds = boundsOf(rangeMs, zoom)
      const body: Record<string, unknown> = { from: bounds.from, to: bounds.to, measure }
      if (project) body.project = project
      const grouped = groupBody(group)
      if (grouped) body.group_by = grouped
      const query = bodyOf(items)
      if (query !== undefined) body.query = query
      const found = await json<Wire>(await post(`/api/orgs/${active?.org}/spans/series`, body))
      const scale = unit === 'ms' ? 1 / 1000 : 1
      return found.series.map<MetricLine>((line) => ({
        labels: line.group === null ? {} : { [groupLabel(group)]: line.group },
        points: line.points.map(([ms, value]) => [ms * NS_PER_MS, value === null ? null : value * scale]),
      }))
    },
  })
  return { lines: data ?? [], unit, loading: isFetching, failed: error }
}
