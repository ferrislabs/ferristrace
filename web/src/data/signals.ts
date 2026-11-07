import { BACKEND, resetSeed, type SignalView } from '@/mocks/signals'
import { useMock } from './seam'

export type { SignalView }

export function useSignalView(view: string) {
  return useMock(['signals', view], () => {
    const found = BACKEND[view]
    if (!found) return null
    resetSeed(view.length * 7)
    return { view: found, a: Array.from({ length: 60 }, (_, i) => found.a(i)), b: Array.from({ length: 60 }, (_, i) => found.b(i)) }
  })
}
