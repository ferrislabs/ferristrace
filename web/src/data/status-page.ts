import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { ApiError, del, get, json, patch, post } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { nothingYet } from './orgs'

export type Accent = 'indigo' | 'teal' | 'orange' | 'rose' | 'slate'
export type Mode = 'light' | 'dark' | 'auto'
export type Theme = { accent: Accent; mode: Mode }
export type ComponentStatus = 'operational' | 'degraded' | 'down'

export type ComponentLink =
  | { kind: 'service'; service: string; project: string | null }
  | { kind: 'alert'; alert: string }
  | { kind: 'manual' }

export type StatusComponent = {
  id: string
  name: string
  link: ComponentLink
  manual_status: ComponentStatus | null
  position: number
  public: boolean
}

export type StatusPage = {
  id: string
  name: string
  slug: string
  public: boolean
  hide_from_search: boolean
  theme: Theme
  intro: string | null
  subscribers: number
  created_ms: number
  updated_ms: number
}

export type StatusSheet = StatusPage & { components: StatusComponent[] }

export type PageDraft = { name: string; slug: string; public?: boolean; hide_from_search?: boolean; theme?: Theme; intro?: string | null }

export type PageChange = Partial<{ name: string; slug: string; public: boolean; hide_from_search: boolean; theme: Theme; intro: string | null }>

export type ComponentDraft = { name: string; link: ComponentLink; manual_status?: ComponentStatus | null; public?: boolean }

export type ComponentChange = Partial<{ name: string; link: ComponentLink; manual_status: ComponentStatus | null; public: boolean }>

export type PublicUpdate = { at_ms: number; kind: 'status' | 'comment'; text: string }

export type PublicIncident = {
  title: string
  status: 'investigating' | 'identified' | 'monitoring' | 'resolved'
  severity: 'sev1' | 'sev2' | 'sev3' | 'sev4'
  opened_ms: number
  resolved_ms: number | null
  updates: PublicUpdate[]
  postmortem_url: string | null
}

export type PublicStatus = {
  page: { name: string; theme: Theme; intro: string | null; hide_from_search: boolean }
  overall: ComponentStatus
  components: { name: string; status: ComponentStatus }[]
  incidents: PublicIncident[]
}

export type PublicPostmortem = { title: string; body: string; published_ms: number }

export const ACCENTS: Accent[] = ['indigo', 'teal', 'orange', 'rose', 'slate']
export const MODES: Mode[] = ['light', 'dark', 'auto']
export const COMPONENT_STATUSES: ComponentStatus[] = ['operational', 'degraded', 'down']
export const SLUG_PATTERN = /^[a-z0-9-]{3,48}$/
const RESERVED_SLUGS = ['confirm', 'unsubscribe']

export const slugProblem = (slug: string): string | null =>
  !SLUG_PATTERN.test(slug)
    ? 'Use 3 to 48 lowercase letters, digits or hyphens.'
    : RESERVED_SLUGS.includes(slug)
      ? 'This address is reserved.'
      : null

const FRESH_MS = 10_000
const PUBLIC_REFRESH_MS = 60_000

const key = (org: string | undefined) => ['org', org, 'status-pages']

const retryable = (count: number, error: unknown) => !(error instanceof ApiError) && count < 2

export function useStatusPages() {
  const found = useOrganisations()
  const { active } = found
  const { data, isPending, error } = useQuery({
    queryKey: [...key(active?.org), 'list'],
    enabled: Boolean(active),
    staleTime: FRESH_MS,
    retry: retryable,
    queryFn: async () => (await json<{ status_pages: StatusPage[] }>(await get(`/api/orgs/${active?.org}/status-pages`))).status_pages,
  })
  return { pages: data, pending: isPending, failed: error, nothing: nothingYet(found, isPending, error, 'status pages') }
}

export function useStatusPage(id: string | null) {
  const { active } = useOrganisations()
  const { data, isPending, error } = useQuery({
    queryKey: [...key(active?.org), 'one', id],
    enabled: Boolean(active) && Boolean(id),
    staleTime: FRESH_MS,
    retry: retryable,
    queryFn: async () => json<StatusSheet>(await get(`/api/orgs/${active?.org}/status-pages/${id}`)),
  })
  return { sheet: data, pending: isPending, failed: error }
}

export function useStatusPreview(id: string | null) {
  const { active } = useOrganisations()
  const { data, error } = useQuery({
    queryKey: [...key(active?.org), 'preview', id],
    enabled: Boolean(active) && Boolean(id),
    staleTime: FRESH_MS,
    refetchInterval: PUBLIC_REFRESH_MS,
    retry: retryable,
    queryFn: async () => json<PublicStatus>(await get(`/api/orgs/${active?.org}/status-pages/${id}/preview`)),
  })
  return { preview: data, failed: error }
}

function useRefreshing<T, R>(send: (org: string, asked: T) => Promise<R>) {
  const { active } = useOrganisations()
  const client = useQueryClient()
  return useMutation({
    mutationFn: async (asked: T) => send(active?.org ?? '', asked),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: key(active?.org) })
    },
  })
}

const at = (org: string, page: string) => `/api/orgs/${org}/status-pages/${page}`

export const useCreateStatusPage = () =>
  useRefreshing<PageDraft, StatusSheet>(async (org, asked) => json(await post(`/api/orgs/${org}/status-pages`, asked)))

export const useChangeStatusPage = () =>
  useRefreshing<{ page: string; change: PageChange }, StatusSheet>(async (org, asked) => json(await patch(at(org, asked.page), asked.change)))

export const useDeleteStatusPage = () =>
  useRefreshing<string, void>(async (org, page) => {
    await del(at(org, page))
  })

export const useAddComponent = () =>
  useRefreshing<{ page: string; component: ComponentDraft }, StatusComponent>(async (org, asked) =>
    json(await post(`${at(org, asked.page)}/components`, asked.component)),
  )

export const useChangeComponent = () =>
  useRefreshing<{ page: string; id: string; change: ComponentChange }, StatusComponent>(async (org, asked) =>
    json(await patch(`${at(org, asked.page)}/components/${asked.id}`, asked.change)),
  )

export const useRemoveComponent = () =>
  useRefreshing<{ page: string; id: string }, void>(async (org, asked) => {
    await del(`${at(org, asked.page)}/components/${asked.id}`)
  })

export const useOrderComponents = () =>
  useRefreshing<{ page: string; ids: string[] }, { components: StatusComponent[] }>(async (org, asked) =>
    json(await post(`${at(org, asked.page)}/components/order`, { ids: asked.ids })),
  )

async function publicly<T>(path: string): Promise<T> {
  const response = await fetch(path, { credentials: 'omit' })
  if (!response.ok) throw new ApiError(response.status, (await response.text()).trim())
  return (await response.json()) as T
}

export function usePublicStatus(slug: string | undefined) {
  const { data, isPending, error } = useQuery({
    queryKey: ['public-status', slug],
    enabled: Boolean(slug),
    staleTime: FRESH_MS,
    refetchInterval: PUBLIC_REFRESH_MS,
    retry: retryable,
    queryFn: async () => publicly<PublicStatus>(`/api/status/${slug}`),
  })
  return { status: data, pending: isPending, failed: error }
}

export function usePublicPostmortem(slug: string | undefined, incident: string | undefined) {
  const { data, isPending, error } = useQuery({
    queryKey: ['public-postmortem', slug, incident],
    enabled: Boolean(slug) && Boolean(incident),
    retry: retryable,
    queryFn: async () => publicly<PublicPostmortem>(`/api/status/${slug}/postmortems/${incident}`),
  })
  return { postmortem: data, pending: isPending, failed: error }
}

export function useSubscribe(slug: string | undefined) {
  return useMutation({
    mutationFn: async (email: string) => {
      const response = await fetch(`/api/status/${slug}/subscribe`, {
        method: 'POST',
        credentials: 'omit',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ email }),
      })
      if (!response.ok) throw new ApiError(response.status, (await response.text()).trim())
    },
  })
}

export const STATUS_WORDS: Record<ComponentStatus, string> = { operational: 'Operational', degraded: 'Degraded', down: 'Down' }

export const OVERALL_WORDS: Record<ComponentStatus, string> = {
  operational: 'All systems operational',
  degraded: 'Some systems are degraded',
  down: 'Some systems are down',
}
