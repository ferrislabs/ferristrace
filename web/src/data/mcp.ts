import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { del, get, json, put } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import type { Agent, AgentScope } from './agents'
import { nothingYet } from './orgs'

export type McpState = {
  configured: boolean
  agents_enabled: boolean
  agents_off_reason: string | null
  gateway_client_id: string | null
  url: string | null
  enabled: boolean
  agent: Agent | null
}

export type McpChoice = { scopes: AgentScope[]; project: string | null }

const key = (org: string | undefined) => ['org', org, 'mcp']

export function useMcp(enabled: boolean) {
  const found = useOrganisations()
  const { active } = found
  const { data, isPending, error } = useQuery({
    queryKey: key(active?.org),
    enabled: Boolean(active) && enabled,
    staleTime: 15_000,
    queryFn: async () => json<McpState>(await get(`/api/orgs/${active?.org}/mcp`)),
  })
  return { state: data, nothing: nothingYet(found, isPending, error, 'MCP settings') }
}

function useRefreshing<T>(send: (org: string, asked: T) => Promise<unknown>) {
  const { active } = useOrganisations()
  const client = useQueryClient()
  return useMutation({
    mutationFn: async (asked: T) => send(active?.org ?? '', asked),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: key(active?.org) })
      void client.invalidateQueries({ queryKey: ['org', active?.org, 'agents'] })
      void client.invalidateQueries({ queryKey: ['org', active?.org, 'audit'] })
    },
  })
}

export const useTurnMcpOn = () => useRefreshing<McpChoice>(async (org, asked) => json(await put(`/api/orgs/${org}/mcp`, asked)))

export const useTurnMcpOff = () =>
  useRefreshing<void>(async (org) => {
    await del(`/api/orgs/${org}/mcp`)
  })
