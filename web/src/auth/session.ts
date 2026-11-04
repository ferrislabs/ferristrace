import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { accountQuery, roleRank, signOut, type Membership } from './api'

export function useAccount() {
  return useQuery(accountQuery).data
}

export function useOrganisations(): {
  list: Membership[]
  active: Membership | undefined
  asking: boolean
} {
  const { data: account, isPending } = useQuery(accountQuery)
  const list = [...(account?.memberships ?? [])].sort(
    (a, b) => roleRank[a.role] - roleRank[b.role] || a.name.localeCompare(b.name),
  )
  const active = list.find((m) => m.org === account?.active_org) ?? list[0]
  return { list, active, asking: isPending }
}

export function useSignOut() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: signOut,
    onSettled: () => client.clear(),
  })
}
