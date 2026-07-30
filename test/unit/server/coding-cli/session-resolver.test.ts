// @vitest-environment node
import { describe, it, expect } from 'vitest'
import {
  resolveSessionCandidates,
  RESOLVE_MATCH_CAP,
  type ResolveMatch,
} from '../../../../server/coding-cli/session-resolver'
import type { ProjectGroup, CodingCliSession } from '../../../../server/coding-cli/types'

function session(overrides: Partial<CodingCliSession> & Pick<CodingCliSession, 'provider' | 'sessionId'>): CodingCliSession {
  return {
    projectPath: '/home/u/proj',
    lastActivityAt: 1000,
    cwd: '/home/u/proj',
    title: 'a session',
    ...overrides,
  }
}

function projects(sessions: CodingCliSession[]): ProjectGroup[] {
  return [{ projectPath: '/home/u/proj', sessions }]
}

const AMPLIFIER_FULL = '417e8345-90ab-4cde-8f01-234567890abc'
const CODEX_V7 = '019fac27-69d7-78a0-b972-b339d551042e'
const CLAUDE_V4 = 'ed2afda6-a340-443e-ba60-024a1b3554b4'
const OPENCODE_ID = 'ses_root0000000000000000000000'

const fourProviderSnapshot = projects([
  session({ provider: 'claude', sessionId: CLAUDE_V4, sessionType: 'claude' }),
  session({ provider: 'codex', sessionId: CODEX_V7, sessionType: 'codex' }),
  session({ provider: 'opencode', sessionId: OPENCODE_ID, sessionType: 'opencode' }),
  session({ provider: 'amplifier', sessionId: AMPLIFIER_FULL, sessionType: 'amplifier' }),
])

describe('resolveSessionCandidates', () => {
  it('exact match wins across all providers at once (claude UUID, no hint needed)', async () => {
    const { matches } = await resolveSessionCandidates([CLAUDE_V4], { getProjects: () => fourProviderSnapshot })
    expect(matches).toHaveLength(1)
    expect(matches[0]).toMatchObject({
      provider: 'claude',
      sessionId: CLAUDE_V4,
      sessionType: 'claude',
      cwd: '/home/u/proj',
      matchType: 'exact',
      matchedToken: CLAUDE_V4,
    })
  })

  it('short hex prefix matches the amplifier session (spec row: 417e8345)', async () => {
    const { matches } = await resolveSessionCandidates(['417e8345'], { getProjects: () => fourProviderSnapshot })
    expect(matches).toHaveLength(1)
    expect(matches[0]).toMatchObject({ provider: 'amplifier', sessionId: AMPLIFIER_FULL, matchType: 'prefix' })
  })

  it('exact-id match is case-insensitive for UUID/hex tokens', async () => {
    const { matches } = await resolveSessionCandidates([CLAUDE_V4.toUpperCase()], { getProjects: () => fourProviderSnapshot })
    expect(matches).toHaveLength(1)
    expect(matches[0].sessionId).toBe(CLAUDE_V4)
  })

  it('ses_ ids are case-SENSITIVE (base62): a case-variant does NOT match', async () => {
    const { matches } = await resolveSessionCandidates(['ses_ROOT0000000000000000000000'], { getProjects: () => fourProviderSnapshot })
    expect(matches).toHaveLength(0)
  })

  it('opencode ses_ id resolves to opencode even though other providers exist', async () => {
    const { matches } = await resolveSessionCandidates([OPENCODE_ID], { getProjects: () => fourProviderSnapshot })
    expect(matches).toHaveLength(1)
    expect(matches[0].provider).toBe('opencode')
  })

  it('exact match takes precedence over prefix matches of the same token', async () => {
    const snapshot = projects([
      session({ provider: 'amplifier', sessionId: '417e8345', lastActivityAt: 1 }),
      session({ provider: 'amplifier', sessionId: '417e8345-90ab-4cde-8f01-234567890abc', lastActivityAt: 2 }),
    ])
    const { matches } = await resolveSessionCandidates(['417e8345'], { getProjects: () => snapshot })
    expect(matches).toHaveLength(1)
    expect(matches[0].matchType).toBe('exact')
  })

  it('ambiguous prefix returns all matches most-recent first, capped', async () => {
    const many = Array.from({ length: RESOLVE_MATCH_CAP + 5 }, (_, i) =>
      session({
        provider: 'amplifier',
        sessionId: `417e8345-90ab-4cde-8f01-${String(i).padStart(12, '0')}`,
        lastActivityAt: i,
      }))
    const { matches } = await resolveSessionCandidates(['417e8345'], { getProjects: () => projects(many) })
    expect(matches).toHaveLength(RESOLVE_MATCH_CAP)
    expect(matches[0].lastActivityAt).toBe(RESOLVE_MATCH_CAP + 4)
    expect(matches[matches.length - 1].lastActivityAt).toBeGreaterThanOrEqual(5)
  })

  it('tries candidates in order until one resolves', async () => {
    const { matches } = await resolveSessionCandidates(['deadbeef1234', CLAUDE_V4], { getProjects: () => fourProviderSnapshot })
    expect(matches).toHaveLength(1)
    expect(matches[0].sessionId).toBe(CLAUDE_V4)
  })

  it('an EXACT id finds a subagent/child session (spec: scan ALL sessions)', async () => {
    const snapshot = projects([
      session({ provider: 'claude', sessionId: CLAUDE_V4, isSubagent: true }),
    ])
    const { matches } = await resolveSessionCandidates([CLAUDE_V4], { getProjects: () => snapshot })
    expect(matches).toHaveLength(1)
    expect(matches[0].sessionId).toBe(CLAUDE_V4)
  })

  it('prefix DISCOVERY does not surface subagent sessions', async () => {
    const snapshot = projects([
      session({ provider: 'claude', sessionId: CLAUDE_V4, isSubagent: true }),
    ])
    const { matches } = await resolveSessionCandidates(['ed2afda6'], { getProjects: () => snapshot })
    expect(matches).toHaveLength(0)
  })

  it('an exact FALLBACK hit beats an indexed PREFIX match of the same token', async () => {
    // Token exactly equals an unindexed session id AND is a prefix of an
    // indexed one: exact must win or the wrong session gets resumed.
    const indexedPrefix = projects([
      session({ provider: 'amplifier', sessionId: `${CLAUDE_V4}9999`, lastActivityAt: 999 }),
    ])
    const fallbackMatch: ResolveMatch = {
      provider: 'claude', sessionId: CLAUDE_V4, cwd: '/tmp/exact', projectPath: '/tmp/exact',
      sessionType: 'claude', lastActivityAt: 1, matchType: 'exact', matchedToken: CLAUDE_V4,
    }
    const { matches } = await resolveSessionCandidates([CLAUDE_V4], {
      getProjects: () => indexedPrefix,
      fallbacks: { claudeTranscriptById: async (id) => (id === CLAUDE_V4 ? fallbackMatch : null) },
    })
    expect(matches).toEqual([fallbackMatch])
  })

  it('sessionType defaults to the provider name when the index has none', async () => {
    const snapshot = projects([session({ provider: 'codex', sessionId: CODEX_V7 })])
    const { matches } = await resolveSessionCandidates([CODEX_V7], { getProjects: () => snapshot })
    expect(matches[0].sessionType).toBe('codex')
  })

  it('index miss consults exact-id fallbacks (claude transcript locator)', async () => {
    const fallbackMatch: ResolveMatch = {
      provider: 'claude', sessionId: CLAUDE_V4, cwd: '/tmp/found', projectPath: '/tmp/found',
      sessionType: 'claude', lastActivityAt: 42, matchType: 'exact', matchedToken: CLAUDE_V4,
    }
    const { matches } = await resolveSessionCandidates([CLAUDE_V4], {
      getProjects: () => projects([]),
      fallbacks: { claudeTranscriptById: async (id) => (id === CLAUDE_V4 ? fallbackMatch : null) },
    })
    expect(matches).toEqual([fallbackMatch])
  })

  it('index miss consults opencode by-id fallback', async () => {
    const fallbackMatch: ResolveMatch = {
      provider: 'opencode', sessionId: OPENCODE_ID, cwd: '/tmp/oc', projectPath: '/tmp/oc',
      sessionType: 'opencode', lastActivityAt: 7, matchType: 'exact', matchedToken: OPENCODE_ID,
    }
    const { matches } = await resolveSessionCandidates([OPENCODE_ID], {
      getProjects: () => projects([]),
      fallbacks: { opencodeSessionById: async () => fallbackMatch },
    })
    expect(matches).toEqual([fallbackMatch])
  })

  it('zero matches when nothing resolves anywhere', async () => {
    const { matches, providerErrors } = await resolveSessionCandidates(['deadbeef1234'], {
      getProjects: () => fourProviderSnapshot,
      fallbacks: { claudeTranscriptById: async () => null, opencodeSessionById: async () => null },
    })
    expect(matches).toEqual([])
    expect(providerErrors).toEqual([])
  })

  it('a THROWING fallback is reported as a provider error, never as "not found"', async () => {
    const { matches, providerErrors } = await resolveSessionCandidates([OPENCODE_ID], {
      getProjects: () => projects([]),
      fallbacks: { opencodeSessionById: async () => { throw new Error('database is locked') } },
    })
    expect(matches).toEqual([])
    expect(providerErrors).toEqual(['opencode'])
  })

  it('a failed exact-id fallback does NOT hide a later lower-priority match — BOTH the match and the provider error are returned', async () => {
    // First token: full claude UUID, index miss, claude fallback THROWS.
    // Second token: prefix that matches an indexed amplifier session.
    // The caller (route) needs the surviving match AND the error so it can
    // report 'degraded' and the client can refuse to auto-resume.
    const MISSING_V4 = 'ed2afda6-a340-443e-ba60-024a1b3554b9'
    const snapshot = projects([session({ provider: 'amplifier', sessionId: '417e8345-90ab-4cde-8f01-234567890abc' })])
    const { matches, providerErrors } = await resolveSessionCandidates([MISSING_V4, '417e8345'], {
      getProjects: () => snapshot,
      fallbacks: { claudeTranscriptById: async () => { throw new Error('EACCES') } },
    })
    expect(matches).toHaveLength(1)
    expect(matches[0]).toMatchObject({ provider: 'amplifier', matchType: 'prefix' })
    expect(providerErrors).toEqual(['claude'])
  })
})
