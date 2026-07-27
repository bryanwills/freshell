import rateLimit from 'express-rate-limit'
import type { Request, Response } from 'express'

export const API_RATE_LIMIT_WINDOW_MS = 60_000
export const API_RATE_LIMIT_MAX = 300

/**
 * Global /api rate limiter. Budget is intentionally identical to the previous
 * inline configuration (300 req / 60 s / IP) — do NOT raise it here; the fix
 * for snapshot storms is client-side scheduling + backoff, not a bigger budget.
 * This factory only adds a JSON 429 body (with retryAfterSeconds) and testability.
 */
export function createApiRateLimiter(options: { windowMs?: number; max?: number } = {}) {
  const windowMs = options.windowMs ?? API_RATE_LIMIT_WINDOW_MS
  return rateLimit({
    windowMs,
    max: options.max ?? API_RATE_LIMIT_MAX,
    standardHeaders: true,
    legacyHeaders: false,
    handler: (_req: Request, res: Response) => {
      const retryAfterHeader = res.getHeader('Retry-After')
      const retryAfterSeconds = Number(retryAfterHeader) || Math.ceil(windowMs / 1000)
      res.status(429).json({ error: 'Too many requests', code: 'RATE_LIMITED', retryAfterSeconds })
    },
  })
}
