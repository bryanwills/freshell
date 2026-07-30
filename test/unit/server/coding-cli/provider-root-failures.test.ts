// @vitest-environment node
// Ported from the reference provider-root-failures suite (feat/resume-button).
// Root-level scan failures must REJECT (provider unavailable ≠ not found) while
// root ABSENCE stays a legitimate empty result. Real fs, throwaway tmp dirs
// only — never a real HOME (session safety rule).
import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import fsp from 'fs/promises'
import os from 'os'
import path from 'path'
import { claudeProvider } from '../../../../server/coding-cli/providers/claude'
import { codexProvider } from '../../../../server/coding-cli/providers/codex'
import { amplifierProvider } from '../../../../server/coding-cli/providers/amplifier'

const CASES = [
  { name: 'claude', provider: claudeProvider, rootSubdir: 'projects' },
  { name: 'codex', provider: codexProvider, rootSubdir: 'sessions' },
  { name: 'amplifier', provider: amplifierProvider, rootSubdir: 'projects' },
] as const

let homeDir: string

beforeEach(async () => {
  homeDir = await fsp.mkdtemp(path.join(os.tmpdir(), 'provider-root-'))
})

afterEach(async () => {
  // Restore modes so cleanup can traverse the tree.
  for (const c of CASES) {
    await fsp.chmod(path.join(homeDir, c.rootSubdir), 0o700).catch(() => {})
  }
  await fsp.rm(homeDir, { recursive: true, force: true })
})

describe.each(CASES)('$name provider root failures', ({ provider, rootSubdir }) => {
  it('missing root resolves to an empty list (absence is not a failure)', async () => {
    const p = { ...provider, homeDir }
    await expect(p.listSessionFiles()).resolves.toEqual([])
  })

  it.skipIf(process.getuid?.() === 0)(
    'unreadable root (EACCES) REJECTS instead of reading as an empty scan',
    async () => {
      const root = path.join(homeDir, rootSubdir)
      await fsp.mkdir(root, { recursive: true })
      await fsp.chmod(root, 0o000)
      const p = { ...provider, homeDir }
      await expect(p.listSessionFiles()).rejects.toThrow()
    },
  )
})
