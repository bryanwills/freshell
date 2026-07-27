import { describe, expect, it } from 'vitest'
import express from 'express'
import request from 'supertest'

import { createFreshAgentIncidentRouter } from '../../../../server/fresh-agent/incident-router.js'

describe('fresh-agent incident router (zrrj)', () => {
  it('returns hashed, content-free incident state', async () => {
    const app = express()
    app.use('/api/debug/fresh-agent', createFreshAgentIncidentRouter({
      runtimeManager: { inspectState: () => ({ sessions: [{ key: 'k', sessionType: 'freshopencode', provider: 'opencode', sessionIdHash: 'abc123', cwdHash: 'def456' }], pendingRecoveries: 0 }) },
      opencode: {
        inspectSessions: () => [{ sessionIdHash: 'abc123', status: 'running', hasRealSession: true, monitorArmed: true }],
        describeSidecar: () => ({ generation: 2, pid: 4242, baseUrl: 'http://127.0.0.1:1234' }),
      },
    }))
    const res = await request(app).get('/api/debug/fresh-agent').expect(200)
    expect(res.body).toMatchObject({ version: 1, opencode: { sidecar: { generation: 2 } } })
    expect(typeof res.body.time).toBe('string')
    // Deps are passed through verbatim under the versioned envelope.
    expect(res.body.runtime).toEqual({
      sessions: [{ key: 'k', sessionType: 'freshopencode', provider: 'opencode', sessionIdHash: 'abc123', cwdHash: 'def456' }],
      pendingRecoveries: 0,
    })
    expect(res.body.opencode.sessions).toEqual([
      { sessionIdHash: 'abc123', status: 'running', hasRealSession: true, monitorArmed: true },
    ])
    expect(res.body.opencode.sidecar).toEqual({ generation: 2, pid: 4242, baseUrl: 'http://127.0.0.1:1234' })
  })

  it('reports sidecar: null when no sidecar is running', async () => {
    const app = express()
    app.use('/api/debug/fresh-agent', createFreshAgentIncidentRouter({
      runtimeManager: { inspectState: () => ({ sessions: [], pendingRecoveries: 0 }) },
      opencode: {
        inspectSessions: () => [],
        describeSidecar: () => undefined,
      },
    }))
    const res = await request(app).get('/api/debug/fresh-agent').expect(200)
    expect(res.body.opencode.sidecar).toBeNull()
    expect(res.body.opencode.sessions).toEqual([])
  })
})
