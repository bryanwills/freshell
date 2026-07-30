/**
 * Extremely permissive resume-input parser (spec: docs/plans/2026-07-29-resume-button-spec.md).
 * Pure and dependency-free: imported by the client (live feedback) AND the Node
 * server (authoritative parse in POST /api/sessions/resolve) AND tests.
 */

export type ResumeProviderName = 'claude' | 'codex' | 'opencode' | 'amplifier'

export type ResumeAgentHint = {
  provider: ResumeProviderName
  /** command shape > bare agent word > id-format heuristic */
  source: 'command' | 'word' | 'id-format'
}

export type ResumeInputParse = {
  /**
   * Candidate session-id tokens, best-first, capped at MAX_RESUME_CANDIDATES.
   * UUID/hex tokens dedupe case-insensitively; ses_-style base62 ids are
   * case-SENSITIVE (distinct case = distinct id).
   */
  candidates: string[]
  /** Advisory only — store evidence always overrides this. */
  agentHint?: ResumeAgentHint
}

/**
 * Work budget: candidates are capped so one pasted blob can never trigger
 * unbounded server-side scans/DB lookups in the resolve endpoint.
 */
export const MAX_RESUME_CANDIDATES = 8

const ANSI_RE = /\u001b\[[0-9;?]*[ -/]*[@-~]/g
const UUID_RE = /[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}/g
/** Known xxx_-prefixed id families (ses_ + 26 base62 is opencode's, first-class). */
const PREFIXED_ID_RE = /\b(?:ses|sess|session|thread|thr|run|msg|task|amp)_[0-9A-Za-z]{8,64}\b/g
const HEX_RE = /[0-9a-fA-F]{8,32}/g

function isTokenBoundary(ch: string | undefined): boolean {
  // Ids may abut quotes, backticks, slashes, dashes (truncated UUIDs), etc.
  return ch === undefined || !/[0-9a-zA-Z_]/.test(ch)
}

function collectWithBoundaries(text: string, re: RegExp): string[] {
  const out: string[] = []
  for (const m of text.matchAll(re)) {
    const start = m.index ?? 0
    const end = start + m[0].length
    if (isTokenBoundary(text[start - 1]) && isTokenBoundary(text[end])) out.push(m[0])
  }
  return out
}

function blank(text: string, tokens: string[]): string {
  let result = text
  for (const t of tokens) result = result.split(t).join(' '.repeat(t.length))
  return result
}

/**
 * UUID/hex-family tokens (only hex digits and dashes) dedupe case-insensitively
 * (UUIDs/hex are case-preserving but case-equal). Anything else — notably
 * ses_ + base62 ids — dedupes case-SENSITIVELY: base62 upper/lower case are
 * distinct values, so two ids differing only in case are DIFFERENT sessions.
 */
function dedupeCandidates(tokens: string[]): string[] {
  const seen = new Set<string>()
  const out: string[] = []
  for (const t of tokens) {
    const key = /^[0-9a-fA-F-]+$/.test(t) ? t.toLowerCase() : t
    if (seen.has(key)) continue
    seen.add(key)
    out.push(t)
  }
  return out
}

function detectCommandHint(text: string): ResumeProviderName | undefined {
  const t = text.toLowerCase()
  if (/\bcodex\s+resume\b/.test(t)) return 'codex'
  if (/\bclaude\b[^\n]*(?:\s--resume\b|\s-r\b)/.test(t)) return 'claude'
  if (/\bamplifier\b[^\n]*\s--resume\b/.test(t)) return 'amplifier'
  if (/\bopencode\b[^\n]*\s--session\b/.test(t)) return 'opencode'
  return undefined
}

function detectWordHint(text: string): ResumeProviderName | undefined {
  const t = text.toLowerCase()
  const providers: ResumeProviderName[] = ['claude', 'codex', 'opencode', 'amplifier']
  let best: { provider: ResumeProviderName; index: number } | undefined
  for (const provider of providers) {
    const index = t.search(new RegExp(`\\b${provider}\\b`))
    if (index >= 0 && (best === undefined || index < best.index)) best = { provider, index }
  }
  return best?.provider
}

function detectIdFormatHint(candidates: string[]): ResumeProviderName | undefined {
  const top = candidates[0]
  if (!top) return undefined
  if (/^ses_/i.test(top)) return 'opencode'
  const groups = top.split('-')
  if (groups.length === 5) {
    const version = groups[2]?.[0]
    if (version === '7') return 'codex'
    if (version === '4') return 'claude'
    return undefined
  }
  if (/^[0-9a-fA-F]{8,32}$/.test(top)) return 'amplifier'
  return undefined
}

export function parseResumeInput(raw: string): ResumeInputParse {
  const text = raw.replace(ANSI_RE, ' ')

  const prefixedIds = collectWithBoundaries(text, PREFIXED_ID_RE)
  let remaining = blank(text, prefixedIds)

  const uuids = collectWithBoundaries(remaining, UUID_RE)
  remaining = blank(remaining, uuids)

  // Hex prefixes: ≥8 hex chars, ≥1 digit (rejects "decade"-style words), ≤32.
  const hexTokens = collectWithBoundaries(remaining, HEX_RE)
    .filter((t) => /\d/.test(t))
    .sort((a, b) => b.length - a.length)

  // Cap = work budget: bounds resolver scans + exact-id fallback lookups per request.
  const candidates = dedupeCandidates([...prefixedIds, ...uuids, ...hexTokens])
    .slice(0, MAX_RESUME_CANDIDATES)

  const command = detectCommandHint(text)
  if (command) return { candidates, agentHint: { provider: command, source: 'command' } }
  const word = detectWordHint(text)
  if (word) return { candidates, agentHint: { provider: word, source: 'word' } }
  const idFormat = detectIdFormatHint(candidates)
  if (idFormat) return { candidates, agentHint: { provider: idFormat, source: 'id-format' } }
  return { candidates }
}
