// @vitest-environment node
// Ported from the reference session-resolver suite (feat/resume-button),
// adapted to the merged resolveResumeInput API and resume-resolve contract.
// Pins the ordering rule: per candidate token — exact index hit, then exact
// fallback lookups, then and only then prefix matches. A prefix match must
// NEVER outrank any exact resolution of the same or higher-priority token.
import { describe, it, expect, vi } from 'vitest'
import { resolveResumeInput, RESOLVE_MATCH_CAP, type ResolveResumeDeps } from '../../../../server/coding-cli/resolve-session'
import type { ResolveFallbacks } from '../../../../server/coding-cli/resolve-fallbacks'
import { ClaudeTranscriptLocatorError } from '../../../../server/coding-cli/claude-transcript-locator'
import type { ResumeResolveMatch } from '../../../../shared/resume-resolve-contract'
import type { ProjectGroup, CodingCliSession } from '../../../../server/coding-cli/types'

function session(
  overrides: Partial<CodingCliSession> & Pick<CodingCliSession, 'provider' | 'sessionId'>,
): CodingCliSession {
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

function deps(groups: ProjectGroup[], fallbacks?: ResolveFallbacks): ResolveResumeDeps {
  return {
    getProjects: () => groups,
    isIndexReady: () => true,
    fallbacks,
  }
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

describe('resolveResumeInput — matching core', () => {
  it('exact match wins across all providers at once (claude UUID, no hint needed)', async () => {
    const { matches } = await resolveResumeInput(CLAUDE_V4, deps(fourProviderSnapshot))
    expect(matches).toHaveLength(1)
    expect(matches[0]).toMatchObject({
      provider: 'claude',
      sessionId: CLAUDE_V4,
      sessionType: 'claude',
      cwd: '/home/u/proj',
      matchKind: 'exact',
    })
  })

  it('short hex prefix matches the amplifier session (spec row: 417e8345)', async () => {
    const { matches } = await resolveResumeInput('417e8345', deps(fourProviderSnapshot))
    expect(matches).toHaveLength(1)
    expect(matches[0]).toMatchObject({
      provider: 'amplifier',
      sessionId: AMPLIFIER_FULL,
      matchKind: 'prefix',
    })
  })

  it('exact-id match is case-insensitive for UUID/hex tokens', async () => {
    const { matches } = await resolveResumeInput(CLAUDE_V4.toUpperCase(), deps(fourProviderSnapshot))
    expect(matches).toHaveLength(1)
    expect(matches[0].sessionId).toBe(CLAUDE_V4)
  })

  it('ses_ ids are case-SENSITIVE (base62): a case-variant does NOT match', async () => {
    const { matches } = await resolveResumeInput(
      'ses_ROOT0000000000000000000000',
      deps(fourProviderSnapshot),
    )
    expect(matches).toHaveLength(0)
  })

  it('opencode ses_ id resolves to opencode even though other providers exist', async () => {
    const { matches } = await resolveResumeInput(OPENCODE_ID, deps(fourProviderSnapshot))
    expect(matches).toHaveLength(1)
    expect(matches[0].provider).toBe('opencode')
  })

  it('exact match takes precedence over prefix matches of the same token', async () => {
    const snapshot = projects([
      session({ provider: 'amplifier', sessionId: '417e8345', lastActivityAt: 1 }),
      session({ provider: 'amplifier', sessionId: AMPLIFIER_FULL, lastActivityAt: 2 }),
    ])
    const { matches } = await resolveResumeInput('417e8345', deps(snapshot))
    expect(matches).toHaveLength(1)
    expect(matches[0].matchKind).toBe('exact')
  })

  it('ambiguous prefix returns all matches most-recent first, capped', async () => {
    const many = Array.from({ length: RESOLVE_MATCH_CAP + 5 }, (_, i) =>
      session({
        provider: 'amplifier',
        sessionId: `417e8345-90ab-4cde-8f01-${String(i).padStart(12, '0')}`,
        lastActivityAt: i,
      }))
    const { matches } = await resolveResumeInput('417e8345', deps(projects(many)))
    expect(matches).toHaveLength(RESOLVE_MATCH_CAP)
    expect(matches[0].lastActivityAt).toBe(RESOLVE_MATCH_CAP + 4)
    expect(matches[matches.length - 1].lastActivityAt).toBeGreaterThanOrEqual(5)
  })

  it('tries candidates in priority order until one resolves', async () => {
    // ses_ token (highest parser priority) misses everywhere; the UUID resolves.
    const { matches } = await resolveResumeInput(
      `ses_zzzzzzzzzzzzzzzzzzzzzzzzzz ${CLAUDE_V4}`,
      deps(fourProviderSnapshot),
    )
    expect(matches).toHaveLength(1)
    expect(matches[0].sessionId).toBe(CLAUDE_V4)
  })

  it('an EXACT id finds a subagent/child session (spec: scan ALL sessions)', async () => {
    const snapshot = projects([
      session({ provider: 'claude', sessionId: CLAUDE_V4, isSubagent: true }),
    ])
    const { matches } = await resolveResumeInput(CLAUDE_V4, deps(snapshot))
    expect(matches).toHaveLength(1)
    expect(matches[0].sessionId).toBe(CLAUDE_V4)
  })

  it('prefix DISCOVERY does not surface subagent sessions', async () => {
    const snapshot = projects([
      session({ provider: 'claude', sessionId: CLAUDE_V4, isSubagent: true }),
    ])
    const { matches } = await resolveResumeInput('ed2afda6', deps(snapshot))
    expect(matches).toHaveLength(0)
  })

  it('an exact FALLBACK hit beats an indexed PREFIX match of the same token', async () => {
    // Token exactly equals an unindexed session id AND is a prefix of an
    // indexed one: exact must win or the wrong session gets resumed.
    const indexedPrefix = projects([
      session({ provider: 'amplifier', sessionId: `${CLAUDE_V4}9999`, lastActivityAt: 999 }),
    ])
    const fallbackMatch: ResumeResolveMatch = {
      provider: 'claude',
      sessionId: CLAUDE_V4,
      cwd: '/tmp/exact',
      sessionType: 'claude',
      lastActivityAt: 1,
      matchKind: 'exact',
    }
    const { matches } = await resolveResumeInput(CLAUDE_V4, deps(indexedPrefix, {
      claudeTranscriptById: async (id) => (id === CLAUDE_V4 ? fallbackMatch : null),
    }))
    expect(matches).toEqual([fallbackMatch])
  })

  it('sessionType defaults to the provider name when the index has none', async () => {
    const snapshot = projects([session({ provider: 'codex', sessionId: CODEX_V7 })])
    const { matches } = await resolveResumeInput(CODEX_V7, deps(snapshot))
    expect(matches[0].sessionType).toBe('codex')
  })

  it('index miss consults exact-id fallbacks (claude transcript locator)', async () => {
    const fallbackMatch: ResumeResolveMatch = {
      provider: 'claude',
      sessionId: CLAUDE_V4,
      cwd: '/tmp/found',
      sessionType: 'claude',
      lastActivityAt: 42,
      matchKind: 'exact',
    }
    const { matches } = await resolveResumeInput(CLAUDE_V4, deps(projects([]), {
      claudeTranscriptById: async (id) => (id === CLAUDE_V4 ? fallbackMatch : null),
    }))
    expect(matches).toEqual([fallbackMatch])
  })

  it('index miss consults opencode by-id fallback', async () => {
    const fallbackMatch: ResumeResolveMatch = {
      provider: 'opencode',
      sessionId: OPENCODE_ID,
      cwd: '/tmp/oc',
      sessionType: 'opencode',
      lastActivityAt: 7,
      matchKind: 'exact',
    }
    const { matches } = await resolveResumeInput(OPENCODE_ID, deps(projects([]), {
      opencodeSessionById: async () => fallbackMatch,
    }))
    expect(matches).toEqual([fallbackMatch])
  })

  it('zero matches when nothing resolves anywhere', async () => {
    const { matches } = await resolveResumeInput('deadbeef1234', deps(fourProviderSnapshot, {
      claudeTranscriptById: async () => null,
      opencodeSessionById: async () => null,
    }))
    expect(matches).toEqual([])
  })

  it('a THROWING fallback never fails the request: it degrades with a provider error summary', async () => {
    // Provider unavailable (locked/corrupt DB) is NOT "not found": the
    // response must carry the failing provider so the route/client can say
    // "something's wrong" instead of "no matching session".
    const response = await resolveResumeInput(OPENCODE_ID, deps(projects([]), {
      opencodeSessionById: async () => {
        throw new Error('database is locked')
      },
    }))
    expect(response).toMatchObject({ status: 'degraded', matches: [] })
    expect(response.providerErrors).toEqual([
      { provider: 'opencode', message: 'database is locked' },
    ])
  })

  it('provider identity in providerErrors comes from the fallback PAIR, not its position', async () => {
    // Only the opencode fallback is supplied. If identity were positional
    // (index 0 = claude), the error would be misattributed to claude.
    const response = await resolveResumeInput(OPENCODE_ID, deps(projects([]), {
      opencodeSessionById: async () => {
        throw new Error('worker crashed')
      },
    }))
    expect(response.providerErrors.map((e) => e.provider)).toEqual(['opencode'])
  })

  it('a typed ClaudeTranscriptLocatorError surfaces its errno code in the provider error', async () => {
    const cause = Object.assign(new Error('EACCES: permission denied'), { code: 'EACCES' })
    const response = await resolveResumeInput(CLAUDE_V4, deps(projects([]), {
      claudeTranscriptById: async () => {
        throw new ClaudeTranscriptLocatorError('failed to list claude projects dir: /tmp/x', cause)
      },
    }))
    expect(response.status).toBe('degraded')
    expect(response.providerErrors).toEqual([
      {
        provider: 'claude',
        code: 'EACCES',
        message: 'failed to list claude projects dir: /tmp/x',
      },
    ])
  })

  it('a healthy resolve reports NO provider errors', async () => {
    const response = await resolveResumeInput(CLAUDE_V4, deps(fourProviderSnapshot, {
      claudeTranscriptById: async () => null,
      opencodeSessionById: async () => null,
    }))
    expect(response.status).toBe('ready')
    expect(response.providerErrors).toEqual([])
  })

  it('a failed exact-id fallback does NOT hide a later lower-priority match — but marks the response degraded', async () => {
    // First token: full claude UUID, index miss, claude fallback THROWS.
    // Second token: prefix that matches an indexed amplifier session.
    // Resolution must continue so the surviving match is still returned —
    // AND the response must be degraded (a failed HIGHER-priority exact
    // search means the surviving match may be the wrong session, so the
    // client must never auto-resume it).
    const MISSING_V4 = 'ed2afda6-a340-443e-ba60-024a1b3554b9'
    const snapshot = projects([session({ provider: 'amplifier', sessionId: AMPLIFIER_FULL })])
    const response = await resolveResumeInput(`${MISSING_V4} 417e8345`, deps(snapshot, {
      claudeTranscriptById: async () => {
        throw new Error('EACCES')
      },
    }))
    expect(response.matches).toHaveLength(1)
    expect(response.matches[0]).toMatchObject({ provider: 'amplifier', matchKind: 'prefix' })
    expect(response.status).toBe('degraded')
    expect(response.providerErrors).toEqual([{ provider: 'claude', message: 'EACCES' }])
  })

  it('a fallback exact hit for a HIGHER-priority token beats an indexed exact hit of a LOWER-priority token', async () => {
    // ses_ (priority 1) resolves only via fallback; the UUID (priority 2) is
    // an indexed exact hit. The higher-priority token must win.
    const fallbackMatch: ResumeResolveMatch = {
      provider: 'opencode',
      sessionId: OPENCODE_ID,
      cwd: '/tmp/oc',
      sessionType: 'opencode',
      matchKind: 'exact',
    }
    const snapshot = projects([session({ provider: 'claude', sessionId: CLAUDE_V4 })])
    const opencodeSessionById = vi.fn(async () => fallbackMatch)
    const { matches } = await resolveResumeInput(
      `${OPENCODE_ID} ${CLAUDE_V4}`,
      deps(snapshot, { opencodeSessionById }),
    )
    expect(matches).toEqual([fallbackMatch])
    expect(opencodeSessionById).toHaveBeenCalledWith(OPENCODE_ID)
  })

  it('dedupes duplicate (provider, sessionId) snapshot entries, keeping the most recent', async () => {
    const snapshot = projects([
      session({ provider: 'claude', sessionId: CLAUDE_V4, title: 'older file', lastActivityAt: 100 }),
      session({ provider: 'claude', sessionId: CLAUDE_V4, title: 'newer file', lastActivityAt: 500 }),
    ])
    const { matches } = await resolveResumeInput(CLAUDE_V4, deps(snapshot))
    expect(matches).toHaveLength(1)
    expect(matches[0]).toMatchObject({ title: 'newer file', lastActivityAt: 500 })
  })

  it('returns warming (not "not found") while the index is not ready', async () => {
    const response = await resolveResumeInput(CLAUDE_V4, {
      getProjects: () => fourProviderSnapshot,
      isIndexReady: () => false,
    })
    expect(response).toMatchObject({ status: 'warming', matches: [] })
  })
})
