import { queryOptions } from '@tanstack/react-query'
import { get, json, patch } from './client'
import { leave } from './oidc'
import { forgetStoredData } from '@/lib/offline'

export type Role = 'owner' | 'admin' | 'member'
export type Membership = { org: string; name: string; role: Role }
export type Account = { account: string; email: string; name: string | null; photo: string | null; active_org: string | null; memberships: Membership[] }
export type Methods = { oidc: boolean; issuer: string | null; client_id: string | null }

export const accountQuery = queryOptions({
  queryKey: ['auth', 'me'],
  queryFn: async () => json<Account>(await get('/auth/me')),
  retry: false,
  staleTime: 30_000,
})

export const methodsQuery = queryOptions({
  queryKey: ['auth', 'methods'],
  queryFn: async () => {
    try {
      return await json<Methods>(await get('/auth/methods'))
    } catch {
      return { oidc: false, issuer: null, client_id: null }
    }
  },
  retry: false,
  staleTime: Infinity,
})

export const healthQuery = queryOptions({
  queryKey: ['health'],
  queryFn: async () => (await (await get('/health')).text()).trim(),
  retry: false,
  refetchInterval: 30_000,
})

export const signOut = async () => {
  forgetStoredData()
  await leave()
}

export const renameMe = (name: string) => patch('/auth/me', { name })

export const roleRank: Record<Role, number> = { owner: 0, admin: 1, member: 2 }
