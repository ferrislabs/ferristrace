import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { ApiError, get, json, patch, post } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import type { Person } from './incidents'

export type PostmortemStatus = 'draft' | 'published'

export type Postmortem = {
  id: string
  incident: string
  status: PostmortemStatus
  body: string
  author: Person | null
  created_ms: number
  updated_ms: number
  published_ms: number | null
  edited: boolean
}

const FRESH_MS = 10_000

const key = (org: string | undefined) => ['org', org, 'postmortems']

const retryable = (count: number, error: unknown) => !(error instanceof ApiError) && count < 2

export function usePostmortem(incident: string) {
  const { active } = useOrganisations()
  const { data, isPending, error } = useQuery({
    queryKey: [...key(active?.org), 'one', incident],
    enabled: Boolean(active),
    staleTime: FRESH_MS,
    retry: retryable,
    queryFn: async (): Promise<Postmortem | null> => {
      try {
        return await json<Postmortem>(await get(`/api/orgs/${active?.org}/incidents/${incident}/postmortem`))
      } catch (failure) {
        if (failure instanceof ApiError && failure.status === 404) return null
        throw failure
      }
    },
  })
  return { postmortem: data, pending: isPending, failed: error }
}

export function usePostmortemStates() {
  const { active } = useOrganisations()
  const { data } = useQuery({
    queryKey: [...key(active?.org), 'list'],
    enabled: Boolean(active),
    staleTime: FRESH_MS,
    retry: retryable,
    queryFn: async () => (await json<{ postmortems: { incident: string; status: PostmortemStatus }[] }>(await get(`/api/orgs/${active?.org}/postmortems`))).postmortems,
  })
  return new Map((data ?? []).map((one) => [one.incident, one.status]))
}

function useRefreshing<T>(send: (org: string, asked: T) => Promise<Postmortem>) {
  const { active } = useOrganisations()
  const client = useQueryClient()
  return useMutation({
    mutationFn: async (asked: T) => send(active?.org ?? '', asked),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: key(active?.org) })
    },
  })
}

const at = (org: string, incident: string) => `/api/orgs/${org}/incidents/${incident}/postmortem`

export const useDraftPostmortem = () =>
  useRefreshing<{ incident: string; force?: boolean }>(async (org, asked) =>
    json(await post(`${at(org, asked.incident)}${asked.force ? '?force=true' : ''}`)),
  )

export const useEditPostmortem = () =>
  useRefreshing<{ incident: string; body: string }>(async (org, asked) => json(await patch(at(org, asked.incident), { body: asked.body })))

export const usePublishPostmortem = () =>
  useRefreshing<{ incident: string; publish: boolean }>(async (org, asked) =>
    json(await post(`${at(org, asked.incident)}/${asked.publish ? 'publish' : 'unpublish'}`)),
  )
