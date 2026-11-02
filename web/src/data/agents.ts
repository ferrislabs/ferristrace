import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { del, get, json, post } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { nothingYet } from './orgs'

export type AgentScope = 'telemetry:read' | 'incidents:comment' | 'incidents:resolve' | 'oncall:acknowledge'

export const AGENT_SCOPES: AgentScope[] = ['telemetry:read', 'incidents:comment', 'incidents:resolve', 'oncall:acknowledge']

export type Agent = {
  id: string
  name: string
  client_id: string
  scopes: AgentScope[]
  project: string | null
  created_at_ms: number
  created_by: string | null
  last_used_at_ms: number | null
  revoked_at_ms: number | null
}

export type AgentRoll = { enabled: boolean; reason: string | null; agents: Agent[] }

export type Enrolment = { name: string; client_id: string; scopes: AgentScope[]; project: string | null }

const key = (org: string | undefined) => ['org', org, 'agents']

export function useAgents(enabled: boolean) {
  const found = useOrganisations()
  const { active } = found
  const { data, isPending, error } = useQuery({
    queryKey: key(active?.org),
    enabled: Boolean(active) && enabled,
    staleTime: 15_000,
    queryFn: async () => json<AgentRoll>(await get(`/api/orgs/${active?.org}/agents`)),
  })
  return { roll: data, nothing: nothingYet(found, isPending, error, 'agents') }
}

function useRefreshing<T, R>(send: (org: string, asked: T) => Promise<R>) {
  const { active } = useOrganisations()
  const client = useQueryClient()
  return useMutation({
    mutationFn: async (asked: T) => send(active?.org ?? '', asked),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: key(active?.org) })
      void client.invalidateQueries({ queryKey: ['org', active?.org, 'audit'] })
    },
  })
}

export const useRegisterAgent = () =>
  useRefreshing<Enrolment, Agent>(async (org, asked) => json(await post(`/api/orgs/${org}/agents`, asked)))

export const useRevokeAgent = () =>
  useRefreshing<string, void>(async (org, id) => {
    await del(`/api/orgs/${org}/agents/${id}`)
  })

export const AGENT_NAME_MAX_BYTES = 64
export const CLIENT_ID_MAX_BYTES = 128

export function agentNameProblem(name: string): string | null {
  const kept = name.trim()
  if (kept.length === 0) return 'an agent needs a name'
  if (new TextEncoder().encode(kept).length > AGENT_NAME_MAX_BYTES) {
    return `an agent name can be at most ${AGENT_NAME_MAX_BYTES} bytes`
  }
  return null
}

export function clientIdProblem(clientId: string): string | null {
  const kept = clientId.trim()
  if (kept.length === 0) return 'an agent needs the client id FerrisKey knows it by'
  if (new TextEncoder().encode(kept).length > CLIENT_ID_MAX_BYTES) {
    return `a client id can be at most ${CLIENT_ID_MAX_BYTES} bytes`
  }
  if (/\s/.test(kept)) return 'a client id holds no whitespace'
  return null
}
