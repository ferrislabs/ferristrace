import { useSyncExternalStore } from 'react'

export const STORED_AT = 'x-ferristrace-stored-at'

type Status = { offline: boolean; since: number | null }

let status: Status = { offline: typeof navigator !== 'undefined' && !navigator.onLine, since: null }
const listeners = new Set<() => void>()

function set(next: Status) {
  if (next.offline === status.offline && next.since === status.since) return
  status = next
  for (const listener of listeners) listener()
}

export function noteResponse(response: Response) {
  const stored = Number(response.headers.get(STORED_AT))
  if (Number.isFinite(stored) && stored > 0) set({ offline: true, since: status.since === null ? stored : Math.min(status.since, stored) })
  else if (status.since !== null && navigator.onLine) set({ offline: false, since: null })
}

if (typeof window !== 'undefined') {
  window.addEventListener('offline', () => set({ ...status, offline: true }))
  window.addEventListener('online', () => set({ offline: false, since: null }))
}

export function useOffline(): Status {
  return useSyncExternalStore(
    (notify) => {
      listeners.add(notify)
      return () => listeners.delete(notify)
    },
    () => status,
  )
}

export function forgetStoredData() {
  navigator.serviceWorker?.controller?.postMessage('forget-data')
}
