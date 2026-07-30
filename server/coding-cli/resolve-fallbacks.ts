import type { CodingCliProvider } from './provider.js'
import type { ResumeResolveMatch } from '../../shared/resume-resolve-contract.js'
import type { SessionMetadataEntry } from '../session-metadata-store.js'
import type { ClaudeTranscriptHit } from './claude-transcript-locator.js'
import { runOpencodeSessionByIdOffThread } from './providers/opencode-by-id-runner.js'
import type { OpencodeSessionRow } from './providers/opencode-listing-query.js'

/** Exact-id resolve fallback: full id in, contract match (or miss) out. */
export type ExactIdFallback = (id: string) => Promise<ResumeResolveMatch | null>

export type ResolveFallbacks = {
  claudeTranscriptById?: ExactIdFallback
  opencodeSessionById?: ExactIdFallback
}

/**
 * FULL-id shape gates, enforced in withRequestBudget BEFORE the budget check:
 * a wrong-shape token is a free no-op miss that must neither do work nor
 * consume budget (otherwise earlier ses_ tokens could exhaust the claude
 * budget before a valid later claude UUID — false negative).
 */
export const FALLBACK_ID_SHAPES: Record<keyof ResolveFallbacks, RegExp> = {
  claudeTranscriptById: /^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$/,
  opencodeSessionById: /^ses_[0-9a-zA-Z]{26}$/,
}

/**
 * Per-request work budget: each fallback may do REAL work at most this many
 * times per request; beyond that it reports a miss without doing work.
 * Combined with the shape gates this bounds the fallback work (FS probes,
 * worker spawns) one request can trigger, no matter how many id-shaped
 * tokens a pasted blob contains.
 */
export const FALLBACK_BUDGET_PER_REQUEST = 2

export function withRequestBudget(
  fallbacks: ResolveFallbacks,
  max = FALLBACK_BUDGET_PER_REQUEST,
): ResolveFallbacks {
  const budgeted = (key: keyof ResolveFallbacks): ExactIdFallback | undefined => {
    const fallback = fallbacks[key]
    if (!fallback) return undefined
    const shape = FALLBACK_ID_SHAPES[key]
    let used = 0
    return async (id) => {
      // Shape FIRST, budget SECOND — order is load-bearing (see FALLBACK_ID_SHAPES).
      if (!shape.test(id)) return null
      if (used >= max) return null
      used += 1
      return fallback(id)
    }
  }
  return {
    claudeTranscriptById: budgeted('claudeTranscriptById'),
    opencodeSessionById: budgeted('opencodeSessionById'),
  }
}

export type BuildResolveFallbacksOptions = {
  /**
   * Metadata store (already a SessionsRouterDeps member): sessions opened via
   * freshclaude/freshopencode/kilroy record their real runtime here, and a
   * resume MUST reopen through that runtime, not the bare provider default.
   */
  sessionMetadataStore?: { getAll(): Promise<Record<string, SessionMetadataEntry>> }
  /** Injectable for unit tests; production default runs the worker-thread runner. */
  runOpencodeById?: (dbPath: string, sessionId: string) => Promise<OpencodeSessionRow | null>
  /**
   * Claude exact-id transcript locate. Injected (rather than defaulted) because
   * the locator needs the claude projects dir, which index.ts owns wiring.
   */
  locateClaudeTranscript?: (sessionId: string) => Promise<ClaudeTranscriptHit | null>
}

/** Build the production exact-id fallbacks from the live provider set. */
export function buildResolveFallbacks(
  providers: CodingCliProvider[],
  opts: BuildResolveFallbacksOptions = {},
): ResolveFallbacks {
  const claude = providers.find((p) => p.name === 'claude')
  const opencode = providers.find((p) => p.name === 'opencode') as
    (CodingCliProvider & { getDatabasePath?: () => string }) | undefined
  const runById = opts.runOpencodeById ?? runOpencodeSessionByIdOffThread

  // Resume tuple correctness: prefer the runtime recorded in session metadata
  // (freshclaude/freshopencode/kilroy), fall back to the provider name.
  // Metadata-store failures degrade to the default — they must not turn a
  // located session into a provider error.
  const sessionTypeFor = async (provider: 'claude' | 'opencode', id: string): Promise<string> => {
    const all = await opts.sessionMetadataStore?.getAll().catch(() => undefined)
    return all?.[`${provider}:${id}`]?.sessionType ?? provider
  }

  const claudeTranscriptById: ExactIdFallback | undefined = claude && opts.locateClaudeTranscript
    ? async (id): Promise<ResumeResolveMatch | null> => {
        const hit = await opts.locateClaudeTranscript!(id)
        if (!hit) return null
        return {
          provider: 'claude',
          sessionId: hit.sessionId,
          // cwd may legitimately be missing here — the CLIENT must then ask
          // for a working directory instead of auto-opening.
          cwd: hit.cwd,
          sessionType: await sessionTypeFor('claude', hit.sessionId),
          matchKind: 'exact',
        }
      }
    : undefined

  const opencodeSessionById: ExactIdFallback | undefined = opencode?.getDatabasePath
    ? async (id): Promise<ResumeResolveMatch | null> => {
        // NO catch here: a locked/corrupt DB or worker failure must PROPAGATE
        // (provider unavailable ≠ not found). Runs OFF the event loop — the
        // worker runner blocks only its own thread.
        const row = await runById(opencode.getDatabasePath!(), id)
        if (!row) return null
        return {
          provider: 'opencode',
          sessionId: row.sessionId,
          // opencode resumes in the SPAWN cwd; the sqlite row's NOT NULL
          // `directory` column supplies it.
          cwd: row.cwd || undefined,
          sessionType: await sessionTypeFor('opencode', row.sessionId),
          title: row.title || undefined,
          // SQLite columns are dynamically typed (REAL possible); the contract
          // requires integer epoch-ms, so floor.
          lastActivityAt:
            typeof row.lastActivityAt === 'number' && Number.isFinite(row.lastActivityAt)
              ? Math.floor(row.lastActivityAt)
              : undefined,
          matchKind: 'exact',
        }
      }
    : undefined

  return { claudeTranscriptById, opencodeSessionById }
}
