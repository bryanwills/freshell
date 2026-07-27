# FreshOpenCode Restart Recovery, Snapshot Storms, and Interrupted-Turn Resilience — Implementation Plan

> **For agentic workers:** This plan is executed task-by-task by the
> workflow's execute stage: a fresh implementer per task, with a spec +
> quality review after each task. Steps use checkbox (`- [ ]`) syntax
> for tracking.

**Goal:** Fix kata `zrrj` — after a Freshell restart, FreshOpenCode panes must not storm the snapshot API into global 429 starvation, must reconcile live status instead of stranding busy/idle state, must recover interrupted turns exactly once with audit, must not lose the final assistant answer on the idle race, must emit permanent identity-bearing structured logs, and must be immune to terminal-stream WS backpressure killing lifecycle events.

**Architecture:** Client-side: a module-singleton per-snapshot-key scheduler (single-flight, trailing coalesce, debounce, Retry-After-honoring 429 backoff) that every refresh trigger routes through, plus HTTP-snapshot-driven busy clearing for opencode. Server-side: the opencode adapter's `reconcileStatus` becomes event-emitting and arms exactly one monitored idle-recovery per durable session; serve-manager subscriptions survive sidecar replacement and carry a generation identity; lost-session errors keep their code across the WS boundary with a one-shot attach-recover-retry; interrupted turns are detected from transcript evidence (never a guessed status field) and recovered with one persisted, loop-proof Freshell-owned continuation; idle is only emitted after the final assistant message is proven queryable. Observability rows go to a dedicated always-on pino logger (the main logger drops `info` in production). The terminal-stream broker gains a foreground buffered-amount pause below the WS handler's 2 MiB kill line so terminal floods can no longer drop freshAgent lifecycle frames.

**Tech Stack:** TypeScript (NodeNext/ESM server — relative imports need `.js`), React 18 + Redux Toolkit client, Express + `express-rate-limit@7.5.1`, pino, ws, Vitest + Testing Library + supertest.

## Global Constraints

- Work happens in this worktree (`.worktrees/freshopencode-restart-recovery`), branch `fix/freshopencode-restart-recovery`, based on `origin/main` @ `d99592b4`. One coherent branch/PR (kata `zrrj` is canonical — do NOT split into new katas).
- Red-Green-Refactor TDD for every task. Write the failing test, run it, watch it fail, implement, watch it pass, commit.
- Server files: NodeNext/ESM — **relative imports must include `.js` extensions**.
- Test runs go through the coordinator: `npm run test:vitest -- --run <paths>` for focused runs (add `--config config/vitest/vitest.server.config.ts` for `test/server/**` and `test/integration/server/**` files if they fail to load under the default config); full suite via `npm test` / `npm run check`. Never raw `npx vitest`. Set `FRESHELL_TEST_SUMMARY` for broad runs.
- **Never restart the self-hosted Freshell server. Never use broad kill patterns** (`pkill -f node`, etc.).
- **Do not weaken or bypass the global `/api` rate limit** (300 req/60s at `server/index.ts:205-212`). Response *shaping* and *observability* are allowed; raising/removing limits is not.
- **Never log raw prompts, assistant text, file contents, or full OpenCode payloads.** Identity convention: `provider` + `sessionIdHash` + `cwdHash` via `hashForLogs()` (`server/fresh-agent/observability.ts:7`).
- Git commits use the repo's Amplifier co-author footer:
  ```
  🤖 Generated with [Amplifier](https://github.com/microsoft/amplifier)

  Co-Authored-By: Amplifier <240397093+microsoft-amplifier@users.noreply.github.com>
  ```
- `test/integration/real/` provider contract tests are opt-in and skipped by default — do not require them.
- Line numbers cited below were verified on `d99592b4`. If a cited anchor has drifted, locate the construct by the quoted code, not the number.
- The kata's copied post-restart aggregate counts are approximate — rely on mechanism findings, not exact totals.

## File Structure (new + touched)

**New files:**
- `src/lib/fresh-agent-snapshot-scheduler.ts` — per-key scheduler singleton (single-flight, coalesce, debounce, backoff)
- `test/unit/client/lib/fresh-agent-snapshot-scheduler.test.ts`
- `server/rate-limit.ts` — global limiter factory with JSON 429 body (same limits)
- `test/unit/server/rate-limit.test.ts`
- `server/fresh-agent/adapters/opencode/interrupted-turn.ts` — pure interrupted-turn detector
- `test/unit/server/fresh-agent/opencode-interrupted-turn.test.ts`
- `server/fresh-agent/recovery-store.ts` — durable interrupt-intent + recovery ledger
- `test/unit/server/fresh-agent/recovery-store.test.ts`
- `server/fresh-agent/incident-router.ts` — read-only incident state endpoint
- `test/unit/server/fresh-agent/incident-router.test.ts`
- `test/unit/server/ws-handler-fresh-agent-backpressure.test.ts` — coupling proof

**Modified (primary):**
- `src/lib/api.ts` — `ApiError.retryAfterMs`, snapshot `trigger` param
- `src/components/fresh-agent/FreshAgentView.tsx` — all triggers through the scheduler; 429 backoff UX; opencode busy clear; lost-session resend-once; idle-incomplete re-poll
- `server/index.ts` — use `createApiRateLimiter()`; mount incident router
- `server/logger.ts` — dedicated `freshAgentObservabilityLogger`
- `server/fresh-agent/observability.ts` — sink swap, new event kinds, `/turns` 429 coverage, retryAfter metadata
- `server/fresh-agent/router.ts` — `trigger` query param, `fresh_agent_snapshot_failed`
- `server/fresh-agent/adapters/opencode/serve-manager.ts` — generation/pid/baseUrl; emitters survive replacement; sidecar lifecycle events
- `server/fresh-agent/adapters/opencode/adapter.ts` — reconcile emits; monitored idle recovery; `'lost'` handling; interrupted-turn recovery; idle freshness
- `server/fresh-agent/adapters/opencode/normalize.ts` — evidence fields into `extensions.opencode`
- `server/fresh-agent/runtime-manager.ts` — recovery-store wiring, observability
- `server/ws-handler.ts` — lost-session error code, send/interrupt/attach/materialization log rows
- `server/terminal-stream/broker.ts` + `server/terminal-stream/constants.ts` — foreground backpressure pause

---

### Task 1: Surface `Retry-After` on `ApiError` (client transport)

The server already sends `Retry-After` on every 429 (`express-rate-limit` with `standardHeaders: true`). The client discards `Response.headers` entirely (`src/lib/api.ts:245-247`), so backoff has nothing to work with. Add `retryAfterMs` to `ApiError`.

**Files:**
- Modify: `src/lib/api.ts` (ApiError class ~`:39-57`; `request()` error tail `:245-247`)
- Test: `test/unit/client/lib/api.test.ts` (existing; error-mapping suite at `:846-869`; fetch helpers `:36-50`)

**Interfaces:**
- Consumes: existing `request()` / `ApiError` in `src/lib/api.ts`.
- Produces: `ApiError` gains `readonly retryAfterMs?: number`; exported helper `parseRetryAfterMs(value: string | null | undefined, nowMs?: number): number | undefined`. Task 2's scheduler reads `error.retryAfterMs` when `error.status === 429`.

- [ ] **Step 1: Write the failing tests**

In `test/unit/client/lib/api.test.ts`, inside the existing `describe('api error mapping', ...)` block, add. Note the existing `mockJson`/`mockJsonResponse` helpers return no `headers` — add a local helper:

```ts
function mockResponseWithHeaders(status: number, value: unknown, headers: Record<string, string>) {
  const headerMap = new Map(Object.entries(headers).map(([k, v]) => [k.toLowerCase(), v]))
  mockFetch.mockResolvedValueOnce({
    ok: status >= 200 && status < 300,
    status,
    statusText: 'Too Many Requests',
    text: async () => JSON.stringify(value),
    headers: { get: (name: string) => headerMap.get(name.toLowerCase()) ?? null },
  })
}

it('carries retryAfterMs from a 429 Retry-After seconds header', async () => {
  mockResponseWithHeaders(429, { error: 'Too many requests' }, { 'retry-after': '17' })
  await expect(api.get('/api/fresh-agent/threads/freshopencode/opencode/ses_1')).rejects.toMatchObject({
    status: 429,
    retryAfterMs: 17_000,
  })
})

it('leaves retryAfterMs undefined when the header is absent', async () => {
  mockResponseWithHeaders(429, { error: 'Too many requests' }, {})
  await expect(api.get('/api/x')).rejects.toMatchObject({ status: 429, retryAfterMs: undefined })
})

it('parses an HTTP-date Retry-After into a forward delta', async () => {
  const future = new Date(Date.now() + 30_000).toUTCString()
  mockResponseWithHeaders(429, { error: 'Too many requests' }, { 'retry-after': future })
  const err = await api.get('/api/x').catch((e) => e)
  expect(err.status).toBe(429)
  expect(err.retryAfterMs).toBeGreaterThan(20_000)
  expect(err.retryAfterMs).toBeLessThanOrEqual(31_000)
})
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npm run test:vitest -- --run test/unit/client/lib/api.test.ts`
Expected: the three new tests FAIL (`retryAfterMs` is `undefined` for the seconds case / property missing).

- [ ] **Step 3: Implement**

In `src/lib/api.ts`:

1. Add the parser near `TRANSIENT_HTTP_STATUSES` (`:122`):

```ts
/** Parse a Retry-After header (delta-seconds or HTTP-date) into milliseconds. */
export function parseRetryAfterMs(value: string | null | undefined, nowMs = Date.now()): number | undefined {
  if (!value) return undefined
  const trimmed = value.trim()
  if (/^\d+$/.test(trimmed)) return Number(trimmed) * 1000
  const dateMs = Date.parse(trimmed)
  if (!Number.isFinite(dateMs)) return undefined
  return Math.max(0, dateMs - nowMs)
}
```

2. Extend `ApiError` (`:39-57`) with an optional fourth constructor arg:

```ts
export class ApiError extends Error {
  // ...existing fields...
  readonly retryAfterMs?: number
  constructor(status: number, message: string, details?: unknown, retryAfterMs?: number) {
    // ...existing body...
    this.retryAfterMs = retryAfterMs
  }
}
```

3. In `request()`'s error tail (`:245-247`), read the header before throwing:

```ts
if (!res.ok) {
  const retryAfterMs = res.status === 429
    ? parseRetryAfterMs(typeof res.headers?.get === 'function' ? res.headers.get('retry-after') : undefined)
    : undefined
  throw new ApiError(res.status, getApiErrorMessage(data, res.statusText), data, retryAfterMs)
}
```

The `typeof res.headers?.get === 'function'` guard keeps every existing test (whose fetch mocks have no `headers`) green.

- [ ] **Step 4: Run tests to verify they pass**

Run: `npm run test:vitest -- --run test/unit/client/lib/api.test.ts`
Expected: PASS (all, including pre-existing).

- [ ] **Step 5: Commit**

```bash
git add src/lib/api.ts test/unit/client/lib/api.test.ts
git commit -m "feat(client): surface Retry-After as ApiError.retryAfterMs on 429 (zrrj)"
```

---

### Task 2: Per-snapshot-key scheduler module (single-flight, coalesce, debounce, 429 backoff)

The current mechanism is a per-component 50 ms debounce with no in-flight guard, no shared key, and three of eight triggers bypassing it (`FreshAgentView.tsx:815-828`, `:1495`, `:1065`, `:1930`). N panes on one session produce N GETs per invalidation. Build a module singleton (precedent: `getRebindQueue()`/`resetRebindQueueForTests()` in `src/lib/rebind-queue.ts`) that owns all of this per key.

**Files:**
- Create: `src/lib/fresh-agent-snapshot-scheduler.ts`
- Test: `test/unit/client/lib/fresh-agent-snapshot-scheduler.test.ts`

**Interfaces:**
- Consumes: `ApiError` (with `retryAfterMs`) from `src/lib/api.ts` (Task 1).
- Produces (Task 3 depends on these exact names):

```ts
export type SnapshotTrigger =
  | 'identity' | 'event' | 'send-accepted' | 'materialized'
  | 'manual' | 'poll' | 'reconnect' | 'reveal' | 'idle-incomplete'

export function makeSnapshotKey(input: {
  sessionType: string; provider: string; threadId: string; cwd?: string
}): string   // `${sessionType}:${provider}:${threadId}:${cwd ?? ''}`

export type SnapshotOutcome<T> =
  | { status: 'ok'; value: T }
  | { status: 'rate-limited'; retryAtMs: number }
  | { status: 'backoff'; retryAtMs: number }      // suppressed without a network call
  | { status: 'coalesced' }                        // this call was absorbed by an already-pending run
  | { status: 'error'; error: unknown }

export class FreshAgentSnapshotScheduler {
  schedule<T>(key: string, trigger: SnapshotTrigger, run: () => Promise<T>): Promise<SnapshotOutcome<T>>
  getBackoffUntil(key: string): number | null
}

export function getSnapshotScheduler(): FreshAgentSnapshotScheduler
export function resetSnapshotSchedulerForTests(): void
```

**Semantics (implement exactly):**
- Debounce: triggers `event`, `send-accepted`, `reveal`, `reconnect` wait `SNAPSHOT_DEBOUNCE_MS = 250` before running (a burst within the window coalesces into one run and all callers share that run's outcome). Triggers `identity`, `manual`, `materialized`, `poll`, `idle-incomplete` run immediately (debounce 0).
- Single-flight: at most one in-flight `run()` per key. Calls arriving mid-flight coalesce into **one** trailing run scheduled after the in-flight completes (+debounce); they all share the trailing run's promise. A caller absorbed by an already-pending debounce timer resolves with the shared run's outcome (not `'coalesced'`); `'coalesced'` is returned only when a trailing run is already fully subscribed and this call adds nothing new (keep it simple: everyone sharing a pending run gets that run's outcome; `'coalesced'` is unused in practice but kept in the type for the trailing-overflow case where a trailing run is already queued).
- 429 backoff: when `run()` rejects with `ApiError` status 429, set `backoffUntil = now + (retryAfterMs ?? nextExponential)` where `nextExponential` doubles from `1000` ms capped at `30_000` ms per consecutive 429 on that key; resolve all sharers with `{ status: 'rate-limited', retryAtMs }`. While backoff is active, `schedule()` resolves immediately `{ status: 'backoff', retryAtMs }` with **no network call**. A successful run resets the exponential counter and clears backoff.
- Other rejections resolve `{ status: 'error', error }` (never throw out of `schedule`).

- [ ] **Step 1: Write the failing tests**

Create `test/unit/client/lib/fresh-agent-snapshot-scheduler.test.ts`:

```ts
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { ApiError } from '@/lib/api'
import {
  makeSnapshotKey,
  getSnapshotScheduler,
  resetSnapshotSchedulerForTests,
} from '@/lib/fresh-agent-snapshot-scheduler'

const KEY = makeSnapshotKey({ sessionType: 'freshopencode', provider: 'opencode', threadId: 'ses_1', cwd: '/w' })

describe('FreshAgentSnapshotScheduler', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    resetSnapshotSchedulerForTests()
  })
  afterEach(() => {
    vi.useRealTimers()
  })

  it('builds the key from the exact request tuple', () => {
    expect(KEY).toBe('freshopencode:opencode:ses_1:/w')
    expect(makeSnapshotKey({ sessionType: 'freshopencode', provider: 'opencode', threadId: 'ses_1' }))
      .toBe('freshopencode:opencode:ses_1:')
  })

  it('coalesces an event burst into a single run shared by all callers', async () => {
    const scheduler = getSnapshotScheduler()
    const run = vi.fn(async () => 'snap')
    const p1 = scheduler.schedule(KEY, 'event', run)
    const p2 = scheduler.schedule(KEY, 'event', run)
    const p3 = scheduler.schedule(KEY, 'event', run)
    await vi.advanceTimersByTimeAsync(250)
    const [o1, o2, o3] = await Promise.all([p1, p2, p3])
    expect(run).toHaveBeenCalledTimes(1)
    expect(o1).toEqual({ status: 'ok', value: 'snap' })
    expect(o2).toEqual({ status: 'ok', value: 'snap' })
    expect(o3).toEqual({ status: 'ok', value: 'snap' })
  })

  it('runs manual triggers immediately and holds a single trailing run while in flight', async () => {
    const scheduler = getSnapshotScheduler()
    let release!: () => void
    const gate = new Promise<void>((r) => { release = r })
    const run = vi.fn(async () => { await gate; return 'v' })
    const first = scheduler.schedule(KEY, 'manual', run)
    // three arrivals while in flight -> exactly one trailing run
    const t1 = scheduler.schedule(KEY, 'event', run)
    const t2 = scheduler.schedule(KEY, 'event', run)
    release()
    await first
    await vi.advanceTimersByTimeAsync(250)
    await Promise.all([t1, t2])
    expect(run).toHaveBeenCalledTimes(2)
  })

  it('keys are independent', async () => {
    const scheduler = getSnapshotScheduler()
    const runA = vi.fn(async () => 'a')
    const runB = vi.fn(async () => 'b')
    const otherKey = makeSnapshotKey({ sessionType: 'freshopencode', provider: 'opencode', threadId: 'ses_2', cwd: '/w' })
    const pa = scheduler.schedule(KEY, 'manual', runA)
    const pb = scheduler.schedule(otherKey, 'manual', runB)
    await Promise.all([pa, pb])
    expect(runA).toHaveBeenCalledTimes(1)
    expect(runB).toHaveBeenCalledTimes(1)
  })

  it('honors Retry-After on 429 and suppresses further runs until expiry', async () => {
    const scheduler = getSnapshotScheduler()
    const run = vi.fn(async () => { throw new ApiError(429, 'Too many requests', undefined, 5_000) })
    const first = await scheduler.schedule(KEY, 'manual', run)
    expect(first.status).toBe('rate-limited')
    expect(run).toHaveBeenCalledTimes(1)

    const during = await scheduler.schedule(KEY, 'poll', run)
    expect(during.status).toBe('backoff')
    expect(run).toHaveBeenCalledTimes(1)          // NO network call during backoff
    expect(scheduler.getBackoffUntil(KEY)).toBeGreaterThan(Date.now())

    await vi.advanceTimersByTimeAsync(5_001)
    const ok = vi.fn(async () => 'fresh')
    const after = await scheduler.schedule(KEY, 'poll', ok)
    expect(after).toEqual({ status: 'ok', value: 'fresh' })
    expect(scheduler.getBackoffUntil(KEY)).toBeNull()
  })

  it('falls back to exponential backoff when Retry-After is absent and doubles per consecutive 429', async () => {
    const scheduler = getSnapshotScheduler()
    const run429 = vi.fn(async () => { throw new ApiError(429, 'Too many requests') })
    const o1 = await scheduler.schedule(KEY, 'manual', run429)
    expect(o1.status).toBe('rate-limited')
    if (o1.status !== 'rate-limited') throw new Error('unreachable')
    const firstDelta = o1.retryAtMs - Date.now()
    expect(firstDelta).toBeGreaterThan(0)
    expect(firstDelta).toBeLessThanOrEqual(1_000)

    await vi.advanceTimersByTimeAsync(1_001)
    const o2 = await scheduler.schedule(KEY, 'manual', run429)
    if (o2.status !== 'rate-limited') throw new Error(`expected rate-limited, got ${o2.status}`)
    expect(o2.retryAtMs - Date.now()).toBeGreaterThan(1_000)   // doubled
  })

  it('resolves non-429 failures as error outcomes without backoff', async () => {
    const scheduler = getSnapshotScheduler()
    const boom = new Error('network down')
    const out = await scheduler.schedule(KEY, 'manual', async () => { throw boom })
    expect(out).toEqual({ status: 'error', error: boom })
    expect(scheduler.getBackoffUntil(KEY)).toBeNull()
  })
})
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npm run test:vitest -- --run test/unit/client/lib/fresh-agent-snapshot-scheduler.test.ts`
Expected: FAIL — module does not exist.

- [ ] **Step 3: Implement `src/lib/fresh-agent-snapshot-scheduler.ts`**

```ts
import { ApiError } from '@/lib/api'

export type SnapshotTrigger =
  | 'identity' | 'event' | 'send-accepted' | 'materialized'
  | 'manual' | 'poll' | 'reconnect' | 'reveal' | 'idle-incomplete'

export type SnapshotOutcome<T> =
  | { status: 'ok'; value: T }
  | { status: 'rate-limited'; retryAtMs: number }
  | { status: 'backoff'; retryAtMs: number }
  | { status: 'coalesced' }
  | { status: 'error'; error: unknown }

export const SNAPSHOT_DEBOUNCE_MS = 250
const BACKOFF_BASE_MS = 1_000
const BACKOFF_MAX_MS = 30_000

const DEBOUNCED_TRIGGERS: ReadonlySet<SnapshotTrigger> = new Set(['event', 'send-accepted', 'reveal', 'reconnect'])

export function makeSnapshotKey(input: {
  sessionType: string; provider: string; threadId: string; cwd?: string
}): string {
  return `${input.sessionType}:${input.provider}:${input.threadId}:${input.cwd ?? ''}`
}

type Resolver<T> = (outcome: SnapshotOutcome<T>) => void

type KeyState = {
  inFlight: boolean
  debounceTimer: ReturnType<typeof setTimeout> | null
  /** Callers waiting on the pending (debounced or trailing) run. */
  pendingResolvers: Resolver<any>[]
  /** Latest run fn wins for the pending run. */
  pendingRun: (() => Promise<any>) | null
  trailingRequested: boolean
  backoffUntil: number | null
  consecutive429: number
}

export class FreshAgentSnapshotScheduler {
  private readonly keys = new Map<string, KeyState>()

  private state(key: string): KeyState {
    let s = this.keys.get(key)
    if (!s) {
      s = {
        inFlight: false, debounceTimer: null, pendingResolvers: [],
        pendingRun: null, trailingRequested: false, backoffUntil: null, consecutive429: 0,
      }
      this.keys.set(key, s)
    }
    return s
  }

  getBackoffUntil(key: string): number | null {
    const s = this.keys.get(key)
    if (!s?.backoffUntil) return null
    if (s.backoffUntil <= Date.now()) return null
    return s.backoffUntil
  }

  schedule<T>(key: string, trigger: SnapshotTrigger, run: () => Promise<T>): Promise<SnapshotOutcome<T>> {
    const s = this.state(key)
    const retryAt = this.getBackoffUntil(key)
    if (retryAt !== null) {
      return Promise.resolve({ status: 'backoff', retryAtMs: retryAt })
    }
    return new Promise<SnapshotOutcome<T>>((resolve) => {
      s.pendingResolvers.push(resolve as Resolver<any>)
      s.pendingRun = run
      if (s.inFlight) {
        s.trailingRequested = true
        return
      }
      const delay = DEBOUNCED_TRIGGERS.has(trigger) ? SNAPSHOT_DEBOUNCE_MS : 0
      if (s.debounceTimer !== null) {
        if (delay === 0) {
          clearTimeout(s.debounceTimer)
          s.debounceTimer = null
          void this.execute(key)
        }
        return // burst absorbed into the pending timer
      }
      if (delay === 0) {
        void this.execute(key)
      } else {
        s.debounceTimer = setTimeout(() => {
          s.debounceTimer = null
          void this.execute(key)
        }, delay)
      }
    })
  }

  private async execute(key: string): Promise<void> {
    const s = this.state(key)
    const run = s.pendingRun
    const resolvers = s.pendingResolvers
    s.pendingRun = null
    s.pendingResolvers = []
    if (!run) return
    s.inFlight = true
    let outcome: SnapshotOutcome<any>
    try {
      const value = await run()
      s.consecutive429 = 0
      s.backoffUntil = null
      outcome = { status: 'ok', value }
    } catch (error: unknown) {
      if (error instanceof ApiError && error.status === 429) {
        s.consecutive429 += 1
        const fallback = Math.min(BACKOFF_MAX_MS, BACKOFF_BASE_MS * 2 ** (s.consecutive429 - 1))
        const retryAtMs = Date.now() + (error.retryAfterMs ?? fallback)
        s.backoffUntil = retryAtMs
        // A 429 supersedes any trailing request: suppress until expiry.
        s.trailingRequested = false
        outcome = { status: 'rate-limited', retryAtMs }
      } else {
        outcome = { status: 'error', error }
      }
    }
    s.inFlight = false
    for (const resolve of resolvers) resolve(outcome)
    if (s.trailingRequested && s.pendingRun) {
      s.trailingRequested = false
      s.debounceTimer = setTimeout(() => {
        s.debounceTimer = null
        void this.execute(key)
      }, SNAPSHOT_DEBOUNCE_MS)
    } else {
      s.trailingRequested = false
    }
  }
}

let singleton: FreshAgentSnapshotScheduler | null = null

export function getSnapshotScheduler(): FreshAgentSnapshotScheduler {
  if (!singleton) singleton = new FreshAgentSnapshotScheduler()
  return singleton
}

export function resetSnapshotSchedulerForTests(): void {
  singleton = null
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npm run test:vitest -- --run test/unit/client/lib/fresh-agent-snapshot-scheduler.test.ts`
Expected: PASS.

- [ ] **Step 5: Refactor check** — confirm no `any` leaks into the public API besides the internal `Resolver<any>` plumbing; run the lint pass: `npm run lint -- src/lib/fresh-agent-snapshot-scheduler.ts` (fix if needed).

- [ ] **Step 6: Commit**

```bash
git add src/lib/fresh-agent-snapshot-scheduler.ts test/unit/client/lib/fresh-agent-snapshot-scheduler.test.ts
git commit -m "feat(client): per-key fresh-agent snapshot scheduler with coalesce and 429 backoff (zrrj)"
```

---

### Task 3: Route every FreshAgentView refresh trigger through the scheduler; 429 keeps last-good snapshot

Replace the per-component debounce (`scheduleSnapshotRefresh`, `FreshAgentView.tsx:815-828`) and the three direct nonce bumps (materialization `:1495`, pane refresh `:1065`, poll `:1930`) with a single trigger-tagged path. The fetch effect executes through the scheduler so N panes on one key share one GET. A 429/backoff outcome keeps the last good snapshot visible (no `setLoadError`) and re-arms a retry at `retryAtMs`.

**Files:**
- Modify: `src/components/fresh-agent/FreshAgentView.tsx` (trigger sites `:815-828`, `:1065`, `:1376-1389`, `:1495`, `:1516-1551`, `:1593-1781`, `:1926-1933`)
- Test: `test/unit/client/components/fresh-agent/FreshAgentView.test.tsx` (mock scaffold at `:20-110`)

**Interfaces:**
- Consumes: `getSnapshotScheduler()`, `makeSnapshotKey()`, `SnapshotTrigger`, `resetSnapshotSchedulerForTests()` from `@/lib/fresh-agent-snapshot-scheduler` (Task 2); `getFreshAgentThreadSnapshot(sessionType, provider, threadId, {revision?, cwd?, signal?})` from `@/lib/api`.
- Produces: internal `requestSnapshotRefresh(trigger: SnapshotTrigger)` callback used by all trigger sites; a `snapshotRefreshTriggerRef: React.MutableRefObject<SnapshotTrigger>` consumed by the fetch effect; component state `rateLimitedUntil: number | null` (used by Task 17's `trigger` query param as well).

**Design (implement exactly):**
1. Keep `snapshotRefreshNonce` as the effect driver, but replace `scheduleSnapshotRefresh` with:

```ts
const snapshotRefreshTriggerRef = useRef<SnapshotTrigger>('identity')
const requestSnapshotRefresh = useCallback((trigger: SnapshotTrigger) => {
  snapshotRefreshTriggerRef.current = trigger
  setSnapshotRefreshNonce((value) => value + 1)
}, [])
```

All eight trigger sites call `requestSnapshotRefresh(<trigger>)` with: identity → (effect deps, unchanged), WS invalidating event → `'event'`, send.accepted → `'send-accepted'` (keep the existing `ownsRequest` + `locatorMatchesPane` gates at `:1516-1543` — they are required by the kata's "filter send.accepted refreshes to the owning pane/session"), materialized → `'materialized'`, pane refresh request → `'manual'`, 3s poll → `'poll'`, reconnect → `'reconnect'`, reveal-after-hidden → `'reveal'`. Delete `snapshotRefreshTimerRef` / `snapshotRefreshFollowUpRef` / `SNAPSHOT_REFRESH_COALESCE_MS` and their cleanup (`:830-836`) — debounce now lives in the scheduler.

2. In the fetch effect (`:1593`), wrap the network call:

```ts
const requestCwd = paneContentRef.current.initialCwd
const key = makeSnapshotKey({ sessionType: requestSessionType, provider, threadId: sessionId, cwd: requestCwd })
const trigger = snapshotRefreshTriggerRef.current
void getSnapshotScheduler().schedule(key, trigger, () =>
  getFreshAgentThreadSnapshot(requestSessionType, provider, sessionId, {
    signal: controller.signal,
    ...(requestCwd ? { cwd: requestCwd } : {}),
  }),
).then((outcome) => {
  if (isStaleSnapshotRequest()) return
  if (outcome.status === 'ok') {
    applySnapshot(outcome.value)            // the existing .then body :1618-1707, extracted
    return
  }
  if (outcome.status === 'rate-limited' || outcome.status === 'backoff') {
    // Keep the last good snapshot visible; no error banner. Re-arm one retry at expiry.
    setRateLimitedUntil(outcome.retryAtMs)
    const delay = Math.max(0, outcome.retryAtMs - Date.now())
    rateLimitRetryTimerRef.current = window.setTimeout(() => {
      rateLimitRetryTimerRef.current = null
      setRateLimitedUntil(null)
      requestSnapshotRefresh('manual')
    }, delay + 50)
    return
  }
  if (outcome.status === 'coalesced') return
  handleSnapshotError(outcome.error)        // the existing .catch body :1708-1752, extracted
})
```

Extract the existing `.then` body into `applySnapshot(next)` and the `.catch` body into `handleSnapshotError(error)` (same closure variables — extraction, not rewrite). Add `rateLimitRetryTimerRef = useRef<number | null>(null)` with clear-on-unmount, and dedupe: if a retry timer is already armed, don't arm a second.

3. Key derivation hazard (report §3): the key MUST be built from the exact request tuple (`initialCwd`), not `freshOpenCodeRouteCwd`.

- [ ] **Step 1: Write the failing tests**

In `test/unit/client/components/fresh-agent/FreshAgentView.test.tsx`, add to the file-top scaffold: `import { resetSnapshotSchedulerForTests } from '@/lib/fresh-agent-snapshot-scheduler'` and call it in the file's `beforeEach`. The scheduler is real (not mocked) — only `@/lib/api` stays mocked. Add a new `describe('snapshot scheduler integration', ...)` block modeled on the existing session.changed test at `:3828+` (same store/render/wsHandler-capture pattern):

```ts
describe('snapshot scheduler integration (zrrj)', () => {
  it('coalesces a burst of freshopencode session.changed events into one snapshot GET', async () => {
    // Arrange: render one freshopencode pane bound to ses_1 (copy the arrange from the
    // ':3828' analogue, provider 'opencode', sessionType 'freshopencode', sessionId 'ses_1').
    apiMock.getFreshAgentThreadSnapshot.mockResolvedValue(makeSnapshot({ threadId: 'ses_1' }))
    render(<StoreBackedFreshAgentView ... />)
    await waitFor(() => expect(apiMock.getFreshAgentThreadSnapshot).toHaveBeenCalledTimes(1)) // identity fetch
    apiMock.getFreshAgentThreadSnapshot.mockClear()

    // Act: 10 invalidations in a burst
    for (let i = 0; i < 10; i++) {
      act(() => wsHandler({
        type: 'freshAgent.event', sessionId: 'ses_1', sessionType: 'freshopencode', provider: 'opencode',
        event: { type: 'freshAgent.session.changed', sessionId: 'ses_1' },
      }))
    }
    // Assert: exactly one trailing GET, not ten
    await waitFor(() => expect(apiMock.getFreshAgentThreadSnapshot).toHaveBeenCalledTimes(1))
    await new Promise((r) => setTimeout(r, 400))
    expect(apiMock.getFreshAgentThreadSnapshot).toHaveBeenCalledTimes(1)
  })

  it('keeps the last good snapshot visible and stops fetching during 429 backoff', async () => {
    apiMock.getFreshAgentThreadSnapshot.mockResolvedValueOnce(makeSnapshot({ threadId: 'ses_1', turns: [turnWithText('hello world')] }))
    render(<StoreBackedFreshAgentView ... />)
    await screen.findByText('hello world')
    apiMock.getFreshAgentThreadSnapshot.mockRejectedValue(new ApiError(429, 'Too many requests', undefined, 60_000))

    act(() => wsHandler(sessionChangedFor('ses_1')))
    await waitFor(() => expect(apiMock.getFreshAgentThreadSnapshot).toHaveBeenCalledTimes(2))
    // transcript still visible, no load error
    expect(screen.getByText('hello world')).toBeInTheDocument()
    expect(screen.queryByText(/Too many requests/)).not.toBeInTheDocument()

    // further invalidations are suppressed without network
    act(() => wsHandler(sessionChangedFor('ses_1')))
    await new Promise((r) => setTimeout(r, 400))
    expect(apiMock.getFreshAgentThreadSnapshot).toHaveBeenCalledTimes(2)
  })

  it('does not refetch when another session sends (send.accepted for a foreign request)', async () => {
    // existing gate regression: a send.accepted with an unowned requestId must trigger zero GETs
    apiMock.getFreshAgentThreadSnapshot.mockResolvedValue(makeSnapshot({ threadId: 'ses_1' }))
    render(<StoreBackedFreshAgentView ... />)
    await waitFor(() => expect(apiMock.getFreshAgentThreadSnapshot).toHaveBeenCalledTimes(1))
    apiMock.getFreshAgentThreadSnapshot.mockClear()
    act(() => wsHandler({
      type: 'freshAgent.send.accepted', requestId: 'someone-elses-request',
      sessionId: 'ses_1', sessionType: 'freshopencode', provider: 'opencode',
    }))
    await new Promise((r) => setTimeout(r, 400))
    expect(apiMock.getFreshAgentThreadSnapshot).not.toHaveBeenCalled()
  })
})
```

Use the file's existing helpers for arranging pane content and snapshots (`makeSnapshot`-style builders exist near the top of the file; if named differently, reuse whatever the `:3828` analogue uses — do not invent parallel builders). `ApiError` is imported from the actual module: `import { ApiError } from '@/lib/api'` (the `vi.mock('@/lib/api')` uses `importActual` spread, so the real class is preserved).

- [ ] **Step 2: Run to verify the new tests fail**

Run: `npm run test:vitest -- --run test/unit/client/components/fresh-agent/FreshAgentView.test.tsx`
Expected: burst test FAILS (multiple GETs today); 429 test FAILS (load error shown, refetches continue).

- [ ] **Step 3: Implement the integration** as specified in the Design block above.

- [ ] **Step 4: Run to verify all pass**

Run: `npm run test:vitest -- --run test/unit/client/components/fresh-agent/FreshAgentView.test.tsx test/unit/client/lib/fresh-agent-snapshot-scheduler.test.ts`
Expected: PASS, including all pre-existing FreshAgentView tests (the refactor must not regress the `:3828` "refreshes freshopencode on session.changed without reopening the bouncer" behavior — one trailing refresh still happens, just debounced 250 ms).

- [ ] **Step 5: Run sibling suites that share the scaffold**

Run: `npm run test:vitest -- --run test/unit/client/components/fresh-agent/`
Expected: PASS (`FreshAgentView.reconcile.test.tsx`, `FreshAgentView.hidden-rebind.test.tsx` unaffected or updated for the new trigger API).

- [ ] **Step 6: Commit**

```bash
git add src/components/fresh-agent/FreshAgentView.tsx test/unit/client/components/fresh-agent/
git commit -m "feat(client): route all snapshot refresh triggers through the per-key scheduler with 429 backoff (zrrj)"
```

---

### Task 4: A successful idle HTTP snapshot clears stale opencode busy state

Today the HTTP snapshot result writes `setSessionStatus` only for `provider === 'codex' && sessionType === 'freshcodex'` (`FreshAgentView.tsx:1670-1687`). For opencode, busy is only cleared by a WS event — if that event was dropped (restart, backpressure), the pane stays busy and the 3s poll runs forever. Extend the same guarded write to freshopencode.

**Files:**
- Modify: `src/components/fresh-agent/FreshAgentView.tsx:1670-1687` (inside `applySnapshot` after Task 3's extraction)
- Test: `test/unit/client/components/fresh-agent/FreshAgentView.test.tsx`

**Interfaces:**
- Consumes: `setSessionStatus` action from `src/store/freshAgentSlice.ts`; snapshot `status` field (`'running' | 'idle'` from `normalizeOpencodeSnapshot`).
- Produces: no new API — behavior change only.

- [ ] **Step 1: Write the failing test**

```ts
it('clears stale opencode busy state from a successful idle HTTP snapshot', async () => {
  // Arrange: store preloaded with freshAgent session status 'running' for
  // key {sessionId: 'ses_1', sessionType: 'freshopencode', provider: 'opencode'}
  // (use the store scaffold's preloadedState like the codex analogue test does).
  apiMock.getFreshAgentThreadSnapshot.mockResolvedValue(makeSnapshot({ threadId: 'ses_1', status: 'idle' }))
  const { store } = renderWithStore(...)   // the file's store-returning arrange helper
  await waitFor(() => expect(apiMock.getFreshAgentThreadSnapshot).toHaveBeenCalled())
  await waitFor(() => {
    const key = makeFreshAgentSessionKey({ sessionId: 'ses_1', sessionType: 'freshopencode', provider: 'opencode' })
    expect(store.getState().freshAgent.sessions[key]?.status).toBe('idle')
  })
})
```

Find the existing codex analogue (search the test file for `setSessionStatus` or `freshcodex.*idle`) and mirror its arrange/assert precisely for opencode.

- [ ] **Step 2: Run to verify it fails**

Run: `npm run test:vitest -- --run test/unit/client/components/fresh-agent/FreshAgentView.test.tsx -t "clears stale opencode busy"`
Expected: FAIL — status stays `'running'`.

- [ ] **Step 3: Implement**

At `:1670-1687`, generalize the provider gate:

```ts
const canAdoptSnapshotStatus =
  (provider === 'codex' && requestSessionType === 'freshcodex')
  || (provider === 'opencode' && requestSessionType === 'freshopencode')
if (
  sessionStatus
  && nextSessionId
  && canAdoptSnapshotStatus
  && !wouldRegressStatus
  && (
    snapshotIsBusy
    || (!hasBlockingLocalEchoForSession && !statusChangedSinceRequest)
  )
) {
  dispatch(setSessionStatus({
    sessionId: nextSessionId,
    sessionType: requestSessionType,
    provider,
    status: sessionStatus,
  }))
}
```

All existing guards (`wouldRegressStatus`, `statusChangedSinceRequest` via `agentSessionStatusVersionRef`, `hasBlockingLocalEchoForSession`) stay — they are the race protection that made the codex version safe.

- [ ] **Step 4: Run to verify pass**

Run: `npm run test:vitest -- --run test/unit/client/components/fresh-agent/FreshAgentView.test.tsx`
Expected: PASS (including the codex analogue tests).

- [ ] **Step 5: Commit**

```bash
git add src/components/fresh-agent/FreshAgentView.tsx test/unit/client/components/fresh-agent/FreshAgentView.test.tsx
git commit -m "fix(client): let idle HTTP snapshots clear stale freshopencode busy state (zrrj)"
```

---

### Task 5: Global limiter factory with JSON 429 body (same limits) + limiter test coverage

The limiter currently returns a plain-text 429 body while every other error in the codebase is JSON, and it has zero test coverage because it's configured inline in `main()` (`server/index.ts:204-212`). Extract a factory with an identical budget and a JSON handler that includes `retryAfterSeconds`. **The budget (300/60s) must not change.**

**Files:**
- Create: `server/rate-limit.ts`
- Modify: `server/index.ts:204-212` (use the factory)
- Test: `test/unit/server/rate-limit.test.ts`

**Interfaces:**
- Consumes: `express-rate-limit` v7.5.1 (`standardHeaders: true` already sets `Retry-After` + `RateLimit-*` on 429s).
- Produces: `createApiRateLimiter(options?: { windowMs?: number; max?: number })` returning Express middleware. 429 body: `{ error: 'Too many requests', code: 'RATE_LIMITED', retryAfterSeconds: number }`. Options exist ONLY so tests can use a tiny budget; production call passes nothing.

- [ ] **Step 1: Write the failing test**

Create `test/unit/server/rate-limit.test.ts`:

```ts
import { describe, it, expect } from 'vitest'
import express from 'express'
import request from 'supertest'
import { createApiRateLimiter } from '../../../server/rate-limit.js'

function makeApp(limiter = createApiRateLimiter({ windowMs: 60_000, max: 2 })) {
  const app = express()
  app.use('/api', limiter)
  app.get('/api/ping', (_req, res) => { res.json({ ok: true }) })
  return app
}

describe('createApiRateLimiter', () => {
  it('defaults to the production budget of 300 per 60s', () => {
    // Guard against accidental weakening: the factory must default to 300/60_000.
    const limiter = createApiRateLimiter() as unknown as { options?: unknown }
    // express-rate-limit v7 middleware does not expose options; assert via the
    // exported constants instead:
    expect(API_RATE_LIMIT_WINDOW_MS).toBe(60_000)
    expect(API_RATE_LIMIT_MAX).toBe(300)
    expect(typeof limiter).toBe('function')
  })

  it('returns a JSON 429 with code RATE_LIMITED and retryAfterSeconds, plus Retry-After header', async () => {
    const app = makeApp()
    await request(app).get('/api/ping').expect(200)
    await request(app).get('/api/ping').expect(200)
    const res = await request(app).get('/api/ping').expect(429)
    expect(res.headers['retry-after']).toMatch(/^\d+$/)
    expect(res.body).toMatchObject({ error: 'Too many requests', code: 'RATE_LIMITED' })
    expect(res.body.retryAfterSeconds).toBe(Number(res.headers['retry-after']))
    expect(res.type).toMatch(/json/)
  })
})
```

(Also import `API_RATE_LIMIT_WINDOW_MS`, `API_RATE_LIMIT_MAX` from the same module.)

- [ ] **Step 2: Run to verify it fails**

Run: `npm run test:vitest -- --run test/unit/server/rate-limit.test.ts`
Expected: FAIL — module missing.

- [ ] **Step 3: Implement `server/rate-limit.ts`**

```ts
import rateLimit from 'express-rate-limit'
import type { Request, Response } from 'express'

export const API_RATE_LIMIT_WINDOW_MS = 60_000
export const API_RATE_LIMIT_MAX = 300

/**
 * Global /api rate limiter. Budget is intentionally identical to the previous
 * inline configuration (300 req / 60 s / IP) — do NOT raise it here; the fix
 * for snapshot storms is client-side scheduling + backoff, not a bigger budget.
 * This factory only adds a JSON 429 body (with retryAfterSeconds) and testability.
 */
export function createApiRateLimiter(options: { windowMs?: number; max?: number } = {}) {
  return rateLimit({
    windowMs: options.windowMs ?? API_RATE_LIMIT_WINDOW_MS,
    max: options.max ?? API_RATE_LIMIT_MAX,
    standardHeaders: true,
    legacyHeaders: false,
    handler: (_req: Request, res: Response) => {
      const retryAfterHeader = res.getHeader('Retry-After')
      const retryAfterSeconds = Number(retryAfterHeader) || Math.ceil((options.windowMs ?? API_RATE_LIMIT_WINDOW_MS) / 1000)
      res.status(429).json({ error: 'Too many requests', code: 'RATE_LIMITED', retryAfterSeconds })
    },
  })
}
```

In `server/index.ts`, replace the inline block at `:204-212` with:

```ts
import { createApiRateLimiter } from './rate-limit.js'
// ...
app.use('/api', createApiRateLimiter())
```

and remove the now-unused `import rateLimit from 'express-rate-limit'` at `:11` if nothing else uses it.

- [ ] **Step 4: Run to verify pass**

Run: `npm run test:vitest -- --run test/unit/server/rate-limit.test.ts`
Expected: PASS. Then `npm run check` typecheck portion (or `npx tsc -p . --noEmit` equivalent used by the repo's `check` script) to confirm `server/index.ts` compiles.

- [ ] **Step 5: Commit**

```bash
git add server/rate-limit.ts server/index.ts test/unit/server/rate-limit.test.ts
git commit -m "feat(server): JSON 429 body with retryAfterSeconds on the global /api limiter, same budget (zrrj)"
```

---

### Task 6: Always-on fresh-agent observability logger, expanded event union, and full 429 route coverage

Two production defects in the observability layer: (1) the sink is the main `logger`, which sits at level `warn` with the Debug toggle off — so the already-shipped `fresh_agent_snapshot_served` / `fresh_agent_opencode_status_observed` `info` rows are **invisible in production** (`server/fresh-agent/observability.ts:44-100`, `server/logger.ts:20,218-220`); (2) `SNAPSHOT_PATH_PATTERN` (`observability.ts:102`) only matches the 3-segment snapshot path, so `/turns` and `/turns/:turnId` 429s are never recorded. Fix both and add the event kinds the rest of this plan emits.

**Files:**
- Modify: `server/logger.ts` (new dedicated logger, factory pattern at `:210-216`)
- Modify: `server/fresh-agent/observability.ts`
- Test: `test/unit/server/fresh-agent/observability.test.ts` (existing patterns at `:1-50`)

**Interfaces:**
- Consumes: `createSessionLifecycleLogger`-style factory in `server/logger.ts:210-216`; `sessionLifecycleLogger` precedent (`:314-317`).
- Produces:
  - `server/logger.ts`: `export const freshAgentObservabilityLogger` — pino instance pinned at `level: 'info'` writing `~/.freshell/logs/fresh-agent.<mode>.<instance>.jsonl` (10 MB × 10, same rotation options as sessionLifecycle; returns a silent logger under test runtime exactly like the existing factory does).
  - `server/fresh-agent/observability.ts`: default sink becomes `freshAgentObservabilityLogger`; union extended with these kinds (all payloads identity-hashed, **no message content**):

```ts
| { kind: 'fresh_agent_send'; sessionType: string; provider: string; sessionIdHash: string; cwdHash?: string; requestId?: string; outcome: 'accepted' | 'failed'; errorCode?: string; durationMs?: number }
| { kind: 'fresh_agent_interrupt'; sessionType: string; provider: string; sessionIdHash: string; cwdHash?: string; outcome: 'ok' | 'failed'; errorCode?: string }
| { kind: 'fresh_agent_attach'; sessionType: string; provider: string; sessionIdHash: string; cwdHash?: string; outcome: 'ok' | 'failed'; errorCode?: string; recovered?: boolean }
| { kind: 'fresh_agent_materialized'; sessionType: string; provider: string; previousSessionIdHash: string; sessionIdHash: string; cwdHash?: string }
| { kind: 'fresh_agent_monitor'; provider: 'opencode'; sessionIdHash: string; cwdHash?: string; phase: 'armed' | 'resolved_idle' | 'timeout' | 'sidecar_lost' | 'duplicate_suppressed'; sidecarGeneration?: number }
| { kind: 'fresh_agent_sidecar'; provider: 'opencode'; phase: 'started' | 'exited' | 'discarded'; generation: number; pid?: number; baseUrl?: string; reason?: string; code?: number | null; signal?: string | null }
| { kind: 'fresh_agent_snapshot_failed'; sessionType: string; provider: string; threadIdHash?: string; httpStatus: number; code?: string; durationMs?: number; trigger?: string; cwdHash?: string }
| { kind: 'fresh_agent_turn_recovery'; provider: 'opencode'; sessionIdHash: string; cwdHash?: string; action: 'continuation_injected' | 'suppressed_user_stop' | 'suppressed_user_followup' | 'suppressed_already_recovered' | 'suppressed_low_confidence'; reason: string; messageIdHash?: string }
```

  - `fresh_agent_snapshot_served` gains optional `trigger?: string`; `fresh_agent_snapshot_rate_limited` gains optional `retryAfterSeconds?: number; trigger?: string` and its route pattern covers all three thread routes.
  - Severity rule extended: `fresh_agent_snapshot_rate_limited`, `fresh_agent_sidecar` with `phase !== 'started'`, `fresh_agent_monitor` with `phase: 'timeout' | 'sidecar_lost'`, and `fresh_agent_snapshot_failed` log at `warn`; everything else at `info`.

- [ ] **Step 1: Write the failing tests**

Extend `test/unit/server/fresh-agent/observability.test.ts` (reuse its `__setFreshAgentObservabilitySinkForTest` seam and supertest middleware harness):

```ts
it('records 429s on the /turns and /turns/:turnId routes with retryAfterSeconds', async () => {
  const app = express()
  app.use('/fresh-agent/threads', createFreshAgentSnapshotRateLimitMiddleware())
  app.get('*', (_req, res) => { res.setHeader('Retry-After', '42'); res.status(429).end() })
  await request(app).get('/fresh-agent/threads/freshopencode/opencode/ses_abc/turns').expect(429)
  await request(app).get('/fresh-agent/threads/freshopencode/opencode/ses_abc/turns/turn_1').expect(429)
  expect(warnSpy).toHaveBeenCalledTimes(2)
  const first = warnSpy.mock.calls[0][0]
  expect(first).toMatchObject({
    event: 'fresh_agent_snapshot_rate_limited',
    sessionType: 'freshopencode',
    provider: 'opencode',
    retryAfterSeconds: 42,
  })
  expect(first.threadIdHash).toBe(hashForLogs('ses_abc'))
})

it('routes info events through the dedicated fresh-agent logger by default', async () => {
  // Reset the sink override, then spy on freshAgentObservabilityLogger directly.
  // (vi.mock server/logger.js already exists at the top of this file — add
  //  freshAgentObservabilityLogger: { info: vi.fn(), warn: vi.fn() } to the mock factory
  //  and assert recordFreshAgentObservabilityEvent hits IT, not `logger`.)
})

it('accepts the new event kinds with warn severity for incident kinds', () => {
  recordFreshAgentObservabilityEvent({
    kind: 'fresh_agent_monitor', provider: 'opencode',
    sessionIdHash: hashForLogs('ses_x'), phase: 'sidecar_lost', sidecarGeneration: 3,
  })
  expect(warnSpy).toHaveBeenCalledWith(
    expect.objectContaining({ event: 'fresh_agent_monitor', phase: 'sidecar_lost' }),
    'fresh_agent_monitor',
  )
  recordFreshAgentObservabilityEvent({
    kind: 'fresh_agent_send', sessionType: 'freshopencode', provider: 'opencode',
    sessionIdHash: hashForLogs('ses_x'), outcome: 'accepted',
  })
  expect(infoSpy).toHaveBeenCalledWith(
    expect.objectContaining({ event: 'fresh_agent_send', outcome: 'accepted' }),
    'fresh_agent_send',
  )
})
```

- [ ] **Step 2: Run to verify they fail**

Run: `npm run test:vitest -- --run test/unit/server/fresh-agent/observability.test.ts`
Expected: FAIL — `/turns` rows not recorded; new kinds rejected by TS; sink still main logger.

- [ ] **Step 3: Implement**

1. `server/logger.ts` — beside `sessionLifecycleLogger` (`:314-317`), using the same factory shape as `createSessionLifecycleLogger` (`:210-216`):

```ts
export const freshAgentObservabilityLogger = createDedicatedFileLogger({
  name: 'fresh-agent',
  level: 'info',
  maxSize: '10M',
  maxFiles: 10,
})
```

If the existing factory is literally `createSessionLifecycleLogger()` (hardcoded name), generalize it minimally: rename its internals to `createDedicatedFileLogger({ name, ... })` and re-express `sessionLifecycleLogger` through it — behavior-preserving refactor, existing session-observability tests must stay green.

2. `server/fresh-agent/observability.ts`:
   - `import { freshAgentObservabilityLogger } from '../logger.js'` and set `const defaultSink: FreshAgentObservabilitySink = freshAgentObservabilityLogger`.
   - Add the new event kinds to the union exactly as specified in Interfaces.
   - Extend the severity split in `recordFreshAgentObservabilityEvent`:

```ts
const WARN_KINDS = new Set(['fresh_agent_snapshot_rate_limited', 'fresh_agent_snapshot_failed'])
function isWarnEvent(event: FreshAgentObservabilityEvent): boolean {
  if (WARN_KINDS.has(event.kind)) return true
  if (event.kind === 'fresh_agent_sidecar') return event.phase !== 'started'
  if (event.kind === 'fresh_agent_monitor') return event.phase === 'timeout' || event.phase === 'sidecar_lost'
  return false
}
```

   - Replace `SNAPSHOT_PATH_PATTERN` and enrich the middleware:

```ts
const SNAPSHOT_PATH_PATTERN = /^\/([^/]+)\/([^/]+)\/([^/]+)(?:\/turns(?:\/[^/]+)?)?$/

export function createFreshAgentSnapshotRateLimitMiddleware() {
  return (req: Request, res: Response, next: NextFunction) => {
    const match = SNAPSHOT_PATH_PATTERN.exec(req.path)
    if (match) {
      const [, sessionType, provider, threadId] = match
      const trigger = typeof req.query.trigger === 'string' ? req.query.trigger.slice(0, 32) : undefined
      res.on('finish', () => {
        if (res.statusCode === 429) {
          const retryAfterSeconds = Number(res.getHeader('Retry-After')) || undefined
          recordFreshAgentObservabilityEvent({
            kind: 'fresh_agent_snapshot_rate_limited',
            sessionType, provider,
            threadIdHash: hashForLogs(threadId),
            httpStatus: 429,
            route: `/fresh-agent/threads/${sessionType}/${provider}/:threadId`,
            ...(retryAfterSeconds !== undefined ? { retryAfterSeconds } : {}),
            ...(trigger ? { trigger } : {}),
          })
        }
      })
    }
    next()
  }
}
```

- [ ] **Step 4: Run to verify pass**

Run: `npm run test:vitest -- --run test/unit/server/fresh-agent/observability.test.ts test/unit/server/session-observability.test.ts`
Expected: PASS (both — the logger refactor must not break session lifecycle logging).

- [ ] **Step 5: Commit**

```bash
git add server/logger.ts server/fresh-agent/observability.ts test/unit/server/fresh-agent/observability.test.ts
git commit -m "feat(server): always-on fresh-agent observability logger, expanded event kinds, /turns 429 coverage (zrrj)"
```

---

### Task 7: Serve-manager: sidecar generation identity + subscriptions survive sidecar replacement

Two defects in `server/fresh-agent/adapters/opencode/serve-manager.ts`: (1) `emitLostForAllSessions()` clears `sessionEmitters` (`serve-manager.ts:126-132`, clear at `:128`), so every adapter listener is orphaned on the dead emitter object while a replacement sidecar dispatches to brand-new emitters — live events are lost forever after any sidecar replacement; (2) there is no generation/PID identity (`RunningServe` at `:61-66` holds only `baseUrl`/`ownershipId`/`child`/`stopEventStream`), so nothing can attribute a monitored wait or a log row to a specific sidecar. The kata offers a choice ("survive replacement, OR invalidate and rebind"); choose **survive**: keep the emitter objects in the map across replacement, emit `'lost'` on them, and let the replacement sidecar dispatch into the same objects. Prove it with tests.

**Files:**
- Modify: `server/fresh-agent/adapters/opencode/serve-manager.ts`
- Test: `test/unit/server/fresh-agent/opencode-serve-manager.test.ts` (harness `makeManager`/`fakeChild`/`jsonResponse` at `:8-45`)

**Interfaces:**
- Consumes: existing `OpencodeServeManager` internals: `start()` `:196-261`, `discardRunning()` `:134-143`, child `'close'` handler `:220-230`, `emitterFor()` `:419-427`, `subscribe()` `:434-438`, `onceIdle()` `:440-520`.
- Produces (Tasks 8, 17, 18 depend on these):
  - `describeSidecar(): { generation: number; pid?: number; baseUrl: string; ownershipId: string } | undefined` — current running sidecar, else `undefined`.
  - `currentGeneration(): number` — monotonically increasing, bumped on every successful `start()`; `0` before first start.
  - `'lost'` events now carry the generation: `emitter.emit('lost', new OpencodeServeLostError(sessionId), { generation })` — existing consumers that only read the first arg keep working.
  - `recordFreshAgentObservabilityEvent({ kind: 'fresh_agent_sidecar', ... })` emitted on `started` (after health gate), `exited` (child close), `discarded` (discardRunning) with `generation`, `pid`, `baseUrl`, `reason`/`code`/`signal`.

- [ ] **Step 1: Write the failing tests**

Add to `test/unit/server/fresh-agent/opencode-serve-manager.test.ts`:

```ts
describe('sidecar replacement resilience (zrrj)', () => {
  it('keeps session subscriptions alive across sidecar replacement', async () => {
    const manager = makeManager()
    const seen: unknown[] = []
    manager.subscribe('ses_1', (ev) => seen.push(ev))
    await manager.ensureStarted()
    // Kill the sidecar (fakeChild.kill() emits 'close') -> replacement on next call
    // then dispatch an event as the REPLACEMENT sidecar would.
    ;(manager as any).running.child.kill()
    await manager.ensureStarted()
    ;(manager as any).dispatchEvent({ kind: 'session.idle', sessionId: 'ses_1', properties: {}, raw: {} })
    expect(seen).toHaveLength(1)   // FAILS today: listener orphaned on cleared emitter
  })

  it('still emits lost to onceIdle waiters when the sidecar dies', async () => {
    const manager = makeManager()
    await manager.ensureStarted()
    const idle = manager.onceIdle('ses_1', 5_000)
    ;(manager as any).running.child.kill()
    await expect(idle).rejects.toBeInstanceOf(OpencodeServeLostError)
  })

  it('increments generation per start and exposes pid/baseUrl via describeSidecar', async () => {
    const manager = makeManager()
    expect(manager.currentGeneration()).toBe(0)
    expect(manager.describeSidecar()).toBeUndefined()
    await manager.ensureStarted()
    expect(manager.currentGeneration()).toBe(1)
    const desc = manager.describeSidecar()
    expect(desc).toMatchObject({ generation: 1, baseUrl: expect.stringContaining('http://127.0.0.1:') })
    ;(manager as any).running.child.kill()
    await manager.ensureStarted()
    expect(manager.currentGeneration()).toBe(2)
  })
})
```

Adjust the private-access spelling (`(manager as any).running` / `dispatchEvent`) to whatever the existing suite already uses to reach internals — this file already drives the real class with injected doubles; reuse its idioms (e.g. if existing tests trigger death via the injected `fetchFn` timeout instead, do the same).

- [ ] **Step 2: Run to verify the replacement test fails**

Run: `npm run test:vitest -- --run test/unit/server/fresh-agent/opencode-serve-manager.test.ts`
Expected: "keeps session subscriptions alive across sidecar replacement" FAILS (0 events seen); generation tests FAIL (methods missing).

- [ ] **Step 3: Implement**

In `serve-manager.ts`:

1. Add fields and accessors:

```ts
private generation = 0

currentGeneration(): number { return this.generation }

describeSidecar(): { generation: number; pid?: number; baseUrl: string; ownershipId: string } | undefined {
  if (!this.running) return undefined
  return {
    generation: this.generation,
    pid: this.running.child.pid ?? undefined,
    baseUrl: this.running.baseUrl,
    ownershipId: this.running.ownershipId,
  }
}
```

2. In `start()` after the health gate passes (`waitForHealth`, `:232` area) and `this.running` is assigned: `this.generation += 1`, then emit:

```ts
recordFreshAgentObservabilityEvent({
  kind: 'fresh_agent_sidecar', provider: 'opencode', phase: 'started',
  generation: this.generation, pid: child.pid ?? undefined, baseUrl,
})
```

3. In the child `'close'` handler (`:220-230`) add `phase: 'exited'` with `{ generation: this.generation, code, signal }`; in `discardRunning(reason)` (`:134-143`) add `phase: 'discarded'` with `{ generation: this.generation, reason }`.

4. Fix `emitLostForAllSessions()` — **do not clear the map**:

```ts
private emitLostForAllSessions(): void {
  const generation = this.generation
  for (const [sessionId, emitter] of this.sessionEmitters.entries()) {
    emitter.emit('lost', new OpencodeServeLostError(sessionId), { generation })
  }
}
```

Memory note: emitters are already created lazily per session id and were only ever GC'd via this clear; to avoid unbounded growth add cleanup to the existing unsubscribe closure in `subscribe()` (`:434-438`): after `emitter.off('event', listener)`, if `emitter.listenerCount('event') === 0 && emitter.listenerCount('lost') === 0`, delete the map entry.

5. `onceIdle`'s `'lost'` handler (`:516-518`) keeps working unchanged (extra emit arg ignored).

- [ ] **Step 4: Run to verify pass**

Run: `npm run test:vitest -- --run test/unit/server/fresh-agent/opencode-serve-manager.test.ts`
Expected: PASS, including all pre-existing lifecycle tests.

- [ ] **Step 5: Commit**

```bash
git add server/fresh-agent/adapters/opencode/serve-manager.ts test/unit/server/fresh-agent/opencode-serve-manager.test.ts
git commit -m "fix(server): opencode serve subscriptions survive sidecar replacement; add generation/pid identity (zrrj)"
```

---

### Task 8: Adapter: reconcile emits status + exactly one monitored idle-recovery per durable session + structured interruption on loss

Gaps G1/G2/G5 (`server/fresh-agent/adapters/opencode/adapter.ts`): `reconcileStatus` (`:155-200`) silently sets `state.status = 'running'` without `emitStatus()` — the client never learns; nothing re-arms an idle waiter for a restored running session — no idle/turn-complete ever arrives; the adapter never listens for `'lost'` — sidecar loss leaves restored panes busy forever.

**Files:**
- Modify: `server/fresh-agent/adapters/opencode/adapter.ts`
- Test: `test/unit/server/fresh-agent/opencode-serve-adapter.test.ts` (harness `makeFakeManager`/`makeAdapter`/`createDeferred` at `:32-87`)

**Interfaces:**
- Consumes: `serveManager.onceIdle(realId, timeoutMs, route?)`, `serveManager.getSessionStatus`, `serveManager.subscribe`, `serveManager.currentGeneration()` (Task 7); `emitStatus(state, status)` `:301-312`; `nextMonotonicTurnCompleteAt` (`turn-complete-clock.ts`); `recordFreshAgentObservabilityEvent` (Task 6).
- Produces:
  - `reconcileStatus` calls `emitStatus(state, 'running' | 'idle')` instead of mutating silently (so `sdk.session.snapshot` reaches the client on restore).
  - Module-scope `const idleRecoveryMonitors = new Map<string, Promise<void>>()` keyed by real `ses_` id — the "exactly one monitored idle-recovery per durable session key" registry.
  - `armIdleRecovery(state: OpencodeSessionState): void` — no-op if a monitor for `state.realSessionId` exists (`fresh_agent_monitor` `duplicate_suppressed`); otherwise arms `onceIdle(realId, DEFAULT_TURN_TIMEOUT_MS, route)`:
    - resolve → `emitStatus(state, 'idle')`; emit `sdk.turn.complete` **only** when `!state.turnAborted && !state.turnErrored` (restored sessions: both `undefined` → falsy → chime allowed; the pre-restart error is unobservable and OpenCode reporting busy→idle is the best signal we have — matches the prior wave's constraint "do not claim recovery unless OpenCode reports busy/retry", which gated *arming*, not the chime); monitor row `resolved_idle`.
    - reject (timeout / `OpencodeServeLostError`) → `emitStatus(state, 'idle')` + `state.events.emit('event', { type: 'sdk.error', sessionId: state.placeholderId, message: 'OpenCode turn interrupted: <timeout|sidecar lost>' })`; monitor row `timeout`/`sidecar_lost`. Never leaves the pane busy.
    - always: delete the registry entry on settle.
  - `bindServeStream` additionally registers a `'lost'` listener (via a new `subscribeLost` manager passthrough or by subscribing on the same emitter — implement as a second `serveManager.subscribe`-style hook; simplest: extend `subscribe(id, listener, onLost?)` in serve-manager with an optional third arg attached to `'lost'`): on lost, if `state.status === 'running'`, `emitStatus(state, 'idle')` + the same `sdk.error` interruption signal. (With Task 7, the same subscription keeps receiving events from the replacement sidecar — no rebind needed.)

- [ ] **Step 1: Write the failing tests**

Add to `opencode-serve-adapter.test.ts`:

```ts
describe('restore reconciliation emits and monitors (zrrj)', () => {
  it('emits a running session snapshot when attach reconciles a busy durable session', async () => {
    const manager = makeFakeManager()
    manager.getSessionStatus.mockResolvedValue({ type: 'busy' })
    const adapter = makeAdapter(manager)
    const events: any[] = []
    await adapter.attach!({ sessionId: 'ses_live', cwd: '/w' })
    adapter.subscribe('ses_live', (ev) => events.push(ev))   // use the adapter's actual subscribe surface
    // If subscription must precede attach in this adapter, reorder — assert on the emitted event either way:
    expect(events.some((e) => e.type === 'sdk.session.snapshot' && e.status === 'running')
      || manager.onceIdle.mock.calls.length > 0).toBe(true)
    // Primary assertion: a status event was EMITTED (not just state mutated)
  })

  it('arms exactly one idle-recovery monitor per durable session and chimes on resolve', async () => {
    const manager = makeFakeManager()
    manager.getSessionStatus.mockResolvedValue({ type: 'busy' })
    const idle = createDeferred<void>()
    manager.onceIdle.mockReturnValue(idle.promise)
    const adapter = makeAdapter(manager)
    const events: any[] = []
    await adapter.attach!({ sessionId: 'ses_live', cwd: '/w' })
    await adapter.attach!({ sessionId: 'ses_live', cwd: '/w' })   // second restore path
    expect(manager.onceIdle).toHaveBeenCalledTimes(1)              // exactly ONE monitor
    adapter.subscribe('ses_live', (ev) => events.push(ev))
    idle.resolve()
    await new Promise((r) => setImmediate(r))
    expect(events.some((e) => e.type === 'sdk.session.snapshot' && e.status === 'idle')).toBe(true)
    expect(events.some((e) => e.type === 'sdk.turn.complete' && typeof e.at === 'number')).toBe(true)
  })

  it('emits idle + a structured interruption signal when the monitor rejects with sidecar loss', async () => {
    const manager = makeFakeManager()
    manager.getSessionStatus.mockResolvedValue({ type: 'busy' })
    const idle = createDeferred<void>()
    manager.onceIdle.mockReturnValue(idle.promise)
    const adapter = makeAdapter(manager)
    const events: any[] = []
    await adapter.attach!({ sessionId: 'ses_live', cwd: '/w' })
    adapter.subscribe('ses_live', (ev) => events.push(ev))
    idle.reject(new OpencodeServeLostError('ses_live'))
    await new Promise((r) => setImmediate(r))
    expect(events.some((e) => e.type === 'sdk.session.snapshot' && e.status === 'idle')).toBe(true)
    expect(events.some((e) => e.type === 'sdk.error' && /interrupted/i.test(e.message))).toBe(true)
    expect(events.some((e) => e.type === 'sdk.turn.complete')).toBe(false)   // no chime on interruption
  })

  it('does not arm a monitor when reconcile finds the session idle/absent', async () => {
    const manager = makeFakeManager()
    manager.getSessionStatus.mockResolvedValue(undefined)   // absent == idle
    const adapter = makeAdapter(manager)
    await adapter.attach!({ sessionId: 'ses_calm', cwd: '/w' })
    expect(manager.onceIdle).not.toHaveBeenCalled()
  })
})
```

Match the adapter's real subscription surface (the existing tests subscribe via `state.events` through the adapter API — copy from the `:584`/`:795` analogues). Import `OpencodeServeLostError` from the serve-manager module as the existing manager test does (`:6`).

**Test-isolation requirement:** `idleRecoveryMonitors` is module-scope — export `resetOpencodeIdleRecoveryForTests()` from the adapter module and call it in this suite's `beforeEach`.

- [ ] **Step 2: Run to verify they fail**

Run: `npm run test:vitest -- --run test/unit/server/fresh-agent/opencode-serve-adapter.test.ts`
Expected: new tests FAIL (no emission, no monitor, no interruption signal).

- [ ] **Step 3: Implement**

In `adapter.ts`:

1. `reconcileStatus` (`:155-200`): replace direct mutations with `emitStatus(state, ...)` at `:158` (initial idle assumption — emit only if it *changes* an existing non-idle status to avoid noise on fresh attach: emit when `state.status !== 'idle'`) and `:189` (running). After the running branch, call `armIdleRecovery(state)`.

2. Add the monitor registry + arm function:

```ts
/** Exactly one monitored idle-recovery per durable session key (kata zrrj). */
const idleRecoveryMonitors = new Map<string, Promise<void>>()

export function resetOpencodeIdleRecoveryForTests(): void {
  idleRecoveryMonitors.clear()
}

function armIdleRecovery(state: OpencodeSessionState): void {
  const realId = state.realSessionId
  if (!realId) return
  if (idleRecoveryMonitors.has(realId)) {
    recordFreshAgentObservabilityEvent({
      kind: 'fresh_agent_monitor', provider: 'opencode',
      sessionIdHash: hashForLogs(realId),
      ...(state.cwd ? { cwdHash: hashForLogs(state.cwd) } : {}),
      phase: 'duplicate_suppressed',
    })
    return
  }
  const route = cwdRoute(state.cwd)
  const generation = typeof serveManager.currentGeneration === 'function' ? serveManager.currentGeneration() : undefined
  const monitorEvent = (phase: 'armed' | 'resolved_idle' | 'timeout' | 'sidecar_lost') =>
    recordFreshAgentObservabilityEvent({
      kind: 'fresh_agent_monitor', provider: 'opencode',
      sessionIdHash: hashForLogs(realId),
      ...(state.cwd ? { cwdHash: hashForLogs(state.cwd) } : {}),
      phase,
      ...(generation !== undefined ? { sidecarGeneration: generation } : {}),
    })
  monitorEvent('armed')
  const idle = route
    ? serveManager.onceIdle(realId, DEFAULT_TURN_TIMEOUT_MS, route)
    : serveManager.onceIdle(realId, DEFAULT_TURN_TIMEOUT_MS)
  const monitored = idle
    .then(() => {
      emitStatus(state, 'idle')
      monitorEvent('resolved_idle')
      if (!state.turnAborted && !state.turnErrored) {
        const completionAt = nextMonotonicTurnCompleteAt(state.lastTurnCompleteAt, Date.now())
        state.lastTurnCompleteAt = completionAt
        state.events.emit('event', { type: 'sdk.turn.complete', sessionId: state.placeholderId, at: completionAt })
      }
    })
    .catch((error: unknown) => {
      const lost = error instanceof OpencodeServeLostError
      emitStatus(state, 'idle')
      monitorEvent(lost ? 'sidecar_lost' : 'timeout')
      state.events.emit('event', {
        type: 'sdk.error',
        sessionId: state.placeholderId,
        message: lost
          ? 'OpenCode turn interrupted: sidecar connection was lost while the turn was running.'
          : 'OpenCode turn interrupted: idle recovery timed out.',
      })
    })
    .finally(() => { idleRecoveryMonitors.delete(realId) })
  idleRecoveryMonitors.set(realId, monitored)
}
```

(Import `OpencodeServeLostError` from `./serve-manager.js`; `hashForLogs`/`recordFreshAgentObservabilityEvent` from `../../observability.js` — the adapter already imports the latter at `:286-311`.)

3. `bindServeStream` (`:273-299`): pass an `onLost` handler through to the manager. In `serve-manager.ts`, extend `subscribe`:

```ts
subscribe(sessionId: string, listener: (parsed: ParsedServeEvent) => void, onLost?: (err: OpencodeServeLostError) => void): () => void {
  const emitter = this.emitterFor(sessionId)
  emitter.on('event', listener)
  if (onLost) emitter.on('lost', onLost)
  return () => {
    emitter.off('event', listener)
    if (onLost) emitter.off('lost', onLost)
    // (Task 7's empty-emitter cleanup)
  }
}
```

In the adapter, the `onLost` handler:

```ts
(err) => {
  if (state.status === 'running') {
    emitStatus(state, 'idle')
    state.events.emit('event', {
      type: 'sdk.error', sessionId: state.placeholderId,
      message: 'OpenCode turn interrupted: sidecar connection was lost while the turn was running.',
    })
  }
}
```

- [ ] **Step 4: Run to verify pass**

Run: `npm run test:vitest -- --run test/unit/server/fresh-agent/opencode-serve-adapter.test.ts test/unit/server/fresh-agent/opencode-serve-manager.test.ts`
Expected: PASS, including the pre-existing reconcile tests at `:584-728` (they assert state, which still holds — `emitStatus` also sets `state.status`) and the three non-chime paths at `:222/:293/:346`.

- [ ] **Step 5: Commit**

```bash
git add server/fresh-agent/adapters/opencode/adapter.ts server/fresh-agent/adapters/opencode/serve-manager.ts test/unit/server/fresh-agent/
git commit -m "fix(server): restored opencode sessions emit status and run one monitored idle recovery; sidecar loss emits interruption instead of stuck busy (zrrj)"
```

---

### Task 9: Preserve lost-session error code over WS + one-shot attach-recover-retry on send

The mechanism behind "materialized real `ses_...` durable/readable but send says *not tracked*": reads bypass tracking (`runtime-manager.ts:331-353` + `adapter.ts:413-418`), mutations require an index hit or a cwd-bearing locator (`canRecoverFreshOpenCode` `:430-437`, throw at `:552-557`), and the WS `freshAgent.send` handler flattens `FreshAgentLostSessionError` into `{ code: 'INTERNAL_ERROR' }` (`ws-handler.ts:3554-3556`) so the client can't distinguish "re-attach and retry" from a real bug. Fix server-side: (a) propagate the code, (b) when the locator carries a cwd and the first send throws lost-session, attach-recover and retry exactly once inside the WS handler.

**Files:**
- Modify: `server/ws-handler.ts` (`freshAgent.send` case `:3512-3558`)
- Test: `test/unit/server/ws-handler-fresh-agent.test.ts` (harness: real http+ws, `vi.fn()` runtime-manager stub, `connectAndAuth` at `:49-72`)

**Interfaces:**
- Consumes: `FreshAgentLostSessionError` (exported from `server/fresh-agent/runtime-manager.ts` or its errors module — import from wherever `ws-handler.ts` can already reach it); `manager.attach(locator)`, `manager.send(locator, input)`.
- Produces: WS error frames for lost sessions now carry `code: 'FRESH_AGENT_LOST_SESSION'` (message unchanged). Client behavior in Task 10 keys off this code. Retry rule: at most one `attach` + one re-`send`; if the retry also fails, send the error frame (with the true code) — no loops.

- [ ] **Step 1: Write the failing tests**

Add to `test/unit/server/ws-handler-fresh-agent.test.ts`:

```ts
it('recovers a durable freshopencode send once via attach when the manager reports lost-session', async () => {
  const send = vi.fn()
    .mockRejectedValueOnce(new FreshAgentLostSessionError('Fresh-agent session freshopencode/opencode/ses_9 is not tracked'))
    .mockResolvedValueOnce({ sessionId: 'ses_9' })
  const attach = vi.fn().mockResolvedValue({ sessionId: 'ses_9' })
  const { server, port } = await createServer({ freshAgentRuntimeManager: makeManagerStub({ send, attach }) })
  const ws = await connectAndAuth(server)
  ws.send(JSON.stringify({
    type: 'freshAgent.send', requestId: 'r1', sessionId: 'ses_9',
    sessionType: 'freshopencode', provider: 'opencode', cwd: '/w', text: 'hello',
  }))
  const accepted = await waitForMessage(ws, (m) => m.type === 'freshAgent.send.accepted')
  expect(accepted.sessionId).toBe('ses_9')
  expect(attach).toHaveBeenCalledTimes(1)
  expect(send).toHaveBeenCalledTimes(2)
})

it('propagates FRESH_AGENT_LOST_SESSION when recovery is impossible (no cwd)', async () => {
  const send = vi.fn().mockRejectedValue(new FreshAgentLostSessionError('Fresh-agent session freshopencode/opencode/ses_9 is not tracked'))
  const attach = vi.fn()
  const { server } = await createServer({ freshAgentRuntimeManager: makeManagerStub({ send, attach }) })
  const ws = await connectAndAuth(server)
  ws.send(JSON.stringify({
    type: 'freshAgent.send', requestId: 'r1', sessionId: 'ses_9',
    sessionType: 'freshopencode', provider: 'opencode', text: 'hello',   // NO cwd
  }))
  const err = await waitForMessage(ws, (m) => m.type === 'error')
  expect(err.code).toBe('FRESH_AGENT_LOST_SESSION')
  expect(attach).not.toHaveBeenCalled()
  expect(send).toHaveBeenCalledTimes(1)   // no blind retry loop
})
```

Reuse the file's existing manager-stub construction and message-frame helpers verbatim (the suite already has patterns for `freshAgent.send` frames including authorization setup — follow the nearest passing send test; the fresh-agent send path requires prior authorization via create/attach in some configurations: mirror whatever arrangement the file's existing send tests use, e.g. an initial `freshAgent.attach` frame).

- [ ] **Step 2: Run to verify they fail**

Run: `npm run test:vitest -- --run test/unit/server/ws-handler-fresh-agent.test.ts`
Expected: FAIL — today's handler sends `code: 'INTERNAL_ERROR'` and never attaches.

- [ ] **Step 3: Implement**

In `ws-handler.ts` `freshAgent.send` catch (`:3554-3556` region), replace the flattening with:

```ts
} catch (error) {
  if (error instanceof FreshAgentLostSessionError) {
    const canRecover = locator.sessionType === 'freshopencode'
      && locator.sessionId.startsWith('ses_')
      && typeof locator.cwd === 'string' && locator.cwd.length > 0
      && typeof manager.attach === 'function'
    if (canRecover) {
      try {
        await manager.attach(locator)
        const retried = await manager.send(locator, adapterInput)   // same input as the first call
        // fall through to the existing send.accepted emission with `retried`
        ...
        return
      } catch (retryError) {
        this.sendError(ws, {
          code: retryError instanceof FreshAgentLostSessionError ? 'FRESH_AGENT_LOST_SESSION' : 'INTERNAL_ERROR',
          message: errorMessage(retryError),
        })
        recordFreshAgentObservabilityEvent({ kind: 'fresh_agent_send', sessionType: locator.sessionType, provider: locator.provider, sessionIdHash: hashForLogs(locator.sessionId), ...(locator.cwd ? { cwdHash: hashForLogs(locator.cwd) } : {}), outcome: 'failed', errorCode: 'FRESH_AGENT_LOST_SESSION' })
        return
      }
    }
    this.sendError(ws, { code: 'FRESH_AGENT_LOST_SESSION', message: errorMessage(error) })
    return
  }
  this.sendError(ws, { code: 'INTERNAL_ERROR', message: errorMessage(error) })
}
```

Structure the success fall-through by extracting the existing "emit `freshAgent.send.accepted` (+ materialization message)" block into a local function used by both the first attempt and the retry. Keep the existing genuinely-invalid case intact: `FreshAgentLostSessionError` raised for a NON-durable placeholder (`resume` throws "not a durable OpenCode session") must NOT be retried — the `startsWith('ses_')` + cwd guard covers this ("do not swallow genuinely invalid placeholder/non-durable lost-session errors").

- [ ] **Step 4: Run to verify pass**

Run: `npm run test:vitest -- --run test/unit/server/ws-handler-fresh-agent.test.ts test/unit/server/ws-handler-fresh-agent-ownership.test.ts`
Expected: PASS (ownership suite guards the cwd/authorization axis — must stay green).

- [ ] **Step 5: Commit**

```bash
git add server/ws-handler.ts test/unit/server/ws-handler-fresh-agent.test.ts
git commit -m "fix(server): preserve FRESH_AGENT_LOST_SESSION over WS and retry durable freshopencode sends once via attach (zrrj)"
```

---

### Task 10: Client: auto-reattach + resend once on FRESH_AGENT_LOST_SESSION

With Task 9, a send that cannot be server-recovered (e.g. no cwd on the locator) reaches the client as `code: 'FRESH_AGENT_LOST_SESSION'`. The client knows the route cwd (`freshOpenCodeRouteCwd`, `FreshAgentView.tsx:613-615` via `src/lib/fresh-opencode-route.ts`) even when the original send omitted it. On this code, re-issue `freshAgent.attach` with the route cwd, then resend the message exactly once.

**Files:**
- Modify: `src/components/fresh-agent/FreshAgentView.tsx` (WS error handling for sends; locate the existing handler for send failures — search the file for `INTERNAL_ERROR` or the `sendError`-frame consumption path used to clear local echo on failure)
- Test: `test/unit/client/components/fresh-agent/FreshAgentView.test.tsx`

**Interfaces:**
- Consumes: `wsMock.send` scaffold; `freshOpenCodeRouteCwdRef`; the pane's pending send metadata (`pendingSendMetadataRef`) which retains the original text/requestId.
- Produces: one retry per failed requestId, tracked in `lostSessionRetryRef: useRef<Set<string>>` (requestIds already retried — never retried twice).

- [ ] **Step 1: Write the failing test**

```ts
it('re-attaches with the route cwd and resends once when a send fails with FRESH_AGENT_LOST_SESSION', async () => {
  // Arrange a freshopencode pane on ses_9 with initialCwd '/w'; send a message via the composer.
  render(<StoreBackedFreshAgentView ... />)
  await typeAndSend('hello again')            // use the file's existing composer interaction helper
  const sendFrame = wsMock.send.mock.calls.map(([f]) => JSON.parse(f)).find((m) => m.type === 'freshAgent.send')
  expect(sendFrame).toBeTruthy()
  wsMock.send.mockClear()

  // Act: server rejects with the lost-session code for that request
  act(() => wsHandler({ type: 'error', code: 'FRESH_AGENT_LOST_SESSION', requestId: sendFrame.requestId, message: 'not tracked' }))

  // Assert: exactly one attach (with cwd) then one resend of the same text
  await waitFor(() => {
    const frames = wsMock.send.mock.calls.map(([f]) => JSON.parse(f))
    expect(frames.some((m) => m.type === 'freshAgent.attach' && m.sessionId === 'ses_9' && m.cwd === '/w')).toBe(true)
    expect(frames.filter((m) => m.type === 'freshAgent.send' && m.text === 'hello again')).toHaveLength(1)
  })

  // Second failure for the retried request must NOT loop
  const retried = wsMock.send.mock.calls.map(([f]) => JSON.parse(f)).find((m) => m.type === 'freshAgent.send')
  wsMock.send.mockClear()
  act(() => wsHandler({ type: 'error', code: 'FRESH_AGENT_LOST_SESSION', requestId: retried.requestId, message: 'still not tracked' }))
  await new Promise((r) => setTimeout(r, 100))
  expect(wsMock.send.mock.calls.map(([f]) => JSON.parse(f)).filter((m) => m.type === 'freshAgent.send')).toHaveLength(0)
})
```

Check how error frames reach the component today (the `wsHandler` capture receives all frames; if send errors arrive with a different shape — e.g. `{ type: 'freshAgent.send.failed' }` or a generic `{ type: 'error' }` — mirror the real shape emitted by `this.sendError` in `ws-handler.ts`, which the existing tests for send failure already model; copy from them).

- [ ] **Step 2: Run to verify it fails**

Run: `npm run test:vitest -- --run test/unit/client/components/fresh-agent/FreshAgentView.test.tsx -t "FRESH_AGENT_LOST_SESSION"`
Expected: FAIL — no attach, no resend.

- [ ] **Step 3: Implement** in `FreshAgentView.tsx`: in the WS error-frame handling for owned send requests, add:

```ts
if (errorCode === 'FRESH_AGENT_LOST_SESSION'
  && paneContentRef.current.sessionType === 'freshopencode'
  && typeof failedRequestId === 'string'
  && !lostSessionRetryRef.current.has(failedRequestId)) {
  const pendingMeta = pendingSendMetadataRef.current.get(failedRequestId)
  const cwd = freshOpenCodeRouteCwdRef.current
  const sessionId = paneContentRef.current.sessionId
  if (pendingMeta && cwd && sessionId) {
    lostSessionRetryRef.current.add(failedRequestId)
    ws.send(JSON.stringify({
      type: 'freshAgent.attach', sessionId, sessionType: 'freshopencode', provider: 'opencode', cwd,
    }))
    const retryRequestId = createRequestId()      // the file's existing id generator
    lostSessionRetryRef.current.add(retryRequestId)   // the retry itself is never retried
    resendPendingMessage(retryRequestId, pendingMeta, cwd)   // re-issue freshAgent.send with the same text + cwd
    return
  }
}
// fall through to the existing error handling (clear echo, show error)
```

Implement `resendPendingMessage` by extracting the existing send-frame construction into a helper reused by the composer submit path and the retry (same fields, plus `cwd`).

- [ ] **Step 4: Run to verify pass**

Run: `npm run test:vitest -- --run test/unit/client/components/fresh-agent/FreshAgentView.test.tsx`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/components/fresh-agent/FreshAgentView.tsx test/unit/client/components/fresh-agent/FreshAgentView.test.tsx
git commit -m "feat(client): auto-reattach and resend once on FRESH_AGENT_LOST_SESSION for freshopencode (zrrj)"
```

---

### Task 11: Surface interrupted-turn evidence fields in the snapshot normalizer

`normalizeOpencodeTurn` (`server/fresh-agent/adapters/opencode/normalize.ts:324-355`) discards nearly every evidence field the detector needs — `info.time.completed`, `info.error`, per-message `info.tokens`/`info.cost` — even though `assembleExport` passes the raw `m.info` through verbatim (`adapter.ts:408`). Surface them under `extensions.opencode` (the low-friction contract path) so both the server-side detector and the client can see them. `step-finish` counts already survive (`normalize.ts:271-322`); tool part `state.status === 'running'` already reaches the client (`normalize.ts:216`).

**Files:**
- Modify: `server/fresh-agent/adapters/opencode/normalize.ts`
- Read first (MANDATORY before coding): `shared/fresh-agent-contract.ts` — confirm the `extensions` Zod shape is permissive (`z.record(z.unknown())` or similar). If it is NOT permissive, extend the contract schema for `extensions.opencode.turnEvidence` and mirror the type on the client import path; the client hard-validates at `src/lib/api.ts:418-421` and will reject snapshots otherwise.
- Test: `test/unit/server/fresh-agent/opencode-normalize.test.ts` (445 lines; step-finish assertions at `:105-133` show the fixture idiom)

**Interfaces:**
- Consumes: raw `OpencodeServeMessage = { info: Record<string, any>; parts: Array<Record<string, any>> }` passthrough.
- Produces: `snapshot.extensions.opencode.turnEvidence: OpencodeTurnEvidence[]` where

```ts
export type OpencodeTurnEvidence = {
  turnId: string
  role: string
  timeCreated?: number
  timeCompleted?: number          // absent => unfinished (assistant)
  error?: { name?: string; message?: string }   // e.g. { name: 'MessageAbortedError' } — NEVER include payload beyond name/message
  tokens?: { input?: number; output?: number }
  cost?: number
  runningToolPartCount: number    // parts with state.status === 'running'
  stepStartCount: number
  stepFinishCount: number
}
```

Field-name caution (kata-critical): the real OpenCode `message.data` field names are not documented in this repo. Extraction must be defensive: read `info.time?.completed`, `info.error` (accept `{ name, message }`, `{ data: { message } }`, or a string — reuse the tolerant shape of `opencodeErrorMessage` in `serve-events.ts:32-39`), `info.tokens?.input/output`, `info.cost`. Absent fields yield absent evidence properties, never crashes. Before implementing, verify against a live DB if available: `sqlite3 ~/.local/share/opencode/opencode.db "SELECT data FROM message ORDER BY time_created DESC LIMIT 3"` — if the machine has real OpenCode sessions, confirm/adjust names and record what you found in the commit message. If no live DB exists, implement the defensive reads exactly as specified.

- [ ] **Step 1: Write the failing tests**

Add to `opencode-normalize.test.ts` (reuse its fixture-building idiom):

```ts
describe('turn evidence extraction (zrrj)', () => {
  it('surfaces missing completion time, abort error, and running tool parts', () => {
    const snapshot = normalizeOpencodeSnapshot({
      sessionType: 'freshopencode',
      threadId: 'ses_1',
      status: 'idle',
      exported: {
        info: { id: 'ses_1', time: { updated: 10 } },
        messages: [
          { info: { id: 'm1', role: 'user', time: { created: 1, completed: 2 } }, parts: [] },
          {
            info: {
              id: 'm2', role: 'assistant', time: { created: 3 },              // NO completed
              error: { name: 'MessageAbortedError', message: 'aborted' },
              tokens: { input: 100, output: 0 }, cost: 0,
            },
            parts: [
              { type: 'tool', state: { status: 'running' } },
              { type: 'step-start' },
            ],
          },
        ],
      },
    })
    const evidence = snapshot.extensions.opencode.turnEvidence
    expect(evidence).toHaveLength(2)
    expect(evidence[1]).toMatchObject({
      turnId: 'm2', role: 'assistant',
      timeCompleted: undefined,
      error: { name: 'MessageAbortedError', message: 'aborted' },
      tokens: { input: 100, output: 0 },
      runningToolPartCount: 1, stepStartCount: 1, stepFinishCount: 0,
    })
    expect(evidence[0]).toMatchObject({ turnId: 'm1', timeCompleted: 2, runningToolPartCount: 0 })
  })

  it('never leaks message text into evidence', () => {
    const snapshot = normalizeOpencodeSnapshot({ /* assistant message with text parts */ })
    const json = JSON.stringify(snapshot.extensions.opencode.turnEvidence)
    expect(json).not.toContain('the secret assistant text')   // seed that string in a text part
  })
})
```

Match `normalizeOpencodeSnapshot`'s actual input signature from the existing tests in this file (it may take `{ exported, revision, ... }` or flattened args — copy the call shape of the nearest existing test exactly).

- [ ] **Step 2: Run to verify they fail**

Run: `npm run test:vitest -- --run test/unit/server/fresh-agent/opencode-normalize.test.ts`
Expected: FAIL — `turnEvidence` undefined.

- [ ] **Step 3: Implement** in `normalize.ts` — a pure helper called from `normalizeOpencodeSnapshot` where `extensions.opencode` is assembled (`:403` area):

```ts
function extractTurnEvidence(messages: Array<{ info: Record<string, any>; parts: Array<Record<string, any>> }>): OpencodeTurnEvidence[] {
  return messages.map(({ info, parts }) => {
    const err = info?.error
    const error = err
      ? {
          ...(typeof err?.name === 'string' ? { name: err.name } : {}),
          ...(typeof err?.message === 'string'
            ? { message: err.message }
            : typeof err?.data?.message === 'string'
              ? { message: err.data.message }
              : typeof err === 'string' ? { message: err } : {}),
        }
      : undefined
    const counts = { running: 0, stepStart: 0, stepFinish: 0 }
    for (const part of parts ?? []) {
      if (part?.type === 'tool' && part?.state?.status === 'running') counts.running += 1
      if (part?.type === 'step-start') counts.stepStart += 1
      if (part?.type === 'step-finish') counts.stepFinish += 1
    }
    const tokens = info?.tokens && typeof info.tokens === 'object'
      ? {
          ...(Number.isFinite(info.tokens.input) ? { input: Number(info.tokens.input) } : {}),
          ...(Number.isFinite(info.tokens.output) ? { output: Number(info.tokens.output) } : {}),
        }
      : undefined
    return {
      turnId: String(info?.id ?? ''),
      role: typeof info?.role === 'string' ? info.role : 'unknown',
      ...(Number.isFinite(info?.time?.created) ? { timeCreated: Number(info.time.created) } : {}),
      ...(Number.isFinite(info?.time?.completed) ? { timeCompleted: Number(info.time.completed) } : {}),
      ...(error && (error.name || error.message) ? { error } : {}),
      ...(tokens && (tokens.input !== undefined || tokens.output !== undefined) ? { tokens } : {}),
      ...(Number.isFinite(info?.cost) ? { cost: Number(info.cost) } : {}),
      runningToolPartCount: counts.running,
      stepStartCount: counts.stepStart,
      stepFinishCount: counts.stepFinish,
    }
  })
}
```

Wire `turnEvidence: extractTurnEvidence(input.exported?.messages ?? [])` into the `extensions.opencode` object. **Sidecar path only** — the DB hydrator clobbers `info.time` (`history-query.ts:373-376`), which is fine because FreshOpenCode snapshots read the sidecar (`assembleExport`), never the DB.

- [ ] **Step 4: Run to verify pass** — `npm run test:vitest -- --run test/unit/server/fresh-agent/opencode-normalize.test.ts test/unit/server/fresh-agent/opencode-serve-adapter.test.ts` (adapter snapshot tests must stay green; if the client contract rejects the new field, this is where it surfaces — fix the contract as noted above). Also run `npm run test:vitest -- --run test/unit/client/lib/api.test.ts` to confirm `FreshAgentSnapshotSchema` accepts snapshots with evidence.

- [ ] **Step 5: Commit**

```bash
git add server/fresh-agent/adapters/opencode/normalize.ts shared/fresh-agent-contract.ts test/unit/server/fresh-agent/opencode-normalize.test.ts
git commit -m "feat(server): surface opencode turn evidence (completion time, error, tokens, tool state) in snapshot extensions (zrrj)"
```

---

### Task 12: Durable interrupt-intent and recovery ledger store

"Never auto-recover after explicit user stop" is unsatisfiable today: interrupt intent lives only in `OpencodeSessionState.turnAborted` (`adapter.ts:52`, set `:521`), cleared on the next send (`:334`) and destroyed by restart (`shutdown()` clears the map, `:625`). Give it a durable home, plus the at-most-one-recovery-per-turn ledger.

**Files:**
- Create: `server/fresh-agent/recovery-store.ts`
- Test: `test/unit/server/fresh-agent/recovery-store.test.ts`

**Interfaces:**
- Consumes: `node:fs/promises`, `node:path`, `node:os`. Storage file: `~/.freshell/fresh-agent-recovery.json` (constructor takes an optional `filePath` for tests). Atomic writes: temp file + rename (repo convention).
- Produces:

```ts
export type RecoveryStoreData = {
  version: 1
  /** sessionId -> ms timestamp of the user's explicit stop. */
  interrupts: Record<string, number>
  /** sessionId -> messageId -> ms timestamp of the injected continuation. */
  recoveries: Record<string, Record<string, number>>
}

export class FreshAgentRecoveryStore {
  constructor(options?: { filePath?: string })
  async recordInterrupt(sessionId: string): Promise<void>
  async clearInterrupt(sessionId: string): Promise<void>       // called on the next user-initiated send
  async hasInterrupt(sessionId: string): Promise<boolean>
  async recordRecovery(sessionId: string, messageId: string): Promise<void>
  async hasRecovery(sessionId: string, messageId: string): Promise<boolean>
}

export function getFreshAgentRecoveryStore(): FreshAgentRecoveryStore   // lazy singleton
export function resetFreshAgentRecoveryStoreForTests(filePath?: string): void
```

Corrupt/missing file → start empty (log a warn, never throw). Keep the file small: cap `interrupts` and per-session `recoveries` at the 100 most recent entries (drop oldest on insert).

- [ ] **Step 1: Write the failing tests**

```ts
import { describe, it, expect, beforeEach } from 'vitest'
import { mkdtemp, readFile, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { FreshAgentRecoveryStore } from '../../../server/fresh-agent/recovery-store.js'

describe('FreshAgentRecoveryStore', () => {
  let filePath: string
  beforeEach(async () => {
    filePath = path.join(await mkdtemp(path.join(tmpdir(), 'recovery-')), 'recovery.json')
  })

  it('persists interrupt intent across store instances (simulated restart)', async () => {
    const store = new FreshAgentRecoveryStore({ filePath })
    await store.recordInterrupt('ses_1')
    const reborn = new FreshAgentRecoveryStore({ filePath })
    expect(await reborn.hasInterrupt('ses_1')).toBe(true)
    await reborn.clearInterrupt('ses_1')
    expect(await new FreshAgentRecoveryStore({ filePath }).hasInterrupt('ses_1')).toBe(false)
  })

  it('records at most one recovery per (session, message) and persists it', async () => {
    const store = new FreshAgentRecoveryStore({ filePath })
    expect(await store.hasRecovery('ses_1', 'm2')).toBe(false)
    await store.recordRecovery('ses_1', 'm2')
    expect(await store.hasRecovery('ses_1', 'm2')).toBe(true)
    expect(await new FreshAgentRecoveryStore({ filePath }).hasRecovery('ses_1', 'm2')).toBe(true)
  })

  it('starts empty on a corrupt file instead of throwing', async () => {
    await writeFile(filePath, '{not json', 'utf8')
    const store = new FreshAgentRecoveryStore({ filePath })
    expect(await store.hasInterrupt('ses_1')).toBe(false)
    await store.recordInterrupt('ses_1')            // and can still write
    expect(JSON.parse(await readFile(filePath, 'utf8')).version).toBe(1)
  })
})
```

- [ ] **Step 2: Run to verify fail** — `npm run test:vitest -- --run test/unit/server/fresh-agent/recovery-store.test.ts` → module missing.

- [ ] **Step 3: Implement `server/fresh-agent/recovery-store.ts`** — straightforward: lazy `load()` (read + JSON.parse with try/catch → default `{ version: 1, interrupts: {}, recoveries: {} }`), every mutator loads, mutates, prunes to the 100-entry caps, then writes `JSON.stringify(data, null, 2)` to `${filePath}.tmp` and `rename`s over `filePath` (`encoding: 'utf8'`, `mkdir` the parent recursively first). Serialize writes through an internal `this.writeQueue = this.writeQueue.then(...)` promise chain so concurrent mutators can't interleave. Default path: `path.join(os.homedir(), '.freshell', 'fresh-agent-recovery.json')`.

- [ ] **Step 4: Run to verify pass** — same command.

- [ ] **Step 5: Commit**

```bash
git add server/fresh-agent/recovery-store.ts test/unit/server/fresh-agent/recovery-store.test.ts
git commit -m "feat(server): durable fresh-agent interrupt-intent and recovery ledger store (zrrj)"
```

---

### Task 13: Pure interrupted-turn detector with stability window

Detect high-confidence interrupted turns from transcript/tool evidence — never from a guessed persisted status field (the OpenCode DB has none). Pure function over the raw sidecar messages; no I/O.

**Files:**
- Create: `server/fresh-agent/adapters/opencode/interrupted-turn.ts`
- Test: `test/unit/server/fresh-agent/opencode-interrupted-turn.test.ts`

**Interfaces:**
- Consumes: raw `OpencodeServeMessage[]` (`{ info, parts }` — same shapes Task 11 reads).
- Produces:

```ts
export type InterruptedTurnVerdict =
  | { interrupted: false; reason: string }
  | {
      interrupted: true
      messageId: string
      /** Which evidence fired — for the audit log and the transcript event. */
      evidence: Array<'missing_completion' | 'aborted_error' | 'running_tool_part' | 'zero_output_tokens' | 'missing_step_finish'>
    }

export const INTERRUPTED_TURN_STABILITY_MS = 15_000

export function detectInterruptedTurn(
  messages: Array<{ info: Record<string, any>; parts: Array<Record<string, any>> }>,
  options: { nowMs: number; stabilityMs?: number },
): InterruptedTurnVerdict
```

**Decision rules (implement exactly, in order):**
1. Find the LAST message. If it is not `role: 'assistant'`, return `{ interrupted: false, reason: 'last_message_not_assistant' }` — a trailing user message means the user already typed a follow-up (kata: never auto-recover then). No messages → not interrupted (`'empty_transcript'`).
2. Stability window: if `max(info.time.created, info.time.updated ?? 0)` of that message is within `stabilityMs` (default 15 s) of `nowMs`, return `{ interrupted: false, reason: 'within_stability_window' }` — a still-writing DB/sidecar row must not be recovered prematurely.
3. Collect evidence on that assistant message:
   - `missing_completion`: `info.time?.completed` is not a finite number.
   - `aborted_error`: `info.error?.name === 'MessageAbortedError'` (or `info.error?.data?.name === 'MessageAbortedError'`).
   - `running_tool_part`: any part with `type === 'tool' && state?.status === 'running'`.
   - `zero_output_tokens`: `info.tokens?.output === 0` AND at least one other evidence item present (zero alone is too weak — a legitimate tool-only turn can have 0 output tokens).
   - `missing_step_finish`: `stepStartCount > 0 && stepFinishCount === 0` over its parts.
4. High-confidence gate: `interrupted: true` only when `missing_completion` is present AND at least one more evidence item fired (or `aborted_error` alone — an explicit abort record is definitive). Otherwise `{ interrupted: false, reason: 'insufficient_evidence' }`.

(The remaining kata signals — sidecar loss, Freshell shutdown, restored running status — are covered by the *callers*: Task 8's monitor emits interruption on sidecar loss/timeout, and Task 14 runs this detector on restore, which is exactly the shutdown/restored-status path.)

- [ ] **Step 1: Write the failing tests**

```ts
import { describe, it, expect } from 'vitest'
import { detectInterruptedTurn, INTERRUPTED_TURN_STABILITY_MS } from '../../../../server/fresh-agent/adapters/opencode/interrupted-turn.js'

const NOW = 1_000_000_000
const OLD = NOW - INTERRUPTED_TURN_STABILITY_MS - 1

function assistant(info: Record<string, any>, parts: Record<string, any>[] = []) {
  return { info: { id: 'm2', role: 'assistant', time: { created: OLD, updated: OLD }, ...info }, parts }
}
const user = { info: { id: 'm1', role: 'user', time: { created: OLD - 10, completed: OLD - 9 } }, parts: [] }

describe('detectInterruptedTurn', () => {
  it('flags missing completion + running tool part as interrupted', () => {
    const verdict = detectInterruptedTurn([user, assistant({}, [{ type: 'tool', state: { status: 'running' } }])], { nowMs: NOW })
    expect(verdict).toEqual({ interrupted: true, messageId: 'm2', evidence: ['missing_completion', 'running_tool_part'] })
  })

  it('flags an explicit MessageAbortedError alone', () => {
    const verdict = detectInterruptedTurn(
      [user, assistant({ time: { created: OLD, completed: OLD + 1 }, error: { name: 'MessageAbortedError' } })],
      { nowMs: NOW },
    )
    expect(verdict.interrupted).toBe(true)
    if (verdict.interrupted) expect(verdict.evidence).toContain('aborted_error')
  })

  it('does not flag missing completion alone (insufficient evidence)', () => {
    const verdict = detectInterruptedTurn([user, assistant({})], { nowMs: NOW })
    expect(verdict).toEqual({ interrupted: false, reason: 'insufficient_evidence' })
  })

  it('respects the stability window for a still-writing row', () => {
    const fresh = { info: { id: 'm2', role: 'assistant', time: { created: NOW - 1_000 } }, parts: [{ type: 'tool', state: { status: 'running' } }] }
    expect(detectInterruptedTurn([user, fresh], { nowMs: NOW })).toEqual({ interrupted: false, reason: 'within_stability_window' })
  })

  it('never flags when the user already typed a follow-up', () => {
    const trailingUser = { info: { id: 'm3', role: 'user', time: { created: OLD + 5 } }, parts: [] }
    const verdict = detectInterruptedTurn(
      [user, assistant({}, [{ type: 'tool', state: { status: 'running' } }]), trailingUser],
      { nowMs: NOW },
    )
    expect(verdict).toEqual({ interrupted: false, reason: 'last_message_not_assistant' })
  })

  it('counts missing step-finish and zero output tokens as corroborating evidence', () => {
    const verdict = detectInterruptedTurn(
      [user, assistant({ tokens: { input: 50, output: 0 } }, [{ type: 'step-start' }])],
      { nowMs: NOW },
    )
    expect(verdict.interrupted).toBe(true)
    if (verdict.interrupted) {
      expect(verdict.evidence).toEqual(expect.arrayContaining(['missing_completion', 'zero_output_tokens', 'missing_step_finish']))
    }
  })

  it('does not flag a cleanly completed turn', () => {
    const done = assistant({ time: { created: OLD, completed: OLD + 2 }, tokens: { input: 10, output: 40 } }, [{ type: 'step-start' }, { type: 'step-finish' }])
    expect(detectInterruptedTurn([user, done], { nowMs: NOW }).interrupted).toBe(false)
  })
})
```

- [ ] **Step 2: Run to verify fail** — `npm run test:vitest -- --run test/unit/server/fresh-agent/opencode-interrupted-turn.test.ts` → module missing.

- [ ] **Step 3: Implement** the pure function exactly per the decision rules. Evidence array order: `['missing_completion', 'aborted_error', 'running_tool_part', 'zero_output_tokens', 'missing_step_finish']` filtered to what fired (stable order makes tests deterministic).

- [ ] **Step 4: Run to verify pass.**

- [ ] **Step 5: Commit**

```bash
git add server/fresh-agent/adapters/opencode/interrupted-turn.ts test/unit/server/fresh-agent/opencode-interrupted-turn.test.ts
git commit -m "feat(server): evidence-based interrupted-turn detector with stability window (zrrj)"
```

---

### Task 14: Recovery orchestration — one auditable Freshell-owned continuation, loop-proof

Wire interrupt intent + detector + ledger into the adapter: record intent on `interrupt()`, clear it on the next user send, and on durable restore (resume/attach) when the reconciled status is idle, run the detector against the live transcript and inject at most ONE continuation per failed turn.

**Files:**
- Modify: `server/fresh-agent/adapters/opencode/adapter.ts` (`interrupt()` `:517-531`, `materializeOrSend` `:324-387`, restore paths `:438-498`)
- Test: `test/unit/server/fresh-agent/opencode-serve-adapter.test.ts`

**Interfaces:**
- Consumes: `FreshAgentRecoveryStore` (Task 12 — injected via adapter options as `recoveryStore`, defaulting to `getFreshAgentRecoveryStore()`; tests inject a store on a temp file); `detectInterruptedTurn` (Task 13); `serveManager.listMessages`; `promptAsyncForState` (`:363-367` idiom); `recordFreshAgentObservabilityEvent` (Task 6).
- Produces:
  - `interrupt()` additionally `await recoveryStore.recordInterrupt(realId)` (fire before abort; roll back with `clearInterrupt` if the abort throws, mirroring the existing `turnAborted` rollback at `:527`).
  - `materializeOrSend` for a **user-initiated** send additionally `await recoveryStore.clearInterrupt(realId)` (a follow-up cancels stop intent). Continuation sends are marked internal and do NOT clear intent: add an options flag `{ freshellContinuation?: boolean }` threaded through the send path.
  - New `maybeRecoverInterruptedTurn(state: OpencodeSessionState): Promise<void>` called from `resume()`/`attach()` after `reconcileStatus` **only when** the reconciled status is `'idle'` (a `running` session is being monitored by Task 8 — not interrupted). Logic:

```ts
async function maybeRecoverInterruptedTurn(state: OpencodeSessionState): Promise<void> {
  const realId = state.realSessionId
  if (!realId) return
  const audit = (action: string, reason: string, messageId?: string) =>
    recordFreshAgentObservabilityEvent({
      kind: 'fresh_agent_turn_recovery', provider: 'opencode',
      sessionIdHash: hashForLogs(realId),
      ...(state.cwd ? { cwdHash: hashForLogs(state.cwd) } : {}),
      action: action as any, reason,
      ...(messageId ? { messageIdHash: hashForLogs(messageId) } : {}),
    })
  try {
    if (await recoveryStore.hasInterrupt(realId)) { audit('suppressed_user_stop', 'user_interrupt_on_record'); return }
    const route = cwdRoute(state.cwd)
    const page = route
      ? await serveManager.listMessages(realId, { limit: DEFAULT_SNAPSHOT_TURN_LIMIT }, route)
      : await serveManager.listMessages(realId, { limit: DEFAULT_SNAPSHOT_TURN_LIMIT })
    const verdict = detectInterruptedTurn(page.messages, { nowMs: Date.now() })
    if (!verdict.interrupted) {
      if (verdict.reason !== 'empty_transcript' && verdict.reason !== 'last_message_not_assistant') {
        audit('suppressed_low_confidence', verdict.reason)
      } else if (verdict.reason === 'last_message_not_assistant') {
        audit('suppressed_user_followup', verdict.reason)
      }
      return
    }
    if (await recoveryStore.hasRecovery(realId, verdict.messageId)) {
      audit('suppressed_already_recovered', 'ledger_hit', verdict.messageId); return
    }
    await recoveryStore.recordRecovery(realId, verdict.messageId)   // record BEFORE injecting: crash-safe loop prevention
    audit('continuation_injected', verdict.evidence.join(','), verdict.messageId)
    state.events.emit('event', {
      type: 'sdk.session.changed', sessionId: state.placeholderId, reason: 'freshell-turn-recovery',
    })
    await sendForState(state, {
      text: 'Freshell detected that your previous response was interrupted (for example by a restart). Please continue exactly where you left off. If the work was already complete, briefly confirm the final result.',
      freshellContinuation: true,
    })
  } catch (error) {
    log.warn({ provider: 'opencode', sessionIdHash: hashForLogs(realId), err: error }, 'interrupted-turn recovery failed')
  }
}
```

  where `sendForState` is the existing send-queue entry (`adapter.ts:507-515` idiom) so the continuation is serialized, route-validated (`ensureMutableRoute`), armed with `onceIdle`, and produces the normal running→idle→turn-complete lifecycle — a clear transcript event plus the audit row satisfy "auditable". The continuation text is Freshell-owned instruction, not user content — safe to hardcode; it appears in the transcript as a user-role message (that IS the visible transcript marker).
  - **Do not auto-recover a `running` session** — only `idle`-reconciled restores are candidates. This encodes "restored running status" recovery as: monitor it (Task 8); if the monitor later reports sidecar loss, the NEXT restore attempt will find the unfinished transcript and recover it here.

- [ ] **Step 1: Write the failing tests** (in `opencode-serve-adapter.test.ts`; build the adapter with an injected recovery store on a temp file — extend `makeAdapter` overrides):

```ts
describe('interrupted-turn recovery (zrrj)', () => {
  const OLD = Date.now() - 60_000
  const interruptedTranscript = {
    messages: [
      { info: { id: 'm1', role: 'user', time: { created: OLD - 10, completed: OLD - 9 } }, parts: [] },
      { info: { id: 'm2', role: 'assistant', time: { created: OLD } }, parts: [{ type: 'tool', state: { status: 'running' } }] },
    ],
    nextCursor: null,
  }

  it('injects exactly one continuation for an interrupted turn on attach', async () => {
    const manager = makeFakeManager()
    manager.getSessionStatus.mockResolvedValue(undefined)          // idle
    manager.listMessages.mockResolvedValue(interruptedTranscript)
    const adapter = makeAdapter(manager, { recoveryStore: makeTempRecoveryStore() })
    await adapter.attach!({ sessionId: 'ses_int', cwd: '/w' })
    await flushSendQueue()   // await adapter's send queue; e.g. await new Promise(r => setTimeout(r, 0)) loops or expose a test hook
    expect(manager.promptAsync).toHaveBeenCalledTimes(1)
    const [, body] = manager.promptAsync.mock.calls[0]
    expect(body.parts[0].text).toMatch(/interrupted/i)

    // Second attach (same store): ledger suppresses
    manager.promptAsync.mockClear()
    await adapter.attach!({ sessionId: 'ses_int', cwd: '/w' })
    await flushSendQueue()
    expect(manager.promptAsync).not.toHaveBeenCalled()
  })

  it('never recovers after an explicit user interrupt, across adapter instances', async () => {
    const store = makeTempRecoveryStore()
    const manager = makeFakeManager()
    manager.getSessionStatus.mockResolvedValue(undefined)
    manager.listMessages.mockResolvedValue(interruptedTranscript)
    const adapter1 = makeAdapter(manager, { recoveryStore: store })
    await adapter1.attach!({ sessionId: 'ses_int', cwd: '/w' })
    await adapter1.interrupt!('ses_int')                            // user stop
    const adapter2 = makeAdapter(makeFakeManager(), { recoveryStore: store })   // simulated restart
    const manager2 = /* the fresh manager used above */
    manager2.getSessionStatus.mockResolvedValue(undefined)
    manager2.listMessages.mockResolvedValue(interruptedTranscript)
    await adapter2.attach!({ sessionId: 'ses_int', cwd: '/w' })
    await flushSendQueue()
    expect(manager2.promptAsync).not.toHaveBeenCalled()
  })

  it('does not recover when the user already sent a follow-up', async () => {
    const manager = makeFakeManager()
    manager.getSessionStatus.mockResolvedValue(undefined)
    manager.listMessages.mockResolvedValue({
      messages: [...interruptedTranscript.messages,
        { info: { id: 'm3', role: 'user', time: { created: OLD + 5 } }, parts: [] }],
      nextCursor: null,
    })
    const adapter = makeAdapter(manager, { recoveryStore: makeTempRecoveryStore() })
    await adapter.attach!({ sessionId: 'ses_int', cwd: '/w' })
    await flushSendQueue()
    expect(manager.promptAsync).not.toHaveBeenCalled()
  })

  it('a normal user send clears recorded interrupt intent', async () => {
    const store = makeTempRecoveryStore()
    const manager = makeFakeManager()
    const adapter = makeAdapter(manager, { recoveryStore: store })
    await adapter.attach!({ sessionId: 'ses_int', cwd: '/w' })
    await adapter.interrupt!('ses_int')
    expect(await store.hasInterrupt('ses_int')).toBe(true)
    await adapter.send!('ses_int', { text: 'user follow-up' })
    expect(await store.hasInterrupt('ses_int')).toBe(false)
  })
})
```

`makeTempRecoveryStore()` = `new FreshAgentRecoveryStore({ filePath: path.join(await mkdtemp(...), 'r.json') })` — add it as a suite helper. Adapt `interrupt`/`send` call signatures to the adapter's real API (the existing suite has both — copy).

- [ ] **Step 2: Run to verify fail** — the recovery tests FAIL (no continuation injected / intent not persisted).

- [ ] **Step 3: Implement** per the Interfaces block: adapter options gain `recoveryStore?: FreshAgentRecoveryStore`; `interrupt()` records intent; user sends clear intent; `resume()`/`attach()` call `void maybeRecoverInterruptedTurn(state)` after `reconcileStatus` **when `state.status === 'idle'`** (fire-and-forget with internal catch — restore must not fail because recovery failed; but for test determinism, expose the promise: store it on `state.pendingRecovery` and have tests await it via the flush helper).

- [ ] **Step 4: Run to verify pass** — `npm run test:vitest -- --run test/unit/server/fresh-agent/opencode-serve-adapter.test.ts` (all 59 pre-existing tests + new ones).

- [ ] **Step 5: Commit**

```bash
git add server/fresh-agent/adapters/opencode/adapter.ts test/unit/server/fresh-agent/opencode-serve-adapter.test.ts
git commit -m "feat(server): one auditable loop-proof Freshell continuation for interrupted opencode turns (zrrj)"
```

---

### Task 15: Idle/persistence freshness — prove the final answer is queryable before completing the turn

The race: `onceIdle` resolves on the first `session.idle` SSE frame (`serve-manager.ts:506-510`) while `message.*` events are invalidations only; nothing sequences the idle edge behind the final assistant message becoming visible on the REST read path (`serve-events.ts:114-116`). Result: the client's post-idle snapshot can miss the final answer, and (pre-Task 3) the pane stopped polling. Fix at the adapter: after `await idle` (`adapter.ts:368`), poll `listMessages` until a completed assistant message newer than the send is visible (bounded), THEN emit idle/turn-complete. Kata accepts "polling for the expected final message" explicitly.

**Files:**
- Modify: `server/fresh-agent/adapters/opencode/adapter.ts` (`materializeOrSend` `:363-381`; also apply after Task 8's monitor resolve)
- Test: `test/unit/server/fresh-agent/opencode-serve-adapter.test.ts`

**Interfaces:**
- Consumes: `serveManager.listMessages(realId, { limit }, route?)`.
- Produces:

```ts
const TRANSCRIPT_SETTLE_POLL_MS = 150
const TRANSCRIPT_SETTLE_MAX_POLLS = 10   // ~1.5 s worst case

/**
 * After idle, verify the final assistant message is queryable via the REST
 * read path before declaring the turn complete (kata zrrj freshness contract).
 * Returns true when settled; false when polling exhausted (idle is still
 * emitted — the turn is not stranded — but the client re-poll (Task 16) covers the gap).
 */
async function awaitTranscriptSettled(state: OpencodeSessionState, sentAtMs: number): Promise<boolean>
```

Settled means: `listMessages` (limit e.g. 20, newest page) contains an assistant message with `info.time.created >= sentAtMs - CLOCK_SKEW_MS` (use `CLOCK_SKEW_MS = 5_000`) AND a finite `info.time.completed`. Poll with `TRANSCRIPT_SETTLE_POLL_MS` sleeps between attempts; inject the sleep fn for tests (`options.sleep`, default `(ms) => new Promise(r => setTimeout(r, ms))`).

In `materializeOrSend`, capture `const sentAtMs = Date.now()` just before `promptAsyncForState` (`:363`), then after `await idle` (`:368`) insert `await awaitTranscriptSettled(state, sentAtMs)` BEFORE `emitStatus(state, 'idle')` (`:371`) and the chime (`:377-381`). Also call it (with `sentAtMs = 0`, i.e. "any completed assistant message") in Task 8's monitor resolve path before its `emitStatus(state, 'idle')`.

- [ ] **Step 1: Write the failing tests**

```ts
describe('idle freshness (zrrj)', () => {
  it('withholds idle and turn-complete until the final assistant message is queryable', async () => {
    const manager = makeFakeManager()
    const idle = createDeferred<void>()
    manager.onceIdle.mockReturnValue(idle.promise)
    // First two polls: transcript still missing the final answer; third poll: it appears.
    const unfinished = { messages: [{ info: { id: 'm1', role: 'user', time: { created: Date.now(), completed: Date.now() } }, parts: [] }], nextCursor: null }
    const finished = { messages: [...unfinished.messages, { info: { id: 'm2', role: 'assistant', time: { created: Date.now(), completed: Date.now() } }, parts: [] }], nextCursor: null }
    manager.listMessages
      .mockResolvedValueOnce(unfinished)
      .mockResolvedValueOnce(unfinished)
      .mockResolvedValue(finished)
    const adapter = makeAdapter(manager, { settleSleep: async () => {} })   // injected no-op sleep
    const events: any[] = []
    // create+subscribe per the suite's send-test idiom, then:
    const sendPromise = adapter.send!('freshopencode-placeholder-1', { text: 'q' })
    idle.resolve()
    await sendPromise
    expect(manager.listMessages).toHaveBeenCalledTimes(3)
    const idleIndex = events.findIndex((e) => e.type === 'sdk.session.snapshot' && e.status === 'idle')
    const completeIndex = events.findIndex((e) => e.type === 'sdk.turn.complete')
    expect(idleIndex).toBeGreaterThanOrEqual(0)
    expect(completeIndex).toBeGreaterThan(idleIndex - 1)   // both emitted, after settling
  })

  it('gives up after the poll budget but still emits idle (never strands the pane busy)', async () => {
    const manager = makeFakeManager()
    const idle = createDeferred<void>()
    manager.onceIdle.mockReturnValue(idle.promise)
    manager.listMessages.mockResolvedValue({ messages: [], nextCursor: null })   // never settles
    const adapter = makeAdapter(manager, { settleSleep: async () => {} })
    const events: any[] = []
    const sendPromise = adapter.send!('freshopencode-placeholder-1', { text: 'q' })
    idle.resolve()
    await sendPromise
    expect(manager.listMessages.mock.calls.length).toBeLessThanOrEqual(11)   // 1 + max 10 polls
    expect(events.some((e) => e.type === 'sdk.session.snapshot' && e.status === 'idle')).toBe(true)
  })
})
```

(Wire `events` collection through the suite's normal subscribe idiom; note `assembleExport` also calls `listMessages` for snapshots — if the existing send path already calls it, adjust expected call counts accordingly by inspecting the first failing run's actual counts, keeping the ORDER assertions as the real contract.)

- [ ] **Step 2: Run to verify fail** — idle is emitted immediately today; `listMessages` not polled.

- [ ] **Step 3: Implement** `awaitTranscriptSettled` + the two call sites; thread `settleSleep` through adapter options (default real sleep). On `false` (exhausted), also emit an observability row: `recordFreshAgentObservabilityEvent({ kind: 'fresh_agent_monitor', provider: 'opencode', sessionIdHash, phase: 'timeout', ... })` is the wrong kind — instead log `log.warn({...}, 'transcript did not settle after idle')`.

- [ ] **Step 4: Run to verify pass** — full adapter suite.

- [ ] **Step 5: Commit**

```bash
git add server/fresh-agent/adapters/opencode/adapter.ts test/unit/server/fresh-agent/opencode-serve-adapter.test.ts
git commit -m "fix(server): prove the final assistant message is queryable before emitting opencode idle/turn-complete (zrrj)"
```

---

### Task 16: Client — never permanently stop refreshing on an incomplete idle snapshot

Belt-and-suspenders for the same race (kata requires the browser-side guarantee explicitly): when a snapshot arrives with `status: 'idle'` while this pane still has an unreconciled local echo (the user's message was sent but the snapshot doesn't contain it or its answer yet), schedule a bounded re-poll instead of going quiet.

**Files:**
- Modify: `src/components/fresh-agent/FreshAgentView.tsx` (inside `applySnapshot` from Task 3)
- Test: `test/unit/client/components/fresh-agent/FreshAgentView.test.tsx`

**Interfaces:**
- Consumes: the existing local-echo reconcile logic inside the old `.then` body (`:1618-1707` — it already computes whether the echo was reconciled); `requestSnapshotRefresh('idle-incomplete')` (Task 3); scheduler debounce=0 for `idle-incomplete`.
- Produces: `idleIncompleteRetryCountRef = useRef(0)`, max `IDLE_INCOMPLETE_MAX_RETRIES = 5`, retry delay 1000 ms via `window.setTimeout`; counter resets whenever a snapshot reconciles the echo or a new send starts.

- [ ] **Step 1: Write the failing test**

```ts
it('keeps re-polling (bounded) when an idle snapshot is missing the just-sent turn', async () => {
  // Arrange: pane with a pending local echo for 'question?' (drive the composer),
  // snapshot responses are idle and DO NOT contain the echo's turn.
  apiMock.getFreshAgentThreadSnapshot.mockResolvedValue(makeSnapshot({ threadId: 'ses_1', status: 'idle', turns: [] }))
  render(<StoreBackedFreshAgentView ... />)
  await typeAndSend('question?')
  act(() => wsHandler({ type: 'freshAgent.send.accepted', requestId: lastSendRequestId(), sessionId: 'ses_1', sessionType: 'freshopencode', provider: 'opencode' }))
  const before = apiMock.getFreshAgentThreadSnapshot.mock.calls.length
  // Assert: more snapshot fetches keep coming (retry loop), then stop at the cap
  await waitFor(() => expect(apiMock.getFreshAgentThreadSnapshot.mock.calls.length).toBeGreaterThan(before), { timeout: 4_000 })
})
```

- [ ] **Step 2: Run to verify fail** — after the send-accepted refresh returns an idle snapshot, no further fetches happen today (busy never set → poll gate off).

- [ ] **Step 3: Implement** in `applySnapshot`: after the existing echo-reconcile computation, add:

```ts
const echoStillPending = /* the existing 'echo not reconciled by this snapshot' condition */
if (snapshotStatus === 'idle' && echoStillPending
  && idleIncompleteRetryCountRef.current < IDLE_INCOMPLETE_MAX_RETRIES) {
  idleIncompleteRetryCountRef.current += 1
  window.setTimeout(() => requestSnapshotRefresh('idle-incomplete'), 1_000)
} else if (!echoStillPending) {
  idleIncompleteRetryCountRef.current = 0
}
```

- [ ] **Step 4: Run to verify pass** — FreshAgentView suite.

- [ ] **Step 5: Commit**

```bash
git add src/components/fresh-agent/FreshAgentView.tsx test/unit/client/components/fresh-agent/FreshAgentView.test.tsx
git commit -m "fix(client): bounded re-poll when an idle snapshot is missing the just-sent turn (zrrj)"
```

---

### Task 17: Identity-bearing structured rows on every FreshAgent action + snapshot trigger metadata

`freshAgent.send`/`interrupt`/`compact` log **nothing** today (`ws-handler.ts:3512-3558`, `:3560-3574`); `create`/`attach` log only on failure; materialization has no row at any of its three points; snapshot rows lack the trigger and never fire on errors. This is exactly what blocked the incident RCA ("the user's manual stop/start action is still not conclusively identifiable").

**Files:**
- Modify: `server/ws-handler.ts` (cases at `:3467`, `:3512`, `:3560`; materialization at `:3534-3541` and `:1408-1434`)
- Modify: `server/fresh-agent/router.ts` (`:169-229` snapshot handler; `sendFreshAgentError` `:99-167`)
- Modify: `src/lib/api.ts` (`getFreshAgentThreadSnapshot` — send `trigger`)
- Modify: `src/components/fresh-agent/FreshAgentView.tsx` (pass trigger into the fetch)
- Test: `test/unit/server/ws-handler-fresh-agent.test.ts`, `test/unit/server/fresh-agent/router.test.ts`, `test/unit/client/lib/api.test.ts`

**Interfaces:**
- Consumes: event kinds from Task 6; `hashForLogs`; `freshAgentLocatorFromMessage` (`ws-handler.ts:1288` — carries `{sessionId, sessionType, provider, cwd?}` at every call site already).
- Produces:
  - `freshAgent.send`: one `fresh_agent_send` row per attempt (`outcome: 'accepted' | 'failed'`, `errorCode`, `durationMs`, `requestId`).
  - `freshAgent.interrupt`: `fresh_agent_interrupt` row (`outcome`).
  - `freshAgent.attach`: `fresh_agent_attach` row on success too (`recovered: true` when it came from Task 9's retry path).
  - Materialization: `fresh_agent_materialized` row emitted where the WS layer sends `freshAgent.session.materialized` (`ws-handler.ts:1408-1434` — single choke point).
  - Snapshot route: accept `trigger` in the query schema (`z.string().max(32).optional()`), include it in `fresh_agent_snapshot_served`; add a `fresh_agent_snapshot_failed` call inside `sendFreshAgentError` with `httpStatus`, `code`, `trigger`, `durationMs`.
  - Client: `getFreshAgentThreadSnapshot(..., { revision?, cwd?, trigger?, signal? })` appends `['trigger', query.trigger]` to `buildQueryString`; FreshAgentView passes the scheduler trigger.

- [ ] **Step 1: Write the failing tests**

Server (ws): in `ws-handler-fresh-agent.test.ts`, mock the observability module like the adapter suite does (`vi.mock` + spy on `recordFreshAgentObservabilityEvent`):

```ts
it('emits identity-hashed send and interrupt rows', async () => {
  // drive a successful freshAgent.send then a freshAgent.interrupt (existing arrange helpers)
  expect(observabilitySpy).toHaveBeenCalledWith(expect.objectContaining({
    kind: 'fresh_agent_send', sessionType: 'freshopencode', provider: 'opencode',
    sessionIdHash: hashForLogs('ses_9'), outcome: 'accepted',
  }))
  expect(observabilitySpy).toHaveBeenCalledWith(expect.objectContaining({
    kind: 'fresh_agent_interrupt', outcome: 'ok', sessionIdHash: hashForLogs('ses_9'),
  }))
  // No raw ids or prompt text anywhere in the payloads:
  for (const [payload] of observabilitySpy.mock.calls) {
    expect(JSON.stringify(payload)).not.toContain('ses_9')
    expect(JSON.stringify(payload)).not.toContain('hello')
  }
})
```

Router: in `test/unit/server/fresh-agent/router.test.ts` (supertest harness already exists):

```ts
it('threads the trigger query param into fresh_agent_snapshot_served', async () => {
  await request(app).get('/api/fresh-agent/threads/freshopencode/opencode/ses_1?trigger=poll').expect(200)
  expect(observabilitySpy).toHaveBeenCalledWith(expect.objectContaining({
    kind: 'fresh_agent_snapshot_served', trigger: 'poll',
  }))
})

it('emits fresh_agent_snapshot_failed with the mapped status on errors', async () => {
  runtimeManagerStub.getSnapshot.mockRejectedValueOnce(new FreshAgentLostSessionError('gone'))
  await request(app).get('/api/fresh-agent/threads/freshopencode/opencode/ses_gone?trigger=event').expect(404)
  expect(observabilitySpy).toHaveBeenCalledWith(expect.objectContaining({
    kind: 'fresh_agent_snapshot_failed', httpStatus: 404, trigger: 'event',
  }))
})
```

Client: in `api.test.ts`, assert the fetch URL contains `trigger=poll` when passed.

- [ ] **Step 2: Run to verify fail** — rows absent; `trigger` rejected/ignored.

- [ ] **Step 3: Implement** per Interfaces. In the ws-handler, wrap each case body: capture `const startedAt = Date.now()` and emit the row in both success and error exits (including Task 9's retry path with `recovered: true` on the attach row). In `sendFreshAgentError`, thread `trigger`/`startedAt` via an optional context arg from the snapshot handler. In `applySnapshot`'s executor (FreshAgentView), pass `trigger` through to `getFreshAgentThreadSnapshot`.

- [ ] **Step 4: Run to verify pass**

Run: `npm run test:vitest -- --run test/unit/server/ws-handler-fresh-agent.test.ts test/unit/server/fresh-agent/router.test.ts test/unit/client/lib/api.test.ts`

- [ ] **Step 5: Commit**

```bash
git add server/ws-handler.ts server/fresh-agent/router.ts src/lib/api.ts src/components/fresh-agent/FreshAgentView.tsx test/unit/server/ test/unit/client/lib/api.test.ts
git commit -m "feat(server,client): identity-hashed structured rows for all fresh-agent actions and snapshot trigger metadata (zrrj)"
```

---

### Task 18: Read-only incident-state inspection endpoint

Future incidents need live state without message content: current live status per tracked session, latest status source, active idle-recovery monitors, materialization map, sidecar generation/pid/baseUrl, and a transcript *summary* (counts + hashes only). Clone the `server/debug-router.ts` pattern (34 lines, mounted at `/api/debug`, `server/index.ts:788-795`).

**Files:**
- Create: `server/fresh-agent/incident-router.ts`
- Modify: `server/fresh-agent/runtime-manager.ts` (add `inspectState()`), `server/fresh-agent/adapters/opencode/adapter.ts` (add `inspectSessions()`), `server/fresh-agent/adapters/opencode/serve-manager.ts` (Task 7's `describeSidecar()`), `server/index.ts` (mount)
- Test: `test/unit/server/fresh-agent/incident-router.test.ts`

**Interfaces:**
- Produces:

```ts
// runtime-manager.ts
inspectState(): {
  sessions: Array<{ key: string; sessionType: string; provider: string; sessionIdHash: string; cwdHash?: string; providerOwned?: boolean }>
  pendingRecoveries: number
}

// adapter.ts (opencode) — module-level registry read
export function inspectOpencodeSessions(): Array<{
  sessionIdHash: string; status: string; hasRealSession: boolean
  cwdHash?: string; monitorArmed: boolean; turnAborted?: boolean; turnErrored?: boolean
  lastTurnCompleteAt?: number
}>

// incident-router.ts
export interface IncidentRouterDeps {
  runtimeManager: { inspectState: () => unknown }
  opencode: { inspectSessions: () => unknown; describeSidecar: () => unknown }
}
export function createFreshAgentIncidentRouter(deps: IncidentRouterDeps): Router
// GET / -> { version: 1, time, runtime: inspectState(), opencode: { sessions, sidecar } }
```

Mounted at `app.use('/api/debug/fresh-agent', createFreshAgentIncidentRouter({...}))` right beside the existing debug router (`server/index.ts:788-795`) — **after** auth (`:215`) so it stays token-protected; it shares the global budget, which is acceptable because a human polls it manually (document this in a comment).

- [ ] **Step 1: Write the failing test** (supertest, structural deps — mirror `test/server/api.test.ts`'s `/api/debug` test):

```ts
it('returns hashed, content-free incident state', async () => {
  const app = express()
  app.use('/api/debug/fresh-agent', createFreshAgentIncidentRouter({
    runtimeManager: { inspectState: () => ({ sessions: [{ key: 'k', sessionType: 'freshopencode', provider: 'opencode', sessionIdHash: 'abc123', cwdHash: 'def456' }], pendingRecoveries: 0 }) },
    opencode: {
      inspectSessions: () => [{ sessionIdHash: 'abc123', status: 'running', hasRealSession: true, monitorArmed: true }],
      describeSidecar: () => ({ generation: 2, pid: 4242, baseUrl: 'http://127.0.0.1:1234' }),
    },
  }))
  const res = await request(app).get('/api/debug/fresh-agent').expect(200)
  expect(res.body).toMatchObject({ version: 1, opencode: { sidecar: { generation: 2 } } })
  expect(typeof res.body.time).toBe('string')
})
```

Plus unit tests for `inspectOpencodeSessions()` in the adapter suite (arrange a running session with an armed monitor, assert the hashed summary and assert `JSON.stringify(result)` contains neither the raw `ses_` id nor any message text).

- [ ] **Step 2: Run to verify fail** — modules missing.

- [ ] **Step 3: Implement** — all three inspection functions are read-only map walks using `hashForLogs`; the router is a `debug-router.ts` clone with the versioned envelope.

- [ ] **Step 4: Run to verify pass** — `npm run test:vitest -- --run test/unit/server/fresh-agent/incident-router.test.ts test/unit/server/fresh-agent/opencode-serve-adapter.test.ts`

- [ ] **Step 5: Commit**

```bash
git add server/fresh-agent/incident-router.ts server/fresh-agent/runtime-manager.ts server/fresh-agent/adapters/opencode/ server/index.ts test/unit/server/fresh-agent/
git commit -m "feat(server): read-only /api/debug/fresh-agent incident-state endpoint, content-free (zrrj)"
```

---

### Task 19: Terminal-stream ↔ freshAgent backpressure coupling — prove it

Kata area 6 requires proof, not assumption. Investigation verdict (to be encoded in tests): **coupled, but not via `retention_lost` itself**. In current main, `retention_lost` is log-only (rate-limited 1/s/terminal, `broker.ts:90-94`, `:1276-1305`, `:2235-2267`) — it writes nothing to the socket (the write-amplifying behavior was removed by `ff8589bb`, 2026-06-25). The real coupling: terminal output and freshAgent events share one socket with **asymmetric policy** — `WsHandler.send()` drops the message AND closes the socket with 4008 when `bufferedAmount > 2 MiB` (`ws-handler.ts:1597-1604`, `:233`, `:265`), while the broker writes with **no** buffered-amount gate (`broker.ts:2107-2113`, `:1347`) and only pauses `background` panes at 512 KiB (`broker.ts:859-869`); its own kill line is 16 MiB/10s (`:1087-1130`) — 8× above the handler's. In the 2–16 MiB window every freshAgent lifecycle event dies and the connection is torn down; after the 4008 close, all freshAgent subscriptions are cancelled (`ws-handler.ts:1579-1595`) with no backlog/replay.

**Files:**
- Create: `test/unit/server/ws-handler-fresh-agent-backpressure.test.ts`
- Reference harness: `test/unit/server/ws-handler-backpressure.test.ts:42-105` (`createMockWs` with settable `bufferedAmount`, `structuredLogs`, `FakeBrokerRegistry`)

**Interfaces:**
- Consumes: `createMockWs`, `FakeBrokerRegistry` (copy the helpers into the new file or export them from a shared test helper — prefer extracting to `test/helpers/ws-backpressure.ts` and importing from both files so the originals stay green).
- Produces: four tests. Tests 1 is written to assert the DESIRED behavior (fails RED now, goes green with Task 20); tests 2–4 pin down current mechanics as regression evidence.

- [ ] **Step 1: Write the tests**

```ts
describe('terminal output vs freshAgent lifecycle on one socket (zrrj)', () => {
  it('delivers freshAgent.turn.complete while terminal output has inflated bufferedAmount to 3 MiB', () => {
    // DESIRED behavior (RED until Task 20): a freshAgent event in the 2-16 MiB window
    // must still be delivered and must NOT close the socket.
    const ws = createMockWs({ bufferedAmount: 3 * 1024 * 1024 })
    // arrange: WsHandler with a freshAgent subscription on this ws (reuse the
    // subscription-listener arrangement from ws-handler-fresh-agent-lifecycle-parity.test.ts)
    fireFreshAgentEvent({ type: 'freshAgent.turn.complete', sessionId: 'ses_1', at: Date.now() })
    const sentTypes = ws.send.mock.calls.map(([f]: [string]) => JSON.parse(f).type)
    expect(sentTypes).toContain('freshAgent.event')
    expect(ws.close).not.toHaveBeenCalledWith(4008, expect.anything())
  })

  it('EVIDENCE: retention_lost is log-only and emits zero WS frames in the current build', () => {
    // registry.setReplayRingMaxBytes(1024); flood terminal.output.raw past the ring;
    // assert structuredLogs('warn', 'terminal.replay.retention') fired
    // and ws.send saw no 'terminal.stream.changed' frames and no send-count delta from retention itself.
  })

  it('EVIDENCE: broker flushes to a foreground attachment without any bufferedAmount gate today', () => {
    // set ws.bufferedAmount = 3 MiB, attach a foreground pane, emit output,
    // assert the broker still called ws.send (this pins the pre-fix asymmetry;
    // Task 20 flips this assertion).
  })

  it('EVIDENCE: after a 4008 backpressure close, freshAgent events are dropped with no replay', () => {
    // drive WsHandler.send over the 2 MiB line -> close(4008); then fire another
    // freshAgent event and assert nothing was queued/replayed for the dead socket.
  })
})
```

Flesh out the arrange code from the two referenced suites — both already construct `WsHandler` + broker against mock sockets; reuse their construction verbatim.

- [ ] **Step 2: Run and record the verdict**

Run: `npm run test:vitest -- --run test/unit/server/ws-handler-fresh-agent-backpressure.test.ts`
Expected: test 1 FAILS (message dropped + socket closed) — that failure IS the coupling proof. Tests 2–4 PASS (they encode current mechanics). Keep test 1 failing (skip-marked `it.fails(...)` is NOT allowed — instead proceed immediately to Task 20 in the same PR; the suite must be green only after Task 20).

- [ ] **Step 3: Commit** (tests only, with test 1 temporarily asserting the CURRENT broken behavior inverted — to keep the tree green mid-plan, write test 1 with the desired assertions but wrap the two assertions in `expect(...)` guarded by a `// zrrj Task 20 flips broker gating` comment and commit it together with Task 20 if the repo's per-task green policy requires it. Preferred: implement Task 19+20 as one RED→GREEN cycle, committing the test file in Task 20's commit.)

```bash
git add test/unit/server/ws-handler-fresh-agent-backpressure.test.ts test/helpers/ws-backpressure.ts
# commit happens with Task 20 (single RED->GREEN cycle across the two tasks)
```

---

### Task 20: Fix the coupling — broker foreground pause below the handler kill line

Chosen fix (kata: "if coupled, fix the broker/backpressure/lifecycle-event-loss path in the same change"): make terminal output self-throttle **before** control messages start dying. Add a foreground pause threshold at 1 MiB — half the handler's 2 MiB kill line — using the exact retry mechanic the background path already uses (`broker.ts:859-869`): skip the flush and retry in 100 ms. Terminal output degrades gracefully (it has queues + replay + `sinceSeq`); freshAgent events keep flowing because `bufferedAmount` can no longer be inflated past 2 MiB by terminal traffic alone. The 4008 close in `WsHandler.send` stays — it now fires only under genuine non-terminal pathology.

**Files:**
- Modify: `server/terminal-stream/constants.ts` (new constant), `server/terminal-stream/broker.ts` (`flushAttachment` `:840-958`)
- Test: `test/unit/server/ws-handler-fresh-agent-backpressure.test.ts` (Task 19 test 1 + flipped test 3), `test/unit/server/ws-handler-backpressure.test.ts` (existing suite must stay green)

**Interfaces:**
- Produces, in `constants.ts` (beside the background threshold at `:23-26`):

```ts
/**
 * Foreground attachments pause flushing when the socket has this much
 * unflushed data. MUST stay below WsHandler's maxWsBufferedAmount (2 MiB)
 * kill line: terminal output must self-throttle before lifecycle messages
 * (freshAgent.*, session updates) start being dropped with a 4008 close. (zrrj)
 */
export const TERMINAL_STREAM_FOREGROUND_PAUSE_BUFFERED_BYTES = 1 * 1024 * 1024
```

- In `flushAttachment` (`:840-958`), where the background pause currently checks `attachment.priority === 'background' && bufferedAmount > TERMINAL_STREAM_BACKGROUND_PAUSE_BUFFERED_BYTES`, add the foreground equivalent so EVERY attachment pauses at its threshold (background keeps its lower 512 KiB threshold; foreground gets 1 MiB), reusing the identical retry-in-100ms scheduling.

- [ ] **Step 1: Update Task 19's test 3** to assert the new behavior (broker does NOT call `ws.send` when `bufferedAmount` = 3 MiB for a foreground pane; instead it schedules a retry — assert via fake timers or the broker's retry bookkeeping the way the existing catastrophic-spike test at `ws-handler-backpressure.test.ts:271` observes deferred behavior).

- [ ] **Step 2: Run to verify RED** — test 1 and updated test 3 fail against the ungated broker.

Run: `npm run test:vitest -- --run test/unit/server/ws-handler-fresh-agent-backpressure.test.ts`

- [ ] **Step 3: Implement** the constant + the `flushAttachment` gate.

- [ ] **Step 4: Run to verify GREEN + no regressions**

Run: `npm run test:vitest -- --run test/unit/server/ws-handler-fresh-agent-backpressure.test.ts test/unit/server/ws-handler-backpressure.test.ts test/unit/server/terminal-stream/`
Expected: all PASS — including the existing "does not close the socket for short-lived catastrophic bufferedAmount spikes" (`:271`) and "closes after sustained catastrophic bufferedAmount" (`:302`) tests (the 16 MiB/10s catastrophic path remains the final backstop).

- [ ] **Step 5: Record the area-6 verdict for the kata.** Write the evidence summary into the plan-adjacent notes file `docs/plans/2026-07-27-freshopencode-restart-recovery-area6-evidence.md`:

```markdown
# zrrj Area 6 verdict: COUPLED (fixed in this change)

- retention_lost itself is log-only in current main (post-ff8589bb, 2026-06-25) and emits
  zero WS frames — proven by test 'retention_lost is log-only...'. The incident's 10k+/10min
  retention_lost count is incompatible with the current 1/s/terminal rate limit, so the
  incident build predated ff8589bb, when retention loss also pushed a terminal.stream.changed
  frame per attached client (a direct WS write amplifier).
- The load-bearing coupling: terminal output and freshAgent lifecycle events share one socket
  with asymmetric backpressure policy (broker ungated vs WsHandler drop+close at 2 MiB).
  Proven by test 'delivers freshAgent.turn.complete while terminal output has inflated
  bufferedAmount' (RED before the fix).
- Fix shipped: TERMINAL_STREAM_FOREGROUND_PAUSE_BUFFERED_BYTES = 1 MiB broker pause below
  the 2 MiB kill line. The incident's 'WebSocket send callback reported failure' rows match
  ws-send.ts:167-174 in the same congestion regime.
```

This file is the evidence the kata-close comment cites.

- [ ] **Step 6: Commit** (includes Task 19's test file)

```bash
git add server/terminal-stream/ test/unit/server/ws-handler-fresh-agent-backpressure.test.ts test/helpers/ws-backpressure.ts docs/plans/2026-07-27-freshopencode-restart-recovery-area6-evidence.md
git commit -m "fix(server): terminal output self-throttles below the WS kill line so freshAgent lifecycle events survive floods; prove retention_lost is log-only (zrrj)"
```

---

### Task 21: Full-suite verification and landing prep

**Files:** none new (fixups only if the suite finds integration breakage).

- [ ] **Step 1: Typecheck + coordinated full suite**

```bash
FRESHELL_TEST_SUMMARY="zrrj freshopencode restart recovery - full verification" npm run check
```

Expected: typecheck clean; both vitest configs green. This change touches client + server, so `npm run check` (not bare `npm test`) is the required gate. Fix any failures with focused RED→GREEN cycles and dedicated commits (`fix(test): ...`).

- [ ] **Step 2: Lint (a11y is CI-required)**

```bash
npm run lint
```

Expected: clean (the FreshAgentView changes add no new interactive elements, but verify).

- [ ] **Step 3: Focused re-run of the kata's named targets** (final receipts):

```bash
npm run test:vitest -- --run test/unit/client/components/fresh-agent/FreshAgentView.test.tsx test/unit/client/lib/fresh-agent-ws.test.ts test/unit/client/lib/api.test.ts
npm run test:vitest -- --run test/unit/server/fresh-agent/opencode-serve-adapter.test.ts test/unit/server/fresh-agent/opencode-serve-manager.test.ts test/unit/server/ws-handler-fresh-agent.test.ts
npm run test:vitest -- --run test/unit/server/coding-cli/opencode-activity-tracker.test.ts test/integration/server/opencode-session-flow.test.ts
```

Expected: all PASS.

- [ ] **Step 4: Commit any stragglers; the branch is ready for the workflow's review/landing stages.**

Landing notes for the finish stage (explicit user pre-approval EXISTS for this change — "fix each of the bugs you indicated with the usual, landing them on main as you go"): push the branch, open the PR targeting `main`, wait for required checks, squash-merge (repo norm), fast-forward local `main`, then close the kata from the repo root with evidence:

```bash
kata close zrrj --done --commit <merged sha> --message "Landed the canonical fix: client per-key snapshot scheduler with Retry-After/exponential 429 backoff and last-good-snapshot retention; idle HTTP snapshots clear stale opencode busy; restore reconciliation now emits status and arms exactly one monitored idle recovery per durable session with structured interruption on sidecar loss/timeout; serve subscriptions survive sidecar replacement (generation/pid/baseUrl identity added); FRESH_AGENT_LOST_SESSION is preserved over WS with one-shot attach-recover-retry on both server and client; interrupted turns are detected from transcript evidence (missing completion, MessageAbortedError, running tool parts, token accounting, missing step-finish) with a durable interrupt-intent store, a crash-safe recovery ledger, and at most one auditable Freshell-owned continuation; idle is only emitted after the final assistant message is queryable (missing-final-answer race); always-on fresh-agent observability logger with identity-hashed send/interrupt/attach/materialization/monitor/sidecar/snapshot rows and /turns 429 coverage; read-only /api/debug/fresh-agent incident endpoint; area-6 verdict: terminal/WS storm was COUPLED via asymmetric backpressure (broker ungated vs 2MiB drop+close) — fixed with a 1MiB foreground broker pause; retention_lost itself proven log-only post-ff8589bb (evidence in docs/plans/2026-07-27-freshopencode-restart-recovery-area6-evidence.md)."
```

Do NOT close the kata if any verification step is unverified.

---

## Self-Review

**1. Spec coverage** (kata Required Work → tasks):

| Kata requirement | Task(s) |
|---|---|
| Per snapshot-key scheduler, one in-flight per key, trailing coalesce | 2, 3 |
| Debounce transcript-change bursts | 2 (250 ms debounced triggers), 3 |
| Route ALL refresh triggers (manual, materialization, send-accepted, lifecycle, fallback) through it | 3 (all eight sites enumerated) |
| Honor Retry-After/backoff on 429, keep last-good snapshot visible, suppress until expiry | 1, 2, 3 |
| Filter send.accepted to owning pane/session | 3 (gates kept + regression test) |
| Successful idle HTTP snapshot clears stale busy | 4 |
| Reconcile live status on durable resume()/attach() before assuming idle | pre-existing `reconcileStatus` + 8 (now emits) |
| If running: emit running + exactly ONE monitored idle recovery per durable key | 8 |
| If idle/absent: keep idle, no long waiter | 8 (no-monitor test) |
| Structured interruption/recovery signals on sidecar loss/timeout/state loss | 8 |
| Serve subscriptions survive sidecar replacement, proven by tests | 7 |
| Materialized ses_ sends route or attach/recover + retry once | 9 (server), 10 (client) |
| Don't swallow genuinely invalid lost-session errors | 9 (placeholder/non-durable guard + test) |
| Interrupted-turn detection from transcript/tool evidence (all listed signals) | 11 (evidence surfacing), 13 (detector: missing completion, MessageAbortedError, running tool, token accounting, missing step-finish), 8+14 (sidecar loss / shutdown / restored-running via monitor+restore paths) |
| Stability window (still-writing rows) | 13 |
| At most ONE continuation, loop prevention, clear transcript event | 12 (ledger), 14 |
| Never auto-recover after user stop / user follow-up | 12 (durable intent), 13 (trailing-user rule), 14 |
| Missing-final-answer race with proven freshness | 15 (server polling contract), 16 (client bounded re-poll) |
| Browser must not permanently stop refreshing on incomplete idle snapshot | 16 |
| Permanent low-volume structured logs, no content, identity on all rows | 6, 17 |
| Snapshot metadata (trigger, status, revision, counts, bytes, duration, HTTP status, retry) | 6, 17 (`fresh_agent_snapshot_served` already carries status/revision/counts/bytes/duration; trigger + failure rows added) |
| Rate-limit metadata for FreshAgent snapshot routes | 6 (retryAfterSeconds + /turns coverage), 5 (JSON 429) |
| Sidecar generation/PID/base-URL, monitored waits bound to generation | 7, 8 (monitor rows carry sidecarGeneration) |
| Read-only incident-state inspection, no message content | 18 |
| Terminal/WS storm: prove coupled vs independent; fix if coupled | 19 (proof), 20 (fix + recorded evidence) |
| Non-goal: don't weaken the global limiter | 5 (same budget, asserted by test) |
| Non-goal: no raw content in logs | 11/17/18 leak-assertions |
| Acceptance: restored pane can't stay stuck busy from process-local loss | 4, 8 |
| Acceptance: bounded refreshes + controlled 429 backoff | 2, 3 |
| Acceptance: unrelated panes don't refetch on another pane's send | 3 (test) |
| Acceptance: no stale "not tracked" for attachable durable sessions | 9, 10 |
| Acceptance: logs can answer the incident questions | 6, 7, 17, 18 |

No unresolved coverage gaps identified. Note one deliberate scoping decision inside the spec's own frame: the Rust crates (`crates/freshell-freshagent`, `crates/freshell-opencode`) mirror some of this logic; the kata's required work and all named test targets are TS-side, and the running product path exercised in the incident is the TS server. The normalizer change (Task 11) is additive under `extensions` so Rust parity is not broken, merely not extended — flag this in the PR description for a product-owner decision on a follow-up.

**1b. No silent deferrals:** Every requirement lands as production behavior in this plan — no stubs, mocks-as-product, or "future work" items. Mocks in tests isolate the sidecar/socket only; the fake-opencode e2e fixture is pre-existing test infrastructure, not a production stand-in. The freshness contract (Task 15) is a production polling implementation, explicitly sanctioned by the kata ("polling for the expected final message ... acceptable if tested").

**2. Placeholder scan:** No TBD/TODO markers. Where a step edits a 4-6k-line existing file, the plan gives exact anchors (file:line + quoted current code) plus complete new code, and where an existing test-file helper must be reused, it names the helper and its location rather than inventing parallel scaffolding — that is an instruction to reuse, not a placeholder. Task 11 carries an explicit field-name verification step because the OpenCode message schema is not documented in-repo; the defensive implementation is fully specified either way.

**3. Type consistency check:** `retryAfterMs` (Task 1) consumed by Task 2's `ApiError` check; `SnapshotTrigger`/`makeSnapshotKey`/`getSnapshotScheduler`/`resetSnapshotSchedulerForTests` (Task 2) consumed in Tasks 3, 16, 17; `describeSidecar`/`currentGeneration` (Task 7) consumed in Tasks 8, 18; `FreshAgentRecoveryStore` methods (Task 12) consumed in Task 14 with matching signatures; `detectInterruptedTurn(messages, { nowMs, stabilityMs? })` (Task 13) called identically in Task 14; observability kinds (Task 6) match every emission site in Tasks 7, 8, 9, 14, 17; `FRESH_AGENT_LOST_SESSION` code string identical in Tasks 9 and 10. Verified consistent.
