import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { accountQuery } from '@/auth/api'
import { because, del, get, json, offer, patch, post, put } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { useNavigate } from '@/lib/router'
import { useScope } from '@/lib/scope'

export type OrgCard = { id: string; name: string; plan: string; created_at_ms: number; version: string; logo?: string | null; retention_days: number | null; retention_cap_days: number | null; default_retention_days: number; timezone: string }

const key = (org: string | undefined) => ['org', org]

export function nothingYet(
  found: { active: unknown; asking: boolean },
  isPending: boolean,
  error: unknown,
  what: string,
) {
  if (found.asking) return 'Asking the server who you are…'
  if (!found.active) {
    return 'This session belongs to no organisation. Create one, or ask an operator to grant you one with `ferristrace grant-role`.'
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

export type FoundedOrg = { id: string; name: string; project: string }

function useEnterOrg() {
  const client = useQueryClient()
  const navigate = useNavigate()
  const { chooseOrg } = useScope()
  return async (org: string, to: string) => {
    chooseOrg(org)
    await client.resetQueries()
    navigate(to)
  }
}

export function useSwitchOrg() {
  const enter = useEnterOrg()
  return useMutation({
    mutationFn: async (org: string) => {
      await put('/auth/session/org', { org })
      return org
    },
    onSuccess: (org) => enter(org, '/'),
  })
}

export function useCreateOrg() {
  const enter = useEnterOrg()
  return useMutation({
    mutationFn: async (name: string) => json<FoundedOrg>(await post('/api/orgs', { name: name.trim() })),
    onSuccess: (founded) => enter(founded.id, '/onboarding'),
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

export const RETENTION_MIN_DAYS = 1
export const RETENTION_MAX_DAYS = 3650

export function effectiveRetention(card: Pick<OrgCard, 'retention_days' | 'retention_cap_days' | 'default_retention_days'>): number {
  const chosen = card.retention_days ?? card.default_retention_days
  return card.retention_cap_days === null ? chosen : Math.min(chosen, card.retention_cap_days)
}

export function retentionProblem(typed: string, cap: number | null): string | null {
  const days = Number(typed)
  if (typed.trim() === '' || !Number.isInteger(days)) return 'a retention is a whole number of days'
  if (days < RETENTION_MIN_DAYS || days > RETENTION_MAX_DAYS) return `a retention is between ${RETENTION_MIN_DAYS} and ${RETENTION_MAX_DAYS} days`
  if (cap !== null && days > cap) return `the plan keeps spans at most ${cap} days`
  return null
}

export function useSetOrgRetention() {
  const { active } = useOrganisations()
  const client = useQueryClient()
  return useMutation({
    mutationFn: async (days: number | null) => json<OrgCard>(await patch(`/api/orgs/${active?.org}/retention`, { days })),
    onSuccess: (card) => client.setQueryData(key(card.id), card),
  })
}

export function timeZoneNames(): string[] {
  const listed = typeof Intl.supportedValuesOf === 'function' ? Intl.supportedValuesOf('timeZone') : []
  return listed.includes('UTC') ? listed : ['UTC', ...listed]
}

export function useSetOrgTimezone() {
  const { active } = useOrganisations()
  const client = useQueryClient()
  return useMutation({
    mutationFn: async (timezone: string) => json<OrgCard>(await patch(`/api/orgs/${active?.org}/timezone`, { timezone })),
    onSuccess: (card) => client.setQueryData(key(card.id), card),
  })
}
