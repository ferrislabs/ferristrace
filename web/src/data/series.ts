import type { Marker } from '@/components/ft/charts'

export const TIME_LABELS = ['13:32', '13:47', '14:02', '14:17', '14:32']

const NO_DEPLOYS: Marker[] = []

export function useDeployMarkers(): Marker[] {
  return NO_DEPLOYS
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
