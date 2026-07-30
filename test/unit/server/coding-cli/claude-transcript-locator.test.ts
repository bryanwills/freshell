// @vitest-environment node
import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import fsp from 'fs/promises'
import os from 'os'
import path from 'path'
import { locateClaudeTranscriptById } from '../../../../server/coding-cli/claude-transcript-locator'

const SESSION_ID = 'ed2afda6-a340-443e-ba60-024a1b3554b4'

let root: string

beforeEach(async () => {
  // Throwaway fixture dir — never the real HOME (session safety rule).
  root = await fsp.mkdtemp(path.join(os.tmpdir(), 'claude-locator-'))
})

afterEach(async () => {
  await fsp.rm(root, { recursive: true, force: true })
})

async function writeTranscript(projectDir: string, id: string, lines: string[]) {
  const dir = path.join(root, projectDir)
  await fsp.mkdir(dir, { recursive: true })
  await fsp.writeFile(path.join(dir, `${id}.jsonl`), lines.join('\n') + '\n')
}

describe('locateClaudeTranscriptById', () => {
  it('finds a transcript by exact id and extracts cwd', async () => {
    await writeTranscript('-home-u-proj', SESSION_ID, [
      JSON.stringify({ type: 'summary', summary: 'hi' }),
      JSON.stringify({ type: 'user', cwd: '/home/u/proj', message: { content: 'hello' } }),
    ])
    const hit = await locateClaudeTranscriptById(SESSION_ID, [root])
    expect(hit).not.toBeNull()
    expect(hit!.sessionId).toBe(SESSION_ID)
    expect(hit!.cwd).toBe('/home/u/proj')
    expect(hit!.filePath).toBe(path.join(root, '-home-u-proj', `${SESSION_ID}.jsonl`))
    expect(hit!.lastActivityAt).toBeGreaterThan(0)
  })

  it('finds a lowercase transcript when the pasted id is UPPERCASE (case-sensitive FS)', async () => {
    await writeTranscript('-home-u-proj', SESSION_ID, [
      JSON.stringify({ type: 'user', cwd: '/home/u/proj', message: { content: 'hello' } }),
    ])
    const hit = await locateClaudeTranscriptById(SESSION_ID.toUpperCase(), [root])
    expect(hit).not.toBeNull()
    expect(hit!.sessionId).toBe(SESSION_ID)
    expect(hit!.filePath).toBe(path.join(root, '-home-u-proj', `${SESSION_ID}.jsonl`))
  })

  it('returns null when no transcript exists', async () => {
    expect(await locateClaudeTranscriptById(SESSION_ID, [root])).toBeNull()
  })

  it('returns null for non-UUID input without touching the filesystem', async () => {
    expect(await locateClaudeTranscriptById('417e8345', ['/does/not/exist'])).toBeNull()
  })

  it('tolerates missing roots', async () => {
    expect(await locateClaudeTranscriptById(SESSION_ID, [path.join(root, 'nope')])).toBeNull()
  })

  it('PROPAGATES non-absence failures (EACCES) instead of swallowing them as a miss', async () => {
    // Provider failure ≠ not found: an unreadable root must reject so the
    // resolver records providerErrors → 'degraded', never "no matching session".
    // chmod-based: CI/dev runs unprivileged (root would bypass the mode bits).
    const lockedRoot = path.join(root, 'locked')
    await fsp.mkdir(lockedRoot, { recursive: true })
    await fsp.chmod(lockedRoot, 0o000)
    try {
      await expect(locateClaudeTranscriptById(SESSION_ID, [lockedRoot])).rejects.toThrow()
    } finally {
      await fsp.chmod(lockedRoot, 0o700)
    }
  })

  it('still returns the hit when no line has a cwd', async () => {
    await writeTranscript('-x', SESSION_ID, [JSON.stringify({ type: 'summary' })])
    const hit = await locateClaudeTranscriptById(SESSION_ID, [root])
    expect(hit).not.toBeNull()
    expect(hit!.cwd).toBeUndefined()
  })

  it('finds a SUBAGENT transcript at <project>/<parent>/subagents/<id>.jsonl (index-missed child session)', async () => {
    // Claude also stores child-session transcripts one level deeper (see
    // claude.ts listSessionFiles): the exact-id contract covers them too.
    const PARENT_ID = 'aaaaaaaa-a340-443e-ba60-024a1b3554b4'
    const dir = path.join(root, '-home-u-proj', PARENT_ID, 'subagents')
    await fsp.mkdir(dir, { recursive: true })
    await fsp.writeFile(path.join(dir, `${SESSION_ID}.jsonl`), JSON.stringify({ cwd: '/home/u/proj' }) + '\n')
    const hit = await locateClaudeTranscriptById(SESSION_ID, [root])
    expect(hit).not.toBeNull()
    expect(hit!.cwd).toBe('/home/u/proj')
    expect(hit!.filePath.endsWith(path.join('subagents', `${SESSION_ID}.jsonl`))).toBe(true)
  })

  it('prefers the direct layout when both layouts contain the id (pass ordering)', async () => {
    await writeTranscript('-x', SESSION_ID, [JSON.stringify({ cwd: '/direct' })])
    const dir = path.join(root, '-x', 'aaaaaaaa-a340-443e-ba60-024a1b3554b4', 'subagents')
    await fsp.mkdir(dir, { recursive: true })
    await fsp.writeFile(path.join(dir, `${SESSION_ID}.jsonl`), JSON.stringify({ cwd: '/sub' }) + '\n')
    const hit = await locateClaudeTranscriptById(SESSION_ID, [root])
    expect(hit!.cwd).toBe('/direct')
  })
})
