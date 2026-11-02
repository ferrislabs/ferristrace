import { useQuery } from '@tanstack/react-query'
import { useLocation, useNavigate } from '@tanstack/react-router'
import { get, json } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { useScope } from '@/lib/scope'

export const PLACES = ['cluster', 'namespace', 'workload', 'pod', 'node'] as const

export type Place = (typeof PLACES)[number]

export type Placement = Record<Place, string>

export const NOWHERE: Placement = { cluster: '', namespace: '', workload: '', pod: '', node: '' }

export type SeenWorkload = { kind: string; name: string; namespace: string }

export type SeenPod = { name: string; namespace: string; node: string }

export type Placements = {
  clusters: string[]
  namespaces: string[]
  workloads: SeenWorkload[]
  pods: SeenPod[]
  nodes: string[]
}

const NONE_SEEN: Placements = { clusters: [], namespaces: [], workloads: [], pods: [], nodes: [] }

const FRESH_MS = 60_000

export const WORKLOAD_KEYS: [string, string][] = [
  ['deployment', 'k8s.deployment.name'],
  ['statefulset', 'k8s.statefulset.name'],
  ['daemonset', 'k8s.daemonset.name'],
  ['cronjob', 'k8s.cronjob.name'],
  ['job', 'k8s.job.name'],
]

export const isPlaced = (placement: Placement) => PLACES.some((place) => placement[place].trim() !== '')

export function placed(params: URLSearchParams, placement: Placement): URLSearchParams {
  for (const place of PLACES) {
    const value = placement[place].trim()
    if (value) params.set(place, value)
  }
  return params
}

function spelled(value: unknown): string {
  return typeof value === 'string' || typeof value === 'number' ? String(value) : ''
}

export function usePlacement(): { placement: Placement; place: (part: Partial<Placement>) => void } {
  const search = useLocation().search as Record<string, unknown>
  const navigate = useNavigate()
  const placement: Placement = {
    cluster: spelled(search.cluster),
    namespace: spelled(search.namespace),
    workload: spelled(search.workload),
    pod: spelled(search.pod),
    node: spelled(search.node),
  }
  const place = (part: Partial<Placement>) => {
    void navigate({
      to: '.' as never,
      replace: true,
      search: ((was: Record<string, unknown>) => {
        const next: Record<string, unknown> = { ...was }
        for (const key of PLACES) {
          if (!(key in part)) continue
          const value = (part[key] ?? '').trim()
          if (value) next[key] = value
          else delete next[key]
        }
        return next
      }) as never,
    })
  }
  return { placement, place }
}

export function placementSearch(part: Partial<Placement>): Record<string, string> {
  const out: Record<string, string> = {}
  for (const key of PLACES) {
    const value = (part[key] ?? '').trim()
    if (value) out[key] = value
  }
  return out
}

export type Chip = { place: Place; value: string; label: string }

export function chipsOf(attributes: Record<string, string>): Chip[] {
  const chips: Chip[] = []
  const owner = WORKLOAD_KEYS.find(([, key]) => attributes[key])
  if (owner) chips.push({ place: 'workload', value: `${owner[0]}:${attributes[owner[1]]}`, label: `${owner[0]}:${attributes[owner[1]]}` })
  if (attributes['k8s.pod.name']) chips.push({ place: 'pod', value: attributes['k8s.pod.name'], label: `pod:${attributes['k8s.pod.name']}` })
  if (attributes['k8s.node.name']) chips.push({ place: 'node', value: attributes['k8s.node.name'], label: `node:${attributes['k8s.node.name']}` })
  return chips
}

export function usePlacements(windowMs: number): { seen: Placements; failed: unknown } {
  const { active } = useOrganisations()
  const { project } = useScope()
  const { data, error } = useQuery({
    queryKey: ['placements', active?.org, project, windowMs],
    enabled: Boolean(active),
    staleTime: FRESH_MS,
    queryFn: async () => {
      const to = Date.now()
      const params = new URLSearchParams({ from: String(to - windowMs), to: String(to) })
      if (project) params.set('project', project)
      return json<Placements>(await get(`/api/orgs/${active?.org}/placements?${params.toString()}`))
    },
  })
  return { seen: data ?? NONE_SEEN, failed: error }
}
