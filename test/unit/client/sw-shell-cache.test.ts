import { describe, it, expect, vi } from 'vitest'
import fs from 'fs'
import path from 'path'

/**
 * Behavior contract for public/sw.js (the PWA shell service worker).
 *
 * Regression context: the SW served /favicon.ico cache-first from
 * 'freshell-shell-v1' with no revalidation. When the cached entry was an
 * HTML document (SPA-shell fallback cached under an icon path -- e.g. the
 * install ran while dist/client was mid-rebuild, or a network failure hit
 * the `.catch(() => caches.match('/index.html'))` asset fallback), Chrome
 * rendered its generic document icon instead of the shell-with-flame,
 * permanently. These tests pin the hardened contract:
 *
 *  1. activate purges the old poisoned cache generation ('freshell-shell-v1')
 *  2. HTML responses are never cached under non-HTML asset paths
 *     (install precache AND fetch-handler runtime caching)
 *  3. a failed asset fetch is NOT answered with index.html
 *  4. install survives individual asset failures (partial precache)
 *  5. navigations keep the offline index.html fallback
 */

const SW_SOURCE = fs.readFileSync(path.resolve(__dirname, '../../../public/sw.js'), 'utf8')

const ORIGIN = 'http://localhost:3002'
const LEGACY_CACHE = 'freshell-shell-v1'

type FetchMock = (input: unknown) => Promise<Response>

interface SwHarness {
  listeners: Record<string, (event: any) => void>
  stores: Map<string, Map<string, Response>>
  skipWaiting: ReturnType<typeof vi.fn>
  claim: ReturnType<typeof vi.fn>
}

function requestKey(input: unknown): string {
  if (typeof input === 'string') return new URL(input, ORIGIN).pathname
  return new URL((input as { url: string }).url).pathname
}

function makeRequest(pathname: string, mode: 'no-cors' | 'navigate' = 'no-cors') {
  return { url: `${ORIGIN}${pathname}`, method: 'GET', mode }
}

function htmlResponse(): Response {
  return new Response('<html><body>spa shell</body></html>', {
    status: 200,
    headers: { 'content-type': 'text/html; charset=utf-8' },
  })
}

function icoResponse(): Response {
  return new Response(new Uint8Array([0, 0, 1, 0]), {
    status: 200,
    headers: { 'content-type': 'image/x-icon' },
  })
}

/**
 * Evaluate public/sw.js against mock `self` / `caches` / `fetch` globals.
 * The caches mock implements the subset of CacheStorage the SW uses,
 * including addAll semantics (fetch each entry, store any ok response --
 * the poisoning vector the fix must no longer rely on).
 */
function loadServiceWorker(fetchMock: FetchMock, initialStores?: Map<string, Map<string, Response>>): SwHarness {
  const listeners: Record<string, (event: any) => void> = {}
  const stores = initialStores ?? new Map<string, Map<string, Response>>()

  const openCache = (name: string) => {
    if (!stores.has(name)) stores.set(name, new Map())
    const store = stores.get(name)!
    return {
      put: async (req: unknown, res: Response) => {
        store.set(requestKey(req), res)
      },
      match: async (req: unknown) => store.get(requestKey(req)),
      addAll: async (paths: string[]) => {
        for (const p of paths) {
          const res = await fetchMock(p)
          if (!res || !res.ok) throw new TypeError(`addAll failed for ${p}`)
          store.set(requestKey(p), res)
        }
      },
    }
  }

  const caches = {
    open: async (name: string) => openCache(name),
    keys: async () => [...stores.keys()],
    delete: async (name: string) => stores.delete(name),
    match: async (req: unknown) => {
      for (const store of stores.values()) {
        const hit = store.get(requestKey(req))
        if (hit) return hit
      }
      return undefined
    },
  }

  const skipWaiting = vi.fn().mockResolvedValue(undefined)
  const claim = vi.fn().mockResolvedValue(undefined)
  const self = {
    addEventListener: (event: string, handler: (event: any) => void) => {
      listeners[event] = handler
    },
    skipWaiting,
    clients: { claim },
    location: { origin: ORIGIN },
  }

  const run = new Function('self', 'caches', 'fetch', SW_SOURCE)
  run(self, caches, fetchMock)

  return { listeners, stores, skipWaiting, claim }
}

async function dispatchLifecycle(harness: SwHarness, name: 'install' | 'activate'): Promise<void> {
  let settled: Promise<unknown> = Promise.resolve()
  harness.listeners[name]({
    waitUntil: (p: Promise<unknown>) => {
      settled = p
    },
  })
  await settled
}

function dispatchFetch(harness: SwHarness, request: ReturnType<typeof makeRequest>): Promise<Response> | undefined {
  let result: Promise<Response> | undefined
  harness.listeners.fetch({
    request,
    respondWith: (p: Response | Promise<Response>) => {
      result = Promise.resolve(p)
    },
  })
  return result
}

function currentCacheName(harness: SwHarness): string {
  const names = [...harness.stores.keys()].filter((name) => name !== LEGACY_CACHE)
  expect(names).toHaveLength(1)
  return names[0]
}

describe('sw.js shell cache hardening', () => {
  it('activate purges the legacy freshell-shell-v1 cache (poisoned favicon recovery)', async () => {
    // A live-site cache generation poisoned with HTML under /favicon.ico.
    const poisoned = new Map<string, Map<string, Response>>()
    poisoned.set(LEGACY_CACHE, new Map([['/favicon.ico', htmlResponse()]]))

    const harness = loadServiceWorker(async () => icoResponse(), poisoned)
    await dispatchLifecycle(harness, 'activate')

    expect(harness.stores.has(LEGACY_CACHE)).toBe(false)
    expect(harness.claim).toHaveBeenCalled()
  })

  it('install does not cache an HTML response under an icon path (mid-rebuild SPA fallback)', async () => {
    // Server answers EVERY path with the SPA shell (dist/client mid-rebuild).
    const harness = loadServiceWorker(async () => htmlResponse())
    await dispatchLifecycle(harness, 'install')

    const store = harness.stores.get(currentCacheName(harness))!
    for (const pathname of ['/favicon.ico', '/icon-192.png', '/icon-512.png', '/apple-touch-icon.png', '/manifest.webmanifest']) {
      expect(store.has(pathname), `${pathname} must not be poisoned with HTML`).toBe(false)
    }
    // The genuinely-HTML shell entries are still allowed to cache.
    expect(store.has('/index.html')).toBe(true)
    expect(harness.skipWaiting).toHaveBeenCalled()
  })

  it('install precaches image assets and survives individual fetch failures', async () => {
    const harness = loadServiceWorker(async (input) => {
      const key = requestKey(input)
      if (key === '/icon-192.png') throw new TypeError('network down')
      if (key === '/' || key === '/index.html') return htmlResponse()
      return icoResponse()
    })
    await dispatchLifecycle(harness, 'install')

    const store = harness.stores.get(currentCacheName(harness))!
    expect(store.has('/favicon.ico')).toBe(true)
    expect(store.has('/icon-192.png')).toBe(false)
    expect(harness.skipWaiting).toHaveBeenCalled()
  })

  it('does not answer a failed asset fetch with index.html', async () => {
    const stores = new Map<string, Map<string, Response>>()
    const harness = loadServiceWorker(async (input) => {
      if (requestKey(input) === '/favicon.ico') throw new TypeError('network down')
      return htmlResponse()
    }, stores)

    // Seed a cached shell so the buggy fallback WOULD have something to return.
    await dispatchLifecycle(harness, 'install')

    const result = dispatchFetch(harness, makeRequest('/favicon.ico'))
    expect(result).toBeDefined()
    await expect(result).rejects.toThrow()
  })

  it('does not cache an HTML runtime response under an asset path', async () => {
    const harness = loadServiceWorker(async () => htmlResponse())

    const result = dispatchFetch(harness, makeRequest('/favicon.ico'))
    expect(result).toBeDefined()
    const response = await result!
    // The response passes through to the caller...
    expect(response.headers.get('content-type')).toContain('text/html')
    // ...but must NOT be persisted under the icon path.
    await Promise.resolve() // allow the async cache.put chain to run
    for (const store of harness.stores.values()) {
      expect(store.has('/favicon.ico')).toBe(false)
    }
  })

  it('caches and then serves a valid icon response cache-first', async () => {
    const fetchMock = vi.fn(async () => icoResponse())
    const harness = loadServiceWorker(fetchMock)

    const first = await dispatchFetch(harness, makeRequest('/favicon.ico'))!
    expect(first.headers.get('content-type')).toBe('image/x-icon')
    await new Promise((resolve) => setTimeout(resolve, 0)) // let cache.put settle

    const second = await dispatchFetch(harness, makeRequest('/favicon.ico'))!
    expect(second.headers.get('content-type')).toBe('image/x-icon')
    expect(fetchMock).toHaveBeenCalledTimes(1)
  })

  it('keeps the offline index.html fallback for navigations', async () => {
    const harness = loadServiceWorker(async (input) => {
      const key = requestKey(input)
      if (key === '/' || key === '/index.html') return htmlResponse()
      if (key === '/some/deep/route') throw new TypeError('offline')
      return icoResponse()
    })
    await dispatchLifecycle(harness, 'install')

    const result = dispatchFetch(harness, makeRequest('/some/deep/route', 'navigate'))
    expect(result).toBeDefined()
    const response = await result!
    expect(await response.clone().text()).toContain('spa shell')
  })

  it('does not intercept /api/ requests', () => {
    const harness = loadServiceWorker(async () => htmlResponse())
    const result = dispatchFetch(harness, makeRequest('/api/repo-icon'))
    expect(result).toBeUndefined()
  })

  it('does not intercept non-shell asset paths', () => {
    const harness = loadServiceWorker(async () => htmlResponse())
    const result = dispatchFetch(harness, makeRequest('/assets/index-ABC123.js'))
    expect(result).toBeUndefined()
  })
})
