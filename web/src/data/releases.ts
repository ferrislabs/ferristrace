import { useQuery } from '@tanstack/react-query'
import { get, json } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { useScope } from '@/lib/scope'

export type Release = {
  service: string
  version: string
  environments: string[]
  firstSeenMs: number
  lastSeenMs: number
  requests: number
  errors: number
  newIssues: number
}

export type Traffic = { requests: number; errors: number; errorRate: number | null }

export type IntroducedIssue = {
  fingerprint: string
  exceptionType: string
  message: string
  service: string
  events: number
  firstSeenMs: number
  lastSeenMs: number
}

export type ReleaseDetail = Release & { before: Traffic; after: Traffic; issues: IntroducedIssue[] }

export type Deploy = { service: string; version: string; environment: string; atMs: number }

type WireRelease = {
  service: string
  version: string
  environments: string[]
  first_seen_at_ms: number
  last_seen_at_ms: number
  requests: number
  errors: number
  new_issues: number
}

type WireTraffic = { requests: number; errors: number; error_rate: number | null }

type WireIssue = {
  fingerprint: string
  exception_type: string
  message: string
  service: string
  events: number
  first_seen_at_ms: number
  last_seen_at_ms: number
}

type WireDetail = WireRelease & { before: WireTraffic; after: WireTraffic; issues: WireIssue[] }

type WireDeploy = { service: string; version: string; environment: string; at_ms: number }

const FRESH_MS = 30_000
const MINUTE_MS = 60_000

export const RELEASE_WINDOWS: { value: string; label: string; ms: number }[] = [
  { value: '1d', label: '24 hours', ms: 24 * 60 * 60 * 1000 },
  { value: '7d', label: '7 days', ms: 7 * 24 * 60 * 60 * 1000 },
  { value: '30d', label: '30 days', ms: 30 * 24 * 60 * 60 * 1000 },
]

export const releasePath = (service: string, version: string) =>
  `/releases/${encodeURIComponent(service)}/${encodeURIComponent(version)}`

const released = (release: WireRelease): Release => ({
  service: release.service,
  version: release.version,
  environments: release.environments,
  firstSeenMs: release.first_seen_at_ms,
  lastSeenMs: release.last_seen_at_ms,
  requests: release.requests,
  errors: release.errors,
  newIssues: release.new_issues,
})

const around = (traffic: WireTraffic): Traffic => ({ requests: traffic.requests, errors: traffic.errors, errorRate: traffic.error_rate })

const detailed = (detail: WireDetail): ReleaseDetail => ({
  ...released(detail),
  before: around(detail.before),
  after: around(detail.after),
  issues: detail.issues.map((issue) => ({
    fingerprint: issue.fingerprint,
    exceptionType: issue.exception_type,
    message: issue.message,
    service: issue.service,
    events: issue.events,
    firstSeenMs: issue.first_seen_at_ms,
    lastSeenMs: issue.last_seen_at_ms,
  })),
})

const marked = (deploy: WireDeploy): Deploy => ({
  service: deploy.service,
  version: deploy.version,
  environment: deploy.environment,
  atMs: deploy.at_ms,
})

export function useReleases(windowMs: number, service = '') {
  const { active } = useOrganisations()
  const { project } = useScope()
  const minute = Math.floor(Date.now() / MINUTE_MS)
  const found = useQuery({
    queryKey: ['releases', active?.org, project, service, windowMs, minute],
    enabled: Boolean(active),
    staleTime: FRESH_MS,
    retry: false,
    queryFn: async () => {
      const to = Date.now()
      const params = new URLSearchParams({ from: String(to - windowMs), to: String(to) })
      if (service) params.set('service', service)
      if (project) params.set('project', project)
      const body = await json<{ releases: WireRelease[] }>(await get(`/api/orgs/${active?.org}/releases?${params.toString()}`))
      return body.releases.map(released)
    },
  })
  return { releases: found.data ?? [], pending: found.isPending, failed: found.error }
}

export function useRelease(service: string | undefined, version: string | undefined) {
  const { active } = useOrganisations()
  const { project } = useScope()
  const found = useQuery({
    queryKey: ['release', active?.org, project, service, version],
    enabled: Boolean(active && service && version),
    staleTime: FRESH_MS,
    retry: false,
    queryFn: async () => {
      const params = new URLSearchParams()
      if (project) params.set('project', project)
      const path = `/api/orgs/${active?.org}/releases/${encodeURIComponent(service ?? '')}/${encodeURIComponent(version ?? '')}`
      return detailed(await json<WireDetail>(await get(`${path}?${params.toString()}`)))
    },
  })
  return { release: found.data ?? null, pending: found.isPending, failed: found.error }
}

export function useDeploys(fromMs: number | null, toMs: number | null, service = '') {
  const { active } = useOrganisations()
  const { project } = useScope()
  const found = useQuery({
    queryKey: ['deploys', active?.org, project, service, fromMs, toMs],
    enabled: Boolean(active) && fromMs !== null && toMs !== null && toMs > fromMs,
    staleTime: FRESH_MS,
    retry: false,
    queryFn: async () => {
      const params = new URLSearchParams({ from: String(fromMs), to: String(toMs) })
      if (service) params.set('service', service)
      if (project) params.set('project', project)
      const body = await json<{ deploys: WireDeploy[] }>(await get(`/api/orgs/${active?.org}/deploys?${params.toString()}`))
      return body.deploys.map(marked)
    },
  })
  return found.data ?? []
}

export function firstDeploys(deploys: Deploy[]): Deploy[] {
  const earliest = new Map<string, Deploy>()
  for (const deploy of deploys) {
    const key = `${deploy.service}\u0000${deploy.version}`
    const held = earliest.get(key)
    if (!held || deploy.atMs < held.atMs) earliest.set(key, deploy)
  }
  return [...earliest.values()].sort((a, b) => a.atMs - b.atMs)
}

export const errorRateOf = (requests: number, errors: number) => (requests ? (errors / requests) * 100 : null)
