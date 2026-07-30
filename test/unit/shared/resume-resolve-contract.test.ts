// @vitest-environment node
// Provider-health contract extension: a broken provider must surface as
// "something's wrong" (status 'degraded' + providerErrors), never as
// "session not found". The extension is ADDITIVE and backward-tolerant:
// legacy responses without the new fields still parse on the client.
import { describe, it, expect } from 'vitest'
import { ResumeResolveResponseSchema } from '../../../shared/resume-resolve-contract'

describe('ResumeResolveResponseSchema (provider-health extension)', () => {
  it('accepts a legacy response without the health fields, defaulting them (backward tolerance)', () => {
    const parsed = ResumeResolveResponseSchema.parse({
      status: 'ready',
      matches: [],
      hint: null,
    })
    expect(parsed.providerErrors).toEqual([])
    expect(parsed.unsearchedProviders).toEqual([])
    expect(parsed.homeDir).toBeUndefined()
  })

  it("accepts status 'degraded' with per-provider error summaries", () => {
    const parsed = ResumeResolveResponseSchema.parse({
      status: 'degraded',
      matches: [],
      hint: null,
      providerErrors: [
        { provider: 'claude', code: 'EACCES', message: 'failed to list claude projects dir' },
        { provider: 'opencode', message: 'database is locked' },
      ],
      unsearchedProviders: ['codex'],
      homeDir: '/home/testuser',
    })
    expect(parsed.status).toBe('degraded')
    expect(parsed.providerErrors).toHaveLength(2)
    expect(parsed.providerErrors[0]).toEqual({
      provider: 'claude',
      code: 'EACCES',
      message: 'failed to list claude projects dir',
    })
    expect(parsed.unsearchedProviders).toEqual(['codex'])
    expect(parsed.homeDir).toBe('/home/testuser')
  })

  it('rejects a provider error entry without a provider name', () => {
    expect(() =>
      ResumeResolveResponseSchema.parse({
        status: 'degraded',
        matches: [],
        hint: null,
        providerErrors: [{ code: 'EACCES' }],
      }),
    ).toThrow()
  })
})
