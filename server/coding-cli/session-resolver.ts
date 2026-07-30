import type { CodingCliProviderName, CodingCliSession, ProjectGroup } from './types.js'

/** Disambiguation cap (spec: "capped, e.g. 20, most-recent first"). */
export const RESOLVE_MATCH_CAP = 20

export type ResolveMatch = {
  provider: CodingCliProviderName
  sessionId: string
  cwd?: string
  projectPath: string
  sessionType: string
  title?: string
  firstUserMessage?: string
  lastActivityAt: number
  matchType: 'exact' | 'prefix'
  matchedToken: string
}

export type ExactIdFallback = (id: string) => Promise<ResolveMatch | null>

export type SessionResolverDeps = {
  getProjects: () => ProjectGroup[]
  /**
   * Exact-id fallbacks for sessions the index misses (claude transcript locate,
   * opencode by-id DB query). ACCEPTED LIMITATION (per spec): PREFIX matching
   * only covers indexed sessions — fallbacks are exact-id only.
   * A fallback that THROWS means "provider unavailable" (locked/corrupt DB,
   * missing roots) — recorded in providerErrors, never treated as "not found".
   */
  fallbacks?: {
    claudeTranscriptById?: ExactIdFallback
    opencodeSessionById?: ExactIdFallback
  }
}

function toMatch(session: CodingCliSession, matchType: 'exact' | 'prefix', matchedToken: string): ResolveMatch {
  return {
    provider: session.provider,
    sessionId: session.sessionId,
    cwd: session.cwd ?? session.projectPath,
    projectPath: session.projectPath,
    sessionType: session.sessionType ?? session.provider,
    title: session.title,
    firstUserMessage: session.firstUserMessage,
    lastActivityAt: session.lastActivityAt,
    matchType,
    matchedToken,
  }
}

function rank(matches: ResolveMatch[]): ResolveMatch[] {
  return [...matches]
    .sort((a, b) => b.lastActivityAt - a.lastActivityAt)
    .slice(0, RESOLVE_MATCH_CAP)
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
 * One scan answers all agents at once (spec: "evidence decides") — no per-agent
 * probe ordering. Candidates are tried best-first; the first token that
 * resolves anywhere wins. A fallback that throws marks its provider in
 * providerErrors (unavailable ≠ not found) — AND resolution still continues
 * to prefix/later-candidate matches, so callers get BOTH the surviving
 * matches and the errors. The route turns non-empty providerErrors into
 * indexState 'degraded' even when matches exist: a failed HIGHER-priority
 * exact search means a surviving lower-priority match may be the wrong
 * session, so the client must never auto-resume it (hard requirement).
 */
export async function resolveSessionCandidates(
  candidates: string[],
  deps: SessionResolverDeps,
): Promise<{ matches: ResolveMatch[]; providerErrors: CodingCliProviderName[] }> {
  // ALL sessions, including subagent children: the spec says scan all
  // sessions — an exact pasted id must resolve even for hidden child
  // sessions (claude/codex subagents, opencode children). Prefix DISCOVERY
  // stays top-level-only below.
  const sessions = deps.getProjects().flatMap((p) => p.sessions)

  const providerErrors = new Set<CodingCliProviderName>()
  const done = (matches: ResolveMatch[]) => ({ matches, providerErrors: [...providerErrors] })

  for (const token of candidates) {
    const ci = isCaseInsensitiveToken(token)
    const norm = (value: string) => (ci ? value.toLowerCase() : value)
    const target = norm(token)

    const exact = sessions.filter((s) => norm(s.sessionId) === target)
    if (exact.length > 0) return done(rank(exact.map((s) => toMatch(s, 'exact', token))))

    // Exact-id fallbacks run BEFORE prefix matching: an unindexed session
    // whose id EQUALS the token must beat any indexed session whose id merely
    // begins with it ("exact takes precedence over prefix"). This is cheap:
    // the production fallbacks are shape-gated to FULL ids (UUID / ses_ +
    // 26 base62) inside withRequestBudget, so prefix-length tokens do no
    // fallback work at all.
    const fallbackHits: ResolveMatch[] = []
    const fallbackEntries: Array<[CodingCliProviderName, ExactIdFallback | undefined]> = [
      ['claude', deps.fallbacks?.claudeTranscriptById],
      ['opencode', deps.fallbacks?.opencodeSessionById],
    ]
    for (const [provider, fallback] of fallbackEntries) {
      if (!fallback) continue
      try {
        const hit = await fallback(token)
        if (hit) fallbackHits.push(hit)
      } catch {
        // Provider unavailable (locked/corrupt DB, unreadable roots) is NOT
        // "not found" — record it so the route reports a degraded state and
        // the client offers retry instead of "no matching session".
        providerErrors.add(provider)
      }
    }
    if (fallbackHits.length > 0) return done(rank(fallbackHits))

    // Prefix DISCOVERY is top-level-only: surfacing hidden subagent children
    // for partial ids would flood disambiguation with noise; exact ids above
    // still reach them.
    const prefix = sessions.filter((s) => !s.isSubagent && norm(s.sessionId).startsWith(target))
    if (prefix.length > 0) return done(rank(prefix.map((s) => toMatch(s, 'prefix', token))))
  }

  return done([])
}
