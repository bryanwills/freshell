// @vitest-environment node
import { describe, it, expect, beforeEach, vi } from 'vitest'
import express, { type Express } from 'express'
import request from 'supertest'
import { createSessionsRouter } from '../../../server/sessions-router.js'
import {
  buildResolveFallbacks,
  FALLBACK_BUDGET_PER_REQUEST,
  type ResolveFallbacks,
} from '../../../server/coding-cli/resolve-fallbacks.js'
import type { CodingCliProvider } from '../../../server/coding-cli/provider.js'
import type { ProjectGroup } from '../../../server/coding-cli/types.js'

const CLAUDE_ID = 'ed2afda6-a340-443e-ba60-024a1b3554b4'
const CODEX_ID = '019fac27-69d7-78a0-b972-b339d551042e'
const OPENCODE_ID = 'ses_root0000000000000000000000'
const AMP_ID_NEW = '417e8345-aaaa-4bbb-8ccc-000000000001'
const AMP_ID_OLD = '417e8345-bbbb-4ccc-8ddd-000000000002'

function fixtureProjects(): ProjectGroup[] {
  return [
    {
      projectPath: '/repo/alpha',
      sessions: [
        {
          provider: 'claude',
          sessionId: CLAUDE_ID,
          projectPath: '/repo/alpha',
          cwd: '/repo/alpha',
          title: 'Fix the parser',
          firstUserMessage: 'fix the parser',
          lastActivityAt: 400,
        },
        {
          provider: 'codex',
          sessionId: CODEX_ID,
          projectPath: '/repo/alpha',
          cwd: '/repo/alpha',
          sessionType: 'codex',
          lastActivityAt: 300,
        },
      ],
    },
    {
      projectPath: '/repo/beta',
      sessions: [
        {
          provider: 'opencode',
          sessionId: OPENCODE_ID,
          projectPath: '/repo/beta',
          cwd: '/repo/beta',
          lastActivityAt: 200,
        },
        {
          provider: 'amplifier',
          sessionId: AMP_ID_NEW,
          projectPath: '/repo/beta',
          cwd: '/repo/beta',
          lastActivityAt: 900,
        },
        {
          provider: 'amplifier',
          sessionId: AMP_ID_OLD,
          projectPath: '/repo/beta',
          cwd: '/repo/beta',
          lastActivityAt: 100,
        },
      ],
    },
  ]
}

interface HarnessOptions {
  projects?: ProjectGroup[]
  ready?: boolean
  /** Omit getIndexReadiness entirely (readiness comes from the indexer signal alone). */
  omitStartupReadiness?: boolean
  resolveFallbacks?: ResolveFallbacks
  settings?: Record<string, unknown>
  indexerIsReady?: () => boolean
  getScanFailures?: () => string[]
  requestRefresh?: () => void
  homeDir?: string
}

const opencodeStub = () =>
  ({ name: 'opencode', getDatabasePath: () => '/tmp/x.db' }) as unknown as CodingCliProvider
const claudeStub = () => ({ name: 'claude' }) as unknown as CodingCliProvider

function buildApp(options: HarnessOptions = {}): Express {
  const app = express()
  app.use(express.json())
  app.use(
    '/api',
    createSessionsRouter({
      configStore: {
        getSettings: vi.fn().mockResolvedValue(options.settings ?? {}),
        patchSessionOverride: vi.fn(),
        deleteSession: vi.fn(),
      },
      codingCliIndexer: {
        getProjects: () => options.projects ?? fixtureProjects(),
        refresh: vi.fn().mockResolvedValue(undefined),
        isReady: options.indexerIsReady,
        getScanFailures: options.getScanFailures,
        requestRefresh: options.requestRefresh,
      },
      codingCliProviders: [],
      perfConfig: { slowSessionRefreshMs: 500 },
      terminalMetadata: { list: () => [] },
      getIndexReadiness: options.omitStartupReadiness ? undefined : () => options.ready ?? true,
      resolveFallbacks: options.resolveFallbacks,
      homeDir: options.homeDir,
    }),
  )
  return app
}

const post = (app: Express, body: unknown) =>
  request(app).post('/api/sessions/resolve').send(body as object)

describe('POST /api/sessions/resolve', () => {
  let app: Express
  beforeEach(() => {
    app = buildApp()
  })

  it.each([
    ['claude exact uuid', CLAUDE_ID, 'claude', CLAUDE_ID],
    ['codex exact via command line', `codex resume ${CODEX_ID}`, 'codex', CODEX_ID],
    ['opencode exact via command line', `opencode --session ${OPENCODE_ID}`, 'opencode', OPENCODE_ID],
  ] as const)('%s resolves to a single exact match', async (_label, input, provider, id) => {
    const res = await post(app, { input })
    expect(res.status).toBe(200)
    expect(res.body.status).toBe('ready')
    expect(res.body.matches).toHaveLength(1)
    expect(res.body.matches[0]).toMatchObject({ provider, sessionId: id, matchKind: 'exact' })
  })

  it('returns full resume metadata on matches', async () => {
    const res = await post(app, { input: CLAUDE_ID })
    expect(res.body.matches[0]).toMatchObject({
      provider: 'claude',
      sessionId: CLAUDE_ID,
      cwd: '/repo/alpha',
      title: 'Fix the parser',
      firstUserMessage: 'fix the parser',
      lastActivityAt: 400,
    })
  })

  it('prefix-matches short hex across providers, most-recent first', async () => {
    const res = await post(app, { input: '417e8345' })
    expect(res.body.status).toBe('ready')
    expect(res.body.matches.map((m: { sessionId: string }) => m.sessionId)).toEqual([
      AMP_ID_NEW,
      AMP_ID_OLD,
    ])
    expect(res.body.matches[0].matchKind).toBe('prefix')
    expect(res.body.matches[0].provider).toBe('amplifier')
  })

  it('caps ambiguous prefix matches at 20', async () => {
    const many: ProjectGroup[] = [
      {
        projectPath: '/repo/many',
        sessions: Array.from({ length: 25 }, (_, i) => ({
          provider: 'amplifier',
          sessionId: `417e8345-0000-4000-8000-${String(i).padStart(12, '0')}`,
          projectPath: '/repo/many',
          lastActivityAt: i,
        })),
      },
    ]
    const res = await post(buildApp({ projects: many }), { input: '417e8345' })
    expect(res.body.matches).toHaveLength(20)
    expect(res.body.matches[0].lastActivityAt).toBe(24) // most recent first
  })

  it('dedupes duplicate (provider, sessionId) snapshot entries, keeping the most recent', async () => {
    // Real-store finding: the same claude sessionId can appear on MULTIPLE
    // snapshot entries (same id, different transcript files).
    const dup: ProjectGroup[] = [
      {
        projectPath: '/repo/alpha',
        sessions: [
          {
            provider: 'claude',
            sessionId: CLAUDE_ID,
            projectPath: '/repo/alpha',
            cwd: '/repo/alpha',
            title: 'older file',
            lastActivityAt: 100,
          },
          {
            provider: 'claude',
            sessionId: CLAUDE_ID,
            projectPath: '/repo/alpha',
            cwd: '/repo/alpha',
            title: 'newer file',
            lastActivityAt: 500,
          },
        ],
      },
    ]
    const res = await post(buildApp({ projects: dup }), { input: CLAUDE_ID })
    expect(res.body.matches).toHaveLength(1)
    expect(res.body.matches[0]).toMatchObject({ title: 'newer file', lastActivityAt: 500 })
  })

  it('reports hint alongside evidence', async () => {
    const res = await post(app, { input: `codex resume ${CODEX_ID}` })
    expect(res.body.hint).toEqual({ provider: 'codex', source: 'command' })
  })

  it('returns ready + empty matches for an unknown id', async () => {
    const res = await post(app, { input: '019fffff-ffff-7fff-bfff-ffffffffffff' })
    expect(res.body).toMatchObject({ status: 'ready', matches: [] })
  })

  it('returns warming (not "not found") while the index is not ready', async () => {
    const res = await post(buildApp({ ready: false }), { input: CLAUDE_ID })
    expect(res.body).toMatchObject({ status: 'warming', matches: [] })
  })

  it('falls back to the off-thread opencode by-id lookup on exact-id index miss (row directory as cwd)', async () => {
    const unknown = 'ses_child000000000000000000000'
    const runOpencodeById = vi.fn().mockResolvedValue({
      sessionId: unknown,
      cwd: '/repo/beta',
      title: 'child session',
      createdAt: 1,
      lastActivityAt: 2,
      projectPath: '/repo/beta',
    })
    const res = await post(
      buildApp({
        resolveFallbacks: buildResolveFallbacks([opencodeStub()], { runOpencodeById }),
      }),
      { input: unknown },
    )
    expect(runOpencodeById).toHaveBeenCalledWith('/tmp/x.db', unknown)
    expect(res.body.matches).toEqual([
      {
        provider: 'opencode',
        sessionId: unknown,
        cwd: '/repo/beta',
        sessionType: 'opencode',
        title: 'child session',
        lastActivityAt: 2,
        matchKind: 'exact',
      },
    ])
  })

  it('falls back to the claude transcript locator on exact-id index miss', async () => {
    const unknown = 'aaaaaaaa-1111-4222-8333-444444444444'
    const res = await post(
      buildApp({
        resolveFallbacks: buildResolveFallbacks([claudeStub()], {
          locateClaudeTranscript: vi.fn().mockResolvedValue({
            sessionId: unknown,
            sourceFile: `/home/u/.claude/projects/x/${unknown}.jsonl`,
            cwd: '/repo/gamma',
          }),
        }),
      }),
      { input: unknown },
    )
    expect(res.body.matches).toEqual([
      {
        provider: 'claude',
        sessionId: unknown,
        cwd: '/repo/gamma',
        sessionType: 'claude',
        matchKind: 'exact',
      },
    ])
  })

  it('shape-gates the opencode fallback: a short ses_ token never touches the DB path', async () => {
    const runOpencodeById = vi.fn().mockResolvedValue(null)
    const res = await post(
      buildApp({
        resolveFallbacks: buildResolveFallbacks([opencodeStub()], { runOpencodeById }),
      }),
      { input: 'ses_short123' },
    )
    expect(res.body).toMatchObject({ status: 'ready', matches: [] })
    expect(runOpencodeById).not.toHaveBeenCalled()
  })

  it('caps opencode by-id fallback work per request, with a FRESH budget on the next request', async () => {
    const runOpencodeById = vi.fn().mockResolvedValue(null)
    const app2 = buildApp({
      resolveFallbacks: buildResolveFallbacks([opencodeStub()], { runOpencodeById }),
    })
    const ids = ['a', 'b', 'c'].map((c) => `ses_${c.repeat(26)}`)
    const input = ids.join(' ')
    const first = await post(app2, { input })
    expect(first.body.matches).toEqual([])
    expect(runOpencodeById).toHaveBeenCalledTimes(FALLBACK_BUDGET_PER_REQUEST)
    // The budget is per-request, not per-server: a second request gets its own.
    await post(app2, { input })
    expect(runOpencodeById).toHaveBeenCalledTimes(FALLBACK_BUDGET_PER_REQUEST * 2)
  })

  it('a failing opencode by-id lookup (locked DB) never fails the request — it degrades with a provider error', async () => {
    const runOpencodeById = vi.fn().mockRejectedValue(new Error('database is locked'))
    const res = await post(
      buildApp({
        resolveFallbacks: buildResolveFallbacks([opencodeStub()], { runOpencodeById }),
      }),
      { input: 'ses_child000000000000000000000' },
    )
    expect(res.status).toBe(200)
    expect(res.body).toMatchObject({ status: 'degraded', matches: [] })
    expect(res.body.providerErrors).toEqual([
      { provider: 'opencode', message: 'database is locked' },
    ])
  })

  it('reports degraded (NOT "not found") when a provider SCAN failed, even with no fallback error', async () => {
    const res = await post(
      buildApp({ projects: [], getScanFailures: () => ['codex'] }),
      { input: CODEX_ID },
    )
    expect(res.status).toBe(200)
    expect(res.body.status).toBe('degraded')
    expect(res.body.providerErrors).toEqual([
      { provider: 'codex', message: 'session scan failed' },
    ])
    expect(res.body.matches).toEqual([])
  })

  it('reports degraded EVEN WITH matches when a higher-priority exact-id fallback failed (client must not auto-resume)', async () => {
    // Unindexed full claude UUID (fallback throws) + a second token that
    // prefix-matches an indexed amplifier session: the match survives, but
    // the response must be degraded so the client refuses to auto-resume.
    const MISSING_V4 = 'ed2afda6-a340-443e-ba60-024a1b3554b9'
    const res = await post(
      buildApp({
        resolveFallbacks: buildResolveFallbacks([claudeStub()], {
          locateClaudeTranscript: vi.fn().mockRejectedValue(new Error('EACCES: permission denied')),
        }),
      }),
      { input: `${MISSING_V4} 417e8345` },
    )
    expect(res.status).toBe(200)
    expect(res.body.status).toBe('degraded')
    expect(res.body.providerErrors.map((e: { provider: string }) => e.provider)).toEqual(['claude'])
    expect(res.body.matches.length).toBeGreaterThan(0)
    expect(res.body.matches[0].provider).toBe('amplifier')
  })

  it('a degraded response fire-and-forgets indexer.requestRefresh() so Retry can converge', async () => {
    const requestRefresh = vi.fn()
    await post(
      buildApp({ projects: [], getScanFailures: () => ['codex'], requestRefresh }),
      { input: CODEX_ID },
    )
    expect(requestRefresh).toHaveBeenCalled()
  })

  it('a scan failure for a DISABLED provider is excluded from providerErrors (unsearched, not degraded)', async () => {
    const res = await post(
      buildApp({
        settings: { codingCli: { enabledProviders: ['claude'] } },
        getScanFailures: () => ['codex'],
      }),
      { input: CLAUDE_ID },
    )
    expect(res.body.providerErrors).toEqual([])
    expect(res.body.unsearchedProviders).toEqual(['codex', 'opencode', 'amplifier'])
    expect(res.body.status).toBe('ready')
  })

  it('reports DISABLED providers as unsearched, never silently as absence', async () => {
    const res = await post(
      buildApp({ settings: { codingCli: { enabledProviders: ['claude', 'opencode'] } } }),
      { input: CLAUDE_ID },
    )
    expect(res.body.unsearchedProviders).toEqual(['codex', 'amplifier'])
  })

  it('returns the server home directory so the client can prefill a concrete cwd', async () => {
    const res = await post(buildApp({ homeDir: '/home/testuser' }), { input: CLAUDE_ID })
    expect(res.body.homeDir).toBe('/home/testuser')
  })

  it("index readiness is startupState OR'd with the indexer signal: indexer-ready wins over a stuck-false startup task", async () => {
    // startupState readiness can stick false forever (its markReady only runs
    // in the start chain's success path); once the indexer has completed a
    // refresh the endpoint must stop reporting warming.
    const res = await post(
      buildApp({ ready: false, indexerIsReady: () => true }),
      { input: CLAUDE_ID },
    )
    expect(res.body.status).toBe('ready')
    expect(res.body.matches).toHaveLength(1)
  })

  it('reports warming from the indexer signal alone when no startup readiness is wired', async () => {
    const res = await post(
      buildApp({ omitStartupReadiness: true, indexerIsReady: () => false }),
      { input: CLAUDE_ID },
    )
    expect(res.body).toMatchObject({ status: 'warming', matches: [] })
  })

  it('returns ready + empty matches for garbage input with no id-like token', async () => {
    const res = await post(app, { input: 'hello decade facade!!' })
    expect(res.body).toMatchObject({ status: 'ready', matches: [], hint: null })
  })

  it('rejects an invalid body with 400', async () => {
    const res = await post(app, { nope: true })
    expect(res.status).toBe(400)
    expect(res.body.error).toBeDefined()
  })
})
