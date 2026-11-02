import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { del, get, json, post } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { nothingYet } from './orgs'

export type KeyScope = 'ingest' | 'read'

export const KEY_SCOPES: KeyScope[] = ['ingest', 'read']

export type ApiKey = {
  id: string
  name: string
  prefix: string
  project: string | null
  scopes: KeyScope[]
  created_at_ms: number
  created_by: string | null
  last_used_at_ms: number | null
}

export type IssuedKey = { key: ApiKey; secret: string }

const key = (org: string | undefined) => ['org', org, 'keys']

export function useKeys() {
  const found = useOrganisations()
  const { active } = found
  const { data, isPending, error } = useQuery({
    queryKey: key(active?.org),
    enabled: Boolean(active),
    staleTime: 30_000,
    queryFn: async () => (await json<{ keys: ApiKey[] }>(await get(`/api/orgs/${active?.org}/keys`))).keys,
  })
  return { keys: data, nothing: nothingYet(found, isPending, error, 'api keys') }
}

function useRefreshing<T, R>(send: (org: string, asked: T) => Promise<R>) {
  const { active } = useOrganisations()
  const client = useQueryClient()
  return useMutation({
    mutationFn: async (asked: T) => send(active?.org ?? '', asked),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: key(active?.org) })
      void client.invalidateQueries({ queryKey: ['projects', active?.org] })
    },
  })
}

export const useIssueKey = () =>
  useRefreshing<{ name: string; scopes: KeyScope[]; project: string | null }, IssuedKey>(async (org, asked) =>
    json(await post(`/api/orgs/${org}/keys`, asked)),
  )

export const useRevokeKey = () =>
  useRefreshing<string, void>(async (org, id) => {
    await del(`/api/orgs/${org}/keys/${id}`)
  })

export const KEY_NAME_MAX_BYTES = 64

export function keyNameProblem(name: string): string | null {
  const kept = name.trim()
  if (new TextEncoder().encode(kept).length > KEY_NAME_MAX_BYTES) {
    return `a key name can be at most ${KEY_NAME_MAX_BYTES} bytes`
  }
  if (kept.length === 0) return 'a key needs a name'
  return null
}
