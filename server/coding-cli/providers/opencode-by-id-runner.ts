import { Worker } from 'node:worker_threads'
import type { OpencodeSessionRow } from './opencode-listing-query.js'
// Importing the worker module on the MAIN thread (or a Vitest worker) is safe:
// its auto-run is sentinel-guarded, so this import never spawns/posts anything.
import { OPENCODE_BYID_WORKER_KIND } from './opencode-by-id.worker.js'

export type OpencodeByIdQueryInput = { dbPath: string; sessionId: string }
export type OpencodeByIdQueryRunner = (input: OpencodeByIdQueryInput) => Promise<OpencodeSessionRow | null>

type WorkerLike = {
  on(event: 'message', listener: (value: unknown) => void): unknown
  on(event: 'error', listener: (err: Error) => void): unknown
  on(event: 'exit', listener: (code: number) => void): unknown
  terminate(): Promise<number> | void
}

export type WorkerSpawnOptions = { workerData: unknown; execArgv: string[] }

export type CreateWorkerByIdRunnerOptions = {
  /** Injectable for unit tests; default spawns a real worker_threads Worker. */
  spawn?: (workerUrl: URL, options: WorkerSpawnOptions) => WorkerLike
  /** Override the query-module URL (used by off-thread integration fixtures). */
  queryModuleUrl?: string
  /** Hard timeout for a single by-id query. Default 15 s (same as the listing runner). */
  timeoutMs?: number
}

const DEFAULT_TIMEOUT_MS = 15_000
// import.meta.url ends with `.ts` in dev/test (tsx / native strip-types) and
// `.js` in prod (compiled dist). Resolve siblings with the matching extension.
const SELF_EXT = import.meta.url.endsWith('.ts') ? '.ts' : '.js'
// Append to process.execArgv (do NOT replace) so tsx's `--import .../loader.mjs`
// is inherited in dev; the flag silences node:sqlite's per-spawn ExperimentalWarning.
const WORKER_EXECARGV = [...process.execArgv, '--disable-warning=ExperimentalWarning']

function defaultWorkerUrl(): URL {
  return new URL(`./opencode-by-id.worker${SELF_EXT}`, import.meta.url)
}
function defaultQueryModuleUrl(): string {
  return new URL(`./opencode-by-id-query${SELF_EXT}`, import.meta.url).href
}
function defaultSpawn(workerUrl: URL, options: WorkerSpawnOptions): WorkerLike {
  return new Worker(workerUrl, options)
}

type OkMessage = { ok: true; row: OpencodeSessionRow | null }
type ErrMessage = { ok: false; error: { name: string; message: string } }

// Validate the FULL shape, not just the presence of `ok` — a truncated/garbled
// message like `{ ok: true }` must NOT resolve garbage as a hit or a miss.
function isOkMessage(value: unknown): value is OkMessage {
  if (typeof value !== 'object' || value === null) return false
  if ((value as { ok?: unknown }).ok !== true) return false
  if (!('row' in (value as object))) return false
  const row = (value as { row?: unknown }).row
  return row === null || (typeof row === 'object' && row !== null)
}
function isErrMessage(value: unknown): value is ErrMessage {
  if (typeof value !== 'object' || value === null) return false
  if ((value as { ok?: unknown }).ok !== false) return false
  const error = (value as { error?: unknown }).error
  return typeof error === 'object' && error !== null
    && typeof (error as { message?: unknown }).message === 'string'
}

export function createWorkerByIdRunner(
  options: CreateWorkerByIdRunnerOptions = {},
): OpencodeByIdQueryRunner {
  const spawn = options.spawn ?? defaultSpawn
  const queryModuleUrl = options.queryModuleUrl ?? defaultQueryModuleUrl()
  const timeoutMs = options.timeoutMs ?? DEFAULT_TIMEOUT_MS
  const workerUrl = defaultWorkerUrl()

  return (input: OpencodeByIdQueryInput): Promise<OpencodeSessionRow | null> => {
    return new Promise<OpencodeSessionRow | null>((resolve, reject) => {
      const worker = spawn(workerUrl, { workerData: { ...input, queryModuleUrl, kind: OPENCODE_BYID_WORKER_KIND }, execArgv: WORKER_EXECARGV })
      let settled = false
      let timer: NodeJS.Timeout | undefined

      const cleanup = () => {
        if (timer) clearTimeout(timer)
        try { void worker.terminate() } catch { /* ignore */ }
      }
      const settleResolve = (row: OpencodeSessionRow | null) => {
        if (settled) return
        settled = true
        cleanup()
        resolve(row)
      }
      const settleReject = (err: Error) => {
        if (settled) return
        settled = true
        cleanup()
        reject(err)
      }

      timer = setTimeout(() => settleReject(new Error(`OpenCode by-id worker timed out after ${timeoutMs}ms`)), timeoutMs)
      if (typeof (timer as NodeJS.Timeout).unref === 'function') (timer as NodeJS.Timeout).unref()

      worker.on('message', (value: unknown) => {
        if (isOkMessage(value)) {
          settleResolve(value.row)
        } else if (isErrMessage(value)) {
          const err = new Error(value.error.message || 'OpenCode by-id worker failed')
          err.name = value.error.name ?? 'Error'
          settleReject(err)
        } else {
          settleReject(new Error('OpenCode by-id worker sent a malformed message'))
        }
      })
      worker.on('error', (err: Error) => settleReject(err))
      worker.on('exit', (code: number) => settleReject(new Error(`OpenCode by-id worker exited (code ${code}) before responding`)))
    })
  }
}

/**
 * Default production runner: one short-lived worker per lookup, hard timeout.
 * Worker/spawn/timeout failures REJECT (provider unavailable ≠ not found).
 * Per-request cost is bounded upstream (shape gate + FALLBACK_BUDGET_PER_REQUEST),
 * and the EVENT LOOP stays free even when the DB is locked for the full
 * 500 ms busy timeout — DatabaseSync blocks only the worker thread.
 */
export async function runOpencodeSessionByIdOffThread(dbPath: string, sessionId: string): Promise<OpencodeSessionRow | null> {
  return createWorkerByIdRunner()({ dbPath, sessionId })
}
