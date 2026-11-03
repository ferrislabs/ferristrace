const SHELL = 'ferristrace-shell-v3'
const DATA = 'ferristrace-data-v1'
const OFFLINE = '/offline.html'
const APP = '/index.html'
const KEPT = [OFFLINE, '/icon-192.png']
const STORED_AT = 'x-ferristrace-stored-at'
const KEPT_DATA = /^\/(auth\/me$|api\/orgs\/[^/]+\/(incidents|members|pages)(\/|$))/

self.addEventListener('install', (event) => {
  event.waitUntil(caches.open(SHELL).then((cache) => cache.addAll(KEPT)).then(() => self.skipWaiting()))
})

self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches.keys()
      .then((names) => Promise.all(names.filter((name) => name !== SHELL && name !== DATA).map((name) => caches.delete(name))))
      .then(() => self.clients.claim()),
  )
})

self.addEventListener('message', (event) => {
  if (event.data === 'forget-data') event.waitUntil(caches.delete(DATA))
})

async function stamped(response) {
  const headers = new Headers(response.headers)
  headers.set(STORED_AT, String(Date.now()))
  return new Response(await response.clone().blob(), { status: response.status, statusText: response.statusText, headers })
}

async function navigate(request) {
  try {
    const response = await fetch(request)
    if (response.ok) {
      const cache = await caches.open(SHELL)
      await cache.put(APP, response.clone())
    }
    return response
  } catch {
    return (await caches.match(APP)) ?? (await caches.match(OFFLINE))
  }
}

async function asset(request) {
  const cached = await caches.match(request)
  if (cached) return cached
  const response = await fetch(request)
  if (response.ok) {
    const cache = await caches.open(SHELL)
    await cache.put(request, response.clone())
  }
  return response
}

const UNREACHABLE = [502, 503, 504]

async function data(request) {
  let response
  try {
    response = await fetch(request)
  } catch (error) {
    const cached = await caches.match(request, { cacheName: DATA })
    if (cached) return cached
    throw error
  }
  if (response.ok) {
    const cache = await caches.open(DATA)
    await cache.put(request, await stamped(response))
  } else if (UNREACHABLE.includes(response.status)) {
    const cached = await caches.match(request, { cacheName: DATA })
    if (cached) return cached
  }
  return response
}

self.addEventListener('fetch', (event) => {
  const request = event.request
  if (request.method !== 'GET') return
  const url = new URL(request.url)
  if (url.origin !== self.location.origin) return
  if (request.mode === 'navigate') event.respondWith(navigate(request))
  else if (url.pathname.startsWith('/assets/')) event.respondWith(asset(request))
  else if (KEPT_DATA.test(url.pathname)) event.respondWith(data(request))
})
