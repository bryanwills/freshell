import { parentPort, workerData } from 'node:worker_threads'
import type { OpencodeSessionRow } from './opencode-listing-query.js'

/**
 * Sentinel proving this thread was spawned by OUR runner. REQUIRED because the
 * server Vitest config runs test files in worker threads (`pool: 'threads'`), so
 * `parentPort` is non-null when a test imports this module. Without the sentinel,
 * the auto-run block below would fire on import using Vitest's OWN workerData and
 * post a message to Vitest's parent port — corrupting/hanging the test worker.
 * The runner injects this exact value in workerData; Vitest's workerData never has it.
 */
export const OPENCODE_BYID_WORKER_KIND = 'opencode-by-id-worker'

export type WorkerByIdInput = {
  kind: typeof OPENCODE_BYID_WORKER_KIND
  queryModuleUrl: string
  dbPath: string
  sessionId: string
}

/**
 * Run the by-id query by dynamically importing the EXACT resolved query-module
 * URL (.ts in dev/test, .js in prod) provided by the spawning code. We pass the
 * exact URL rather than a static relative import because NodeNext `.js`→`.ts`
 * remapping fails inside a worker thread (same constraint as the listing worker).
 */
export async function executeById(
  input: { queryModuleUrl: string; dbPath: string; sessionId: string },
): Promise<OpencodeSessionRow | null> {
  const mod = await import(input.queryModuleUrl) as typeof import('./opencode-by-id-query.js')
  return mod.runOpencodeSessionByIdQuery(input.dbPath, input.sessionId)
}

// Auto-run ONLY when we are a real worker spawned by our runner (parentPort present
// AND our sentinel in workerData). This is import-safe under Vitest's thread pool.
if (parentPort && (workerData as Partial<WorkerByIdInput> | undefined)?.kind === OPENCODE_BYID_WORKER_KIND) {
  const port = parentPort
  executeById(workerData as WorkerByIdInput)
    .then((row) => port.postMessage({ ok: true, row }))
    .catch((err: unknown) => {
      const error = err instanceof Error ? { name: err.name, message: err.message } : { name: 'Error', message: String(err) }
      port.postMessage({ ok: false, error })
    })
}
