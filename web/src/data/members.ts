import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { roleRank, type Role } from '@/auth/api'
import { del, get, json, patch, post } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { nothingYet } from './orgs'

export type Member = { account: string; email: string; name: string | null; photo: string | null; role: Role; joined_at_ms: number }

export type Welcomed = { member: Member }

const key = (org: string | undefined) => ['org', org, 'members']

export function useMembers() {
  const found = useOrganisations()
  const { active } = found
  const { data, isPending, error } = useQuery({
    queryKey: key(active?.org),
    enabled: Boolean(active),
    staleTime: 30_000,
    queryFn: async () => (await json<{ members: Member[] }>(await get(`/api/orgs/${active?.org}/members`))).members,
  })
  return { members: data, nothing: nothingYet(found, isPending, error, 'members') }
}

export function useGrantRole() {
  const { active } = useOrganisations()
  const client = useQueryClient()
  return useMutation({
    mutationFn: async (asked: { account: string; role: Role }) =>
      json<Member>(await patch(`/api/orgs/${active?.org}/members/${asked.account}`, { role: asked.role })),
    onSuccess: (changed) =>
      client.setQueryData<Member[]>(key(active?.org), (roll) =>
        roll ? sorted(roll.map((one) => (one.account === changed.account ? changed : one))) : undefined,
      ),
  })
}

export function useRemoveMember() {
  const { active } = useOrganisations()
  const client = useQueryClient()
  return useMutation({
    mutationFn: async (account: string) => {
      await del(`/api/orgs/${active?.org}/members/${account}`)
      return account
    },
    onSuccess: (account) =>
      client.setQueryData<Member[]>(key(active?.org), (roll) => roll?.filter((one) => one.account !== account)),
  })
}

export function useInvite() {
  const { active } = useOrganisations()
  const client = useQueryClient()
  return useMutation({
    mutationFn: async (asked: { email: string; role: Role }) =>
      json<Welcomed>(await post(`/api/orgs/${active?.org}/members`, asked)),
    onSuccess: (welcomed) =>
      client.setQueryData<Member[]>(key(active?.org), (roll) => (roll ? sorted([...roll, welcomed.member]) : undefined)),
  })
}

export const sorted = (roll: Member[]) =>
  [...roll].sort((a, b) => roleRank[a.role] - roleRank[b.role] || a.email.localeCompare(b.email))

export const mayChange = (mine: Role | undefined, theirs: Role) =>
  mine === 'owner' || (mine === 'admin' && theirs !== 'owner')

export const theLastOwner = (roll: Member[], member: Member) =>
  member.role === 'owner' && roll.filter((one) => one.role === 'owner').length <= 1

export const mayHandOut = (mine: Role | undefined): Role[] =>
  mine === 'owner' ? ['owner', 'admin', 'member'] : ['admin', 'member']
