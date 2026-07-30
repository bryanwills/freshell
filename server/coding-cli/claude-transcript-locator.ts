import fsp from 'fs/promises'
import path from 'path'

export type ClaudeTranscriptHit = {
  sessionId: string
  cwd?: string
  filePath: string
  lastActivityAt: number
}

const UUID_SHAPE = /^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$/
const CWD_SCAN_BYTES = 64 * 1024

/** Expected-absence errors are misses; EVERYTHING else is a provider failure. */
function isAbsenceError(err: unknown): boolean {
  const code = (err as NodeJS.ErrnoException | null)?.code
  return code === 'ENOENT' || code === 'ENOTDIR'
}

/**
 * Exact-id fallback for claude sessions the index missed. Claude stores
 * transcripts in TWO layouts (both indexed by the provider — see claude.ts
 * listSessionFiles ~line 571):
 *   1. direct:   <root>/<project-dir>/<sessionId>.jsonl
 *   2. subagent: <root>/<project-dir>/<parent-session>/subagents/<sessionId>.jsonl
 * An exact pasted id must resolve for BOTH — child sessions included — so the
 * locator probes the cheap direct layout first (one readdir per root + one
 * stat per project dir) and only on a total miss falls back to the subagent
 * layout (one readdir per project dir + one stat per session subdirectory).
 *
 * ERROR CONTRACT: expected absence (ENOENT/ENOTDIR — missing root, missing
 * transcript, non-directory entries probed as directories) is a miss (null).
 * Any OTHER failure (EACCES, EMFILE, EIO, …) PROPAGATES: the resolver records
 * it as a provider error and the route reports 'degraded' — a provider
 * failure must never read as "not found".
 */
export async function locateClaudeTranscriptById(
  sessionId: string,
  roots: string[],
): Promise<ClaudeTranscriptHit | null> {
  if (!UUID_SHAPE.test(sessionId)) return null
  // Claude writes lowercase-UUID transcript filenames; the input contract
  // accepts UUIDs in ANY case and Linux filesystems are case-sensitive, so
  // normalize before building paths and return the canonical lowercase id.
  const id = sessionId.toLowerCase()
  // PASS 1 — direct layout.
  for (const root of roots) {
    for (const dir of await readdirOrEmpty(root)) {
      const hit = await probeTranscript(path.join(root, dir, `${id}.jsonl`), id)
      if (hit) return hit
    }
  }
  // PASS 2 — subagent layout (only when the direct layout missed everywhere).
  for (const root of roots) {
    for (const dir of await readdirOrEmpty(root)) {
      const projectDir = path.join(root, dir)
      for (const entry of await readdirOrEmpty(projectDir)) {
        const hit = await probeTranscript(path.join(projectDir, entry, 'subagents', `${id}.jsonl`), id)
        if (hit) return hit
      }
    }
  }
  return null
}

/** readdir treating absence as empty; anything else PROPAGATES (provider failure). */
async function readdirOrEmpty(dir: string): Promise<string[]> {
  try {
    return await fsp.readdir(dir)
  } catch (err) {
    if (isAbsenceError(err)) return []
    throw err
  }
}

/** stat + cwd-read for one candidate; absence = null, anything else PROPAGATES. */
async function probeTranscript(candidate: string, id: string): Promise<ClaudeTranscriptHit | null> {
  let stat
  try {
    stat = await fsp.stat(candidate)
  } catch (err) {
    if (isAbsenceError(err)) return null
    throw err
  }
  const cwd = await readCwdFromTranscriptHead(candidate)
  return { sessionId: id, cwd, filePath: candidate, lastActivityAt: stat.mtimeMs }
}

async function readCwdFromTranscriptHead(filePath: string): Promise<string | undefined> {
  let handle
  try {
    handle = await fsp.open(filePath, 'r')
  } catch (err) {
    // The file existed a moment ago (stat succeeded): absence = raced
    // deletion (miss the cwd only); anything else is a provider failure.
    if (isAbsenceError(err)) return undefined
    throw err
  }
  try {
    const buf = Buffer.alloc(CWD_SCAN_BYTES)
    const { bytesRead } = await handle.read(buf, 0, buf.length, 0)
    for (const line of buf.toString('utf8', 0, bytesRead).split('\n')) {
      const trimmed = line.trim()
      if (!trimmed) continue
      try {
        const obj = JSON.parse(trimmed) as { cwd?: unknown }
        if (typeof obj.cwd === 'string' && obj.cwd) return obj.cwd
      } catch {
        // Truncated tail line of the 64KB window — ignore.
      }
    }
    return undefined
  } finally {
    await handle.close()
  }
}
