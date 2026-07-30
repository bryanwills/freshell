// @vitest-environment node
import { describe, expect, it, beforeEach, afterEach } from 'vitest'
import fsp from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import {
  ClaudeTranscriptLocatorError,
  locateClaudeTranscript,
} from '../../../server/coding-cli/claude-transcript-locator.js'

const SESSION_ID = 'ed2afda6-a340-443e-ba60-024a1b3554b4'

describe('locateClaudeTranscript', () => {
  let projectsDir: string

  beforeEach(async () => {
    projectsDir = await fsp.mkdtemp(path.join(os.tmpdir(), 'claude-projects-'))
  })

  afterEach(async () => {
    await fsp.rm(projectsDir, { recursive: true, force: true })
  })

  async function writeTranscript(dirName: string, id: string, lines: string[]) {
    const dir = path.join(projectsDir, dirName)
    await fsp.mkdir(dir, { recursive: true })
    const file = path.join(dir, `${id}.jsonl`)
    await fsp.writeFile(file, lines.join('\n'), 'utf8')
    return file
  }

  it('finds a transcript by exact id and reads cwd from the first entry', async () => {
    const file = await writeTranscript('-repo-alpha', SESSION_ID, [
      JSON.stringify({ type: 'summary', summary: 'hello' }),
      JSON.stringify({ type: 'user', cwd: '/repo/alpha', message: 'hi' }),
    ])
    await expect(locateClaudeTranscript(SESSION_ID, projectsDir)).resolves.toEqual({
      sessionId: SESSION_ID,
      sourceFile: file,
      cwd: '/repo/alpha',
    })
  })

  it('matches case-insensitively and returns the normalized id', async () => {
    await writeTranscript('-repo-alpha', SESSION_ID, [JSON.stringify({ cwd: '/repo/alpha' })])
    const hit = await locateClaudeTranscript(SESSION_ID.toUpperCase(), projectsDir)
    expect(hit?.sessionId).toBe(SESSION_ID)
  })

  it('returns undefined cwd when no entry carries one', async () => {
    await writeTranscript('-repo-beta', SESSION_ID, [JSON.stringify({ type: 'summary' })])
    const hit = await locateClaudeTranscript(SESSION_ID, projectsDir)
    expect(hit).not.toBeNull()
    expect(hit?.cwd).toBeUndefined()
  })

  it('returns null for an unknown id', async () => {
    await expect(
      locateClaudeTranscript('019fac27-69d7-78a0-b972-b339d551042e', projectsDir),
    ).resolves.toBeNull()
  })

  it('returns null for non-uuid input without touching the fs', async () => {
    await expect(locateClaudeTranscript('417e8345', projectsDir)).resolves.toBeNull()
  })

  it('returns null when the projects dir does not exist', async () => {
    await expect(
      locateClaudeTranscript(SESSION_ID, path.join(projectsDir, 'missing')),
    ).resolves.toBeNull()
  })

  it('tolerates a plain file entry inside the projects dir (ENOTDIR is a miss)', async () => {
    await fsp.writeFile(path.join(projectsDir, 'stray.txt'), 'not a project dir', 'utf8')
    await expect(locateClaudeTranscript(SESSION_ID, projectsDir)).resolves.toBeNull()
  })

  it('searches SECONDARY roots when given multiple project roots', async () => {
    // Claude can have more than one projects root; the locator must not be
    // single-root only.
    const secondRoot = await fsp.mkdtemp(path.join(os.tmpdir(), 'claude-projects-2-'))
    try {
      const dir = path.join(secondRoot, '-repo-delta')
      await fsp.mkdir(dir, { recursive: true })
      const file = path.join(dir, `${SESSION_ID}.jsonl`)
      await fsp.writeFile(file, JSON.stringify({ cwd: '/repo/delta' }), 'utf8')
      await expect(locateClaudeTranscript(SESSION_ID, [projectsDir, secondRoot])).resolves.toEqual({
        sessionId: SESSION_ID,
        sourceFile: file,
        cwd: '/repo/delta',
      })
    } finally {
      await fsp.rm(secondRoot, { recursive: true, force: true })
    }
  })

  it('finds a SUBAGENT transcript at <project>/<parent>/subagents/<id>.jsonl (index-missed child session)', async () => {
    // Claude also stores child-session transcripts one level deeper (see
    // claude.ts listSessionFiles): the exact-id contract covers them too.
    const PARENT_ID = 'aaaaaaaa-a340-443e-ba60-024a1b3554b4'
    const dir = path.join(projectsDir, '-repo-alpha', PARENT_ID, 'subagents')
    await fsp.mkdir(dir, { recursive: true })
    const file = path.join(dir, `${SESSION_ID}.jsonl`)
    await fsp.writeFile(file, JSON.stringify({ cwd: '/repo/alpha' }) + '\n', 'utf8')
    const hit = await locateClaudeTranscript(SESSION_ID, projectsDir)
    expect(hit).not.toBeNull()
    expect(hit?.cwd).toBe('/repo/alpha')
    expect(hit?.sourceFile).toBe(file)
  })

  it('prefers the direct layout when both layouts contain the id (pass ordering)', async () => {
    await writeTranscript('-repo-alpha', SESSION_ID, [JSON.stringify({ cwd: '/direct' })])
    const dir = path.join(projectsDir, '-repo-alpha', 'aaaaaaaa-a340-443e-ba60-024a1b3554b4', 'subagents')
    await fsp.mkdir(dir, { recursive: true })
    await fsp.writeFile(path.join(dir, `${SESSION_ID}.jsonl`), JSON.stringify({ cwd: '/sub' }) + '\n', 'utf8')
    const hit = await locateClaudeTranscript(SESSION_ID, projectsDir)
    expect(hit?.cwd).toBe('/direct')
  })

  it('PROPAGATES non-absence failures on the projects root (EACCES) instead of swallowing them as a miss', async () => {
    // Provider failure ≠ not found: an unreadable root must reject so the
    // provider-health lane can report 'degraded', never "no matching session".
    // chmod-based: CI/dev runs unprivileged (root would bypass the mode bits).
    const lockedRoot = path.join(projectsDir, 'locked')
    await fsp.mkdir(lockedRoot, { recursive: true })
    await fsp.chmod(lockedRoot, 0o000)
    try {
      await expect(locateClaudeTranscript(SESSION_ID, lockedRoot)).rejects.toThrow()
    } finally {
      await fsp.chmod(lockedRoot, 0o700)
    }
  })

  it('PROPAGATES non-absence failures on a project dir probe (EACCES on stat) instead of swallowing', async () => {
    const dir = path.join(projectsDir, '-repo-locked')
    await fsp.mkdir(dir, { recursive: true })
    await fsp.chmod(dir, 0o000)
    try {
      await expect(locateClaudeTranscript(SESSION_ID, projectsDir)).rejects.toThrow()
    } finally {
      await fsp.chmod(dir, 0o700)
    }
  })
})
