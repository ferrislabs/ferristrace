import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { accountQuery } from '@/auth/api'
import { because, del, get, json, offer, patch } from '@/auth/client'
import { useOrganisations } from '@/auth/session'

export type OrgCard = { id: string; name: string; plan: string; created_at_ms: number; version: string; logo?: string | null }

const key = (org: string | undefined) => ['org', org]

export function nothingYet(
  found: { active: unknown; asking: boolean },
  isPending: boolean,
  error: unknown,
  what: string,
) {
  if (found.asking) return 'Asking the server who you are…'
  if (!found.active) {
    return 'This session belongs to no organisation. An operator grants one with `ferristrace grant-role`.'
  }
  if (isPending) return `Reading the ${what}…`
  return because(error, `The ${what} could not be read.`)
}

export function useOrgCard() {
  const found = useOrganisations()
  const { active } = found
  const { data, isPending, error } = useQuery({
    queryKey: key(active?.org),
    enabled: Boolean(active),
    staleTime: 60_000,
    queryFn: async () => json<OrgCard>(await get(`/api/orgs/${active?.org}`)),
  })
  return { card: data, nothing: nothingYet(found, isPending, error, 'organisation') }
}

export function useRenameOrg() {
  const { active } = useOrganisations()
  const client = useQueryClient()
  return useMutation({
    mutationFn: async (name: string) => json<OrgCard>(await patch(`/api/orgs/${active?.org}`, { name })),
    onSuccess: (card) => {
      client.setQueryData(key(card.id), card)
      void client.invalidateQueries({ queryKey: accountQuery.queryKey })
    },
  })
}

export const MAX_LOGO_BYTES = 1024 * 1024

export function logoProblem(file: File): string | null {
  if (file.size > MAX_LOGO_BYTES) return 'a logo can be at most 1 MB'
  return null
}

export function usePutOrgLogo() {
  const { active } = useOrganisations()
  const client = useQueryClient()
  return useMutation({
    mutationFn: async (file: File) => json<OrgCard>(await offer(`/api/orgs/${active?.org}/logo`, file)),
    onSuccess: (card) => client.setQueryData(key(card.id), card),
  })
}

export function useRemoveOrgLogo() {
  const { active } = useOrganisations()
  const client = useQueryClient()
  return useMutation({
    mutationFn: async () => json<OrgCard>(await del(`/api/orgs/${active?.org}/logo`)),
    onSuccess: (card) => client.setQueryData(key(card.id), card),
  })
}

export const ORG_NAME_MAX_BYTES = 128

export function orgNameProblem(name: string): string | null {
  const kept = name.trim()
  if (new TextEncoder().encode(kept).length > ORG_NAME_MAX_BYTES) return `a name can be at most ${ORG_NAME_MAX_BYTES} bytes`
  if (kept.length === 0) return 'an organisation needs a name'
  return null
}
