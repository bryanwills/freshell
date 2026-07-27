import { createHash } from 'node:crypto'

const HASH_LENGTH = 12

/**
 * Content-free identity for logs and incident inspection: a short, stable
 * sha256 prefix of the raw value (session id, cwd, message id, ...).
 *
 * Leaf module on purpose (node:crypto only): callers that must stay
 * logger-free — e.g. the runtime manager's read-only inspectState() — can hash
 * identity without pulling the observability sink (and its logger dependency)
 * into their module graph. observability.ts re-exports this so existing
 * importers are unaffected.
 */
export function hashForLogs(value: string): string {
  return createHash('sha256').update(value).digest('hex').slice(0, HASH_LENGTH)
}
