import { Router } from 'express'

/**
 * Read-only incident-state inspection endpoint (kata zrrj).
 *
 * Mirrors server/debug-router.ts: structural deps, a single GET '/', and a
 * versioned envelope. Every value it serves comes from the deps' own read-only
 * inspection functions, which report identity ONLY as hashForLogs() hashes —
 * never raw session ids, prompts, assistant text, or OpenCode payloads.
 */
export interface IncidentRouterDeps {
  runtimeManager: { inspectState: () => unknown }
  opencode: { inspectSessions: () => unknown; describeSidecar: () => unknown }
}

export function createFreshAgentIncidentRouter(deps: IncidentRouterDeps): Router {
  const { runtimeManager, opencode } = deps
  const router = Router()

  router.get('/', (_req, res) => {
    res.json({
      version: 1,
      time: new Date().toISOString(),
      runtime: runtimeManager.inspectState(),
      opencode: {
        sessions: opencode.inspectSessions(),
        // Explicit null (instead of dropping the key) when no sidecar is running,
        // so incident readers can distinguish "not running" from "field missing".
        sidecar: opencode.describeSidecar() ?? null,
      },
    })
  })

  return router
}
