import fsp from 'node:fs/promises'
import path from 'node:path'

export interface ClaudeTranscriptHit {
  sessionId: string
  sourceFile: string
  cwd?: string
}

/**
 * HEALTH SEAM: typed wrapper for locator provider failures.
 *
 * The locator distinguishes "transcript absent" (null) from "the claude store
 * could not be searched" (this error). Callers today (resolve-session.ts) do
 * not catch it, so a provider failure propagates instead of silently reading
 * as "not found". The later provider-health lane catches THIS class at the
 * resolve seam to record providerErrors and report 'degraded'.
 */
export class ClaudeTranscriptLocatorError extends Error {
  /** errno code from the underlying fs failure (e.g. 'EACCES', 'EIO'). */
  readonly code?: string

  constructor(message: string, cause: unknown) {
    super(message, { cause })
    this.name = 'ClaudeTranscriptLocatorError'
    this.code = (cause as NodeJS.ErrnoException | null)?.code
  }
}

const UUID_ONLY_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/
const CWD_SCAN_BYTES = 64 * 1024

/** Expected-absence errors are misses; EVERYTHING else is a provider failure. */
function isAbsenceError(err: unknown): boolean {
  const code = (err as NodeJS.ErrnoException | null)?.code
  return code === 'ENOENT' || code === 'ENOTDIR'
}

/**
 * Exact-id fallback for claude sessions the index cannot see (e.g. cold-start
 * skipped cwd-less transcripts, subagent child sessions). Claude stores
 * transcripts in TWO layouts:
 *   1. direct:   <root>/<project-dir>/<sessionId>.jsonl
 *   2. subagent: <root>/<project-dir>/<parent-session>/subagents/<sessionId>.jsonl
 * An exact pasted id must resolve for BOTH — child sessions included — so the
 * locator probes the cheap direct layout first (one readdir per root + one
 * stat per project dir) and only on a total miss falls back to the subagent
 * layout (one readdir per project dir + one stat per session subdirectory).
 *
 * Accepts one root or several (claude can have secondary project roots).
 *
 * ERROR CONTRACT: expected absence (ENOENT/ENOTDIR — missing root, missing
 * transcript, non-directory entries probed as directories) is a miss (null).
 * Any OTHER failure (EACCES, EMFILE, EIO, …) PROPAGATES as
 * ClaudeTranscriptLocatorError — a provider failure must never read as
 * "not found".
 */
export async function locateClaudeTranscript(
  sessionId: string,
  projectsDir: string | readonly string[],
): Promise<ClaudeTranscriptHit | null> {
  // Claude writes lowercase-UUID transcript filenames; the input contract
  // accepts UUIDs in ANY case and Linux filesystems are case-sensitive, so
  // normalize before building paths and return the canonical lowercase id.
  const normalized = sessionId.toLowerCase()
  if (!UUID_ONLY_RE.test(normalized)) return null
  const roots = typeof projectsDir === 'string' ? [projectsDir] : projectsDir

  // PASS 1 — direct layout.
  for (const root of roots) {
    for (const dir of await readdirOrEmpty(root)) {
      const hit = await probeTranscript(path.join(root, dir, `${normalized}.jsonl`), normalized)
      if (hit) return hit
    }
  }
  // PASS 2 — subagent layout (only when the direct layout missed everywhere).
  for (const root of roots) {
    for (const dir of await readdirOrEmpty(root)) {
      const projectDir = path.join(root, dir)
      for (const entry of await readdirOrEmpty(projectDir)) {
        const hit = await probeTranscript(
          path.join(projectDir, entry, 'subagents', `${normalized}.jsonl`),
          normalized,
        )
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
    throw new ClaudeTranscriptLocatorError(`failed to list claude projects dir: ${dir}`, err)
  }
}

/** stat + cwd-read for one candidate; absence = null, anything else PROPAGATES. */
async function probeTranscript(
  candidate: string,
  id: string,
): Promise<ClaudeTranscriptHit | null> {
  try {
    const stat = await fsp.stat(candidate)
    if (!stat.isFile()) return null
  } catch (err) {
    if (isAbsenceError(err)) return null
    throw new ClaudeTranscriptLocatorError(`failed to probe claude transcript: ${candidate}`, err)
  }
  return {
    sessionId: id,
    sourceFile: candidate,
    cwd: await readCwdFromTranscript(candidate),
  }
}

async function readCwdFromTranscript(filePath: string): Promise<string | undefined> {
  let handle
  try {
    handle = await fsp.open(filePath, 'r')
  } catch (err) {
    // The file existed a moment ago (stat succeeded): absence = raced
    // deletion (miss the cwd only); anything else is a provider failure.
    if (isAbsenceError(err)) return undefined
    throw new ClaudeTranscriptLocatorError(`failed to open claude transcript: ${filePath}`, err)
  }
  let head: string
  try {
    const buffer = Buffer.alloc(CWD_SCAN_BYTES)
    const { bytesRead } = await handle.read(buffer, 0, buffer.length, 0)
    head = buffer.subarray(0, bytesRead).toString('utf8')
  } catch (err) {
    throw new ClaudeTranscriptLocatorError(`failed to read claude transcript: ${filePath}`, err)
  } finally {
    await handle.close()
  }
  for (const line of head.split('\n')) {
    const trimmed = line.trim()
    if (!trimmed.startsWith('{')) continue
    try {
      const parsed = JSON.parse(trimmed) as { cwd?: unknown }
      if (typeof parsed.cwd === 'string' && parsed.cwd.length > 0) return parsed.cwd
    } catch {
      continue // truncated tail line etc.
    }
  }
  return undefined
}
