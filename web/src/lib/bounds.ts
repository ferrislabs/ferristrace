import { useLocation } from './router'

export type Zoom = { from: number; to: number }

const asMs = (value: unknown): number | null => {
  const number = typeof value === 'number' ? value : typeof value === 'string' && value.trim() !== '' ? Number(value) : NaN
  return Number.isFinite(number) && number > 0 ? Math.round(number) : null
}

export function useZoom(): Zoom | null {
  const search = useLocation().search as Record<string, unknown>
  const from = asMs(search.from)
  const to = asMs(search.to)
  return from !== null && to !== null && to > from ? { from, to } : null
}

export function boundsOf(rangeMs: number, zoom: Zoom | null): { from: number; to: number } {
  if (zoom) return { from: zoom.from, to: zoom.to }
  const to = Date.now()
  return { from: to - rangeMs, to }
}

export const zoomKey = (zoom: Zoom | null): string => (zoom ? `${zoom.from}-${zoom.to}` : '')
