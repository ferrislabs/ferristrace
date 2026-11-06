import { useNavigate as useRouterNavigate } from '@tanstack/react-router'
import { useSearchParam } from './router'

const MINUTE = 60 * 1000
const HOUR = 60 * MINUTE

export const WINDOWS = [
  { key: '15m', label: '15 minutes', short: '15 m', ms: 15 * MINUTE },
  { key: '1h', label: '1 hour', short: '1 h', ms: HOUR },
  { key: '6h', label: '6 hours', short: '6 h', ms: 6 * HOUR },
  { key: '24h', label: '24 hours', short: '24 h', ms: 24 * HOUR },
  { key: '7d', label: '7 days', short: '7 d', ms: 7 * 24 * HOUR },
] as const

export type Window = (typeof WINDOWS)[number]

export const DEFAULT_WINDOW: Window = WINDOWS[1]

export const windowOf = (key: unknown): Window => WINDOWS.find((w) => w.key === key) ?? DEFAULT_WINDOW

function unzoomed(f: unknown): string | undefined {
  if (typeof f !== 'string') return undefined
  const p = new URLSearchParams(f)
  p.delete('from')
  p.delete('to')
  p.delete('range')
  return p.toString() || undefined
}

export function useWindow(): Window & { choose: (key: Window['key']) => void } {
  const current = windowOf(useSearchParam('window'))
  const navigate = useRouterNavigate()
  const choose = (key: Window['key']) =>
    void navigate({
      to: '.' as never,
      search: ((was: Record<string, unknown>) => ({ ...was, f: unzoomed(was.f), from: undefined, to: undefined, window: key === DEFAULT_WINDOW.key ? undefined : key })) as never,
      replace: true,
    })
  return { ...current, choose }
}
