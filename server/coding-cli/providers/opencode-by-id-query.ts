import type { OpencodeSessionRow } from './opencode-listing-query.js'

/**
 * SHORT busy timeout, deliberately much smaller than the listing query's 5 s:
 * a locked DB must fail fast (the failure is surfaced as provider-unavailable,
 * NOT "not found"). This synchronous function runs INSIDE THE WORKER THREAD
 * (opencode-by-id.worker.ts) — same rule as the listing query, which was moved
 * off the event loop even at ~180 ms. Never call it on the main thread in
 * production: `DatabaseSync` blocks whatever thread runs it for up to this
 * timeout when the DB is locked.
 */
const OPENCODE_BYID_BUSY_TIMEOUT_MS = 500

/**
 * Exact-id opencode lookup for the resolve endpoint's fallback path — the
 * Node-server sibling of the Rust by-id existence probe (#579). Unlike the
 * listing query it deliberately includes ARCHIVED and CHILD sessions: an
 * exact id pasted by the user must resolve even when the listing hides it.
 * Lazy `node:sqlite` import for the same vi.mock/TDZ reason documented in
 * opencode-listing-query.ts. Errors PROPAGATE to the caller (provider
 * unavailable ≠ not found).
 */
export async function runOpencodeSessionByIdQuery(
  dbPath: string,
  sessionId: string,
): Promise<OpencodeSessionRow | null> {
  const { DatabaseSync } = await import('node:sqlite')
  const db = new DatabaseSync(dbPath, { readOnly: true })
  try {
    db.exec(`PRAGMA busy_timeout = ${OPENCODE_BYID_BUSY_TIMEOUT_MS}`)
    const tableNames = new Set(
      (db.prepare("SELECT name FROM sqlite_master WHERE type = 'table'").all() as Array<{ name?: unknown }>)
        .map((row) => row.name),
    )
    if (!tableNames.has('session')) return null
    const hasProject = tableNames.has('project')
    const projectSelect = hasProject ? 'p.worktree' : 'NULL'
    const projectJoin = hasProject ? 'LEFT JOIN project p ON p.id = s.project_id' : ''
    const row = db.prepare(`
      SELECT
        s.id AS sessionId,
        s.directory AS cwd,
        s.title AS title,
        s.time_created AS createdAt,
        s.time_updated AS lastActivityAt,
        ${projectSelect} AS projectPath
      FROM session s
      ${projectJoin}
      WHERE s.id = ?
      LIMIT 1
    `).get(sessionId) as OpencodeSessionRow | undefined
    return row ?? null
  } finally {
    db.close()
  }
}
