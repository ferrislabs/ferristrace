import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { get, json, post, put } from '@/auth/client'
import { useOrganisations } from '@/auth/session'

export type NotificationKind = 'issue' | 'service_silent' | 'status_page' | 'api_key_expiring' | 'usage' | 'membership'

export type Notification = {
  id: string
  kind: NotificationKind
  title: string
  body: string
  link: string | null
  created_ms: number
  read_ms: number | null
}

const FRESH_MS = 30_000

const key = (org: string | undefined) => ['notifications', org]

export function useNotifications() {
  const { active } = useOrganisations()
  const { data, isPending, error } = useQuery({
    queryKey: key(active?.org),
    enabled: Boolean(active),
    staleTime: FRESH_MS,
    refetchInterval: FRESH_MS,
    retry: false,
    queryFn: async () => (await json<{ notifications: Notification[] }>(await get(`/api/orgs/${active?.org}/notifications`))).notifications,
  })
  return { notifications: data, pending: isPending, failed: error }
}

export function useUnreadCount(): number {
  const { notifications } = useNotifications()
  return (notifications ?? []).filter((one) => one.read_ms === null).length
}

export function useMarkRead() {
  const { active } = useOrganisations()
  const client = useQueryClient()
  return useMutation({
    mutationFn: async (id: string) => {
      await post(`/api/orgs/${active?.org}/notifications/${id}/read`)
    },
    onSuccess: () => void client.invalidateQueries({ queryKey: key(active?.org) }),
  })
}

export function useMarkAllRead() {
  const { active } = useOrganisations()
  const client = useQueryClient()
  return useMutation({
    mutationFn: async () => json<{ marked: number }>(await post(`/api/orgs/${active?.org}/notifications/read-all`)),
    onSuccess: () => void client.invalidateQueries({ queryKey: key(active?.org) }),
  })
}

export type Channel = { kind: NotificationKind; in_app: boolean; email: boolean }

export type NotificationPreferences = {
  email_enabled: boolean
  weekly_digest: boolean
  channels: Channel[]
}

const preferencesKey = ['auth', 'notification-preferences']

export function useNotificationPreferences() {
  return useQuery({
    queryKey: preferencesKey,
    staleTime: 60_000,
    queryFn: async () => json<NotificationPreferences>(await get('/auth/me/notification-preferences')),
  })
}

export function useSetNotificationPreferences() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: async (asked: NotificationPreferences) => json<NotificationPreferences>(await put('/auth/me/notification-preferences', asked)),
    onSuccess: (preferences) => client.setQueryData(preferencesKey, preferences),
  })
}
