import { useMutation, useQueries, useQuery, useQueryClient } from '@tanstack/react-query'
import { ApiError, del, get, json, patch, post } from '@/auth/client'
import { useAccount, useOrganisations } from '@/auth/session'
import type { Person } from './incidents'
import { nothingYet } from './orgs'
import { useTeams } from './teams'

export type Weekday = 'monday' | 'tuesday' | 'wednesday' | 'thursday' | 'friday' | 'saturday' | 'sunday'

export const WEEKDAYS: Weekday[] = ['monday', 'tuesday', 'wednesday', 'thursday', 'friday', 'saturday', 'sunday']

export const LENGTH_MIN_DAYS = 1

export const LENGTH_MAX_DAYS = 28

export type Handoff = { weekday: Weekday; time: string }

export type Override = { id: string; account: string; from_ms: number; to_ms: number }

export type Schedule = {
  id: string
  team: string
  name: string
  timezone: string
  rotation: string[]
  handoff: Handoff
  length_days: number
  starts_on: string
  overrides: Override[]
}

export type ScheduleDraft = Omit<Schedule, 'id' | 'team' | 'overrides'>

export type OnCallNow = { account: Person | null; until_ms: number | null }

export type PageState = 'triggered' | 'acknowledged' | 'escalated' | 'resolved'

export type PageSource =
  | { kind: 'alert'; rule: string; event: string; name: string }
  | { kind: 'incident'; incident: string; title: string }

export type Page = {
  id: string
  schedule: { id: string; name: string | null }
  target: Person
  source: PageSource
  headline: string
  state: PageState
  step: number
  created_ms: number
  escalate_at_ms: number | null
  acknowledged_by: string | null
  acknowledged_ms: number | null
  resolved_ms: number | null
}

const FRESH_MS = 15_000

const retryable = (count: number, error: unknown) => !(error instanceof ApiError) && count < 2

const teamKey = (org: string | undefined, team: string) => ['org', org, 'teams', team, 'schedules']

const scheduleKey = (org: string | undefined) => ['org', org, 'schedules']

const pageKey = (org: string | undefined) => ['org', org, 'pages']

async function schedulesOf(org: string, team: string) {
  return (await json<{ schedules: Schedule[] }>(await get(`/api/orgs/${org}/teams/${team}/schedules`))).schedules
}

export function useTeamSchedules(team: string | undefined) {
  const found = useOrganisations()
  const { active } = found
  const { data, isPending, error } = useQuery({
    queryKey: teamKey(active?.org, team ?? ''),
    enabled: Boolean(active) && Boolean(team),
    staleTime: FRESH_MS,
    retry: retryable,
    queryFn: async () => schedulesOf(active?.org ?? '', team ?? ''),
  })
  return { schedules: data, nothing: nothingYet(found, isPending, error, 'schedules') }
}

export function useSchedules(): { schedules: (Schedule & { teamName: string })[] } {
  const { active } = useOrganisations()
  const { teams } = useTeams()
  const found = useQueries({
    queries: (teams ?? []).map((team) => ({
      queryKey: teamKey(active?.org, team.id),
      enabled: Boolean(active),
      staleTime: FRESH_MS,
      retry: retryable,
      queryFn: async () => schedulesOf(active?.org ?? '', team.id),
    })),
  })
  const schedules = found.flatMap((query, at) =>
    (query.data ?? []).map((schedule) => ({ ...schedule, teamName: teams?.[at]?.name ?? '' })),
  )
  return { schedules }
}

export function useOnCallNow(schedule: string | undefined) {
  const { active } = useOrganisations()
  const { data, error } = useQuery({
    queryKey: [...scheduleKey(active?.org), schedule, 'now'],
    enabled: Boolean(active) && Boolean(schedule),
    staleTime: FRESH_MS,
    refetchInterval: 60_000,
    retry: retryable,
    queryFn: async () => json<OnCallNow>(await get(`/api/orgs/${active?.org}/schedules/${schedule}/now`)),
  })
  return { now: data, failed: error }
}

function useScheduling<T, R>(send: (org: string, asked: T) => Promise<R>) {
  const { active } = useOrganisations()
  const client = useQueryClient()
  return useMutation({
    mutationFn: async (asked: T) => send(active?.org ?? '', asked),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: ['org', active?.org, 'teams'] })
      void client.invalidateQueries({ queryKey: scheduleKey(active?.org) })
    },
  })
}

export const useCreateSchedule = () =>
  useScheduling<{ team: string; draft: ScheduleDraft }, Schedule>(async (org, asked) =>
    json(await post(`/api/orgs/${org}/teams/${asked.team}/schedules`, asked.draft)),
  )

export const useUpdateSchedule = () =>
  useScheduling<{ id: string; change: Partial<ScheduleDraft> }, Schedule>(async (org, asked) =>
    json(await patch(`/api/orgs/${org}/schedules/${asked.id}`, asked.change)),
  )

export const useDeleteSchedule = () =>
  useScheduling<string, string>(async (org, id) => {
    await del(`/api/orgs/${org}/schedules/${id}`)
    return id
  })

export const useAddOverride = () =>
  useScheduling<{ schedule: string; account: string; from_ms: number; to_ms: number }, Override>(async (org, asked) =>
    json(
      await post(`/api/orgs/${org}/schedules/${asked.schedule}/overrides`, {
        account: asked.account,
        from_ms: asked.from_ms,
        to_ms: asked.to_ms,
      }),
    ),
  )

export const useRemoveOverride = () =>
  useScheduling<{ schedule: string; id: string }, string>(async (org, asked) => {
    await del(`/api/orgs/${org}/schedules/${asked.schedule}/overrides/${asked.id}`)
    return asked.id
  })

export function usePages(state: 'open' | 'all' = 'open') {
  const found = useOrganisations()
  const { active } = found
  const { data, isPending, error } = useQuery({
    queryKey: [...pageKey(active?.org), state],
    enabled: Boolean(active),
    staleTime: FRESH_MS,
    refetchInterval: 30_000,
    retry: retryable,
    queryFn: async () => (await json<{ pages: Page[] }>(await get(`/api/orgs/${active?.org}/pages?state=${state}`))).pages,
  })
  return { pages: data, nothing: nothingYet(found, isPending, error, 'pages') }
}

export function useMyPages() {
  const account = useAccount()
  const { pages } = usePages('open')
  const mine = (pages ?? []).filter((page) => page.target.id === account?.account && page.state !== 'acknowledged')
  return { mine, all: pages ?? [] }
}

export function useAcknowledgePage() {
  const { active } = useOrganisations()
  const client = useQueryClient()
  return useMutation({
    mutationFn: async (id: string) => json<Page>(await post(`/api/orgs/${active?.org}/pages/${id}/ack`)),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: pageKey(active?.org) })
      void client.invalidateQueries({ queryKey: ['org', active?.org, 'incidents'] })
    },
  })
}

export const pageLink = (page: Page): { to: string; search?: Record<string, string> } =>
  page.source.kind === 'incident' ? { to: `/incidents/${page.source.incident}` } : { to: '/alerts', search: { rule: page.source.rule } }

export const TIME = /^([01]?\d|2[0-3]):[0-5]\d$/

export const DAY = /^\d{4}-\d{2}-\d{2}$/

export function draftProblem(draft: ScheduleDraft): string | null {
  if (!draft.name.trim()) return 'A schedule needs a name'
  if (!draft.timezone.trim()) return 'A schedule names its timezone, like Europe/Paris'
  if (!TIME.test(draft.handoff.time)) return 'The handoff time is written HH:MM'
  if (!Number.isInteger(draft.length_days) || draft.length_days < LENGTH_MIN_DAYS || draft.length_days > LENGTH_MAX_DAYS) {
    return `A rotation lasts ${LENGTH_MIN_DAYS} to ${LENGTH_MAX_DAYS} days`
  }
  if (!DAY.test(draft.starts_on)) return 'The start date is written YYYY-MM-DD'
  return null
}

export function moved<T>(list: T[], at: number, by: -1 | 1): T[] {
  const to = at + by
  if (to < 0 || to >= list.length) return list
  const next = [...list]
  const [taken] = next.splice(at, 1)
  next.splice(to, 0, taken)
  return next
}

export const pageStateVariant = (state: PageState): 'destructive' | 'warning' | 'success' | 'neutral' =>
  state === 'escalated' ? 'destructive' : state === 'triggered' ? 'warning' : state === 'acknowledged' ? 'success' : 'neutral'
