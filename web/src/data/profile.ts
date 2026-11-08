import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { accountQuery, renameMe, type Account } from '@/auth/api'
import { del, get, json, offer, put } from '@/auth/client'
import { useAccount, useOrganisations } from '@/auth/session'
import { useMembers } from './members'
import { useTeams } from './teams'

export const nameOf = (who: { email: string; name?: string | null }) => who.name ?? who.email

export function initialsOf(spelling: string) {
  const letters = spelling
    .split('@')[0]
    .split(/[.\-_+\s]/)
    .filter(Boolean)
    .map((word) => word[0])
    .join('')
  return (letters || spelling.slice(0, 2)).slice(0, 2).toUpperCase()
}

export const ACCOUNT_NAME_MAX_BYTES = 128

export function accountNameProblem(name: string): string | null {
  if (new TextEncoder().encode(name.trim()).length > ACCOUNT_NAME_MAX_BYTES) {
    return `a name can be at most ${ACCOUNT_NAME_MAX_BYTES} bytes`
  }
  return null
}

export function useRenameMe() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: async (name: string) => json<Account>(await renameMe(name)),
    onSuccess: (standing) => client.setQueryData(accountQuery.queryKey, standing),
  })
}

export const MAX_PHOTO_BYTES = 1024 * 1024

export function photoProblem(file: File): string | null {
  if (file.size > MAX_PHOTO_BYTES) return 'a photo can be at most 1 MB'
  return null
}

export function usePutPhoto() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: async (file: File) => json<Account>(await offer('/auth/me/photo', file)),
    onSuccess: (standing) => client.setQueryData(accountQuery.queryKey, standing),
  })
}

export function useRemovePhoto() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: async () => json<Account>(await del('/auth/me/photo')),
    onSuccess: (standing) => client.setQueryData(accountQuery.queryKey, standing),
  })
}

export type TimestampPrecision = 'ms' | 'us' | 'ns'

export type RowDensity = 'compact' | 'comfortable'

export type StartPage = 'overview' | 'services' | 'traces'

export type AccountPreferences = {
  timezone: string
  precision: TimestampPrecision
  density: RowDensity
  start_page: StartPage
}

const preferencesKey = ['auth', 'preferences']

export function usePreferences() {
  return useQuery({
    queryKey: preferencesKey,
    staleTime: 60_000,
    queryFn: async () => json<AccountPreferences>(await get('/auth/me/preferences')),
  })
}

export function useSetPreferences() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: async (asked: AccountPreferences) => json<AccountPreferences>(await put('/auth/me/preferences', asked)),
    onSuccess: (preferences) => client.setQueryData(preferencesKey, preferences),
  })
}

export function useMyStanding() {
  const account = useAccount()
  const { active } = useOrganisations()
  const { members, nothing: noRoll } = useMembers()
  const { teams, nothing: noTeams } = useTeams()
  return {
    active,
    mine: members?.find((one) => one.account === account?.account),
    noRoll,
    teams: teams?.filter((team) => team.members.includes(account?.account ?? '')),
    noTeams,
  }
}
