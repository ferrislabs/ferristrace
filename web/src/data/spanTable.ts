import { useQuery } from '@tanstack/react-query'
import { json, post } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { bodyOf, type Node } from '@/lib/queryLang'
import { boundsOf, useZoom, zoomKey } from '@/lib/bounds'
import { useScope } from '@/lib/scope'
import { groupBody } from '@/data/spanSeries'
import type { Measure } from '@/data/views'

export type TableRow = {
  group: string
  spans: number
  errors: number
  errorRate: number
  averageUs: number
  p50Us: number
  p95Us: number
  p99Us: number
}

type Wire = {
  group: string
  spans: number
  errors: number
  error_rate: number
  average_us: number
  p50_us: number
  p95_us: number
  p99_us: number
}

const FRESH_MS = 15_000

export function useSpanTable(rangeMs: number, order: Measure, group: string, items: Node[], enabled: boolean): { rows: TableRow[]; loading: boolean; failed: unknown } {
  const { active } = useOrganisations()
  const { project } = useScope()
  const zoom = useZoom()
  const { data, isFetching, error } = useQuery({
    queryKey: ['span-table', active?.org, project, rangeMs, order, group, items, zoomKey(zoom)],
    enabled: Boolean(active) && enabled,
    staleTime: FRESH_MS,
    queryFn: async () => {
      const bounds = boundsOf(rangeMs, zoom)
      const body: Record<string, unknown> = { from: bounds.from, to: bounds.to, order, group_by: groupBody(group), limit: 50 }
      if (project) body.project = project
      const query = bodyOf(items)
      if (query !== undefined) body.query = query
      const found = await json<{ rows: Wire[] }>(await post(`/api/orgs/${active?.org}/spans/table`, body))
      return found.rows.map<TableRow>((row) => ({
        group: row.group,
        spans: row.spans,
        errors: row.errors,
        errorRate: row.error_rate,
        averageUs: row.average_us,
        p50Us: row.p50_us,
        p95Us: row.p95_us,
        p99Us: row.p99_us,
      }))
    },
  })
  return { rows: data ?? [], loading: isFetching, failed: error }
}
