import { describe, it, expect } from 'vitest'
import express from 'express'
import request from 'supertest'
import {
  createApiRateLimiter,
  API_RATE_LIMIT_WINDOW_MS,
  API_RATE_LIMIT_MAX,
} from '../../../server/rate-limit.js'

function makeApp(limiter = createApiRateLimiter({ windowMs: 60_000, max: 2 })) {
  const app = express()
  app.use('/api', limiter)
  app.get('/api/ping', (_req, res) => {
    res.json({ ok: true })
  })
  return app
}

describe('createApiRateLimiter', () => {
  it('defaults to the production budget of 300 per 60s', async () => {
    // Guard against accidental weakening: the constants must be the production budget...
    expect(API_RATE_LIMIT_WINDOW_MS).toBe(60_000)
    expect(API_RATE_LIMIT_MAX).toBe(300)
    // ...and the factory must actually USE them as its defaults. express-rate-limit
    // v7 does not expose options on the middleware, so verify behaviorally: a
    // default limiter must allow request 300 and reject request 301 within one
    // window. A weakened default (higher max / shorter window) fails this test.
    const app = makeApp(createApiRateLimiter())
    const agent = request(app)
    for (let i = 0; i < API_RATE_LIMIT_MAX; i++) {
      await agent.get('/api/ping').expect(200)
    }
    await agent.get('/api/ping').expect(429)
  })

  it('returns a JSON 429 with code RATE_LIMITED and retryAfterSeconds, plus Retry-After header', async () => {
    const app = makeApp()
    await request(app).get('/api/ping').expect(200)
    await request(app).get('/api/ping').expect(200)
    const res = await request(app).get('/api/ping').expect(429)
    expect(res.headers['retry-after']).toMatch(/^\d+$/)
    expect(res.body).toMatchObject({ error: 'Too many requests', code: 'RATE_LIMITED' })
    expect(res.body.retryAfterSeconds).toBe(Number(res.headers['retry-after']))
    expect(res.type).toMatch(/json/)
  })
})
