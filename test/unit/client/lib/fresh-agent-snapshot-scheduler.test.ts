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

  it('resolves mid-flight sharers with rate-limited on a 429 instead of stranding their promises', async () => {
    const scheduler = getSnapshotScheduler()
    let reject!: (e: unknown) => void
    const gated = new Promise<never>((_, r) => { reject = r })
    const run = vi.fn(() => gated)
    const first = scheduler.schedule(KEY, 'manual', run)
    const midFlight = scheduler.schedule(KEY, 'event', run)   // arrives while in flight -> queued for the trailing run
    reject(new ApiError(429, 'Too many requests', undefined, 5_000))
    const [o1, o2] = await Promise.all([first, midFlight])    // BOTH must settle
    expect(o1.status).toBe('rate-limited')
    expect(o2.status).toBe('rate-limited')
    expect(run).toHaveBeenCalledTimes(1)                      // the trailing run is cancelled: no second network call
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
