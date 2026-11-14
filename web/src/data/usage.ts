import { useQuery } from '@tanstack/react-query'
import { get, json } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { nothingYet } from './orgs'

export type Signal = 'spans' | 'logs' | 'metrics'

export type Hour = {
  started_at_ms: number
  ingested_bytes: number
  spans_bytes: number | null
  logs_bytes: number | null
  metrics_points: number | null
}

export type Metered = {
  ingested_bytes: number
  included_bytes: number | null
  by_signal: { spans_bytes: number; logs_bytes: number; metrics_points: number; unsplit_hours: number }
  hours: Hour[]
}

export const SIGNALS: { key: Signal; label: string; unit: 'bytes' | 'points'; read: (hour: Hour) => number | null }[] = [
  { key: 'logs', label: 'Log records', unit: 'bytes', read: (hour) => hour.logs_bytes },
  { key: 'spans', label: 'Spans', unit: 'bytes', read: (hour) => hour.spans_bytes },
  { key: 'metrics', label: 'Metric datapoints', unit: 'points', read: (hour) => hour.metrics_points },
]

export type Day = { at: number; bytes: number }

export function useMetered(from: number, to: number) {
  const found = useOrganisations()
  const { active } = found
  const { data, isPending, error } = useQuery({
    queryKey: ['usage', active?.org, from, to],
    enabled: Boolean(active),
    staleTime: 60_000,
    queryFn: async () => json<Metered>(await get(`/api/orgs/${active?.org}/usage?from=${from}&to=${to}`)),
  })
  return { metered: data, nothing: nothingYet(found, isPending, error, 'usage') }
}

const DAY = 86_400_000

export const startOfMonth = (at: number) => Date.UTC(new Date(at).getUTCFullYear(), new Date(at).getUTCMonth(), 1)

export const startOfNextMonth = (at: number) => Date.UTC(new Date(at).getUTCFullYear(), new Date(at).getUTCMonth() + 1, 1)

export function byDay(hours: Metered['hours'], from: number, to: number): Day[] {
  const totals = new Map<number, number>()
  for (const hour of hours) {
    const day = Math.floor(hour.started_at_ms / DAY) * DAY
    totals.set(day, (totals.get(day) ?? 0) + hour.ingested_bytes)
  }
  const days: Day[] = []
  for (let at = Math.floor(from / DAY) * DAY; at < to; at += DAY) {
    days.push({ at, bytes: totals.get(at) ?? 0 })
  }
  return days
}

export const HOUR = 3_600_000

export function dailyOf(hours: Hour[], read: (hour: Hour) => number | null, from: number, to: number, step: number = DAY): Day[] {
  const totals = new Map<number, number>()
  for (const hour of hours) {
    const value = read(hour)
    if (value === null) continue
    const bucket = Math.floor(hour.started_at_ms / step) * step
    totals.set(bucket, (totals.get(bucket) ?? 0) + value)
  }
  const buckets: Day[] = []
  for (let at = Math.floor(from / step) * step; at < to; at += step) {
    buckets.push({ at, bytes: totals.get(at) ?? 0 })
  }
  return buckets
}

export function projected(used: number, at: number): number {
  const start = startOfMonth(at)
  const elapsed = at - start
  const whole = startOfNextMonth(at) - start
  if (elapsed <= 0) return used
  return Math.round((used / elapsed) * whole)
}

const UNITS = ['B', 'kB', 'MB', 'GB', 'TB', 'PB']

export function bytes(count: number): string {
  if (count < 1000) return `${count} B`
  let scaled = count
  let unit = 0
  while (scaled >= 1000 && unit < UNITS.length - 1) {
    scaled /= 1000
    unit += 1
  }
  return `${scaled.toFixed(scaled < 10 ? 2 : scaled < 100 ? 1 : 0)} ${UNITS[unit]}`
}

const COUNT_UNITS = ['', 'k', 'M', 'B', 'T']

export function count(value: number): string {
  if (value < 1000) return `${value}`
  let scaled = value
  let unit = 0
  while (scaled >= 1000 && unit < COUNT_UNITS.length - 1) {
    scaled /= 1000
    unit += 1
  }
  return `${scaled.toFixed(scaled < 10 ? 2 : scaled < 100 ? 1 : 0)} ${COUNT_UNITS[unit]}`.trim()
}
