import { useInfiniteQuery } from '@tanstack/react-query'
import { get, json } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { nothingYet } from './orgs'

export type ActorKind = 'account' | 'api_key' | 'agent' | 'operator'

export type AuditEntry = {
  id: string
  at_ms: number
  actor: { kind: ActorKind; id: string | null; label: string | null }
  action: string
  target: { kind: string; id: string | null }
  detail: Record<string, string | null>
}

export type AuditPage = { entries: AuditEntry[]; next: string | null }

export type AuditFilter = { action: string; actor: string }

export const AUDIT_ACTIONS = [
  'org.founded',
  'org.renamed',
  'org.logo_set',
  'org.logo_removed',
  'org.retention_set',
  'org.exported',
  'project.created',
  'project.renamed',
  'project.deleted',
  'project.assigned',
  'team.created',
  'team.renamed',
  'team.disbanded',
  'team.seated',
  'team.projects_owned',
  'member.invited',
  'member.role_granted',
  'member.removed',
  'key.issued',
  'key.revoked',
  'issue.decided',
  'issue.assigned',
  'account.created_by_operator',
  'reset_link.issued_by_operator',
  'alert.created',
  'alert.updated',
  'alert.deleted',
  'alert.enabled',
  'alert.disabled',
  'channel.created',
  'channel.updated',
  'channel.deleted',
  'incident.opened',
  'incident.updated',
  'incident.commented',
  'incident.attached',
  'incident.resolved',
  'incident.reopened',
  'agent.registered',
  'agent.revoked',
  'agent.read',
] as const

export const OPERATOR = 'operator'

const PAGE = 50

function query(filter: AuditFilter, before: string | null) {
  const asked = new URLSearchParams({ limit: String(PAGE) })
  if (filter.action) asked.set('action', filter.action)
  if (filter.actor === OPERATOR) asked.set('actor_kind', OPERATOR)
  else if (filter.actor) {
    asked.set('actor_kind', 'account')
    asked.set('actor_id', filter.actor)
  }
  if (before) asked.set('before', before)
  return asked.toString()
}

export function useAuditLog(filter: AuditFilter, enabled: boolean) {
  const found = useOrganisations()
  const { active } = found
  const { data, isPending, error, fetchNextPage, hasNextPage, isFetchingNextPage } = useInfiniteQuery({
    queryKey: ['org', active?.org, 'audit', filter],
    enabled: Boolean(active) && enabled,
    staleTime: 15_000,
    initialPageParam: null as string | null,
    queryFn: async ({ pageParam }) => json<AuditPage>(await get(`/api/orgs/${active?.org}/audit?${query(filter, pageParam)}`)),
    getNextPageParam: (last) => last.next,
  })
  return {
    entries: data?.pages.flatMap((page) => page.entries),
    nothing: nothingYet(found, isPending, error, 'audit log'),
    more: hasNextPage ? () => void fetchNextPage() : null,
    loadingMore: isFetchingNextPage,
  }
}

export function actorName(entry: AuditEntry) {
  if (entry.actor.kind === 'operator') return 'Operator (command line)'
  return entry.actor.label ?? entry.actor.id ?? 'unknown'
}
