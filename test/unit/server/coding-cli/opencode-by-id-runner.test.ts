// @vitest-environment node
import { EventEmitter } from 'events'
import { describe, expect, it, beforeEach, afterEach, vi } from 'vitest'
import fsp from 'fs/promises'
import os from 'os'
import path from 'path'
import {
  createWorkerByIdRunner,
  runOpencodeSessionByIdOffThread,
} from '../../../../server/coding-cli/providers/opencode-by-id-runner'

class FakeWorker extends EventEmitter {
  terminated = 0
  postedData: unknown
  execArgv: string[]
  constructor(public url: URL, public options: { workerData: unknown; execArgv: string[] }) {
    super()
    this.postedData = options.workerData
    this.execArgv = options.execArgv
  }
  terminate() { this.terminated += 1; return Promise.resolve(0) }
  // helpers
  emitMessage(msg: unknown) { this.emit('message', msg) }
  emitError(err: Error) { this.emit('error', err) }
  emitExit(code: number) { this.emit('exit', code) }
}

function makeRunner(overrides: Partial<Parameters<typeof createWorkerByIdRunner>[0]> = {}) {
  const workers: FakeWorker[] = []
  const spawn = vi.fn((url: URL, options: { workerData: unknown; execArgv: string[] }) => {
    const w = new FakeWorker(url, options)
    workers.push(w)
    return w
  })
  const runner = createWorkerByIdRunner({ spawn: spawn as any, timeoutMs: 50, ...overrides })
  return { runner, workers, spawn }
}

const SES_ROOT = 'ses_root0000000000000000000000'
const input = { dbPath: '/tmp/opencode.db', sessionId: SES_ROOT }
const row = { sessionId: SES_ROOT, cwd: '/home/u/oc-proj', title: 'root session', createdAt: 100, lastActivityAt: 200, projectPath: '/home/u/oc-proj' }

describe('createWorkerByIdRunner', () => {
  it('resolves a row from an ok message and terminates the worker', async () => {
    const { runner, workers } = makeRunner()
    const promise = runner(input)
    await Promise.resolve()
    workers[0].emitMessage({ ok: true, row })
    await expect(promise).resolves.toEqual(row)
    expect(workers[0].terminated).toBe(1)
  })

  it('resolves null from an ok-null message (id not in the DB)', async () => {
    const { runner, workers } = makeRunner()
    const promise = runner(input)
    await Promise.resolve()
    workers[0].emitMessage({ ok: true, row: null })
    await expect(promise).resolves.toBeNull()
    expect(workers[0].terminated).toBe(1)
  })

  it('passes dbPath, sessionId, queryModuleUrl and the sentinel in workerData, and suppresses the experimental warning via execArgv', async () => {
    const { runner, workers } = makeRunner()
    const promise = runner(input)
    await Promise.resolve()
    const data = workers[0].postedData as any
    expect(data.dbPath).toBe(input.dbPath)
    expect(data.sessionId).toBe(SES_ROOT)
    expect(String(data.queryModuleUrl)).toContain('opencode-by-id-query')
    expect(data.kind).toBe('opencode-by-id-worker') // sentinel that gates the worker auto-run
    // Appended to process.execArgv so the tsx loader (dev) survives AND the
    // per-spawn node:sqlite ExperimentalWarning is silenced.
    expect(workers[0].execArgv).toEqual([...process.execArgv, '--disable-warning=ExperimentalWarning'])
    workers[0].emitMessage({ ok: true, row: null })
    await promise
  })

  it('ignores a late exit event after a successful message (no double-settle)', async () => {
    const { runner, workers } = makeRunner()
    const promise = runner(input)
    await Promise.resolve()
    workers[0].emitMessage({ ok: true, row })
    workers[0].emitExit(0)
    await expect(promise).resolves.toEqual(row)
    expect(workers[0].terminated).toBe(1)
  })

  it('rejects on an error message and terminates', async () => {
    const { runner, workers } = makeRunner()
    const promise = runner(input)
    await Promise.resolve()
    workers[0].emitMessage({ ok: false, error: { name: 'SqliteError', message: 'database is locked' } })
    await expect(promise).rejects.toThrow(/database is locked/)
    expect(workers[0].terminated).toBe(1)
  })

  it.each([
    ['truncated ok without row key', { ok: true }],
    ['ok:true with a non-object row', { ok: true, row: 'nope' }],
    ['ok:false without error', { ok: false }],
    ['no ok key', { row: null }],
  ])('rejects a malformed message (%s) instead of resolving garbage', async (_label, msg) => {
    const { runner, workers } = makeRunner()
    const promise = runner(input)
    await Promise.resolve()
    workers[0].emitMessage(msg)
    await expect(promise).rejects.toThrow(/malformed|failed/i)
    expect(workers[0].terminated).toBe(1)
  })

  it('rejects on a worker error event and terminates', async () => {
    const { runner, workers } = makeRunner()
    const promise = runner(input)
    await Promise.resolve()
    workers[0].emitError(new Error('worker crashed'))
    await expect(promise).rejects.toThrow(/worker crashed/)
    expect(workers[0].terminated).toBe(1)
  })

  it('rejects when the worker exits before sending a message', async () => {
    const { runner, workers } = makeRunner()
    const promise = runner(input)
    await Promise.resolve()
    workers[0].emitExit(1)
    await expect(promise).rejects.toThrow(/exit/i)
  })

  it('rejects and terminates on timeout', async () => {
    vi.useFakeTimers()
    try {
      const { runner, workers } = makeRunner({ timeoutMs: 25 })
      const promise = runner(input)
      await Promise.resolve()
      const expectation = expect(promise).rejects.toThrow(/timed out/i)
      await vi.advanceTimersByTimeAsync(30)
      await expectation
      expect(workers[0].terminated).toBe(1)
    } finally {
      vi.useRealTimers()
    }
  })
})

describe('runOpencodeSessionByIdOffThread (real worker integration)', () => {
  // Throwaway tmp DB — never the user's real opencode data dir (session safety rule).
  let dir: string
  let dbPath: string

  beforeEach(async () => {
    dir = await fsp.mkdtemp(path.join(os.tmpdir(), 'opencode-byid-runner-'))
    dbPath = path.join(dir, 'opencode.db')
    const { DatabaseSync } = await import('node:sqlite')
    const db = new DatabaseSync(dbPath)
    db.exec(`
      CREATE TABLE project (id TEXT PRIMARY KEY, worktree TEXT);
      CREATE TABLE session (
        id TEXT PRIMARY KEY,
        project_id TEXT,
        parent_id TEXT,
        directory TEXT,
        title TEXT,
        time_created INTEGER,
        time_updated INTEGER,
        time_archived INTEGER
      );
      INSERT INTO project (id, worktree) VALUES ('p1', '/home/u/oc-proj');
      INSERT INTO session (id, project_id, parent_id, directory, title, time_created, time_updated, time_archived)
        VALUES ('${SES_ROOT}', 'p1', NULL, '/home/u/oc-proj', 'root session', 100, 200, NULL);
    `)
    db.close()
  })

  afterEach(async () => {
    await fsp.rm(dir, { recursive: true, force: true })
  })

  it('resolves the root session through a REAL worker (off-thread wiring end to end)', async () => {
    const found = await runOpencodeSessionByIdOffThread(dbPath, SES_ROOT)
    expect(found?.sessionId).toBe(SES_ROOT)
    expect(found?.projectPath).toBe('/home/u/oc-proj')
  })
})
