import type { Marker } from '@/components/ft/charts'
import { firstDeploys, releasePath, useDeploys } from '@/data/releases'
import type { Stepped } from '@/data/services'

export const TIME_LABELS = ['13:32', '13:47', '14:02', '14:17', '14:32']

export function spanOf(series: Stepped | null): { from: number | null; to: number | null } {
  const points = series?.points ?? []
  if (!series || points.length < 2) return { from: null, to: null }
  return { from: points[0].atMs, to: points[points.length - 1].atMs + series.stepMs }
}

export function useDeployMarkers(series: Stepped | null, service = ''): Marker[] {
  const { from, to } = spanOf(series)
  const deploys = firstDeploys(useDeploys(from, to, service))
  const points = series?.points ?? []
  if (!series || from === null || points.length < 2) return []
  return deploys
    .map((deploy) => ({
      at: (deploy.atMs - from) / series.stepMs,
      label: service ? deploy.version : `${deploy.service} ${deploy.version}`,
      to: releasePath(deploy.service, deploy.version),
    }))
    .filter((marker) => marker.at >= 0 && marker.at <= points.length - 1)
}

const pad = (n: number) => String(n).padStart(2, '0')

export function timeLabels(fromMs: number, toMs: number, count = 5): string[] {
  const days = toMs - fromMs > 24 * 60 * 60 * 1000
  return Array.from({ length: count }, (_, at) => {
    const when = new Date(fromMs + ((toMs - fromMs) * at) / (count - 1))
    const clock = `${pad(when.getHours())}:${pad(when.getMinutes())}`
    return days ? `${pad(when.getMonth() + 1)}-${pad(when.getDate())} ${clock}` : clock
  })
}

export function ceiling(values: number[]): number {
  const top = Math.max(0, ...values)
  if (top <= 0) return 1
  const magnitude = 10 ** Math.floor(Math.log10(top))
  const steps = [1, 2, 2.5, 5, 10]
  return (steps.find((step) => step * magnitude >= top) ?? 10) * magnitude
}
