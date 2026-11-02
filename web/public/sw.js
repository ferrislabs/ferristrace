const SHELL = 'ferristrace-shell-v2'
const OFFLINE = '/offline.html'
const KEPT = [OFFLINE, '/icon-192.png']

self.addEventListener('install', (event) => {
  event.waitUntil(caches.open(SHELL).then((cache) => cache.addAll(KEPT)).then(() => self.skipWaiting()))
})

self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches.keys()
      .then((names) => Promise.all(names.filter((name) => name !== SHELL).map((name) => caches.delete(name))))
      .then(() => self.clients.claim()),
  )
})

self.addEventListener('fetch', (event) => {
  const request = event.request
  if (request.method !== 'GET' || request.mode !== 'navigate') return
  event.respondWith(fetch(request).catch(() => caches.match(OFFLINE)))
})
