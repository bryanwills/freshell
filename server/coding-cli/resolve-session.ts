import { parseResumeInput } from '../../shared/resume-input-parser.js'
import type {
  ResumeResolveHint,
  ResumeResolveMatch,
  ResumeResolveProviderError,
} from '../../shared/resume-resolve-contract.js'
import type { CodingCliSession, ProjectGroup } from './types.js'
import { withRequestBudget, type ExactIdFallback, type ResolveFallbacks } from './resolve-fallbacks.js'
import { ClaudeTranscriptLocatorError } from './claude-transcript-locator.js'
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

/**
 * Resolve pipeline result. providerErrors carries fallback failures only
 * (provider unavailable ≠ not found); the route merges in indexer scan
 * failures and adds unsearchedProviders/homeDir before responding.
 */
export interface ResolveResumeResult {
  status: 'ready' | 'warming' | 'degraded'
  matches: ResumeResolveMatch[]
  hint: ResumeResolveHint | null
  providerErrors: ResumeResolveProviderError[]
}

/** Errno code for a provider-error summary: typed locator errors carry it in .code. */
function errnoCodeOf(err: unknown): string | undefined {
  if (err instanceof ClaudeTranscriptLocatorError) return err.code
  const code = (err as NodeJS.ErrnoException | null)?.code
  return typeof code === 'string' ? code : undefined
}

/**
 * UUID/hex-family tokens (hex digits + dashes only) match case-insensitively.
 * Everything else — notably ses_ + base62 ids — matches case-SENSITIVELY:
 * base62 upper/lower case are distinct values, so case-folding could resolve
 * the WRONG session.
 */
function isCaseInsensitiveToken(token: string): boolean {
  return /^[0-9a-fA-F-]+$/.test(token)
}

/**
 * One scan answers all providers at once (spec: "evidence decides"). Candidate
 * tokens are tried in priority order; PER TOKEN the resolution order is:
 *
 *   1. exact index hits (ALL sessions, including subagent children — an exact
 *      pasted id must resolve even for hidden child sessions),
 *   2. exact-id fallbacks for sessions the index cannot see (opencode child
 *      sessions; cwd-less claude transcripts skipped on cold start),
 *   3. and only then prefix matches (top-level sessions only — surfacing
 *      hidden subagent children for partial ids would flood disambiguation
 *      with noise).
 *
 * A prefix match must NEVER outrank any exact resolution of the same or a
 * higher-priority token: an unindexed session whose id EQUALS the token beats
 * any indexed session whose id merely begins with it, or the wrong session
 * gets resumed.
 */
export async function resolveResumeInput(
  input: string,
  deps: ResolveResumeDeps,
): Promise<ResolveResumeResult> {
  const { candidates, hint } = parseResumeInput(input)

  if (!deps.isIndexReady()) {
    return { status: 'warming', matches: [], hint, providerErrors: [] }
  }
  if (candidates.length === 0) {
    return { status: 'ready', matches: [], hint, providerErrors: [] }
  }

  const sessions = deps.getProjects().flatMap((group) => group.sessions)

  // The fallback budget is PER REQUEST (not per server): wrap once, before
  // the token loop. Full-id shape gates make wrong-shape tokens free no-ops,
  // the budget bounds the real work a pasted blob can trigger, and the
  // opencode lookup runs OFF the event loop (worker thread) — a locked DB
  // can never stall the server.
  const fallbacks = deps.fallbacks ? withRequestBudget(deps.fallbacks) : undefined

  // Provider failure ≠ not found: a throwing fallback records a per-provider
  // error summary while resolution CONTINUES (prefix/later tokens). Any entry
  // here makes the result 'degraded' — even with matches, because a failed
  // HIGHER-priority exact search may have hidden the right session.
  const errorsByProvider = new Map<string, ResumeResolveProviderError>()

  const finish = (matches: ResumeResolveMatch[]): ResolveResumeResult => {
    matches.sort((a, b) => (b.lastActivityAt ?? 0) - (a.lastActivityAt ?? 0))
    const providerErrors = [...errorsByProvider.values()]
    return {
      status: providerErrors.length > 0 ? 'degraded' : 'ready',
      matches: dedupe(matches).slice(0, RESOLVE_MATCH_CAP),
      hint,
      providerErrors,
    }
  }

  for (const candidate of candidates) {
    const ci = isCaseInsensitiveToken(candidate.token)
    const norm = (value: string) => (ci ? value.toLowerCase() : value)
    const target = norm(candidate.token)

    // 1. Exact index hits — scan ALL sessions, subagent children included.
    const exact = sessions.filter((session) => norm(session.sessionId) === target)
    if (exact.length > 0) {
      return finish(exact.map((session) => toMatch(session, 'exact')))
    }

    // 2. Exact-id fallbacks run BEFORE prefix matching. Cheap: the shape
    // gates inside withRequestBudget mean prefix-length tokens do no
    // fallback work at all. Iterated as [provider, fallback] PAIRS so a
    // failure is attributed to the RIGHT provider (identity travels with
    // the entry, never its position).
    if (fallbacks) {
      const hits: ResumeResolveMatch[] = []
      const entries: Array<[string, ExactIdFallback | undefined]> = [
        ['claude', fallbacks.claudeTranscriptById],
        ['opencode', fallbacks.opencodeSessionById],
      ]
      for (const [provider, fallback] of entries) {
        if (!fallback) continue
        try {
          const match = await fallback(candidate.token)
          if (match) hits.push(match)
        } catch (err) {
          // Never reject: an async express 4 handler would surface that as
          // an unhandled rejection, not a response. Typed locator errors
          // (ClaudeTranscriptLocatorError) arrive here intact — errno in .code.
          const code = errnoCodeOf(err)
          if (!errorsByProvider.has(provider)) {
            errorsByProvider.set(provider, {
              provider,
              ...(code ? { code } : {}),
              message: err instanceof Error ? err.message : String(err),
            })
          }
          log.warn(
            { provider, candidateKind: candidate.kind, error: err instanceof Error ? err.message : String(err) },
            'Resume resolve exact-id fallback failed',
          )
        }
      }
      if (hits.length > 0) return finish(hits)
    }

    // 3. Prefix DISCOVERY — top-level sessions only; exact ids above still
    // reach subagent children.
    const prefix = sessions.filter(
      (session) => !session.isSubagent && norm(session.sessionId).startsWith(target),
    )
    if (prefix.length > 0) {
      return finish(prefix.map((session) => toMatch(session, 'prefix')))
    }
  }

  return finish([])
}

function toMatch(session: CodingCliSession, matchKind: 'exact' | 'prefix'): ResumeResolveMatch {
  return {
    provider: session.provider,
    sessionId: session.sessionId,
    cwd: session.cwd ?? session.projectPath,
    sessionType: session.sessionType ?? session.provider,
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
