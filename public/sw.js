// Cache generation. Bumping this purges every previous cache on activate.
// v1 -> v2: recovery from poisoned v1 caches that held the SPA-shell HTML
// under asset paths (e.g. /favicon.ico), which made browsers render a
// generic document icon instead of the real favicon.
const CACHE_NAME = 'freshell-shell-v2'
const ASSETS = ['/', '/index.html', '/manifest.webmanifest', '/icon-192.png', '/icon-512.png', '/apple-touch-icon.png', '/favicon.ico']
const ASSET_PATHS = new Set(ASSETS)

// The only shell paths whose body is legitimately HTML.
const HTML_PATHS = new Set(['/', '/index.html'])

// A text/html response for any OTHER shell asset means the server answered
// with the SPA-shell fallback (e.g. dist/client mid-rebuild, asset missing).
// Caching it would poison the entry until the whole cache is deleted, so
// such responses are passed through but never persisted.
function isCacheableShellResponse(pathname, response) {
  if (!response || response.status !== 200) return false
  if (HTML_PATHS.has(pathname)) return true
  const type = (response.headers.get('content-type') || '').toLowerCase()
  return !type.includes('text/html')
}

self.addEventListener('install', (event) => {
  event.waitUntil(
    caches.open(CACHE_NAME).then((cache) =>
      Promise.all(
        ASSETS.map((path) =>
          fetch(path)
            .then((response) => {
              if (isCacheableShellResponse(path, response)) return cache.put(path, response)
              return undefined
            })
            // A failed precache fetch must not fail the install: the fetch
            // handler falls through to the network on cache misses anyway.
            .catch(() => undefined)
        )
      )
    ).then(() => self.skipWaiting())
  )
})

self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches.keys().then((keys) => Promise.all(
      keys
        .filter((key) => key !== CACHE_NAME)
        .map((key) => caches.delete(key))
    )).then(() => self.clients.claim())
  )
})

self.addEventListener('fetch', (event) => {
  if (event.request.method !== 'GET') return
  const url = new URL(event.request.url)

  // Never cache API responses.
  if (url.origin === self.location.origin && url.pathname.startsWith('/api/')) {
    return
  }

  // SPA navigations should prefer network, with cached shell fallback for offline.
  if (event.request.mode === 'navigate') {
    event.respondWith(
      fetch(event.request).catch(() => caches.match('/index.html'))
    )
    return
  }

  // Only cache explicit shell assets.
  if (url.origin !== self.location.origin || !ASSET_PATHS.has(url.pathname)) {
    return
  }

  event.respondWith(
    caches.match(event.request).then((cached) => {
      if (cached) return cached

      // No index.html fallback for asset requests: answering an icon or
      // manifest fetch with the SPA shell corrupts the browser's icon state.
      // A network failure here must surface as a network error.
      return fetch(event.request).then((response) => {
        if (isCacheableShellResponse(url.pathname, response)) {
          const copy = response.clone()
          caches.open(CACHE_NAME).then((cache) => cache.put(event.request, copy))
        }
        return response
      })
    })
  )
})
