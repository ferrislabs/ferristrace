import { useQuery } from '@tanstack/react-query'
import { json, post } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { dur } from '@/lib/format'
import { bodyOf, type Node } from '@/lib/queryLang'
import { useScope } from '@/lib/scope'
import type { Order } from '@/data/views'

export type SpanRow = {
  traceId: string
  spanId: string
  service: string
  name: string
  kind: string
  status: string
  code: number | null
  start: string
  startedAtMs: number
  dur: string
}

type Wire = {
  trace_id: string
  span_id: string
  service: string
  name: string
  kind: string
  status: string
  status_code: number | null
  started_at_ms: number
  duration_us: number
}

const FRESH_MS = 15_000

const clock = (ms: number) => new Date(ms).toTimeString().slice(0, 8) + `.${String(ms % 1000).padStart(3, '0')}`

const row = (wire: Wire): SpanRow => ({
  traceId: wire.trace_id,
  spanId: wire.span_id,
  service: wire.service,
  name: wire.name,
  kind: wire.kind,
  status: wire.status,
  code: wire.status_code,
  start: clock(wire.started_at_ms),
  startedAtMs: wire.started_at_ms,
  dur: dur(wire.duration_us / 1000),
})

export function useSpans(rangeMs: number, order: Order, limit: number, items: Node[], enabled: boolean): { rows: SpanRow[]; loading: boolean; failed: unknown } {
  const { active } = useOrganisations()
  const { project } = useScope()
  const { data, isFetching, error } = useQuery({
    queryKey: ['spans', active?.org, project, rangeMs, order, limit, items],
    enabled: Boolean(active) && enabled,
    staleTime: FRESH_MS,
    queryFn: async () => {
      const to = Date.now()
      const body: Record<string, unknown> = { from: to - rangeMs, to, limit, order: order === 'duration' ? 'duration' : 'started' }
      if (project) body.project = project
      const query = bodyOf(items)
      if (query !== undefined) body.query = query
      const found = await json<{ spans: Wire[] }>(await post(`/api/orgs/${active?.org}/spans/search`, body))
      return found.spans.map(row)
    },
  })
  return { rows: data ?? [], loading: isFetching, failed: error }
}
