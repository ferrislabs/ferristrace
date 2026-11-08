import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { ApiError, get, json, patch, post } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import type { AlertContext, AlertStatus } from './alerts'
import { nothingYet } from './orgs'

export type IncidentSeverity = 'sev1' | 'sev2' | 'sev3' | 'sev4'

export type IncidentStatus = 'investigating' | 'identified' | 'monitoring' | 'resolved'

export type IncidentScope = 'open' | 'resolved' | 'all'

export type Person = { id: string; email: string | null; name: string | null }

export type Incident = {
  id: string
  title: string
  severity: IncidentSeverity
  status: IncidentStatus
  owner: Person | null
  public: boolean
  project: string | null
  opened_ms: number
  resolved_ms: number | null
  alert: { id: string; name: string } | null
  services: string[]
  paged_schedule: string | null
}

export type AttachmentKind = 'trace' | 'logs' | 'metric'

export type Attachment = { id: string; kind: AttachmentKind; reference: string; label: string; at_ms: number }

export type Author = { kind: 'account' | 'api_key' | 'agent' | 'system'; id: string | null; label: string; on_behalf_of?: string }

type Pair<T> = { from: T; to: T }

export type TimelineEntry = { at_ms: number; author: Author } & (
  | { kind: 'opened'; detail: { title: string; severity: IncidentSeverity; alert: { id: string; name: string | null } | null } }
  | { kind: 'status'; detail: Pair<IncidentStatus> }
  | { kind: 'severity'; detail: Pair<IncidentSeverity> }
  | { kind: 'owner'; detail: Pair<Person | null> }
  | { kind: 'title'; detail: Pair<string> }
  | { kind: 'public'; detail: Pair<boolean> }
  | { kind: 'comment'; detail: { body: string; public: boolean } }
  | { kind: 'alert'; detail: { alert: { id: string; name: string | null }; from: AlertStatus; to: AlertStatus; value: number | null; labels: Record<string, string>; context?: AlertContext } }
  | { kind: 'attachment'; detail: Attachment }
  | { kind: 'paged'; detail: { page: string; step: number; target: Person | null } }
  | { kind: 'acknowledged'; detail: { page: string } }
  | { kind: 'deploy'; detail: { service: string; version: string; environment: string } }
)

export type Dossier = Incident & { attachments: Attachment[]; timeline: TimelineEntry[] }

export type Declaration = {
  title?: string
  severity?: IncidentSeverity
  project?: string | null
  alert?: string
  public?: boolean
  page?: { schedule: string }
}

export type IncidentChange = Partial<{ title: string; severity: IncidentSeverity; status: IncidentStatus; owner: string | null; public: boolean }>

export type AttachmentDraft = { kind: AttachmentKind; reference: string; label: string }

export const SEVERITIES: IncidentSeverity[] = ['sev1', 'sev2', 'sev3', 'sev4']

export const STATUSES: IncidentStatus[] = ['investigating', 'identified', 'monitoring', 'resolved']

const FRESH_MS = 10_000

const key = (org: string | undefined) => ['org', org, 'incidents']

const retryable = (count: number, error: unknown) => !(error instanceof ApiError) && count < 2

export function useIncidents(scope: IncidentScope = 'open') {
  const found = useOrganisations()
  const { active } = found
  const { data, isPending, error } = useQuery({
    queryKey: [...key(active?.org), 'list', scope],
    enabled: Boolean(active),
    staleTime: FRESH_MS,
    refetchInterval: 30_000,
    retry: retryable,
    queryFn: async () => (await json<{ incidents: Incident[] }>(await get(`/api/orgs/${active?.org}/incidents?status=${scope}`))).incidents,
  })
  return { incidents: data, pending: isPending, failed: error, nothing: nothingYet(found, isPending, error, 'incidents') }
}

export function useIncident(id: string | undefined) {
  const { active } = useOrganisations()
  const { data, isPending, error } = useQuery({
    queryKey: [...key(active?.org), 'one', id],
    enabled: Boolean(active) && Boolean(id),
    staleTime: FRESH_MS,
    refetchInterval: 30_000,
    retry: retryable,
    queryFn: async () => json<Dossier>(await get(`/api/orgs/${active?.org}/incidents/${id}`)),
  })
  return { incident: data, pending: isPending, failed: error }
}

export type IncidentContext = {
  services: string[]
  from_ms: number
  to_ms: number
  deploys: { service: string; version: string; environment: string; at_ms: number }[]
  issues: { print: string; title: string; message: string; service: string; first_seen_ms: number; occurrences: number }[]
  resources: { kind: 'cluster' | 'host' | 'container'; id: string; name: string; silent: boolean; cpu_percent: number | null; memory_bytes: number | null }[]
  traces: { trace_id: string; service: string; operation: string; duration_ms: number; at_ms: number }[]
  left_out: { deploys: number; issues: number; resources: number; traces: number }
}

export function useIncidentContext(id: string) {
  const { active } = useOrganisations()
  const { data, isPending, error } = useQuery({
    queryKey: [...key(active?.org), 'context', id],
    enabled: Boolean(active),
    staleTime: FRESH_MS,
    refetchInterval: 60_000,
    retry: false,
    queryFn: async () => {
      const body = await json<Partial<IncidentContext>>(await get(`/api/orgs/${active?.org}/incidents/${id}/context`))
      return {
        services: body.services ?? [],
        from_ms: body.from_ms ?? 0,
        to_ms: body.to_ms ?? 0,
        deploys: body.deploys ?? [],
        issues: body.issues ?? [],
        resources: body.resources ?? [],
        traces: body.traces ?? [],
        left_out: body.left_out ?? { deploys: 0, issues: 0, resources: 0, traces: 0 },
      } satisfies IncidentContext
    },
  })
  return { context: data, pending: isPending, failed: error }
}

export type Timing = { samples: number; mean_ms: number | null; median_ms: number | null }

export type InsightGroup = { incidents: number; open: number; resolved: number; acknowledge: Timing; resolve: Timing }

export type IncidentInsights = {
  from_ms: number
  to_ms: number
  overall: InsightGroup
  by_severity: (InsightGroup & { severity: IncidentSeverity })[]
  by_service: (InsightGroup & { service: string | null })[]
  per_day: { day_ms: number; incidents: number }[]
}

export function useIncidentInsights(days: number) {
  const { active } = useOrganisations()
  const { data, isPending, error } = useQuery({
    queryKey: [...key(active?.org), 'insights', days],
    enabled: Boolean(active),
    staleTime: FRESH_MS,
    refetchInterval: 60_000,
    retry: retryable,
    queryFn: async () => json<IncidentInsights>(await get(`/api/orgs/${active?.org}/incidents/insights?from_ms=${Date.now() - days * 86_400_000}&to_ms=${Date.now() + 60_000}`)),
  })
  return { insights: data, pending: isPending, failed: error }
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

export const useDeclareIncident = () =>
  useRefreshing<Declaration, Incident>(async (org, asked) => json(await post(`/api/orgs/${org}/incidents`, asked)))

export const useChangeIncident = () =>
  useRefreshing<{ id: string; change: IncidentChange }, Incident>(async (org, asked) =>
    json(await patch(`/api/orgs/${org}/incidents/${asked.id}`, asked.change)),
  )

export const useCommentIncident = () =>
  useRefreshing<{ id: string; body: string; public?: boolean }, TimelineEntry>(async (org, asked) =>
    json(await post(`/api/orgs/${org}/incidents/${asked.id}/comments`, { body: asked.body, public: asked.public ?? false })),
  )

export const useAttachToIncident = () =>
  useRefreshing<{ id: string; attachment: AttachmentDraft }, Attachment>(async (org, asked) =>
    json(await post(`/api/orgs/${org}/incidents/${asked.id}/attachments`, asked.attachment)),
  )

export const isOpen = (incident: Incident) => incident.status !== 'resolved'

export const personOf = (person: Person | null) => (person === null ? 'Nobody' : person.name ?? person.email ?? 'a former member')

export function attachmentLink(attachment: Pick<Attachment, 'kind' | 'reference'>): { to: string; search?: Record<string, string> } {
  if (attachment.kind === 'trace') return { to: `/traces/${attachment.reference}` }
  if (attachment.kind === 'logs') return { to: '/logs', search: { f: attachment.reference } }
  return { to: '/metrics', search: { f: attachment.reference } }
}

export const severityVariant = (severity: IncidentSeverity): 'destructive' | 'warning' | 'neutral' =>
  severity === 'sev1' || severity === 'sev2' ? 'destructive' : severity === 'sev3' ? 'warning' : 'neutral'

export const statusVariant = (status: IncidentStatus): 'destructive' | 'warning' | 'accent' | 'success' =>
  status === 'investigating' ? 'destructive' : status === 'identified' ? 'warning' : status === 'monitoring' ? 'accent' : 'success'
