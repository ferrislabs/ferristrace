import { UserManager, WebStorageStateStore, type User } from 'oidc-client-ts'

export const RENEW_MARGIN_SECONDS = 60
const LOCK = 'ferristrace:session-renewal'
const LOOP_GUARD_KEY = 'ferristrace.reauthenticatedAt'
const LOOP_GUARD_MS = 30_000

let manager: UserManager | null = null

export function createUserManager(authority: string, clientId: string): UserManager {
  manager = new UserManager({
    authority,
    client_id: clientId,
    redirect_uri: `${window.location.origin}/`,
    post_logout_redirect_uri: `${window.location.origin}/`,
    response_type: 'code',
    scope: 'openid profile email',
    userStore: new WebStorageStateStore({ store: window.localStorage }),
    automaticSilentRenew: false,
    revokeTokensOnSignout: true,
  })
  return manager
}

export const userManager = () => manager

const now = () => Math.floor(Date.now() / 1000)

const secondsLeft = (user: User) => (user.expires_at === undefined ? 0 : user.expires_at - now())

const due = (user: User) => secondsLeft(user) <= RENEW_MARGIN_SECONDS

async function exclusively<T>(work: () => Promise<T>): Promise<T> {
  return navigator.locks ? navigator.locks.request(LOCK, work) : work()
}

async function renew(current: UserManager): Promise<User | null> {
  return exclusively(async () => {
    const stored = await current.getUser()
    if (!stored) return null
    if (!due(stored)) {
      await current.events.load(stored)
      return stored
    }
    return stored.refresh_token ? current.signinSilent() : null
  })
}

let reading: Promise<string | null> | null = null

async function readToken(current: UserManager): Promise<string | null> {
  const stored = await current.getUser()
  if (!stored) return null
  if (!due(stored) || !stored.refresh_token) return stored.access_token
  try {
    return (await renew(current))?.access_token ?? stored.access_token
  } catch {
    return stored.access_token
  }
}

export function accessToken(): Promise<string | null> {
  if (!manager) return Promise.resolve(null)
  reading ??= readToken(manager).finally(() => {
    reading = null
  })
  return reading
}

export function keepFresh(onLost: () => void): () => void {
  const current = manager
  if (!current) return () => undefined
  let timer: ReturnType<typeof setTimeout> | undefined
  let stopped = false

  const refresh = async () => {
    try {
      if ((await renew(current)) === null) onLost()
    } catch {
      onLost()
    }
  }

  const plan = async () => {
    clearTimeout(timer)
    const user = await current.getUser()
    if (stopped || !user) return
    timer = setTimeout(() => void refresh(), Math.max(0, secondsLeft(user) - RENEW_MARGIN_SECONDS) * 1000)
  }

  const check = async () => {
    const user = await current.getUser()
    if (stopped || !user) return
    if (due(user)) void refresh()
    else void plan()
  }

  const onVisible = () => {
    if (document.visibilityState === 'visible') void check()
  }

  const unloaded = () => clearTimeout(timer)
  current.events.addUserLoaded(plan)
  current.events.addUserUnloaded(unloaded)
  document.addEventListener('visibilitychange', onVisible)
  window.addEventListener('focus', onVisible)
  void check()

  return () => {
    stopped = true
    clearTimeout(timer)
    current.events.removeUserLoaded(plan)
    current.events.removeUserUnloaded(unloaded)
    document.removeEventListener('visibilitychange', onVisible)
    window.removeEventListener('focus', onVisible)
  }
}

export const safeTarget = (target: unknown) =>
  typeof target === 'string' && target.startsWith('/') && !target.startsWith('//') ? target : '/'

export const here = () => `${window.location.pathname}${window.location.search}${window.location.hash}`

export async function signIn(target: string): Promise<void> {
  await manager?.signinRedirect({ state: safeTarget(target) })
}

export async function reauthenticate(): Promise<void> {
  const current = manager
  if (!current) return
  const last = Number(window.sessionStorage.getItem(LOOP_GUARD_KEY))
  await current.removeUser()
  if (Number.isFinite(last) && Date.now() - last < LOOP_GUARD_MS) {
    window.location.assign(`/sign-in?redirect=${encodeURIComponent(safeTarget(here()))}`)
    return
  }
  window.sessionStorage.setItem(LOOP_GUARD_KEY, String(Date.now()))
  await signIn(here())
}

export async function leave(): Promise<void> {
  const current = manager
  if (!current) return
  try {
    await current.signoutRedirect()
  } catch {
    await current.removeUser()
  }
}
