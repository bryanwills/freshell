// @vitest-environment node
import { describe, it, expect, vi } from 'vitest'
import express from 'express'
import request from 'supertest'
import { createSessionsRouter, type SessionsRouterDeps } from '../../../server/sessions-router'
import { FALLBACK_BUDGET_PER_REQUEST } from '../../../server/coding-cli/resolve-fallbacks'
import type { ProjectGroup } from '../../../server/coding-cli/types'

const CLAUDE_V4 = 'ed2afda6-a340-443e-ba60-024a1b3554b4'
const CODEX_V7 = '019fac27-69d7-78a0-b972-b339d551042e'
const OPENCODE_ID = 'ses_root0000000000000000000000'

// ALL FOUR providers — the endpoint's core claim is one scan answers every
// agent, so the fixture must be able to falsify a provider-specific miss.
function snapshot(): ProjectGroup[] {
  return [{
    projectPath: '/home/u/proj',
    sessions: [
      {
        provider: 'claude', sessionId: CLAUDE_V4, projectPath: '/home/u/proj',
        lastActivityAt: 111, cwd: '/home/u/proj', title: 'claude one', sessionType: 'claude',
      },
      {
        provider: 'codex', sessionId: CODEX_V7, projectPath: '/home/u/proj',
        lastActivityAt: 333, cwd: '/home/u/proj', title: 'codex one', sessionType: 'codex',
      },
      {
        provider: 'opencode', sessionId: OPENCODE_ID, projectPath: '/home/u/proj',
        lastActivityAt: 444, cwd: '/home/u/proj', title: 'oc one', sessionType: 'opencode',
      },
      {
        provider: 'amplifier', sessionId: '417e8345-90ab-4cde-8f01-234567890abc', projectPath: '/home/u/proj',
        lastActivityAt: 222, cwd: '/home/u/proj', title: 'amp one', sessionType: 'amplifier',
      },
    ],
  }]
}

function makeApp(overrides: Partial<SessionsRouterDeps> = {}) {
  const deps: SessionsRouterDeps = {
    configStore: {
      getSettings: async () => ({}),
      patchSessionOverride: async () => ({}),
      deleteSession: async () => {},
    },
    codingCliIndexer: {
      getProjects: () => snapshot(),
      refresh: async () => {},
      isReady: () => true,
    },
    codingCliProviders: [],
    perfConfig: { slowSessionRefreshMs: 1000 },
    homeDir: '/home/testuser',
    resolveFallbacks: {},
    ...overrides,
  }
  const app = express()
  app.use(express.json())
  app.use('/api', createSessionsRouter(deps))
  return app
}

describe('POST /api/sessions/resolve', () => {
  it('resolves an exact claude UUID with full resume metadata', async () => {
    const res = await request(makeApp()).post('/api/sessions/resolve').send({ input: CLAUDE_V4 })
    expect(res.status).toBe(200)
    expect(res.body.indexState).toBe('ready')
    expect(res.body.tokens).toEqual([CLAUDE_V4])
    expect(res.body.homeDir).toBe('/home/testuser')
    expect(res.body.matches).toHaveLength(1)
    expect(res.body.matches[0]).toMatchObject({
      provider: 'claude', sessionId: CLAUDE_V4, cwd: '/home/u/proj',
      sessionType: 'claude', matchType: 'exact',
    })
  })

  it('resolves a short hex prefix to the amplifier session', async () => {
    const res = await request(makeApp()).post('/api/sessions/resolve').send({ input: '417e8345' })
    expect(res.body.matches).toHaveLength(1)
    expect(res.body.matches[0].provider).toBe('amplifier')
  })

  it('resolves an exact CODEX UUID from the snapshot', async () => {
    const res = await request(makeApp()).post('/api/sessions/resolve').send({ input: CODEX_V7 })
    expect(res.body.matches).toHaveLength(1)
    expect(res.body.matches[0]).toMatchObject({
      provider: 'codex', sessionId: CODEX_V7, sessionType: 'codex', matchType: 'exact',
    })
  })

  it('resolves an exact OPENCODE ses_ id from the snapshot', async () => {
    const res = await request(makeApp()).post('/api/sessions/resolve').send({ input: OPENCODE_ID })
    expect(res.body.matches).toHaveLength(1)
    expect(res.body.matches[0]).toMatchObject({
      provider: 'opencode', sessionId: OPENCODE_ID, sessionType: 'opencode', matchType: 'exact',
    })
  })

  it('carries the advisory agent hint', async () => {
    const res = await request(makeApp()).post('/api/sessions/resolve')
      .send({ input: `codex resume ${CLAUDE_V4}` })
    expect(res.body.agentHint).toEqual({ provider: 'codex', source: 'command' })
    // Evidence still wins: the store found it under claude.
    expect(res.body.matches[0].provider).toBe('claude')
  })

  it('reports warming when the index is not ready (NOT "not found")', async () => {
    const res = await request(makeApp({
      codingCliIndexer: { getProjects: () => [], refresh: async () => {}, isReady: () => false },
    })).post('/api/sessions/resolve').send({ input: CLAUDE_V4 })
    expect(res.status).toBe(200)
    expect(res.body.indexState).toBe('warming')
    expect(res.body.matches).toEqual([])
  })

  it('returns empty tokens for garbage input', async () => {
    const res = await request(makeApp()).post('/api/sessions/resolve').send({ input: 'no ids here' })
    expect(res.status).toBe(200)
    expect(res.body.tokens).toEqual([])
    expect(res.body.matches).toEqual([])
  })

  it('uses injected exact-id fallbacks on index miss', async () => {
    const fallback = vi.fn().mockResolvedValue({
      provider: 'claude', sessionId: CLAUDE_V4, cwd: '/tmp/fb', projectPath: '/tmp/fb',
      sessionType: 'claude', lastActivityAt: 5, matchType: 'exact', matchedToken: CLAUDE_V4,
    })
    const res = await request(makeApp({
      codingCliIndexer: { getProjects: () => [], refresh: async () => {}, isReady: () => true },
      resolveFallbacks: { claudeTranscriptById: fallback },
    })).post('/api/sessions/resolve').send({ input: CLAUDE_V4 })
    expect(fallback).toHaveBeenCalledWith(CLAUDE_V4)
    expect(res.body.matches[0].cwd).toBe('/tmp/fb')
  })

  it('reports degraded (NOT "not found") when a provider fallback FAILS', async () => {
    const res = await request(makeApp({
      codingCliIndexer: { getProjects: () => [], refresh: async () => {}, isReady: () => true },
      resolveFallbacks: { opencodeSessionById: async () => { throw new Error('database is locked') } },
    })).post('/api/sessions/resolve').send({ input: 'ses_root0000000000000000000000' })
    expect(res.status).toBe(200)
    expect(res.body.indexState).toBe('degraded')
    expect(res.body.providerErrors).toEqual(['opencode'])
    expect(res.body.matches).toEqual([])
  })

  it('bounds per-request fallback work to FALLBACK_BUDGET_PER_REQUEST — and wrong-shape tokens do NOT consume it', async () => {
    const fallback = vi.fn().mockResolvedValue(null)
    await request(makeApp({
      codingCliIndexer: { getProjects: () => [], refresh: async () => {}, isReady: () => true },
      resolveFallbacks: { claudeTranscriptById: fallback },
    })).post('/api/sessions/resolve').send({
      // Parser emits prefixed ids BEFORE UUIDs: if no-op wrong-shape calls
      // counted against the claude budget, the two ses_ tokens would exhaust
      // it and the valid claude UUIDs would never be probed (false negative).
      input: [
        'ses_aaaaaaaaaaaaaaaaaaaaaaaaaa',
        'ses_bbbbbbbbbbbbbbbbbbbbbbbbbb',
        'ed2afda6-a340-443e-ba60-024a1b3554b1',
        'ed2afda6-a340-443e-ba60-024a1b3554b2',
        'ed2afda6-a340-443e-ba60-024a1b3554b3',
      ].join(' '),
    })
    // Exactly the first FALLBACK_BUDGET_PER_REQUEST UUID-shaped tokens reach
    // the claude fallback; ses_ tokens are shape-gated out before the budget.
    expect(fallback.mock.calls.map((c) => c[0])).toEqual([
      'ed2afda6-a340-443e-ba60-024a1b3554b1',
      'ed2afda6-a340-443e-ba60-024a1b3554b2',
    ].slice(0, FALLBACK_BUDGET_PER_REQUEST))
  })

  it('reports degraded when a provider SCAN failed (indexer.getScanFailures), even with no fallback error', async () => {
    const res = await request(makeApp({
      codingCliIndexer: {
        getProjects: () => [], refresh: async () => {}, isReady: () => true,
        getScanFailures: () => ['codex'],
      },
    })).post('/api/sessions/resolve').send({ input: CODEX_V7 })
    expect(res.status).toBe(200)
    expect(res.body.indexState).toBe('degraded')
    expect(res.body.providerErrors).toEqual(['codex'])
    expect(res.body.matches).toEqual([])
  })

  it('reports degraded EVEN WITH matches when a higher-priority exact-id fallback failed (client must not auto-resume a lower-priority match)', async () => {
    // Unindexed full claude UUID (fallback throws) + a second token that
    // prefix-matches the indexed amplifier session: the match survives, but
    // the response must be degraded so the client refuses to auto-resume.
    const MISSING_V4 = 'ed2afda6-a340-443e-ba60-024a1b3554b9'
    const res = await request(makeApp({
      resolveFallbacks: { claudeTranscriptById: async () => { throw new Error('EACCES') } },
    })).post('/api/sessions/resolve').send({ input: `${MISSING_V4} 417e8345` })
    expect(res.status).toBe(200)
    expect(res.body.indexState).toBe('degraded')
    expect(res.body.providerErrors).toEqual(['claude'])
    expect(res.body.matches).toHaveLength(1)
    expect(res.body.matches[0].provider).toBe('amplifier')
  })

  it('a degraded response fire-and-forgets indexer.requestRefresh() so Retry can converge', async () => {
    const requestRefresh = vi.fn()
    await request(makeApp({
      codingCliIndexer: {
        getProjects: () => [], refresh: async () => {}, isReady: () => true,
        getScanFailures: () => ['codex'], requestRefresh,
      },
    })).post('/api/sessions/resolve').send({ input: CODEX_V7 })
    expect(requestRefresh).toHaveBeenCalled()
  })

  it('a scan failure for a DISABLED provider is excluded from providerErrors (unsearched, not degraded)', async () => {
    const res = await request(makeApp({
      configStore: {
        getSettings: async () => ({ codingCli: { enabledProviders: ['claude'] } }),
        patchSessionOverride: async () => ({}),
        deleteSession: async () => {},
      },
      codingCliIndexer: {
        getProjects: () => snapshot(), refresh: async () => {}, isReady: () => true,
        getScanFailures: () => ['codex'],
      },
    })).post('/api/sessions/resolve').send({ input: CLAUDE_V4 })
    expect(res.body.providerErrors).toEqual([])
    expect(res.body.unsearchedProviders).toEqual(['codex', 'opencode', 'amplifier'])
    expect(res.body.indexState).toBe('ready')
  })

  it('reports DISABLED providers as unsearched, never silently as absence', async () => {
    const res = await request(makeApp({
      configStore: {
        getSettings: async () => ({ codingCli: { enabledProviders: ['claude', 'opencode'] } }),
        patchSessionOverride: async () => ({}),
        deleteSession: async () => {},
      },
    })).post('/api/sessions/resolve').send({ input: CLAUDE_V4 })
    expect(res.body.unsearchedProviders).toEqual(['codex', 'amplifier'])
  })

  it('400s on a missing/invalid body', async () => {
    const res = await request(makeApp()).post('/api/sessions/resolve').send({})
    expect(res.status).toBe(400)
  })
})
