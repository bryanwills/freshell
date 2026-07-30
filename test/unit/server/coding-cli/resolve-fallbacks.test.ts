// @vitest-environment node
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import fsp from 'fs/promises'
import os from 'os'
import path from 'path'
import {
  buildResolveFallbacks,
  withRequestBudget,
  FALLBACK_BUDGET_PER_REQUEST,
} from '../../../../server/coding-cli/resolve-fallbacks'
import { locateClaudeTranscript } from '../../../../server/coding-cli/claude-transcript-locator'
import type { CodingCliProvider } from '../../../../server/coding-cli/provider'

const CLAUDE_V4 = 'ed2afda6-a340-443e-ba60-024a1b3554b4'
const OPENCODE_ID = 'ses_root0000000000000000000000'

describe('withRequestBudget', () => {
  it('shape gate runs BEFORE the budget: wrong-shape tokens do no work and consume no budget', async () => {
    const inner = vi.fn().mockResolvedValue(null)
    const budgeted = withRequestBudget({ claudeTranscriptById: inner })
    // Prefix hex and ses_ tokens are not full claude UUIDs — free no-op misses.
    expect(await budgeted.claudeTranscriptById!('417e8345')).toBeNull()
    expect(await budgeted.claudeTranscriptById!(OPENCODE_ID)).toBeNull()
    expect(inner).not.toHaveBeenCalled()
    // Budget still fully available for later valid-shape tokens.
    const ids = [
      'ed2afda6-a340-443e-ba60-024a1b3554b1',
      'ed2afda6-a340-443e-ba60-024a1b3554b2',
      'ed2afda6-a340-443e-ba60-024a1b3554b3',
    ]
    for (const id of ids) await budgeted.claudeTranscriptById!(id)
    expect(inner.mock.calls.map((c) => c[0])).toEqual(ids.slice(0, FALLBACK_BUDGET_PER_REQUEST))
  })

  it('the (budget+1)-th valid-shape token returns null WITHOUT calling the inner fallback', async () => {
    const inner = vi.fn().mockResolvedValue(null)
    const budgeted = withRequestBudget({ opencodeSessionById: inner }, 1)
    expect(await budgeted.opencodeSessionById!(OPENCODE_ID)).toBeNull()
    expect(inner).toHaveBeenCalledTimes(1)
    expect(await budgeted.opencodeSessionById!('ses_next0000000000000000000000')).toBeNull()
    expect(inner).toHaveBeenCalledTimes(1)
  })

  it('leaves absent fallbacks undefined', () => {
    const budgeted = withRequestBudget({})
    expect(budgeted.claudeTranscriptById).toBeUndefined()
    expect(budgeted.opencodeSessionById).toBeUndefined()
  })
})

describe('buildResolveFallbacks — claude wiring', () => {
  // Throwaway fixture root — never the real HOME (session safety rule).
  let root: string

  beforeEach(async () => {
    root = await fsp.mkdtemp(path.join(os.tmpdir(), 'resolve-fallbacks-'))
  })

  afterEach(async () => {
    await fsp.rm(root, { recursive: true, force: true })
  })

  function claudeStub(): CodingCliProvider {
    return { name: 'claude' } as unknown as CodingCliProvider
  }

  // The locator itself stays injected (the merged locator takes a projects
  // dir); wire it to the REAL locator over the fixture root, as index.ts does.
  const locateInRoot = (id: string) => locateClaudeTranscript(id, root)

  async function writeTranscript(projectDir: string, id: string, lines: string[]) {
    const dir = path.join(root, projectDir)
    await fsp.mkdir(dir, { recursive: true })
    await fsp.writeFile(path.join(dir, `${id}.jsonl`), lines.join('\n') + '\n')
  }

  it('locates a real transcript and returns the full match tuple (no metadata → sessionType claude)', async () => {
    await writeTranscript('-home-u-proj', CLAUDE_V4, [
      JSON.stringify({ type: 'user', cwd: '/home/u/proj', message: { content: 'hello' } }),
    ])
    const { claudeTranscriptById } = buildResolveFallbacks([claudeStub()], {
      locateClaudeTranscript: locateInRoot,
    })
    const match = await claudeTranscriptById!(CLAUDE_V4)
    expect(match).toEqual({
      provider: 'claude',
      sessionId: CLAUDE_V4,
      cwd: '/home/u/proj',
      sessionType: 'claude',
      matchKind: 'exact',
    })
  })

  it('uses the metadata-store sessionType (freshclaude) so resume reopens the right runtime', async () => {
    await writeTranscript('-home-u-proj', CLAUDE_V4, [JSON.stringify({ cwd: '/home/u/proj' })])
    const sessionMetadataStore = {
      getAll: async () => ({ [`claude:${CLAUDE_V4}`]: { sessionType: 'freshclaude' } }),
    }
    const { claudeTranscriptById } = buildResolveFallbacks([claudeStub()], {
      sessionMetadataStore: sessionMetadataStore as any,
      locateClaudeTranscript: locateInRoot,
    })
    const match = await claudeTranscriptById!(CLAUDE_V4)
    expect(match?.sessionType).toBe('freshclaude')
  })

  it('a REJECTING metadata store degrades to the provider default (never a provider error)', async () => {
    await writeTranscript('-home-u-proj', CLAUDE_V4, [JSON.stringify({ cwd: '/home/u/proj' })])
    const sessionMetadataStore = {
      getAll: async () => { throw new Error('metadata store corrupt') },
    }
    const { claudeTranscriptById } = buildResolveFallbacks([claudeStub()], {
      sessionMetadataStore: sessionMetadataStore as any,
      locateClaudeTranscript: locateInRoot,
    })
    const match = await claudeTranscriptById!(CLAUDE_V4)
    expect(match?.sessionType).toBe('claude')
  })

  it('resolves null on a genuine miss', async () => {
    const { claudeTranscriptById } = buildResolveFallbacks([claudeStub()], {
      locateClaudeTranscript: locateInRoot,
    })
    expect(await claudeTranscriptById!(CLAUDE_V4)).toBeNull()
  })
})

describe('buildResolveFallbacks — opencode wiring', () => {
  const row = {
    sessionId: OPENCODE_ID,
    cwd: '/home/u/oc',
    title: 'oc root',
    createdAt: 1,
    lastActivityAt: 7,
    projectPath: '/home/u/oc-proj',
  }

  function opencodeStub(): CodingCliProvider {
    return { name: 'opencode', getDatabasePath: () => '/tmp/x.db' } as unknown as CodingCliProvider
  }

  it('returns the full match tuple from an injected runOpencodeById (metadata-driven sessionType)', async () => {
    const runOpencodeById = vi.fn().mockResolvedValue(row)
    const sessionMetadataStore = {
      getAll: async () => ({ [`opencode:${OPENCODE_ID}`]: { sessionType: 'freshopencode' } }),
    }
    const { opencodeSessionById } = buildResolveFallbacks([opencodeStub()], {
      sessionMetadataStore: sessionMetadataStore as any,
      runOpencodeById,
    })
    const match = await opencodeSessionById!(OPENCODE_ID)
    expect(runOpencodeById).toHaveBeenCalledWith('/tmp/x.db', OPENCODE_ID)
    expect(match).toEqual({
      provider: 'opencode',
      sessionId: OPENCODE_ID,
      cwd: '/home/u/oc',
      sessionType: 'freshopencode',
      title: 'oc root',
      lastActivityAt: 7,
      matchKind: 'exact',
    })
  })

  it('floors a REAL lastActivityAt so the zod contract (int) stays satisfiable', async () => {
    const runOpencodeById = vi.fn().mockResolvedValue({ ...row, lastActivityAt: 7.9 })
    const { opencodeSessionById } = buildResolveFallbacks([opencodeStub()], { runOpencodeById })
    const match = await opencodeSessionById!(OPENCODE_ID)
    expect(match?.lastActivityAt).toBe(7)
  })

  it('REJECTS when runOpencodeById rejects (error propagation, not null)', async () => {
    const runOpencodeById = vi.fn().mockRejectedValue(new Error('database is locked'))
    const { opencodeSessionById } = buildResolveFallbacks([opencodeStub()], { runOpencodeById })
    await expect(opencodeSessionById!(OPENCODE_ID)).rejects.toThrow(/locked/)
  })

  it('resolves null when the DB has no such session', async () => {
    const runOpencodeById = vi.fn().mockResolvedValue(null)
    const { opencodeSessionById } = buildResolveFallbacks([opencodeStub()], { runOpencodeById })
    expect(await opencodeSessionById!(OPENCODE_ID)).toBeNull()
  })
})

describe('buildResolveFallbacks — provider absence', () => {
  it('returns undefined fallbacks when no claude/opencode provider is in the set', () => {
    const { claudeTranscriptById, opencodeSessionById } = buildResolveFallbacks([], {
      locateClaudeTranscript: async () => null,
    })
    expect(claudeTranscriptById).toBeUndefined()
    expect(opencodeSessionById).toBeUndefined()
  })
})
