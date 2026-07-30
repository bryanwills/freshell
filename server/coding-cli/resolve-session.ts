import { parseResumeInput } from '../../shared/resume-input-parser.js'
import type {
  ResumeResolveMatch,
  ResumeResolveResponse,
} from '../../shared/resume-resolve-contract.js'
import type { CodingCliSession, ProjectGroup } from './types.js'
import { withRequestBudget, type ResolveFallbacks } from './resolve-fallbacks.js'
import { logger } from '../logger.js'

export const RESOLVE_MATCH_CAP = 20

const log = logger.child({ component: 'resolve-session' })

export interface ResolveResumeDeps {
  getProjects: () => ProjectGroup[]
  isIndexReady: () => boolean
  /**
   * Exact-id fallbacks (buildResolveFallbacks). Shape-gated and budget-capped
   * PER REQUEST here; the opencode lookup runs off the event loop.
   */
  fallbacks?: ResolveFallbacks
}

export async function resolveResumeInput(
  input: string,
  deps: ResolveResumeDeps,
): Promise<ResumeResolveResponse> {
  const { candidates, hint } = parseResumeInput(input)

  if (!deps.isIndexReady()) {
    return { status: 'warming', matches: [], hint }
  }
  if (candidates.length === 0) {
    return { status: 'ready', matches: [], hint }
  }

  const sessions = deps.getProjects().flatMap((group) => group.sessions)

  // Evidence pass: one scan answers all providers at once. Candidates are
  // tried in priority order until one resolves.
  for (const candidate of candidates) {
    const needle = candidate.token.toLowerCase()
    const exact: ResumeResolveMatch[] = []
    const prefix: ResumeResolveMatch[] = []
    for (const session of sessions) {
      const id = session.sessionId.toLowerCase()
      if (id === needle) exact.push(toMatch(session, 'exact'))
      else if (id.startsWith(needle)) prefix.push(toMatch(session, 'prefix'))
    }
    const matches = exact.length > 0 ? exact : prefix
    if (matches.length > 0) {
      matches.sort((a, b) => (b.lastActivityAt ?? 0) - (a.lastActivityAt ?? 0))
      return { status: 'ready', matches: dedupe(matches).slice(0, RESOLVE_MATCH_CAP), hint }
    }
  }

  // Exact-id fallbacks for sessions the index cannot see (opencode child
  // sessions; cwd-less claude transcripts skipped on cold start). Full-id
  // shape gates make wrong-shape tokens free no-ops, the per-request budget
  // bounds the real work a pasted blob can trigger, and the opencode lookup
  // runs OFF the event loop (worker thread) — a locked DB can never stall
  // the server.
  if (deps.fallbacks) {
    const fallbacks = withRequestBudget(deps.fallbacks)
    for (const candidate of candidates) {
      for (const fallback of [fallbacks.opencodeSessionById, fallbacks.claudeTranscriptById]) {
        if (!fallback) continue
        let match: ResumeResolveMatch | null = null
        try {
          match = await fallback(candidate.token)
        } catch (err) {
          // Provider failure ≠ not found, but the contract has no degraded
          // channel yet (follow-up work). Log and keep resolving — never
          // reject: an async express 4 handler would surface that as an
          // unhandled rejection, not a response.
          log.warn(
            { candidateKind: candidate.kind, error: err instanceof Error ? err.message : String(err) },
            'Resume resolve exact-id fallback failed',
          )
        }
        if (match) return { status: 'ready', matches: [match], hint }
      }
    }
  }

  return { status: 'ready', matches: [], hint }
}

function toMatch(session: CodingCliSession, matchKind: 'exact' | 'prefix'): ResumeResolveMatch {
  return {
    provider: session.provider,
    sessionId: session.sessionId,
    cwd: session.cwd ?? session.projectPath,
    sessionType: session.sessionType,
    title: session.title,
    firstUserMessage: session.firstUserMessage,
    lastActivityAt: session.lastActivityAt,
    matchKind,
  }
}

// Real stores carry the SAME (provider, sessionId) on multiple snapshot
// entries (observed: one claude id across 3 transcript files). Matches are
// sorted lastActivityAt desc BEFORE deduping, so the survivor is the entry
// with the most recent activity.
function dedupe(matches: ResumeResolveMatch[]): ResumeResolveMatch[] {
  const seen = new Set<string>()
  return matches.filter((match) => {
    const key = `${match.provider}:${match.sessionId}`
    if (seen.has(key)) return false
    seen.add(key)
    return true
  })
}
