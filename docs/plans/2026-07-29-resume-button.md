# Resume Session Button Implementation Plan

> **For agentic workers:** This plan is executed task-by-task by the
> workflow's execute stage: a fresh implementer per task, with a spec +
> quality review after each task. Steps use checkbox (`- [ ]`) syntax
> for tracking.

**Goal:** An always-visible **Resume** button pinned below the Sidebar's session list that opens a dialog where the user pastes any text containing a session id; freshell resolves it across all four CLI providers (claude, codex, opencode, amplifier) via a new `POST /api/sessions/resolve` endpoint on the Node server and resumes the session in a tab with the correct agent.

**Architecture:** A pure shared parser (`shared/resume-input-parser.ts`) extracts candidate id tokens + advisory agent hints from arbitrary pasted text. A new server resolver (`server/coding-cli/session-resolver.ts`) scans the existing `CodingCliSessionIndexer` snapshot across ALL providers at once for exact + prefix matches, falling back to two exact-id locators (claude transcript file scan, opencode by-id sqlite query) on index misses. A new route in `server/sessions-router.ts` exposes this as `POST /api/sessions/resolve`. The client adds a `ResumeSessionDialog` component and a pinned footer in `Sidebar.tsx`; resuming reuses the existing `openSessionTab` thunk (which already dedupes against open panes).

**Tech Stack:** TypeScript (NodeNext ESM on the server — ALL relative server/shared imports need explicit `.js` extensions), Express + zod + supertest (server), React + Redux Toolkit + Testing Library (client), vitest via the repo's coordinated wrappers, `node:sqlite` for the opencode DB.

**Authoritative spec:** `docs/plans/2026-07-29-resume-button-spec.md` (committed alongside this plan — read it before each task; the spec wins over this plan on any conflict).

## Global Constraints

- Spec: providers are exactly `claude`, `codex`, `opencode`, `amplifier` (`DEFAULT_ENABLED_CLI_PROVIDERS` in `shared/coding-cli-defaults.ts`).
- Spec: the Resume button lives in a pinned footer that is a **sibling AFTER** the Sidebar's `flex flex-1 min-h-0` scroll wrapper — never inside the scrollable list — with a `data-testid`, visible at every scroll position and in `fullWidth` mobile mode.
- Spec: hex token rule — **≥8 hex chars containing at least one digit, up to 32 chars** ("decade"/"facade" must NOT match). Opencode ids: `ses_` + 26 base62 chars, first-class.
- Spec: **evidence decides, hints only assist the UI** — store scan results override the picker; picker/hints only pre-fill the picker and drive the "resume anyway" default.
- Spec: disambiguation list capped at **20**, most-recent first.
- Spec: index warming is NOT "not found" — distinct loading/retry state.
- Spec-sanctioned accepted limitation: **prefix matching only matches indexed sessions**; exact-id fallbacks cover index misses. Document this in code comments where implemented.
- Deployment reality (AGENTS.md "Rust Server (Self-Hosted Production)"): the canonical self-hosted production deployment is the **Rust server** on port 3002, which serves the same `dist/client` from disk but has **no** `/api/sessions/resolve` route. Per spec, this feature's API is implemented on the Node server only (the Rust `IndexExistenceProbe` is NOT this feature's API) — so the client MUST degrade gracefully when the endpoint is missing: a 404 from resolve shows an explicit "this server build does not support resume-by-id" error (never a generic failure, never a broken-looking button). Rust parity is a recorded follow-up (Task 1 verification log), not a silent scope cut.
- Id-family case rules: UUID/hex tokens compare and dedupe **case-insensitively**; `ses_` + base62 ids are **case-SENSITIVE** everywhere (base62 upper/lower case are distinct values — two ids differing only in case are different sessions).
- Server work budget: resolve requests are bounded — candidate tokens capped (`MAX_RESUME_CANDIDATES`), exact-id fallbacks gated on strict id shape and limited per request, and the opencode by-id busy timeout is short (500 ms). One request must never stall the event loop for a prolonged period.
- Repo: TDD red-green-refactor for every task; run tests via the coordinated wrapper `npm run test:vitest -- --config <config> <files> --run` (check `npm run test:status` before broad runs).
- Repo: NodeNext ESM — server/shared relative imports use `.js` extensions. Client imports use `@/` and `@shared/` aliases (no extension).
- Repo: a11y for all new UI — semantic elements, labels/aria, testable via role/label.
- Repo: `README.md` is the only end-user markdown doc; `docs/index.html` must be updated for major UI changes (this is one).
- Repo: conventional commits, focused and atomic, each with the Amplifier co-author footer (exact footer shown in every commit step).
- SAFETY (session-level, non-negotiable): **never touch ports 3001/3002 or any process you did not spawn** — the user's production server + live tabs run there. All server testing uses supertest (no listening port) or ephemeral ports with throwaway HOMEs/tmp dirs. Repeat this to any subagent.
- Do NOT create or open a PR. Prepare the branch, commit, and stop.

## Acceptance examples → covering tests

| Spec acceptance row | Covering test (task) |
|---|---|
| `417e8345` → amplifier prefix match | resolver Task 3; flow Task 9 |
| `codex resume 019fac27-…` → codex | parser Task 2; flow Task 9 |
| bare claude UUID → claude | resolver Task 3; flow Task 9 |
| `opencode --session ses_…` → opencode | parser Task 2; resolver Task 3 |
| bare `ses_…` with picker=claude → opencode + note | flow Task 9 |
| quoted `claude --resume …` with picker=codex → claude + note | flow Task 9 |
| prefix matching multiple → capped disambiguation list, most-recent first | resolver Task 3; dialog Task 7 |
| valid id, index warming → loading/retry, NOT "not found" | router Task 6; dialog Task 7 |
| garbage, no token → inline error, no tab | parser Task 2; flow Task 9 |
| session already open → focus existing pane, no duplicate | flow Task 9 |

---

### Task 1: Confirm the target backend AND record the Rust-deployment degradation contract (spec-mandated verification)

The spec requires the planner/implementer to confirm which server serves the sidebar in the default dev/start path before implementing there. Planner pre-verification: `package.json` `dev` runs `tsx watch server/index.ts` (+ vite client), and `start` runs `node dist/server/index.js` — the **Node server**. HOWEVER, per AGENTS.md ("Rust Server (Self-Hosted Production)") the canonical self-hosted PRODUCTION deployment is the **Rust server** (`scripts/launch-rust.sh`, port 3002), which serves the **same `dist/client`** from disk and has NO `/api/sessions/resolve` route. The spec scopes this feature's API to the Node server (the Rust `IndexExistenceProbe` is NOT this feature's API), so the client shipped in `dist/client` MUST degrade gracefully on a resolve 404 (Task 7 implements + tests this) and the parity gap MUST be recorded as an explicit follow-up — not silently scope-cut. This task re-verifies and records both facts.

**Files:**
- Modify: `docs/plans/2026-07-29-resume-button.md` (this file — append verification log)

**Interfaces:**
- Consumes: `package.json` scripts; AGENTS.md "Rust Server (Self-Hosted Production)" section; `crates/freshell-server` route surface.
- Produces: a committed verification record; all later server tasks target `server/index.ts` + `server/sessions-router.ts`; Task 7's 404-degradation behavior is the deployment-safety contract for Rust-served clients; Rust-server endpoint parity is a RECORDED follow-up.

- [ ] **Step 1: Verify the default dev/start scripts and the Rust deployment facts**

Run:
```bash
cd /home/dan/code/freshell/.worktrees/resume-button
node -e "const s=require('./package.json').scripts; console.log('dev:', s.dev); console.log('start:', s.start)"
grep -n "sessions/resolve" -r crates/freshell-server/src || echo "RUST-HAS-NO-RESOLVE-ROUTE (expected)"
grep -n "Rust Server (Self-Hosted Production)" AGENTS.md
```
Expected: `dev` contains `tsx watch server/index.ts`, `start` contains `node dist/server/index.js`, the Rust crate has NO resolve route (the `RUST-HAS-NO-RESOLVE-ROUTE` echo fires), and the AGENTS.md section exists. If the dev/start expectation does NOT hold, STOP — the spec's target-backend premise is wrong; surface this to review instead of proceeding. If the Rust crate DOES already have a resolve route, STOP and surface that too (the degradation contract may be unnecessary or conflicting).

- [ ] **Step 2: Record the verification in this plan**

Append to the END of this plan file (`docs/plans/2026-07-29-resume-button.md`):

```markdown

## Verification log

- Target backend confirmed (Task 1): `npm run dev` → `tsx watch server/index.ts`; `npm run start` → `node dist/server/index.js`. The Node server (`server/index.ts`) serves the sidebar in the default dev/start path; the feature's API is implemented there per spec.
- Deployment gap recorded (Task 1): the canonical self-hosted production is the RUST server (AGENTS.md, `scripts/launch-rust.sh`, port 3002) serving the same `dist/client`; it has no `/api/sessions/resolve`. The client degrades gracefully on resolve 404 (explicit "this server build does not support resume-by-id" message — implemented and tested in Task 7). FOLLOW-UP: Rust-server `/api/sessions/resolve` parity is required before the Resume button is fully functional on the canonical production deployment.
```

- [ ] **Step 3: Commit**

```bash
git add docs/plans/2026-07-29-resume-button.md
git commit -m "$(cat <<'EOF'
docs: record target-backend verification for resume button (Node server)

🤖 Generated with [Amplifier](https://github.com/microsoft/amplifier)

Co-Authored-By: Amplifier <240397093+microsoft-amplifier@users.noreply.github.com>
EOF
)"
```

---

### Task 2: Shared resume-input parser (pure function)

**Files:**
- Create: `shared/resume-input-parser.ts`
- Test: `test/unit/shared/resume-input-parser.test.ts`

**Interfaces:**
- Consumes: nothing (pure, dependency-free — it must stay importable from both client and server).
- Produces: `parseResumeInput(raw: string): ResumeInputParse` where `ResumeInputParse = { candidates: string[]; agentHint?: ResumeAgentHint }` and `ResumeAgentHint = { provider: 'claude' | 'codex' | 'opencode' | 'amplifier'; source: 'command' | 'word' | 'id-format' }`; also `MAX_RESUME_CANDIDATES = 8` (work-budget cap on candidates). Task 6 (server route) imports it as `'../shared/resume-input-parser.js'`; Task 7 (client) as `'@shared/resume-input-parser'`.

- [ ] **Step 1: Write the failing tests**

Create `test/unit/shared/resume-input-parser.test.ts`:

```typescript
import { describe, it, expect } from 'vitest'
import { parseResumeInput, MAX_RESUME_CANDIDATES } from '@shared/resume-input-parser'

describe('parseResumeInput — token extraction', () => {
  const cases: Array<{ name: string; input: string; expected: string[] }> = [
    {
      name: 'full UUID',
      input: 'ed2afda6-a340-443e-ba60-024a1b3554b4',
      expected: ['ed2afda6-a340-443e-ba60-024a1b3554b4'],
    },
    {
      name: 'opencode ses_ id (26 base62)',
      input: 'ses_root0000000000000000000000',
      expected: ['ses_root0000000000000000000000'],
    },
    {
      name: 'short hex prefix with a digit',
      input: '417e8345',
      expected: ['417e8345'],
    },
    {
      name: 'codex resume command line',
      input: 'codex resume 019fac27-69d7-78a0-b972-b339d551042e',
      expected: ['019fac27-69d7-78a0-b972-b339d551042e'],
    },
    {
      name: 'claude -r short flag',
      input: 'claude -r ed2afda6-a340-443e-ba60-024a1b3554b4',
      expected: ['ed2afda6-a340-443e-ba60-024a1b3554b4'],
    },
    {
      name: 'opencode --session command',
      input: 'opencode --session ses_root0000000000000000000000',
      expected: ['ses_root0000000000000000000000'],
    },
    {
      name: 'quoted + shell prompt + whitespace',
      input: '  $ "claude --resume ed2afda6-a340-443e-ba60-024a1b3554b4"  ',
      expected: ['ed2afda6-a340-443e-ba60-024a1b3554b4'],
    },
    {
      name: 'backticks and trailing punctuation',
      input: 'try `amplifier --resume 417e8345`, it works.',
      expected: ['417e8345'],
    },
    {
      name: 'truncated UUID pasted with trailing dash and ellipsis',
      input: '"claude --resume ed2afda6-…"',
      expected: ['ed2afda6'],
    },
    {
      name: 'id embedded in a path',
      input: '/home/u/.claude/projects/x/ed2afda6-a340-443e-ba60-024a1b3554b4.jsonl',
      expected: ['ed2afda6-a340-443e-ba60-024a1b3554b4'],
    },
    {
      name: 'multi-line paste with ANSI codes',
      input: '\u001b[32m> session started\u001b[0m\nresume with:\n  codex resume 019fac27-69d7-78a0-b972-b339d551042e\n',
      expected: ['019fac27-69d7-78a0-b972-b339d551042e'],
    },
    {
      name: 'multiple candidates: prefixed id and UUID before hex, hex longest-first',
      input: 'ses_root0000000000000000000000 ed2afda6-a340-443e-ba60-024a1b3554b4 417e8345 417e8345abcd',
      expected: [
        'ses_root0000000000000000000000',
        'ed2afda6-a340-443e-ba60-024a1b3554b4',
        '417e8345abcd',
        '417e8345',
      ],
    },
  ]

  for (const c of cases) {
    it(c.name, () => {
      expect(parseResumeInput(c.input).candidates).toEqual(c.expected)
    })
  }

  const rejects: Array<{ name: string; input: string }> = [
    { name: 'hex-looking English word without a digit', input: 'decade facade' },
    { name: 'all-letter hex ≥8 chars without a digit', input: 'deadbeefcafebabe'.replace(/[0-9]/g, 'a') },
    { name: 'short hex (<8 chars)', input: '417e83' },
    { name: 'snake_case identifier is not an id family', input: 'snake_casedword my_function_name' },
    { name: 'plain prose', input: 'please resume my last session thanks' },
    { name: 'empty string', input: '' },
    { name: 'flags only', input: 'claude --resume --verbose' },
  ]

  for (const c of rejects) {
    it(`rejects: ${c.name}`, () => {
      expect(parseResumeInput(c.input).candidates).toEqual([])
    })
  }

  it('dedupes hex/UUID tokens case-insensitively, keeping first casing', () => {
    expect(parseResumeInput('417E8345 417e8345').candidates).toEqual(['417E8345'])
  })

  it('keeps case-distinct ses_ ids separate (base62 is case-SENSITIVE)', () => {
    const a = 'ses_root0000000000000000000000'
    const b = 'ses_ROOT0000000000000000000000'
    expect(parseResumeInput(`${a} ${b}`).candidates).toEqual([a, b])
  })

  it('caps candidates at MAX_RESUME_CANDIDATES (server work budget)', () => {
    const tokens = Array.from({ length: MAX_RESUME_CANDIDATES + 4 }, (_, i) => `417e83450a${String(i).padStart(2, '0')}`)
    const { candidates } = parseResumeInput(tokens.join(' '))
    expect(candidates).toHaveLength(MAX_RESUME_CANDIDATES)
  })
})

describe('parseResumeInput — agent hints (advisory only)', () => {
  it('command shape beats id-format: codex resume <v7 uuid>', () => {
    const r = parseResumeInput('codex resume 019fac27-69d7-78a0-b972-b339d551042e')
    expect(r.agentHint).toEqual({ provider: 'codex', source: 'command' })
  })
  it('claude --resume command', () => {
    const r = parseResumeInput('claude --resume ed2afda6-a340-443e-ba60-024a1b3554b4')
    expect(r.agentHint).toEqual({ provider: 'claude', source: 'command' })
  })
  it('opencode --session command', () => {
    const r = parseResumeInput('opencode --session ses_root0000000000000000000000')
    expect(r.agentHint).toEqual({ provider: 'opencode', source: 'command' })
  })
  it('amplifier --resume command', () => {
    const r = parseResumeInput('amplifier --resume 417e8345')
    expect(r.agentHint).toEqual({ provider: 'amplifier', source: 'command' })
  })
  it('bare agent word', () => {
    const r = parseResumeInput('from my codex run: ed2afda6-a340-443e-ba60-024a1b3554b4')
    expect(r.agentHint).toEqual({ provider: 'codex', source: 'word' })
  })
  it('id-format: ses_ suggests opencode', () => {
    expect(parseResumeInput('ses_root0000000000000000000000').agentHint)
      .toEqual({ provider: 'opencode', source: 'id-format' })
  })
  it('id-format: UUIDv7 suggests codex', () => {
    expect(parseResumeInput('019fac27-69d7-78a0-b972-b339d551042e').agentHint)
      .toEqual({ provider: 'codex', source: 'id-format' })
  })
  it('id-format: UUIDv4 suggests claude', () => {
    expect(parseResumeInput('ed2afda6-a340-443e-ba60-024a1b3554b4').agentHint)
      .toEqual({ provider: 'claude', source: 'id-format' })
  })
  it('id-format: bare short hex suggests amplifier', () => {
    expect(parseResumeInput('417e8345').agentHint)
      .toEqual({ provider: 'amplifier', source: 'id-format' })
  })
  it('no token, no hint', () => {
    expect(parseResumeInput('nothing here').agentHint).toBeUndefined()
  })
})
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
npm run test:vitest -- --config config/vitest/vitest.config.ts test/unit/shared/resume-input-parser.test.ts --run
```
Expected: FAIL — cannot resolve `@shared/resume-input-parser` (module does not exist).

- [ ] **Step 3: Write the implementation**

Create `shared/resume-input-parser.ts`:

```typescript
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
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
npm run test:vitest -- --config config/vitest/vitest.config.ts test/unit/shared/resume-input-parser.test.ts --run
```
Expected: PASS (all cases). If a boundary case fails, fix the parser (not the test) — the table encodes the spec.

- [ ] **Step 5: Typecheck and commit**

```bash
npm run typecheck
git add shared/resume-input-parser.ts test/unit/shared/resume-input-parser.test.ts
git commit -m "$(cat <<'EOF'
feat(shared): permissive resume-input parser with advisory agent hints

🤖 Generated with [Amplifier](https://github.com/microsoft/amplifier)

Co-Authored-By: Amplifier <240397093+microsoft-amplifier@users.noreply.github.com>
EOF
)"
```

---

### Task 3: Indexer readiness getter + cross-provider session resolver

**Files:**
- Modify: `server/coding-cli/session-indexer.ts` (add one public method to `CodingCliSessionIndexer`, class starts ~line 442; `initialized` is an existing private field)
- Create: `server/coding-cli/session-resolver.ts`
- Test: `test/unit/server/coding-cli/session-resolver.test.ts`

**Interfaces:**
- Consumes: `ProjectGroup`, `CodingCliSession`, `CodingCliProviderName` from `server/coding-cli/types.js`; indexer snapshot shape `getProjects(): ProjectGroup[]` (already public — see `SessionsRouterDeps.codingCliIndexer`).
- Produces (used by Tasks 4–6):
  - `CodingCliSessionIndexer.isReady(): boolean`
  - `resolveSessionCandidates(candidates: string[], deps: SessionResolverDeps): Promise<{ matches: ResolveMatch[]; providerErrors: CodingCliProviderName[] }>` — `providerErrors` lists providers whose exact-id fallback THREW (provider unavailable ≠ not found)
  - `type ResolveMatch = { provider: CodingCliProviderName; sessionId: string; cwd?: string; projectPath: string; sessionType: string; title?: string; firstUserMessage?: string; lastActivityAt: number; matchType: 'exact' | 'prefix'; matchedToken: string }`
  - `type ExactIdFallback = (id: string) => Promise<ResolveMatch | null>`
  - `type SessionResolverDeps = { getProjects: () => ProjectGroup[]; fallbacks?: { claudeTranscriptById?: ExactIdFallback; opencodeSessionById?: ExactIdFallback } }`
  - `const RESOLVE_MATCH_CAP = 20`

- [ ] **Step 1: Write the failing tests**

Create `test/unit/server/coding-cli/session-resolver.test.ts`:

```typescript
// @vitest-environment node
import { describe, it, expect } from 'vitest'
import {
  resolveSessionCandidates,
  RESOLVE_MATCH_CAP,
  type ResolveMatch,
} from '../../../../server/coding-cli/session-resolver'
import type { ProjectGroup, CodingCliSession } from '../../../../server/coding-cli/types'

function session(overrides: Partial<CodingCliSession> & Pick<CodingCliSession, 'provider' | 'sessionId'>): CodingCliSession {
  return {
    projectPath: '/home/u/proj',
    lastActivityAt: 1000,
    cwd: '/home/u/proj',
    title: 'a session',
    ...overrides,
  }
}

function projects(sessions: CodingCliSession[]): ProjectGroup[] {
  return [{ projectPath: '/home/u/proj', sessions }]
}

const AMPLIFIER_FULL = '417e8345-90ab-4cde-8f01-234567890abc'
const CODEX_V7 = '019fac27-69d7-78a0-b972-b339d551042e'
const CLAUDE_V4 = 'ed2afda6-a340-443e-ba60-024a1b3554b4'
const OPENCODE_ID = 'ses_root0000000000000000000000'

const fourProviderSnapshot = projects([
  session({ provider: 'claude', sessionId: CLAUDE_V4, sessionType: 'claude' }),
  session({ provider: 'codex', sessionId: CODEX_V7, sessionType: 'codex' }),
  session({ provider: 'opencode', sessionId: OPENCODE_ID, sessionType: 'opencode' }),
  session({ provider: 'amplifier', sessionId: AMPLIFIER_FULL, sessionType: 'amplifier' }),
])

describe('resolveSessionCandidates', () => {
  it('exact match wins across all providers at once (claude UUID, no hint needed)', async () => {
    const { matches } = await resolveSessionCandidates([CLAUDE_V4], { getProjects: () => fourProviderSnapshot })
    expect(matches).toHaveLength(1)
    expect(matches[0]).toMatchObject({
      provider: 'claude',
      sessionId: CLAUDE_V4,
      sessionType: 'claude',
      cwd: '/home/u/proj',
      matchType: 'exact',
      matchedToken: CLAUDE_V4,
    })
  })

  it('short hex prefix matches the amplifier session (spec row: 417e8345)', async () => {
    const { matches } = await resolveSessionCandidates(['417e8345'], { getProjects: () => fourProviderSnapshot })
    expect(matches).toHaveLength(1)
    expect(matches[0]).toMatchObject({ provider: 'amplifier', sessionId: AMPLIFIER_FULL, matchType: 'prefix' })
  })

  it('exact-id match is case-insensitive for UUID/hex tokens', async () => {
    const { matches } = await resolveSessionCandidates([CLAUDE_V4.toUpperCase()], { getProjects: () => fourProviderSnapshot })
    expect(matches).toHaveLength(1)
    expect(matches[0].sessionId).toBe(CLAUDE_V4)
  })

  it('ses_ ids are case-SENSITIVE (base62): a case-variant does NOT match', async () => {
    const { matches } = await resolveSessionCandidates(['ses_ROOT0000000000000000000000'], { getProjects: () => fourProviderSnapshot })
    expect(matches).toHaveLength(0)
  })

  it('opencode ses_ id resolves to opencode even though other providers exist', async () => {
    const { matches } = await resolveSessionCandidates([OPENCODE_ID], { getProjects: () => fourProviderSnapshot })
    expect(matches).toHaveLength(1)
    expect(matches[0].provider).toBe('opencode')
  })

  it('exact match takes precedence over prefix matches of the same token', async () => {
    const snapshot = projects([
      session({ provider: 'amplifier', sessionId: '417e8345', lastActivityAt: 1 }),
      session({ provider: 'amplifier', sessionId: '417e8345-90ab-4cde-8f01-234567890abc', lastActivityAt: 2 }),
    ])
    const { matches } = await resolveSessionCandidates(['417e8345'], { getProjects: () => snapshot })
    expect(matches).toHaveLength(1)
    expect(matches[0].matchType).toBe('exact')
  })

  it('ambiguous prefix returns all matches most-recent first, capped', async () => {
    const many = Array.from({ length: RESOLVE_MATCH_CAP + 5 }, (_, i) =>
      session({
        provider: 'amplifier',
        sessionId: `417e8345-90ab-4cde-8f01-${String(i).padStart(12, '0')}`,
        lastActivityAt: i,
      }))
    const { matches } = await resolveSessionCandidates(['417e8345'], { getProjects: () => projects(many) })
    expect(matches).toHaveLength(RESOLVE_MATCH_CAP)
    expect(matches[0].lastActivityAt).toBe(RESOLVE_MATCH_CAP + 4)
    expect(matches[matches.length - 1].lastActivityAt).toBeGreaterThanOrEqual(5)
  })

  it('tries candidates in order until one resolves', async () => {
    const { matches } = await resolveSessionCandidates(['deadbeef1234', CLAUDE_V4], { getProjects: () => fourProviderSnapshot })
    expect(matches).toHaveLength(1)
    expect(matches[0].sessionId).toBe(CLAUDE_V4)
  })

  it('subagent sessions are excluded', async () => {
    const snapshot = projects([
      session({ provider: 'claude', sessionId: CLAUDE_V4, isSubagent: true }),
    ])
    const { matches } = await resolveSessionCandidates([CLAUDE_V4], { getProjects: () => snapshot })
    expect(matches).toHaveLength(0)
  })

  it('sessionType defaults to the provider name when the index has none', async () => {
    const snapshot = projects([session({ provider: 'codex', sessionId: CODEX_V7 })])
    const { matches } = await resolveSessionCandidates([CODEX_V7], { getProjects: () => snapshot })
    expect(matches[0].sessionType).toBe('codex')
  })

  it('index miss consults exact-id fallbacks (claude transcript locator)', async () => {
    const fallbackMatch: ResolveMatch = {
      provider: 'claude', sessionId: CLAUDE_V4, cwd: '/tmp/found', projectPath: '/tmp/found',
      sessionType: 'claude', lastActivityAt: 42, matchType: 'exact', matchedToken: CLAUDE_V4,
    }
    const { matches } = await resolveSessionCandidates([CLAUDE_V4], {
      getProjects: () => projects([]),
      fallbacks: { claudeTranscriptById: async (id) => (id === CLAUDE_V4 ? fallbackMatch : null) },
    })
    expect(matches).toEqual([fallbackMatch])
  })

  it('index miss consults opencode by-id fallback', async () => {
    const fallbackMatch: ResolveMatch = {
      provider: 'opencode', sessionId: OPENCODE_ID, cwd: '/tmp/oc', projectPath: '/tmp/oc',
      sessionType: 'opencode', lastActivityAt: 7, matchType: 'exact', matchedToken: OPENCODE_ID,
    }
    const { matches } = await resolveSessionCandidates([OPENCODE_ID], {
      getProjects: () => projects([]),
      fallbacks: { opencodeSessionById: async () => fallbackMatch },
    })
    expect(matches).toEqual([fallbackMatch])
  })

  it('zero matches when nothing resolves anywhere', async () => {
    const { matches, providerErrors } = await resolveSessionCandidates(['deadbeef1234'], {
      getProjects: () => fourProviderSnapshot,
      fallbacks: { claudeTranscriptById: async () => null, opencodeSessionById: async () => null },
    })
    expect(matches).toEqual([])
    expect(providerErrors).toEqual([])
  })

  it('a THROWING fallback is reported as a provider error, never as "not found"', async () => {
    const { matches, providerErrors } = await resolveSessionCandidates([OPENCODE_ID], {
      getProjects: () => projects([]),
      fallbacks: { opencodeSessionById: async () => { throw new Error('database is locked') } },
    })
    expect(matches).toEqual([])
    expect(providerErrors).toEqual(['opencode'])
  })
})
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
npm run test:vitest -- --config config/vitest/vitest.server.config.ts test/unit/server/coding-cli/session-resolver.test.ts --run
```
Expected: FAIL — `session-resolver` module not found.

- [ ] **Step 3: Implement the resolver**

Create `server/coding-cli/session-resolver.ts`:

```typescript
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
 * providerErrors (unavailable ≠ not found).
 */
export async function resolveSessionCandidates(
  candidates: string[],
  deps: SessionResolverDeps,
): Promise<{ matches: ResolveMatch[]; providerErrors: CodingCliProviderName[] }> {
  const sessions = deps.getProjects()
    .flatMap((p) => p.sessions)
    .filter((s) => !s.isSubagent)

  const providerErrors = new Set<CodingCliProviderName>()
  const done = (matches: ResolveMatch[]) => ({ matches, providerErrors: [...providerErrors] })

  for (const token of candidates) {
    const ci = isCaseInsensitiveToken(token)
    const norm = (value: string) => (ci ? value.toLowerCase() : value)
    const target = norm(token)

    const exact = sessions.filter((s) => norm(s.sessionId) === target)
    if (exact.length > 0) return done(rank(exact.map((s) => toMatch(s, 'exact', token))))

    const prefix = sessions.filter((s) => norm(s.sessionId).startsWith(target))
    if (prefix.length > 0) return done(rank(prefix.map((s) => toMatch(s, 'prefix', token))))

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
  }

  return done([])
}
```

- [ ] **Step 4: Add `isReady()` to the indexer**

In `server/coding-cli/session-indexer.ts`, inside `class CodingCliSessionIndexer`, directly ABOVE the existing `onUpdate(...)` method (~line 687), add:

```typescript
  /** True once the initial full scan has completed (resolve endpoint: 'warming' until then). */
  isReady(): boolean {
    return this.initialized
  }
```

- [ ] **Step 5: Run tests to verify they pass**

```bash
npm run test:vitest -- --config config/vitest/vitest.server.config.ts test/unit/server/coding-cli/session-resolver.test.ts --run
npm run typecheck:server
```
Expected: PASS, clean typecheck.

- [ ] **Step 6: Commit**

```bash
git add server/coding-cli/session-resolver.ts server/coding-cli/session-indexer.ts test/unit/server/coding-cli/session-resolver.test.ts
git commit -m "$(cat <<'EOF'
feat(server): cross-provider session resolver over the index snapshot

🤖 Generated with [Amplifier](https://github.com/microsoft/amplifier)

Co-Authored-By: Amplifier <240397093+microsoft-amplifier@users.noreply.github.com>
EOF
)"
```

---

### Task 4: Claude transcript-by-id locator (exact-id fallback)

**Files:**
- Create: `server/coding-cli/claude-transcript-locator.ts`
- Test: `test/unit/server/coding-cli/claude-transcript-locator.test.ts`

**Interfaces:**
- Consumes: `fs/promises`, `path`; roots come from the claude provider's `getSessionRoots(): string[]` (each root contains per-project directories holding `<sessionId>.jsonl` transcripts whose lines are JSON objects with a `cwd` field).
- Produces: `locateClaudeTranscriptById(sessionId: string, roots: string[]): Promise<ClaudeTranscriptHit | null>` with `ClaudeTranscriptHit = { sessionId: string; cwd?: string; filePath: string; lastActivityAt: number }`. Task 6 wraps this into an `ExactIdFallback`.

- [ ] **Step 1: Write the failing tests**

Create `test/unit/server/coding-cli/claude-transcript-locator.test.ts`:

```typescript
// @vitest-environment node
import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import fsp from 'fs/promises'
import os from 'os'
import path from 'path'
import { locateClaudeTranscriptById } from '../../../../server/coding-cli/claude-transcript-locator'

const SESSION_ID = 'ed2afda6-a340-443e-ba60-024a1b3554b4'

let root: string

beforeEach(async () => {
  // Throwaway fixture dir — never the real HOME (session safety rule).
  root = await fsp.mkdtemp(path.join(os.tmpdir(), 'claude-locator-'))
})

afterEach(async () => {
  await fsp.rm(root, { recursive: true, force: true })
})

async function writeTranscript(projectDir: string, id: string, lines: string[]) {
  const dir = path.join(root, projectDir)
  await fsp.mkdir(dir, { recursive: true })
  await fsp.writeFile(path.join(dir, `${id}.jsonl`), lines.join('\n') + '\n')
}

describe('locateClaudeTranscriptById', () => {
  it('finds a transcript by exact id and extracts cwd', async () => {
    await writeTranscript('-home-u-proj', SESSION_ID, [
      JSON.stringify({ type: 'summary', summary: 'hi' }),
      JSON.stringify({ type: 'user', cwd: '/home/u/proj', message: { content: 'hello' } }),
    ])
    const hit = await locateClaudeTranscriptById(SESSION_ID, [root])
    expect(hit).not.toBeNull()
    expect(hit!.sessionId).toBe(SESSION_ID)
    expect(hit!.cwd).toBe('/home/u/proj')
    expect(hit!.filePath).toBe(path.join(root, '-home-u-proj', `${SESSION_ID}.jsonl`))
    expect(hit!.lastActivityAt).toBeGreaterThan(0)
  })

  it('finds a lowercase transcript when the pasted id is UPPERCASE (case-sensitive FS)', async () => {
    await writeTranscript('-home-u-proj', SESSION_ID, [
      JSON.stringify({ type: 'user', cwd: '/home/u/proj', message: { content: 'hello' } }),
    ])
    const hit = await locateClaudeTranscriptById(SESSION_ID.toUpperCase(), [root])
    expect(hit).not.toBeNull()
    expect(hit!.sessionId).toBe(SESSION_ID)
    expect(hit!.filePath).toBe(path.join(root, '-home-u-proj', `${SESSION_ID}.jsonl`))
  })

  it('returns null when no transcript exists', async () => {
    expect(await locateClaudeTranscriptById(SESSION_ID, [root])).toBeNull()
  })

  it('returns null for non-UUID input without touching the filesystem', async () => {
    expect(await locateClaudeTranscriptById('417e8345', ['/does/not/exist'])).toBeNull()
  })

  it('tolerates missing roots', async () => {
    expect(await locateClaudeTranscriptById(SESSION_ID, [path.join(root, 'nope')])).toBeNull()
  })

  it('still returns the hit when no line has a cwd', async () => {
    await writeTranscript('-x', SESSION_ID, [JSON.stringify({ type: 'summary' })])
    const hit = await locateClaudeTranscriptById(SESSION_ID, [root])
    expect(hit).not.toBeNull()
    expect(hit!.cwd).toBeUndefined()
  })
})
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
npm run test:vitest -- --config config/vitest/vitest.server.config.ts test/unit/server/coding-cli/claude-transcript-locator.test.ts --run
```
Expected: FAIL — module not found.

- [ ] **Step 3: Implement the locator**

Create `server/coding-cli/claude-transcript-locator.ts`:

```typescript
import fsp from 'fs/promises'
import path from 'path'

export type ClaudeTranscriptHit = {
  sessionId: string
  cwd?: string
  filePath: string
  lastActivityAt: number
}

const UUID_SHAPE = /^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$/
const CWD_SCAN_BYTES = 64 * 1024

/**
 * Exact-id fallback for claude sessions the index missed: claude stores one
 * transcript per session at <root>/<project-dir>/<sessionId>.jsonl, so an
 * exact id can be located with one readdir per root + one stat per project dir
 * (no full scan, no glob).
 */
export async function locateClaudeTranscriptById(
  sessionId: string,
  roots: string[],
): Promise<ClaudeTranscriptHit | null> {
  if (!UUID_SHAPE.test(sessionId)) return null
  // Claude writes lowercase-UUID transcript filenames; the input contract
  // accepts UUIDs in ANY case and Linux filesystems are case-sensitive, so
  // normalize before building paths and return the canonical lowercase id.
  const id = sessionId.toLowerCase()
  for (const root of roots) {
    let projectDirs: string[]
    try {
      projectDirs = await fsp.readdir(root)
    } catch {
      continue
    }
    for (const dir of projectDirs) {
      const candidate = path.join(root, dir, `${id}.jsonl`)
      let stat
      try {
        stat = await fsp.stat(candidate)
      } catch {
        continue
      }
      const cwd = await readCwdFromTranscriptHead(candidate)
      return { sessionId: id, cwd, filePath: candidate, lastActivityAt: stat.mtimeMs }
    }
  }
  return null
}

async function readCwdFromTranscriptHead(filePath: string): Promise<string | undefined> {
  let handle
  try {
    handle = await fsp.open(filePath, 'r')
  } catch {
    return undefined
  }
  try {
    const buf = Buffer.alloc(CWD_SCAN_BYTES)
    const { bytesRead } = await handle.read(buf, 0, buf.length, 0)
    for (const line of buf.toString('utf8', 0, bytesRead).split('\n')) {
      const trimmed = line.trim()
      if (!trimmed) continue
      try {
        const obj = JSON.parse(trimmed) as { cwd?: unknown }
        if (typeof obj.cwd === 'string' && obj.cwd) return obj.cwd
      } catch {
        // Truncated tail line of the 64KB window — ignore.
      }
    }
    return undefined
  } finally {
    await handle.close()
  }
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
npm run test:vitest -- --config config/vitest/vitest.server.config.ts test/unit/server/coding-cli/claude-transcript-locator.test.ts --run
```
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add server/coding-cli/claude-transcript-locator.ts test/unit/server/coding-cli/claude-transcript-locator.test.ts
git commit -m "$(cat <<'EOF'
feat(server): claude transcript-by-id locator for resolve fallback

🤖 Generated with [Amplifier](https://github.com/microsoft/amplifier)

Co-Authored-By: Amplifier <240397093+microsoft-amplifier@users.noreply.github.com>
EOF
)"
```

---

### Task 5: Opencode session-by-id DB query (exact-id fallback)

This is the Node-server sibling of the just-landed Rust opencode by-id existence work (#579, commits `3e6584b2`/`eb9b6011`): exact-id lookups must hit the opencode sqlite DB directly so child sessions and unindexed sessions resolve. Mirror the column mapping of `runOpencodeListingQuery` in `server/coding-cli/providers/opencode-listing-query.ts` — but WITHOUT the `time_archived IS NULL` and root-session filters (an exact id must find archived and child sessions too).

**Files:**
- Create: `server/coding-cli/providers/opencode-by-id-query.ts`
- Test: `test/unit/server/coding-cli/opencode-by-id-query.test.ts`

**Interfaces:**
- Consumes: `node:sqlite` (lazy `await import` — same pattern and reason as `opencode-listing-query.ts`); `OpencodeSessionRow` type from `./opencode-listing-query.js`.
- Produces: `runOpencodeSessionByIdQuery(dbPath: string, sessionId: string): Promise<OpencodeSessionRow | null>`. Task 6 wraps it into an `ExactIdFallback` using `OpencodeProvider.getDatabasePath()` (`<opencode-data>/opencode.db`).

- [ ] **Step 1: Write the failing tests**

Create `test/unit/server/coding-cli/opencode-by-id-query.test.ts`:

```typescript
// @vitest-environment node
import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import fsp from 'fs/promises'
import os from 'os'
import path from 'path'
import { runOpencodeSessionByIdQuery } from '../../../../server/coding-cli/providers/opencode-by-id-query'

const SES_ROOT = 'ses_root0000000000000000000000'
const SES_CHILD = 'ses_child000000000000000000000'

let dir: string
let dbPath: string

beforeEach(async () => {
  // Throwaway tmp DB — never the user's real opencode data dir (session safety rule).
  dir = await fsp.mkdtemp(path.join(os.tmpdir(), 'opencode-byid-'))
  dbPath = path.join(dir, 'opencode.db')
  const { DatabaseSync } = await import('node:sqlite')
  const db = new DatabaseSync(dbPath)
  db.exec(`
    CREATE TABLE project (id TEXT PRIMARY KEY, worktree TEXT);
    CREATE TABLE session (
      id TEXT PRIMARY KEY,
      project_id TEXT,
      parent_id TEXT,
      directory TEXT,
      title TEXT,
      time_created INTEGER,
      time_updated INTEGER,
      time_archived INTEGER
    );
    INSERT INTO project (id, worktree) VALUES ('p1', '/home/u/oc-proj');
    INSERT INTO session (id, project_id, parent_id, directory, title, time_created, time_updated, time_archived)
      VALUES ('${SES_ROOT}', 'p1', NULL, '/home/u/oc-proj', 'root session', 100, 200, NULL);
    INSERT INTO session (id, project_id, parent_id, directory, title, time_created, time_updated, time_archived)
      VALUES ('${SES_CHILD}', 'p1', '${SES_ROOT}', '/home/u/oc-proj', 'child session', 110, 210, 999);
  `)
  db.close()
})

afterEach(async () => {
  await fsp.rm(dir, { recursive: true, force: true })
})

describe('runOpencodeSessionByIdQuery', () => {
  it('finds a root session by exact id with metadata', async () => {
    const row = await runOpencodeSessionByIdQuery(dbPath, SES_ROOT)
    expect(row).toMatchObject({
      sessionId: SES_ROOT,
      cwd: '/home/u/oc-proj',
      title: 'root session',
      lastActivityAt: 200,
      projectPath: '/home/u/oc-proj',
    })
  })

  it('finds CHILD and ARCHIVED sessions too (unlike the listing query)', async () => {
    const row = await runOpencodeSessionByIdQuery(dbPath, SES_CHILD)
    expect(row?.sessionId).toBe(SES_CHILD)
  })

  it('returns null for an unknown id', async () => {
    expect(await runOpencodeSessionByIdQuery(dbPath, 'ses_missing0000000000000000000')).toBeNull()
  })

  it('works when the project table is absent (degraded schema)', async () => {
    const bare = path.join(dir, 'bare.db')
    const { DatabaseSync } = await import('node:sqlite')
    const db = new DatabaseSync(bare)
    db.exec(`
      CREATE TABLE session (id TEXT PRIMARY KEY, directory TEXT, title TEXT, time_created INTEGER, time_updated INTEGER);
      INSERT INTO session VALUES ('${SES_ROOT}', '/d', 't', 1, 2);
    `)
    db.close()
    const row = await runOpencodeSessionByIdQuery(bare, SES_ROOT)
    expect(row?.sessionId).toBe(SES_ROOT)
    expect(row?.projectPath ?? null).toBeNull()
  })
})
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
npm run test:vitest -- --config config/vitest/vitest.server.config.ts test/unit/server/coding-cli/opencode-by-id-query.test.ts --run
```
Expected: FAIL — module not found.

- [ ] **Step 3: Implement the query**

Create `server/coding-cli/providers/opencode-by-id-query.ts`:

```typescript
import type { OpencodeSessionRow } from './opencode-listing-query.js'

/**
 * SHORT busy timeout, deliberately much smaller than the listing query's 5 s:
 * this synchronous lookup runs on the Node event loop, so a locked DB must
 * fail fast (the failure is surfaced as provider-unavailable, NOT "not found").
 * Event-loop-blocking budget rationale: unlike the ~180 ms listing scan (which
 * runs in a worker thread for that reason), this is an indexed primary-key
 * point lookup (`WHERE id = ? LIMIT 1`) — sub-millisecond even on a 531 MB DB.
 * Per-request work is additionally bounded by MAX_RESUME_CANDIDATES, the
 * strict ses_ shape gate, and the per-request fallback budget (Task 6), so the
 * worst case is a handful of point lookups + at most this timeout when locked.
 */
const OPENCODE_BYID_BUSY_TIMEOUT_MS = 500

/**
 * Exact-id opencode lookup for the resolve endpoint's fallback path — the
 * Node-server sibling of the Rust by-id existence probe (#579). Unlike the
 * listing query it deliberately includes ARCHIVED and CHILD sessions: an
 * exact id pasted by the user must resolve even when the listing hides it.
 * Lazy `node:sqlite` import for the same vi.mock/TDZ reason documented in
 * opencode-listing-query.ts. Errors PROPAGATE to the caller (provider
 * unavailable ≠ not found — the resolver records them as providerErrors).
 */
export async function runOpencodeSessionByIdQuery(
  dbPath: string,
  sessionId: string,
): Promise<OpencodeSessionRow | null> {
  const { DatabaseSync } = await import('node:sqlite')
  const db = new DatabaseSync(dbPath, { readOnly: true })
  try {
    db.exec(`PRAGMA busy_timeout = ${OPENCODE_BYID_BUSY_TIMEOUT_MS}`)
    const tableNames = new Set(
      (db.prepare("SELECT name FROM sqlite_master WHERE type = 'table'").all() as Array<{ name?: unknown }>)
        .map((row) => row.name),
    )
    if (!tableNames.has('session')) return null
    const hasProject = tableNames.has('project')
    const projectSelect = hasProject ? 'p.worktree' : 'NULL'
    const projectJoin = hasProject ? 'LEFT JOIN project p ON p.id = s.project_id' : ''
    const row = db.prepare(`
      SELECT
        s.id AS sessionId,
        s.directory AS cwd,
        s.title AS title,
        s.time_created AS createdAt,
        s.time_updated AS lastActivityAt,
        ${projectSelect} AS projectPath
      FROM session s
      ${projectJoin}
      WHERE s.id = ?
      LIMIT 1
    `).get(sessionId) as OpencodeSessionRow | undefined
    return row ?? null
  } finally {
    db.close()
  }
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
npm run test:vitest -- --config config/vitest/vitest.server.config.ts test/unit/server/coding-cli/opencode-by-id-query.test.ts --run
npm run typecheck:server
```
Expected: PASS, clean typecheck.

- [ ] **Step 5: Commit**

```bash
git add server/coding-cli/providers/opencode-by-id-query.ts test/unit/server/coding-cli/opencode-by-id-query.test.ts
git commit -m "$(cat <<'EOF'
feat(server): opencode session-by-id DB query for resolve fallback

🤖 Generated with [Amplifier](https://github.com/microsoft/amplifier)

Co-Authored-By: Amplifier <240397093+microsoft-amplifier@users.noreply.github.com>
EOF
)"
```

---

### Task 6: `POST /api/sessions/resolve` endpoint

**Files:**
- Create: `server/coding-cli/resolve-fallbacks.ts`
- Modify: `server/sessions-router.ts` (extend `SessionsRouterDeps` ~line 39; add route inside `createSessionsRouter` after the existing `router.post('/session-metadata', …)` handler ~line 239)
- Modify: `server/coding-cli/providers/opencode.ts` (one-word visibility change: `getDatabasePath()` is declared `private` at ~line 78 — remove the `private` keyword so `buildResolveFallbacks` can call it; verified during load-bearing validation)
- Test: `test/unit/server/sessions-resolve-router.test.ts`

**Interfaces:**
- Consumes: `parseResumeInput` from `../shared/resume-input-parser.js` (Task 2); `resolveSessionCandidates`, `ResolveMatch`, `ExactIdFallback` from `./coding-cli/session-resolver.js` (Task 3); `locateClaudeTranscriptById` (Task 4); `runOpencodeSessionByIdQuery` (Task 5); `CodingCliProvider` interface (`getSessionRoots()`, `name`); `OpencodeProvider.getDatabasePath()`.
- Produces: HTTP contract used by the client (Task 7):
  - Request: `POST /api/sessions/resolve` body `{ input: string }` (1–20000 chars; 400 on invalid body; candidate tokens are already capped by `MAX_RESUME_CANDIDATES` in the parser).
  - Response 200: `{ indexState: 'ready' | 'warming' | 'degraded', tokens: string[], agentHint: { provider, source } | null, homeDir: string, providerErrors: ('claude' | 'opencode')[], matches: ResolveMatch[] }`. `degraded` = zero matches AND at least one provider fallback FAILED (provider unavailable ≠ not found — the client shows retry, never "no matching session").
- Wiring note: `server/index.ts` (~line 748) passes the live `CodingCliSessionIndexer` instance as `codingCliIndexer`, so the new optional `isReady` dep is picked up structurally — verify this when editing; if `index.ts` instead builds an object literal for `codingCliIndexer`, add `isReady: () => codingCliIndexer.isReady()` to that literal.

- [ ] **Step 1: Write the failing tests**

Create `test/unit/server/sessions-resolve-router.test.ts`:

```typescript
// @vitest-environment node
import { describe, it, expect, vi } from 'vitest'
import express from 'express'
import request from 'supertest'
import { createSessionsRouter, type SessionsRouterDeps } from '../../../server/sessions-router'
import { FALLBACK_BUDGET_PER_REQUEST } from '../../../server/coding-cli/resolve-fallbacks'
import type { ProjectGroup } from '../../../server/coding-cli/types'

const CLAUDE_V4 = 'ed2afda6-a340-443e-ba60-024a1b3554b4'

function snapshot(): ProjectGroup[] {
  return [{
    projectPath: '/home/u/proj',
    sessions: [
      {
        provider: 'claude', sessionId: CLAUDE_V4, projectPath: '/home/u/proj',
        lastActivityAt: 111, cwd: '/home/u/proj', title: 'claude one', sessionType: 'claude',
      },
      {
        provider: 'amplifier', sessionId: '417e8345-90ab-4cde-8f01-234567890abc', projectPath: '/home/u/proj',
        lastActivityAt: 222, cwd: '/home/u/proj', title: 'amp one', sessionType: 'amplifier',
      },
    ],
  }]
}

function makeApp(overrides: Partial<SessionsRouterDeps> = {}) {
  const deps: SessionsRouterDeps = {
    configStore: {
      getSettings: async () => ({}),
      patchSessionOverride: async () => ({}),
      deleteSession: async () => {},
    },
    codingCliIndexer: {
      getProjects: () => snapshot(),
      refresh: async () => {},
      isReady: () => true,
    },
    codingCliProviders: [],
    perfConfig: { slowSessionRefreshMs: 1000 },
    homeDir: '/home/testuser',
    resolveFallbacks: {},
    ...overrides,
  }
  const app = express()
  app.use(express.json())
  app.use('/api', createSessionsRouter(deps))
  return app
}

describe('POST /api/sessions/resolve', () => {
  it('resolves an exact claude UUID with full resume metadata', async () => {
    const res = await request(makeApp()).post('/api/sessions/resolve').send({ input: CLAUDE_V4 })
    expect(res.status).toBe(200)
    expect(res.body.indexState).toBe('ready')
    expect(res.body.tokens).toEqual([CLAUDE_V4])
    expect(res.body.homeDir).toBe('/home/testuser')
    expect(res.body.matches).toHaveLength(1)
    expect(res.body.matches[0]).toMatchObject({
      provider: 'claude', sessionId: CLAUDE_V4, cwd: '/home/u/proj',
      sessionType: 'claude', matchType: 'exact',
    })
  })

  it('resolves a short hex prefix to the amplifier session', async () => {
    const res = await request(makeApp()).post('/api/sessions/resolve').send({ input: '417e8345' })
    expect(res.body.matches).toHaveLength(1)
    expect(res.body.matches[0].provider).toBe('amplifier')
  })

  it('carries the advisory agent hint', async () => {
    const res = await request(makeApp()).post('/api/sessions/resolve')
      .send({ input: `codex resume ${CLAUDE_V4}` })
    expect(res.body.agentHint).toEqual({ provider: 'codex', source: 'command' })
    // Evidence still wins: the store found it under claude.
    expect(res.body.matches[0].provider).toBe('claude')
  })

  it('reports warming when the index is not ready (NOT "not found")', async () => {
    const res = await request(makeApp({
      codingCliIndexer: { getProjects: () => [], refresh: async () => {}, isReady: () => false },
    })).post('/api/sessions/resolve').send({ input: CLAUDE_V4 })
    expect(res.status).toBe(200)
    expect(res.body.indexState).toBe('warming')
    expect(res.body.matches).toEqual([])
  })

  it('returns empty tokens for garbage input', async () => {
    const res = await request(makeApp()).post('/api/sessions/resolve').send({ input: 'no ids here' })
    expect(res.status).toBe(200)
    expect(res.body.tokens).toEqual([])
    expect(res.body.matches).toEqual([])
  })

  it('uses injected exact-id fallbacks on index miss', async () => {
    const fallback = vi.fn().mockResolvedValue({
      provider: 'claude', sessionId: CLAUDE_V4, cwd: '/tmp/fb', projectPath: '/tmp/fb',
      sessionType: 'claude', lastActivityAt: 5, matchType: 'exact', matchedToken: CLAUDE_V4,
    })
    const res = await request(makeApp({
      codingCliIndexer: { getProjects: () => [], refresh: async () => {}, isReady: () => true },
      resolveFallbacks: { claudeTranscriptById: fallback },
    })).post('/api/sessions/resolve').send({ input: CLAUDE_V4 })
    expect(fallback).toHaveBeenCalledWith(CLAUDE_V4)
    expect(res.body.matches[0].cwd).toBe('/tmp/fb')
  })

  it('reports degraded (NOT "not found") when a provider fallback FAILS', async () => {
    const res = await request(makeApp({
      codingCliIndexer: { getProjects: () => [], refresh: async () => {}, isReady: () => true },
      resolveFallbacks: { opencodeSessionById: async () => { throw new Error('database is locked') } },
    })).post('/api/sessions/resolve').send({ input: 'ses_root0000000000000000000000' })
    expect(res.status).toBe(200)
    expect(res.body.indexState).toBe('degraded')
    expect(res.body.providerErrors).toEqual(['opencode'])
    expect(res.body.matches).toEqual([])
  })

  it('bounds per-request fallback work to FALLBACK_BUDGET_PER_REQUEST', async () => {
    const fallback = vi.fn().mockResolvedValue(null)
    await request(makeApp({
      codingCliIndexer: { getProjects: () => [], refresh: async () => {}, isReady: () => true },
      resolveFallbacks: { claudeTranscriptById: fallback },
    })).post('/api/sessions/resolve').send({
      input: [
        'ed2afda6-a340-443e-ba60-024a1b3554b1',
        'ed2afda6-a340-443e-ba60-024a1b3554b2',
        'ed2afda6-a340-443e-ba60-024a1b3554b3',
        'ed2afda6-a340-443e-ba60-024a1b3554b4',
      ].join(' '),
    })
    expect(fallback.mock.calls.length).toBeLessThanOrEqual(FALLBACK_BUDGET_PER_REQUEST)
  })

  it('400s on a missing/invalid body', async () => {
    const res = await request(makeApp()).post('/api/sessions/resolve').send({})
    expect(res.status).toBe(400)
  })
})
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
npm run test:vitest -- --config config/vitest/vitest.server.config.ts test/unit/server/sessions-resolve-router.test.ts --run
```
Expected: FAIL — TS error on unknown deps (`homeDir`, `resolveFallbacks`, `isReady`) and/or 404 on the route.

- [ ] **Step 3: Implement the default fallback wiring**

Create `server/coding-cli/resolve-fallbacks.ts`:

```typescript
import type { CodingCliProvider } from './provider.js'
import type { ExactIdFallback, ResolveMatch } from './session-resolver.js'
import { locateClaudeTranscriptById } from './claude-transcript-locator.js'
import { runOpencodeSessionByIdQuery } from './providers/opencode-by-id-query.js'

export type ResolveFallbacks = {
  claudeTranscriptById?: ExactIdFallback
  opencodeSessionById?: ExactIdFallback
}

/** Strict opencode id shape — the by-id DB query only runs for real ses_ ids (work budget). */
const OPENCODE_ID_SHAPE = /^ses_[0-9a-zA-Z]{26}$/

/**
 * Per-request work budget: each fallback may do real work at most this many
 * times per request; beyond that it reports a miss without doing work.
 * Combined with MAX_RESUME_CANDIDATES and the id-shape gates this bounds the
 * synchronous work one request can put on the event loop.
 */
export const FALLBACK_BUDGET_PER_REQUEST = 2

export function withRequestBudget(
  fallbacks: ResolveFallbacks,
  max = FALLBACK_BUDGET_PER_REQUEST,
): ResolveFallbacks {
  const budgeted = (fallback?: ExactIdFallback): ExactIdFallback | undefined => {
    if (!fallback) return undefined
    let used = 0
    return async (id) => {
      if (used >= max) return null
      used += 1
      return fallback(id)
    }
  }
  return {
    claudeTranscriptById: budgeted(fallbacks.claudeTranscriptById),
    opencodeSessionById: budgeted(fallbacks.opencodeSessionById),
  }
}

/** Build the production exact-id fallbacks from the live provider set. */
export function buildResolveFallbacks(providers: CodingCliProvider[]): ResolveFallbacks {
  const claude = providers.find((p) => p.name === 'claude')
  const opencode = providers.find((p) => p.name === 'opencode') as
    (CodingCliProvider & { getDatabasePath?: () => string }) | undefined

  const claudeTranscriptById: ExactIdFallback | undefined = claude
    ? async (id): Promise<ResolveMatch | null> => {
        const hit = await locateClaudeTranscriptById(id, claude.getSessionRoots())
        if (!hit) return null
        return {
          provider: 'claude',
          sessionId: hit.sessionId,
          cwd: hit.cwd,
          projectPath: hit.cwd ?? '',
          sessionType: 'claude',
          lastActivityAt: hit.lastActivityAt,
          matchType: 'exact',
          matchedToken: id,
        }
      }
    : undefined

  const opencodeSessionById: ExactIdFallback | undefined = opencode?.getDatabasePath
    ? async (id): Promise<ResolveMatch | null> => {
        // Shape gate (work budget): never open the DB for non-opencode tokens.
        if (!OPENCODE_ID_SHAPE.test(id)) return null
        // NO catch here: a locked/corrupt/unreadable DB must PROPAGATE so the
        // resolver records provider-unavailable (degraded), never "not found".
        const row = await runOpencodeSessionByIdQuery(opencode.getDatabasePath!(), id)
        if (!row) return null
        return {
          provider: 'opencode',
          sessionId: row.sessionId,
          cwd: row.cwd || undefined,
          projectPath: row.projectPath ?? row.cwd ?? '',
          sessionType: 'opencode',
          title: row.title || undefined,
          lastActivityAt: row.lastActivityAt,
          matchType: 'exact',
          matchedToken: id,
        }
      }
    : undefined

  return { claudeTranscriptById, opencodeSessionById }
}
```

Note (VERIFIED during load-bearing validation): `getDatabasePath()` exists at `server/coding-cli/providers/opencode.ts:78-79` (`return path.join(this.homeDir, 'opencode.db')`) but is declared **`private`**. As part of this step, remove the `private` keyword (make it public — no other change to that file) so `buildResolveFallbacks` compiles. Alternative considered and rejected: `getSessionRoots()[0]` also returns the DB path (opencode.ts:332-333), but an explicit public getter is clearer than an indexed root.

- [ ] **Step 4: Add the route to `server/sessions-router.ts`**

Add imports at the top (keep `.js` extensions — NodeNext):

```typescript
import os from 'os'
import { parseResumeInput } from '../shared/resume-input-parser.js'
import { resolveSessionCandidates } from './coding-cli/session-resolver.js'
import { buildResolveFallbacks, withRequestBudget, type ResolveFallbacks } from './coding-cli/resolve-fallbacks.js'
```

Extend `SessionsRouterDeps`: inside the existing `codingCliIndexer` object type add `isReady?: () => boolean`, and add two new optional top-level deps:

```typescript
  codingCliIndexer: {
    getProjects: () => any[]
    refresh: () => Promise<void>
    isReady?: () => boolean
  }
  /** Test override for the exact-id fallbacks (defaults to buildResolveFallbacks(codingCliProviders)). */
  resolveFallbacks?: ResolveFallbacks
  /** Test override for the "resume anyway" default cwd (defaults to os.homedir()). */
  homeDir?: string
```

Add the schema near `SessionPatchSchema`:

```typescript
export const ResolveBodySchema = z.object({ input: z.string().min(1).max(20_000) })
```

Add the route inside `createSessionsRouter(deps)`, after the `router.post('/session-metadata', …)` handler:

```typescript
  // Resume-button resolve: one scan answers all providers at once (spec:
  // docs/plans/2026-07-29-resume-button-spec.md). Evidence decides the agent;
  // the parsed hint is advisory UI state only. ACCEPTED LIMITATION (spec-
  // sanctioned): prefix matching covers indexed sessions only; exact-id
  // misses fall back to the claude transcript locator + opencode by-id query.
  const resolveFallbacks = deps.resolveFallbacks ?? buildResolveFallbacks(deps.codingCliProviders)
  router.post('/sessions/resolve', async (req, res) => {
    const parsed = ResolveBodySchema.safeParse(req.body)
    if (!parsed.success) {
      res.status(400).json({ error: 'body must be { input: string } (1-20000 chars)' })
      return
    }
    try {
      const { candidates, agentHint } = parseResumeInput(parsed.data.input)
      const { matches, providerErrors } = await resolveSessionCandidates(candidates, {
        getProjects: () => deps.codingCliIndexer.getProjects(),
        // Fresh budget per request: bounds fallback work (event-loop safety).
        fallbacks: withRequestBudget(resolveFallbacks),
      })
      // degraded = zero matches AND a provider fallback FAILED: absence needs
      // evidence (spec) — the client shows retry, never "no matching session".
      const indexState: 'ready' | 'warming' | 'degraded' =
        deps.codingCliIndexer.isReady?.() === false ? 'warming'
        : matches.length === 0 && providerErrors.length > 0 ? 'degraded'
        : 'ready'
      res.json({
        indexState,
        tokens: candidates,
        agentHint: agentHint ?? null,
        homeDir: deps.homeDir ?? os.homedir(),
        providerErrors,
        matches,
      })
    } catch (err) {
      log.warn({ err }, 'sessions/resolve failed')
      res.status(500).json({ error: 'resolve failed' })
    }
  })
```

- [ ] **Step 5: Run tests to verify they pass**

```bash
npm run test:vitest -- --config config/vitest/vitest.server.config.ts test/unit/server/sessions-resolve-router.test.ts --run
npm run typecheck:server
```
Expected: PASS, clean typecheck. Also confirm the wiring assumption: open `server/index.ts` around line 748 and verify `createSessionsRouter({ ... codingCliIndexer ... })` receives the indexer INSTANCE (then `isReady()` from Task 3 is picked up automatically); if it is an object literal, add `isReady: () => codingCliIndexer.isReady()`.

- [ ] **Step 6: Run the pre-existing sessions-router + indexer test files to catch regressions**

```bash
npm run test:vitest -- --config config/vitest/vitest.server.config.ts test/unit/server --run
```
Expected: PASS (no regressions from the deps change).

- [ ] **Step 7: Commit**

```bash
git add server/sessions-router.ts server/coding-cli/resolve-fallbacks.ts server/coding-cli/providers/opencode.ts test/unit/server/sessions-resolve-router.test.ts
git commit -m "$(cat <<'EOF'
feat(server): POST /api/sessions/resolve scanning all providers with exact-id fallbacks

🤖 Generated with [Amplifier](https://github.com/microsoft/amplifier)

Co-Authored-By: Amplifier <240397093+microsoft-amplifier@users.noreply.github.com>
EOF
)"
```

---

### Task 7: Client API function + ResumeSessionDialog component

**Files:**
- Modify: `src/lib/api.ts` (append at the end; the module exposes `api.post<T>(path, body)` ~line 275)
- Create: `src/components/ResumeSessionDialog.tsx`
- Test: `test/unit/client/resume-session-dialog.test.tsx`

**Interfaces:**
- Consumes: `api.post` from `src/lib/api.ts`; `parseResumeInput` from `@shared/resume-input-parser` (live hint pre-fill); `DEFAULT_ENABLED_CLI_PROVIDERS` from `@shared/coding-cli-defaults`; `OVERLAY_Z` from `@/components/ui/overlay`; the focus-trap pattern from `src/components/ui/confirm-modal.tsx`.
- Produces (consumed by Task 8):
  - `resolveResumeInput(input: string): Promise<ResumeResolveResponse>` in `src/lib/api.ts`, with exported types `ResumeResolveMatch` and `ResumeResolveResponse` (shapes mirror Task 6's HTTP contract exactly).
  - `ResumeSessionDialog` default-exported component with props `{ open: boolean; onClose: () => void; onResume: (opts: { provider: 'claude' | 'codex' | 'opencode' | 'amplifier'; sessionId: string; sessionType: string; cwd?: string; title?: string; firstUserMessage?: string }) => void }`.

- [ ] **Step 1: Write the failing tests**

Create `test/unit/client/resume-session-dialog.test.tsx`:

```tsx
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { cleanup, fireEvent, render, screen, act } from '@testing-library/react'
import ResumeSessionDialog from '@/components/ResumeSessionDialog'
import { ApiError, type ResumeResolveResponse } from '@/lib/api'

const mockResolve = vi.fn<(input: string) => Promise<ResumeResolveResponse>>()
vi.mock('@/lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/api')>()
  return { ...actual, resolveResumeInput: (input: string) => mockResolve(input) }
})

const CLAUDE_V4 = 'ed2afda6-a340-443e-ba60-024a1b3554b4'

function response(overrides: Partial<ResumeResolveResponse>): ResumeResolveResponse {
  return {
    indexState: 'ready',
    tokens: [CLAUDE_V4],
    agentHint: null,
    homeDir: '/home/testuser',
    providerErrors: [],
    matches: [],
    ...overrides,
  }
}

function match(overrides: Record<string, unknown> = {}) {
  return {
    provider: 'claude' as const,
    sessionId: CLAUDE_V4,
    cwd: '/home/u/proj',
    projectPath: '/home/u/proj',
    sessionType: 'claude',
    title: 'claude one',
    lastActivityAt: 111,
    matchType: 'exact' as const,
    matchedToken: CLAUDE_V4,
    ...overrides,
  }
}

function renderDialog(onResume = vi.fn(), onClose = vi.fn()) {
  render(<ResumeSessionDialog open onClose={onClose} onResume={onResume} />)
  return { onResume, onClose }
}

async function pasteAndResolve(text: string) {
  const input = screen.getByLabelText(/resume string/i)
  fireEvent.change(input, { target: { value: text } })
  fireEvent.keyDown(input, { key: 'Enter' })
  // Two flushes: runResolve awaits the API promise, then updates state.
  await act(async () => { await Promise.resolve(); await Promise.resolve() })
}

beforeEach(() => {
  vi.useFakeTimers()
  mockResolve.mockReset()
})

afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

describe('ResumeSessionDialog', () => {
  it('is an accessible modal dialog with picker and paste field', () => {
    renderDialog()
    const dialog = screen.getByRole('dialog', { name: /resume a session/i })
    expect(dialog).toHaveAttribute('aria-modal', 'true')
    expect(screen.getByLabelText(/agent/i)).toBeInTheDocument()
    expect(screen.getByLabelText(/resume string/i)).toBeInTheDocument()
  })

  it('Escape closes the dialog via the DOCUMENT-level listener', () => {
    const { onClose } = renderDialog()
    fireEvent.keyDown(document, { key: 'Escape' })
    expect(onClose).toHaveBeenCalled()
  })

  it('traps Tab focus: wraps from the last focusable to the first and back (Shift+Tab)', () => {
    renderDialog()
    const dialog = screen.getByRole('dialog')
    const picker = screen.getByLabelText(/agent/i)
    // With an empty input, "Find session" is disabled, so Cancel is the last focusable.
    const cancel = screen.getByRole('button', { name: /cancel/i })
    cancel.focus()
    fireEvent.keyDown(dialog, { key: 'Tab' })
    expect(document.activeElement).toBe(picker)
    fireEvent.keyDown(dialog, { key: 'Tab', shiftKey: true })
    expect(document.activeElement).toBe(cancel)
  })

  it('locks background scroll while open; restores scroll and focus on close', () => {
    const outside = document.createElement('button')
    document.body.appendChild(outside)
    outside.focus()
    const onClose = vi.fn()
    const onResume = vi.fn()
    const { rerender } = render(<ResumeSessionDialog open onClose={onClose} onResume={onResume} />)
    expect(document.body.style.overflow).toBe('hidden')
    rerender(<ResumeSessionDialog open={false} onClose={onClose} onResume={onResume} />)
    expect(document.body.style.overflow).toBe('')
    expect(document.activeElement).toBe(outside)
    outside.remove()
  })

  it('single match: resumes with the STORE provider even when the picker disagrees, and shows a note', async () => {
    mockResolve.mockResolvedValue(response({ matches: [match()] }))
    const { onResume } = renderDialog()
    fireEvent.change(screen.getByLabelText(/agent/i), { target: { value: 'codex' } })
    await pasteAndResolve(CLAUDE_V4)
    expect(onResume).toHaveBeenCalledWith(expect.objectContaining({
      provider: 'claude', sessionId: CLAUDE_V4, sessionType: 'claude', cwd: '/home/u/proj',
    }))
    expect(screen.getByRole('status')).toHaveTextContent(/found in claude/i)
  })

  it('auto-resolves on paste (paste-then-Enter fast path)', async () => {
    mockResolve.mockResolvedValue(response({ matches: [match()] }))
    const { onResume } = renderDialog()
    const input = screen.getByLabelText(/resume string/i)
    fireEvent.paste(input, { clipboardData: { getData: () => CLAUDE_V4 } })
    await act(async () => { vi.advanceTimersByTime(1); await Promise.resolve(); await Promise.resolve() })
    expect(mockResolve).toHaveBeenCalled()
    expect(onResume).toHaveBeenCalled()
  })

  it('multiple matches: shows a disambiguation list, one click resumes', async () => {
    mockResolve.mockResolvedValue(response({
      matches: [
        match({ sessionId: 'aaa11111-1111-4111-8111-111111111111', title: 'newer', lastActivityAt: 2 }),
        match({ provider: 'codex', sessionId: 'bbb22222-2222-4222-8222-222222222222', sessionType: 'codex', title: 'older', lastActivityAt: 1 }),
      ],
    }))
    const { onResume } = renderDialog()
    await pasteAndResolve('aaa')
    const options = screen.getAllByRole('button', { name: /resume .*(newer|older)/i })
    expect(options).toHaveLength(2)
    fireEvent.click(options[1])
    expect(onResume).toHaveBeenCalledWith(expect.objectContaining({ provider: 'codex' }))
  })

  it('zero matches + ready: inline error, input preserved, resume-anyway with editable cwd prefilled to home', async () => {
    mockResolve.mockResolvedValue(response({ matches: [] }))
    const { onResume } = renderDialog()
    await pasteAndResolve(CLAUDE_V4)
    expect(screen.getByRole('alert')).toHaveTextContent(/no matching session/i)
    expect(screen.getByLabelText(/resume string/i)).toHaveValue(CLAUDE_V4)
    const cwdInput = screen.getByLabelText(/working directory/i)
    expect(cwdInput).toHaveValue('/home/testuser')
    fireEvent.change(cwdInput, { target: { value: '/home/testuser/elsewhere' } })
    fireEvent.click(screen.getByRole('button', { name: /resume anyway/i }))
    expect(onResume).toHaveBeenCalledWith(expect.objectContaining({
      sessionId: CLAUDE_V4, cwd: '/home/testuser/elsewhere',
    }))
  })

  it('resume-anyway uses the picker-selected agent', async () => {
    mockResolve.mockResolvedValue(response({ matches: [] }))
    const { onResume } = renderDialog()
    fireEvent.change(screen.getByLabelText(/agent/i), { target: { value: 'amplifier' } })
    await pasteAndResolve('417e8345aa')
    fireEvent.click(screen.getByRole('button', { name: /resume anyway/i }))
    expect(onResume).toHaveBeenCalledWith(expect.objectContaining({ provider: 'amplifier', sessionType: 'amplifier' }))
  })

  it('warming: shows retry state, NOT "not found", and re-resolves on retry', async () => {
    mockResolve.mockResolvedValueOnce(response({ indexState: 'warming', matches: [] }))
    mockResolve.mockResolvedValueOnce(response({ matches: [match()] }))
    const { onResume } = renderDialog()
    await pasteAndResolve(CLAUDE_V4)
    expect(screen.queryByRole('alert')).toBeNull()
    expect(screen.getByRole('status')).toHaveTextContent(/index is still warming/i)
    fireEvent.click(screen.getByRole('button', { name: /retry/i }))
    await act(async () => { await Promise.resolve(); await Promise.resolve() })
    expect(onResume).toHaveBeenCalled()
  })

  it('degraded (provider unavailable): retry state, NOT "no matching session"', async () => {
    mockResolve.mockResolvedValueOnce(response({ indexState: 'degraded', providerErrors: ['opencode'], matches: [] }))
    mockResolve.mockResolvedValueOnce(response({ matches: [match()] }))
    const { onResume } = renderDialog()
    await pasteAndResolve(CLAUDE_V4)
    expect(screen.queryByRole('alert')).toBeNull()
    expect(screen.getByRole('status')).toHaveTextContent(/could not be searched/i)
    fireEvent.click(screen.getByRole('button', { name: /retry/i }))
    await act(async () => { await Promise.resolve(); await Promise.resolve() })
    expect(onResume).toHaveBeenCalled()
  })

  it('ignores STALE responses: a late first response cannot override or auto-resume', async () => {
    let resolveFirst!: (r: ResumeResolveResponse) => void
    mockResolve.mockReturnValueOnce(new Promise<ResumeResolveResponse>((r) => { resolveFirst = r }))
    mockResolve.mockResolvedValueOnce(response({ matches: [] }))
    const { onResume } = renderDialog()
    const input = screen.getByLabelText(/resume string/i)
    fireEvent.change(input, { target: { value: 'ed2afda6' } })
    fireEvent.keyDown(input, { key: 'Enter' })
    fireEvent.change(input, { target: { value: CLAUDE_V4 } })
    fireEvent.keyDown(input, { key: 'Enter' })
    await act(async () => { await Promise.resolve(); await Promise.resolve() })
    expect(screen.getByRole('alert')).toHaveTextContent(/no matching session/i)
    // The stale FIRST response now arrives with a single match: it must be
    // ignored — a stale auto-resume would open the WRONG session.
    await act(async () => {
      resolveFirst(response({ matches: [match()] }))
      await Promise.resolve(); await Promise.resolve()
    })
    expect(onResume).not.toHaveBeenCalled()
    expect(screen.getByRole('alert')).toHaveTextContent(/no matching session/i)
  })

  it('resolve 404 (endpoint absent, e.g. Rust-served client): explicit unsupported message', async () => {
    mockResolve.mockRejectedValue(new ApiError(404, 'Not Found'))
    const { onResume } = renderDialog()
    await pasteAndResolve(CLAUDE_V4)
    expect(screen.getByRole('alert')).toHaveTextContent(/does not support resume-by-id/i)
    expect(onResume).not.toHaveBeenCalled()
  })

  it('garbage input: inline error, no resume', async () => {
    mockResolve.mockResolvedValue(response({ tokens: [], matches: [] }))
    const { onResume } = renderDialog()
    await pasteAndResolve('total garbage')
    expect(screen.getByRole('alert')).toHaveTextContent(/no session id/i)
    expect(onResume).not.toHaveBeenCalled()
  })

  it('pre-fills the picker from the advisory hint when untouched', async () => {
    mockResolve.mockResolvedValue(response({
      agentHint: { provider: 'opencode', source: 'id-format' }, matches: [],
      tokens: ['ses_root0000000000000000000000'],
    }))
    renderDialog()
    await pasteAndResolve('ses_root0000000000000000000000')
    expect(screen.getByLabelText(/agent/i)).toHaveValue('opencode')
  })
})
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
npm run test:vitest -- --config config/vitest/vitest.config.ts test/unit/client/resume-session-dialog.test.tsx --run
```
Expected: FAIL — component/module not found.

- [ ] **Step 3: Add the API function**

Append to `src/lib/api.ts`:

```typescript
export type ResumeResolveMatch = {
  provider: 'claude' | 'codex' | 'opencode' | 'amplifier'
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

export type ResumeResolveResponse = {
  /** 'degraded' = zero matches AND a provider fallback failed — retry, NOT "not found". */
  indexState: 'ready' | 'warming' | 'degraded'
  tokens: string[]
  agentHint: { provider: ResumeResolveMatch['provider']; source: 'command' | 'word' | 'id-format' } | null
  homeDir: string
  providerErrors: Array<'claude' | 'opencode'>
  matches: ResumeResolveMatch[]
}

/** Resume-button resolve (POST /api/sessions/resolve) — see docs/plans/2026-07-29-resume-button-spec.md. */
export async function resolveResumeInput(input: string): Promise<ResumeResolveResponse> {
  return api.post<ResumeResolveResponse>('/api/sessions/resolve', { input })
}
```

- [ ] **Step 4: Implement the dialog**

Create `src/components/ResumeSessionDialog.tsx`:

```tsx
import { useCallback, useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { Loader2 } from 'lucide-react'
import { OVERLAY_Z } from '@/components/ui/overlay'
import { ApiError, resolveResumeInput, type ResumeResolveMatch, type ResumeResolveResponse } from '@/lib/api'
import { parseResumeInput, type ResumeProviderName } from '@shared/resume-input-parser'
import { DEFAULT_ENABLED_CLI_PROVIDERS } from '@shared/coding-cli-defaults'

// Local copy of Sidebar.tsx's module-private formatRelativeTime (verified NOT
// exported anywhere; importing from Sidebar would create a circular import
// since Sidebar renders this dialog).
function formatRelativeTime(timestamp: number): string {
  const now = Date.now()
  const diff = now - timestamp
  const minutes = Math.floor(diff / 60000)
  const hours = Math.floor(diff / 3600000)
  const days = Math.floor(diff / 86400000)

  if (minutes < 1) return 'now'
  if (minutes < 60) return `${minutes}m`
  if (hours < 24) return `${hours}h`
  if (days < 7) return `${days}d`
  return new Date(timestamp).toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
}

// Focus-trap helper — same pattern as src/components/ui/confirm-modal.tsx
// (repo modal a11y convention: trap Tab, restore focus, lock scroll, doc-level Escape).
function getFocusable(container: HTMLElement): HTMLElement[] {
  const selectors = [
    'button',
    '[href]',
    'input',
    'select',
    'textarea',
    '[tabindex]:not([tabindex="-1"])',
  ]
  return Array.from(container.querySelectorAll<HTMLElement>(selectors.join(',')))
    .filter((el) => !el.hasAttribute('disabled') && !el.getAttribute('aria-hidden'))
}

export type ResumeSessionDialogProps = {
  open: boolean
  onClose: () => void
  onResume: (opts: {
    provider: ResumeProviderName
    sessionId: string
    sessionType: string
    cwd?: string
    title?: string
    firstUserMessage?: string
  }) => void
}

const PROVIDER_LABELS: Record<ResumeProviderName, string> = {
  claude: 'Claude Code',
  codex: 'Codex',
  opencode: 'OpenCode',
  amplifier: 'Amplifier',
}

const CLOSE_AFTER_RESUME_MS = 1500

export default function ResumeSessionDialog({ open, onClose, onResume }: ResumeSessionDialogProps) {
  const [inputValue, setInputValue] = useState('')
  const [picker, setPicker] = useState<ResumeProviderName>('claude')
  const [pickerTouched, setPickerTouched] = useState(false)
  const [resolving, setResolving] = useState(false)
  const [result, setResult] = useState<ResumeResolveResponse | null>(null)
  const [errorText, setErrorText] = useState<string | null>(null)
  const [note, setNote] = useState<string | null>(null)
  const [anywayCwd, setAnywayCwd] = useState('')
  const inputRef = useRef<HTMLTextAreaElement>(null)
  const dialogRef = useRef<HTMLDivElement>(null)
  const closeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const previousFocusRef = useRef<HTMLElement | null>(null)
  const previousOverflowRef = useRef<string | null>(null)
  // Stale-response guard: only the LATEST resolve request may mutate state.
  const resolveSeqRef = useRef(0)

  useEffect(() => {
    if (!open) {
      setInputValue(''); setResult(null); setErrorText(null); setNote(null)
      setResolving(false); setPickerTouched(false)
      resolveSeqRef.current += 1 // invalidate any in-flight resolve
    }
    return () => { if (closeTimerRef.current) clearTimeout(closeTimerRef.current) }
  }, [open])

  // Modal a11y (mirrors src/components/ui/confirm-modal.tsx): capture + restore
  // the previously focused element, lock background scroll, focus the paste field.
  useEffect(() => {
    if (!open) return
    previousFocusRef.current = document.activeElement as HTMLElement | null
    previousOverflowRef.current = document.body.style.overflow
    document.body.style.overflow = 'hidden'
    inputRef.current?.focus()
    return () => {
      document.body.style.overflow = previousOverflowRef.current || ''
      previousFocusRef.current?.focus()
    }
  }, [open])

  // Document-level Escape (works regardless of where focus sits).
  useEffect(() => {
    if (!open) return
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') { e.preventDefault(); onClose() }
    }
    document.addEventListener('keydown', handleKey)
    return () => document.removeEventListener('keydown', handleKey)
  }, [open, onClose])

  const finishResume = useCallback((m: ResumeResolveMatch) => {
    onResume({
      provider: m.provider,
      sessionId: m.sessionId,
      sessionType: m.sessionType,
      cwd: m.cwd,
      title: m.title,
      firstUserMessage: m.firstUserMessage,
    })
    setNote(`Found in ${PROVIDER_LABELS[m.provider] ?? m.provider}`)
    closeTimerRef.current = setTimeout(onClose, CLOSE_AFTER_RESUME_MS)
  }, [onClose, onResume])

  const runResolve = useCallback(async (text: string) => {
    const trimmed = text.trim()
    if (!trimmed) return
    // Stale-response guard: bump the sequence; only the LATEST request may
    // mutate state. A stale single-match response must NEVER auto-resume —
    // it could open the WRONG session.
    const seq = ++resolveSeqRef.current
    setResolving(true); setErrorText(null); setNote(null); setResult(null)
    // Live local hint (advisory only — server evidence decides).
    const localParse = parseResumeInput(trimmed)
    if (localParse.agentHint && !pickerTouched) setPicker(localParse.agentHint.provider)
    let response: ResumeResolveResponse
    try {
      response = await resolveResumeInput(trimmed)
    } catch (err) {
      if (seq !== resolveSeqRef.current) return // stale — ignore
      setResolving(false)
      // Deployment degradation contract (Task 1): the canonical Rust-served
      // production has no resolve endpoint — a 404 gets an explicit message.
      setErrorText(err instanceof ApiError && err.status === 404
        ? 'This server build does not support resume-by-id yet.'
        : 'Could not reach the server. Try again.')
      return
    }
    if (seq !== resolveSeqRef.current) return // stale — ignore
    setResolving(false)
    setResult(response)
    setAnywayCwd(response.homeDir)
    if (response.agentHint && !pickerTouched) setPicker(response.agentHint.provider)
    if (response.tokens.length === 0) {
      setErrorText('No session id found in the pasted text.')
      return
    }
    if (response.matches.length === 1) {
      finishResume(response.matches[0])
      return
    }
    if (response.matches.length === 0 && response.indexState === 'ready') {
      setErrorText('No matching session found in any agent.')
    }
  }, [finishResume, pickerTouched])

  const handleResumeAnyway = useCallback(() => {
    const token = result?.tokens[0] ?? parseResumeInput(inputValue).candidates[0]
    if (!token) return
    onResume({ provider: picker, sessionId: token, sessionType: picker, cwd: anywayCwd || undefined })
    onClose()
  }, [anywayCwd, inputValue, onClose, onResume, picker, result])

  if (!open) return null

  // warming AND degraded are retry states — NEITHER is "not found" (spec:
  // absence needs evidence; provider unavailable gets loading/retry).
  const retryState = result !== null && result.tokens.length > 0 && result.matches.length === 0
    && (result.indexState === 'warming' || result.indexState === 'degraded')
    ? result.indexState
    : null
  const showDisambiguation = (result?.matches.length ?? 0) > 1
  const showResumeAnyway = errorText !== null && errorText.startsWith('No matching session')

  return createPortal(
    <div
      className={`fixed inset-0 bg-black/50 flex items-center justify-center p-4 ${OVERLAY_Z.modal}`}
      onMouseDown={(e) => { if (e.target === e.currentTarget) onClose() }}
    >
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="resume-session-dialog-title"
        data-testid="resume-session-dialog"
        className="bg-card border border-border rounded-lg shadow-lg w-full max-w-lg p-4 flex flex-col gap-3"
        onKeyDown={(e) => {
          if (e.key === 'Escape') { e.stopPropagation(); onClose(); return }
          if (e.key !== 'Tab') return
          // Focus trap — repo modal pattern (see src/components/ui/confirm-modal.tsx).
          const dialog = dialogRef.current
          if (!dialog) return
          const focusables = getFocusable(dialog)
          if (focusables.length === 0) { e.preventDefault(); return }
          const first = focusables[0]
          const last = focusables[focusables.length - 1]
          const active = document.activeElement as HTMLElement | null
          if (e.shiftKey) {
            if (active === first || !dialog.contains(active)) {
              e.preventDefault()
              last.focus()
            }
          } else if (active === last) {
            e.preventDefault()
            first.focus()
          }
        }}
      >
        <h2 id="resume-session-dialog-title" className="text-sm font-medium">Resume a session</h2>

        <div className="flex flex-col gap-1">
          <label htmlFor="resume-agent-picker" className="text-xs text-muted-foreground">Agent (auto-detected — used only when no match is found)</label>
          <select
            id="resume-agent-picker"
            className="bg-background border border-border rounded px-2 py-1.5 text-sm"
            value={picker}
            onChange={(e) => { setPicker(e.target.value as ResumeProviderName); setPickerTouched(true) }}
          >
            {DEFAULT_ENABLED_CLI_PROVIDERS.map((p) => (
              <option key={p} value={p}>{PROVIDER_LABELS[p as ResumeProviderName] ?? p}</option>
            ))}
          </select>
        </div>

        <div className="flex flex-col gap-1">
          <label htmlFor="resume-input" className="text-xs text-muted-foreground">Resume string (paste anything containing a session id)</label>
          <textarea
            id="resume-input"
            ref={inputRef}
            rows={3}
            className="bg-background border border-border rounded px-2 py-1.5 text-sm font-mono resize-none"
            placeholder='e.g. "codex resume 019fac27-…" or 417e8345'
            value={inputValue}
            onChange={(e) => setInputValue(e.target.value)}
            onPaste={(e) => {
              // Paste-then-Enter fast path: auto-resolve on paste. Read BOTH the
              // element value (real browsers update it after the event) and the
              // clipboard payload (jsdom never updates the value on paste).
              const pasted = e.clipboardData?.getData('text') ?? ''
              const target = e.currentTarget
              setTimeout(() => {
                const value = target.value.trim() ? target.value : pasted
                if (!value.trim()) return
                if (!target.value.trim()) setInputValue(pasted)
                void runResolve(value)
              }, 0)
            }}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); void runResolve(inputValue) }
            }}
          />
        </div>

        {resolving && (
          <div role="status" className="flex items-center gap-2 text-sm text-muted-foreground">
            <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" /> Searching all agents…
          </div>
        )}

        {note && <div role="status" className="text-sm text-emerald-500">{note}</div>}

        {retryState && (
          <div role="status" className="flex items-center gap-2 text-sm text-muted-foreground">
            <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
            {retryState === 'warming'
              ? 'Session index is still warming — this is not a "not found".'
              : 'Some agents could not be searched right now — this is not a "not found".'}
            <button
              type="button"
              className="underline hover:text-foreground"
              onClick={() => void runResolve(inputValue)}
            >
              Retry
            </button>
          </div>
        )}

        {showDisambiguation && (
          <ul aria-label="Matching sessions" className="flex flex-col gap-1 max-h-64 overflow-y-auto">
            {result!.matches.map((m) => (
              <li key={`${m.provider}:${m.sessionId}`}>
                <button
                  type="button"
                  className="w-full text-left rounded border border-border px-2 py-1.5 hover:bg-muted/50"
                  aria-label={`Resume ${PROVIDER_LABELS[m.provider] ?? m.provider} session ${m.title ?? m.sessionId}`}
                  onClick={() => finishResume(m)}
                >
                  <div className="text-sm truncate">{m.title || m.firstUserMessage || m.sessionId}</div>
                  <div className="text-2xs text-muted-foreground truncate">
                    {PROVIDER_LABELS[m.provider] ?? m.provider} · {m.cwd ?? m.projectPath} · {formatRelativeTime(m.lastActivityAt)}
                  </div>
                </button>
              </li>
            ))}
          </ul>
        )}

        {errorText && <div role="alert" className="text-sm text-red-500">{errorText}</div>}

        {showResumeAnyway && (
          <div className="flex flex-col gap-2 border-t border-border pt-2">
            <div className="flex flex-col gap-1">
              <label htmlFor="resume-anyway-cwd" className="text-xs text-muted-foreground">Working directory</label>
              <input
                id="resume-anyway-cwd"
                className="bg-background border border-border rounded px-2 py-1.5 text-sm font-mono"
                value={anywayCwd}
                onChange={(e) => setAnywayCwd(e.target.value)}
              />
            </div>
            <button
              type="button"
              className="self-start rounded border border-border px-3 py-1.5 text-sm hover:bg-muted/50"
              onClick={handleResumeAnyway}
            >
              Resume anyway with {PROVIDER_LABELS[picker]}
            </button>
          </div>
        )}

        <div className="flex justify-end gap-2 pt-1">
          <button
            type="button"
            className="rounded px-3 py-1.5 text-sm text-muted-foreground hover:text-foreground hover:bg-muted/50"
            onClick={onClose}
          >
            Cancel
          </button>
          <button
            type="button"
            className="rounded border border-border px-3 py-1.5 text-sm hover:bg-muted/50"
            onClick={() => void runResolve(inputValue)}
            disabled={resolving || inputValue.trim() === ''}
          >
            Find session
          </button>
        </div>
      </div>
    </div>,
    document.body,
  )
}
```

Implementation notes for this step (facts verified during load-bearing validation):
- `formatRelativeTime` is a module-LOCAL (non-exported) function in `src/components/Sidebar.tsx` (~line 107) and exists nowhere else — that is why the dialog defines its own local copy above. Do NOT import it from Sidebar.tsx (Sidebar renders this dialog; that import would be circular) and do NOT add it to `@/lib/utils` (keeps this change's blast radius to new files).
- `OVERLAY_Z` (`src/components/ui/overlay.ts`) is a map of Tailwind z-index classes `{ tooltip: 'z-40', menu: 'z-50', modal: 'z-[60]' }`, NOT a number — hence `${OVERLAY_Z.modal}` appended to the overlay `className` (never `style={{ zIndex: … }}`).
- `DEFAULT_ENABLED_CLI_PROVIDERS` is `readonly ['claude','codex','opencode','amplifier']` — the `.map` cast shown handles it.
- Modal a11y is MANDATORY (repo a11y rule), not conditional: the component mirrors `src/components/ui/confirm-modal.tsx` exactly — `getFocusable` + Tab/Shift+Tab focus trap on the dialog, previous-focus capture and restore on close, `document.body.style.overflow` scroll lock while open, and a document-level Escape listener. The Step 1 tests cover each of these behaviors; do not remove any of them.
- Stale-response guard is MANDATORY: `resolveSeqRef` invalidates in-flight resolves on every new resolve and on close; a stale response (including a stale single-match auto-resume) must be ignored. The Step 1 reversed-ordering test proves it.

- [ ] **Step 5: Run tests to verify they pass**

```bash
npm run test:vitest -- --config config/vitest/vitest.config.ts test/unit/client/resume-session-dialog.test.tsx --run
npm run typecheck:client
```
Expected: PASS, clean typecheck.

- [ ] **Step 6: Commit**

```bash
git add src/lib/api.ts src/components/ResumeSessionDialog.tsx test/unit/client/resume-session-dialog.test.tsx
git commit -m "$(cat <<'EOF'
feat(client): resume-session dialog with cross-provider resolve

🤖 Generated with [Amplifier](https://github.com/microsoft/amplifier)

Co-Authored-By: Amplifier <240397093+microsoft-amplifier@users.noreply.github.com>
EOF
)"
```

---

### Task 8: Sidebar pinned footer + resume wiring

**Files:**
- Modify: `src/components/Sidebar.tsx` (imports ~lines 1–35; footer inserted after the session-list wrapper — the `<div className="flex flex-1 min-h-0 flex-col">` that opens at ~line 833 and contains `data-testid="sidebar-session-list"`)
- Test: `test/unit/client/sidebar-resume-footer.test.tsx`

**Interfaces:**
- Consumes: `ResumeSessionDialog` (Task 7); `openSessionTab` thunk from `@/store/tabsSlice` — signature `openSessionTab({ sessionId, title?, cwd?, provider?, sessionType?, … })`; NOTE: `openSessionTab` already dedupes against open panes internally (see the "Dedupe by session is handled in openSessionTab" comment in `src/store/tabsSlice.ts` ~line 297) — do NOT reimplement dedup. Also consumes the Sidebar's existing `onNavigate` prop: every session-open path in Sidebar.tsx calls `onNavigate('terminal')` (see ~lines 401/430/450) and the resume handler MUST too, or the resumed tab stays hidden behind non-terminal views.
- Produces: `data-testid="sidebar-footer"` (pinned footer container) and `data-testid="sidebar-resume-button"` — the flow tests (Task 9) and the spec's UI acceptance depend on these exact testids.

- [ ] **Step 1: Write the failing tests**

Create `test/unit/client/sidebar-resume-footer.test.tsx`:

```tsx
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { Provider } from 'react-redux'
import { configureStore } from '@reduxjs/toolkit'
import Sidebar from '@/components/Sidebar'
import tabsReducer from '@/store/tabsSlice'
import panesReducer from '@/store/panesSlice'
import settingsReducer from '@/store/settingsSlice'
import connectionReducer from '@/store/connectionSlice'
import sessionsReducer from '@/store/sessionsSlice'
import sessionActivityReducer from '@/store/sessionActivitySlice'

vi.mock('@/lib/ws-client', () => ({
  getWsClient: () => ({
    send: vi.fn(),
    onMessage: vi.fn(() => () => {}),
    connect: vi.fn().mockResolvedValue(undefined),
  }),
}))

function makeStore() {
  return configureStore({
    reducer: {
      tabs: tabsReducer,
      panes: panesReducer,
      settings: settingsReducer,
      connection: connectionReducer,
      sessions: sessionsReducer,
      sessionActivity: sessionActivityReducer,
    },
  })
}

function renderSidebar(props: Partial<React.ComponentProps<typeof Sidebar>> = {}) {
  return render(
    <Provider store={makeStore()}>
      <Sidebar view="terminal" onNavigate={vi.fn()} {...props} />
    </Provider>,
  )
}

beforeEach(() => vi.clearAllMocks())
afterEach(() => cleanup())

describe('Sidebar pinned resume footer', () => {
  it('pins the footer: IMMEDIATE next sibling of the flex-1 min-h-0 scroll wrapper, outside it, non-shrinking', () => {
    // jsdom cannot do layout, so this asserts the EXACT pinning mechanism the
    // spec mandates instead of faking scroll events: the scroll wrapper is the
    // `flex flex-1 min-h-0` div that CONTAINS the session list; the footer is
    // that wrapper's IMMEDIATE next sibling inside the same flex-column parent
    // and carries flex-shrink-0 so it can never be scrolled away or squeezed
    // out. Any placement that violates the spec (inside the list, deeper in the
    // tree, or after other siblings) fails one of these assertions.
    renderSidebar()
    const footer = screen.getByTestId('sidebar-footer')
    const list = screen.getByTestId('sidebar-session-list')
    const wrapper = footer.previousElementSibling as HTMLElement | null
    expect(wrapper).not.toBeNull()
    expect(wrapper!.className).toContain('flex-1')
    expect(wrapper!.className).toContain('min-h-0')
    expect(wrapper!.contains(list)).toBe(true)
    expect(wrapper!.contains(footer)).toBe(false)
    expect(footer.parentElement).toBe(wrapper!.parentElement)
    expect(footer.className).toContain('flex-shrink-0')
  })

  it('renders in fullWidth mobile mode', () => {
    renderSidebar({ fullWidth: true })
    expect(screen.getByTestId('sidebar-resume-button')).toBeInTheDocument()
  })

  it('button is keyboard accessible with an accessible name and opens the dialog', () => {
    renderSidebar()
    const button = screen.getByRole('button', { name: /resume a session/i })
    expect(button).toHaveAttribute('data-testid', 'sidebar-resume-button')
    fireEvent.click(button)
    expect(screen.getByRole('dialog', { name: /resume a session/i })).toBeInTheDocument()
  })
})
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
npm run test:vitest -- --config config/vitest/vitest.config.ts test/unit/client/sidebar-resume-footer.test.tsx --run
```
Expected: FAIL — `sidebar-footer` testid not found. (If the harness itself fails to render for a missing-reducer reason, mirror the fuller `createStore` helper from `test/e2e/sidebar-click-opens-pane.test.tsx` lines 68–147 — same reducers, richer preloaded state — then re-run and confirm the failure is the missing footer.)

- [ ] **Step 3: Implement the footer + wiring in `Sidebar.tsx`**

Add imports (top of file, merging into existing import lines):

```tsx
import { RotateCcw } from 'lucide-react'
import ResumeSessionDialog from '@/components/ResumeSessionDialog'
```

Inside the `Sidebar` component body, near the other `useState` hooks:

```tsx
const [resumeDialogOpen, setResumeDialogOpen] = useState(false)

const handleResumeResolved = useCallback((opts: {
  provider: CodingCliProviderName
  sessionId: string
  sessionType: string
  cwd?: string
  title?: string
  firstUserMessage?: string
}) => {
  // openSessionTab dedupes against already-open panes internally and focuses
  // the existing pane instead of spawning a duplicate (sidebar convention).
  void dispatch(openSessionTab({
    sessionId: opts.sessionId,
    provider: opts.provider,
    sessionType: opts.sessionType,
    cwd: opts.cwd,
    title: opts.title,
    firstUserMessage: opts.firstUserMessage,
    hasTitle: Boolean(opts.title),
  }))
  // Sidebar convention — every session-open path in this file calls
  // onNavigate('terminal') so the opened tab is actually VISIBLE even when the
  // user was on the Tabs/Panes/etc. view. Task 9's flow tests render with
  // view="tabs" and assert this navigation happens.
  onNavigate('terminal')
}, [dispatch, onNavigate])
```

In the JSX: find the session-list wrapper `<div className="flex flex-1 min-h-0 flex-col">` (opens ~line 833, contains `data-testid="sidebar-session-list"`). Immediately AFTER that wrapper's closing `</div>` — as its next sibling, still inside the component's root `h-full flex flex-col` div — insert:

```tsx
      {/* Pinned footer — spec: sibling AFTER the scroll wrapper, visible at every
          scroll position and in fullWidth mode. Never move inside the list. */}
      <div data-testid="sidebar-footer" className="flex-shrink-0 border-t border-border px-2 py-2">
        <button
          type="button"
          data-testid="sidebar-resume-button"
          aria-label="Resume a session by ID"
          aria-haspopup="dialog"
          onClick={() => setResumeDialogOpen(true)}
          className="w-full flex items-center justify-center gap-1.5 rounded px-2 py-1.5 text-sm text-muted-foreground hover:text-foreground hover:bg-muted/50"
        >
          <RotateCcw className="h-3.5 w-3.5" aria-hidden="true" />
          Resume
        </button>
      </div>
      <ResumeSessionDialog
        open={resumeDialogOpen}
        onClose={() => setResumeDialogOpen(false)}
        onResume={handleResumeResolved}
      />
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
npm run test:vitest -- --config config/vitest/vitest.config.ts test/unit/client/sidebar-resume-footer.test.tsx --run
```
Expected: PASS.

- [ ] **Step 5: Run the existing Sidebar test suites to catch layout regressions**

```bash
npm run test:vitest -- --config config/vitest/vitest.config.ts test/e2e/sidebar-click-opens-pane.test.tsx test/e2e/mobile-sidebar-fullwidth-flow.test.tsx test/e2e/sidebar-refresh-dom-stability.test.tsx --run
```
Expected: PASS (footer addition must not break existing sidebar behavior).

- [ ] **Step 6: Typecheck and commit**

```bash
npm run typecheck:client
git add src/components/Sidebar.tsx test/unit/client/sidebar-resume-footer.test.tsx
git commit -m "$(cat <<'EOF'
feat(client): always-visible Resume button pinned below the sidebar list

🤖 Generated with [Amplifier](https://github.com/microsoft/amplifier)

Co-Authored-By: Amplifier <240397093+microsoft-amplifier@users.noreply.github.com>
EOF
)"
```

---

### Task 9: End-to-end acceptance flow tests (user stories)

These are the highest-level tests proving the spec's acceptance table end-to-end in jsdom: real store + real `openSessionTab` thunk + real Sidebar + real dialog, with only the HTTP layer and WS mocked.

**Files:**
- Test: `test/e2e/resume-button-flow.test.tsx`

**Interfaces:**
- Consumes: everything from Tasks 7–8 (testids `sidebar-resume-button`, dialog labels) plus the real `tabsSlice`; mocks `resolveResumeInput` in `@/lib/api`.
- Produces: regression wall for the feature; no new exports.

- [ ] **Step 1: Write the failing tests**

Create `test/e2e/resume-button-flow.test.tsx`:

```tsx
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { act, cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import { Provider } from 'react-redux'
import { configureStore } from '@reduxjs/toolkit'
import Sidebar from '@/components/Sidebar'
import tabsReducer from '@/store/tabsSlice'
import panesReducer from '@/store/panesSlice'
import settingsReducer from '@/store/settingsSlice'
import connectionReducer from '@/store/connectionSlice'
import sessionsReducer from '@/store/sessionsSlice'
import sessionActivityReducer from '@/store/sessionActivitySlice'
import type { ResumeResolveResponse } from '@/lib/api'

// Scope queries to the dialog: the sidebar itself may render other role=status nodes.
function dialog() {
  return within(screen.getByRole('dialog', { name: /resume a session/i }))
}

const mockResolve = vi.fn<(input: string) => Promise<ResumeResolveResponse>>()
vi.mock('@/lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/api')>()
  return { ...actual, resolveResumeInput: (input: string) => mockResolve(input) }
})

vi.mock('@/lib/ws-client', () => ({
  getWsClient: () => ({
    send: vi.fn(),
    onMessage: vi.fn(() => () => {}),
    connect: vi.fn().mockResolvedValue(undefined),
  }),
}))

const CLAUDE_V4 = 'ed2afda6-a340-443e-ba60-024a1b3554b4'
const AMPLIFIER_FULL = '417e8345-90ab-4cde-8f01-234567890abc'

function response(overrides: Partial<ResumeResolveResponse> = {}): ResumeResolveResponse {
  return { indexState: 'ready', tokens: [CLAUDE_V4], agentHint: null, homeDir: '/home/t', providerErrors: [], matches: [], ...overrides }
}

function claudeMatch() {
  return {
    provider: 'claude' as const, sessionId: CLAUDE_V4, cwd: '/home/u/proj', projectPath: '/home/u/proj',
    sessionType: 'claude', title: 'claude one', lastActivityAt: 111,
    matchType: 'exact' as const, matchedToken: CLAUDE_V4,
  }
}

function makeStore() {
  return configureStore({
    reducer: {
      tabs: tabsReducer,
      panes: panesReducer,
      settings: settingsReducer,
      connection: connectionReducer,
      sessions: sessionsReducer,
      sessionActivity: sessionActivityReducer,
    },
  })
}

// Render on a NON-terminal view: resume must both open the tab AND navigate
// to the terminal view (Sidebar convention), which view="terminal" would hide.
function renderApp() {
  const store = makeStore()
  const onNavigate = vi.fn()
  render(
    <Provider store={store}>
      <Sidebar view="tabs" onNavigate={onNavigate} />
    </Provider>,
  )
  return { store, onNavigate }
}

// The resumed TUPLE must be verified — not just tabs.length (an implementation
// that always opened a claude pane would otherwise pass every flow).
function expectResumedTab(
  store: ReturnType<typeof makeStore>,
  expected: { provider: string; sessionId: string; cwd?: string },
) {
  const tabs = store.getState().tabs.tabs
  expect(tabs).toHaveLength(1)
  expect(tabs[0]).toMatchObject({
    codingCliProvider: expected.provider,
    mode: expected.provider,
    ...(expected.cwd !== undefined ? { initialCwd: expected.cwd } : {}),
    sessionRef: { provider: expected.provider, sessionId: expected.sessionId },
  })
}

async function openDialogAndResolve(text: string) {
  fireEvent.click(screen.getByTestId('sidebar-resume-button'))
  const input = screen.getByLabelText(/resume string/i)
  fireEvent.change(input, { target: { value: text } })
  fireEvent.keyDown(input, { key: 'Enter' })
  await act(async () => { await Promise.resolve(); await Promise.resolve() })
}

beforeEach(() => mockResolve.mockReset())
afterEach(() => cleanup())

describe('resume button end-to-end flows (spec acceptance)', () => {
  it('paste claude UUID with no hint → resolve finds claude → a claude tab opens AND navigates to terminal view', async () => {
    mockResolve.mockResolvedValue(response({ matches: [claudeMatch()] }))
    const { store, onNavigate } = renderApp()
    await openDialogAndResolve(CLAUDE_V4)
    expectResumedTab(store, { provider: 'claude', sessionId: CLAUDE_V4, cwd: '/home/u/proj' })
    expect(mockResolve).toHaveBeenCalledWith(CLAUDE_V4)
    // Sidebar convention: the resumed tab must become VISIBLE (we rendered view="tabs").
    expect(onNavigate).toHaveBeenCalledWith('terminal')
  })

  it('codex resume command → resolve finds codex → a CODEX tab opens (full id, correct provider)', async () => {
    const CODEX_V7 = '019fac27-69d7-78a0-b972-b339d551042e'
    mockResolve.mockResolvedValue(response({
      tokens: [CODEX_V7],
      agentHint: { provider: 'codex', source: 'command' },
      matches: [{ ...claudeMatch(), provider: 'codex' as const, sessionId: CODEX_V7, sessionType: 'codex', matchedToken: CODEX_V7 }],
    }))
    const { store } = renderApp()
    await openDialogAndResolve(`codex resume ${CODEX_V7}`)
    expect(mockResolve).toHaveBeenCalledWith(`codex resume ${CODEX_V7}`)
    expectResumedTab(store, { provider: 'codex', sessionId: CODEX_V7, cwd: '/home/u/proj' })
  })

  it('quoted claude --resume with picker set to codex → evidence wins → CLAUDE tab + note', async () => {
    mockResolve.mockResolvedValue(response({
      agentHint: { provider: 'claude', source: 'command' },
      matches: [claudeMatch()],
    }))
    const { store } = renderApp()
    fireEvent.click(screen.getByTestId('sidebar-resume-button'))
    fireEvent.change(screen.getByLabelText(/agent/i), { target: { value: 'codex' } })
    const input = screen.getByLabelText(/resume string/i)
    fireEvent.change(input, { target: { value: `"claude --resume ${CLAUDE_V4}"` } })
    fireEvent.keyDown(input, { key: 'Enter' })
    await act(async () => { await Promise.resolve(); await Promise.resolve() })
    expect(dialog().getByRole('status')).toHaveTextContent(/found in claude/i)
    expectResumedTab(store, { provider: 'claude', sessionId: CLAUDE_V4, cwd: '/home/u/proj' })
  })

  it('bare ses_ id with picker set to claude → evidence wins → opencode resume + note', async () => {
    const ocMatch = {
      provider: 'opencode' as const, sessionId: 'ses_root0000000000000000000000',
      cwd: '/home/u/oc', projectPath: '/home/u/oc', sessionType: 'opencode',
      title: 'oc root', lastActivityAt: 5, matchType: 'exact' as const,
      matchedToken: 'ses_root0000000000000000000000',
    }
    mockResolve.mockResolvedValue(response({ tokens: [ocMatch.sessionId], matches: [ocMatch] }))
    const { store } = renderApp()
    fireEvent.click(screen.getByTestId('sidebar-resume-button'))
    fireEvent.change(screen.getByLabelText(/agent/i), { target: { value: 'claude' } })
    const input = screen.getByLabelText(/resume string/i)
    fireEvent.change(input, { target: { value: ocMatch.sessionId } })
    fireEvent.keyDown(input, { key: 'Enter' })
    await act(async () => { await Promise.resolve(); await Promise.resolve() })
    expect(dialog().getByRole('status')).toHaveTextContent(/found in opencode/i)
    // Evidence wins over the picker: the opened tab must be the OPENCODE tuple.
    expectResumedTab(store, { provider: 'opencode', sessionId: ocMatch.sessionId, cwd: '/home/u/oc' })
  })

  it('prefix 417e8345 resolves to the amplifier session (FULL id, amplifier tuple)', async () => {
    mockResolve.mockResolvedValue(response({
      tokens: ['417e8345'],
      matches: [{ ...claudeMatch(), provider: 'amplifier' as const, sessionId: AMPLIFIER_FULL, sessionType: 'amplifier', matchType: 'prefix' as const, matchedToken: '417e8345' }],
    }))
    const { store } = renderApp()
    await openDialogAndResolve('417e8345')
    expectResumedTab(store, { provider: 'amplifier', sessionId: AMPLIFIER_FULL, cwd: '/home/u/proj' })
  })

  it('session already open in a pane → focuses it, no duplicate tab', async () => {
    mockResolve.mockResolvedValue(response({ matches: [claudeMatch()] }))
    const { store } = renderApp()
    await openDialogAndResolve(CLAUDE_V4)
    expectResumedTab(store, { provider: 'claude', sessionId: CLAUDE_V4 })
    const firstTabId = store.getState().tabs.tabs[0].id
    // Resume the SAME session again.
    cleanup()
    render(
      <Provider store={store}>
        <Sidebar view="tabs" onNavigate={vi.fn()} />
      </Provider>,
    )
    await openDialogAndResolve(CLAUDE_V4)
    const tabs = store.getState().tabs.tabs
    expect(tabs).toHaveLength(1)
    expect(store.getState().tabs.activeTabId).toBe(firstTabId)
  })

  it('garbage with no id-like token → inline error, NO tab created', async () => {
    mockResolve.mockResolvedValue(response({ tokens: [], matches: [] }))
    const { store } = renderApp()
    await openDialogAndResolve('garbage text with no ids')
    expect(dialog().getByRole('alert')).toHaveTextContent(/no session id/i)
    expect(store.getState().tabs.tabs).toHaveLength(0)
  })

  it('valid id while index warming → retry state, no error, NO tab', async () => {
    mockResolve.mockResolvedValue(response({ indexState: 'warming', matches: [] }))
    const { store } = renderApp()
    await openDialogAndResolve(CLAUDE_V4)
    expect(dialog().queryByRole('alert')).toBeNull()
    expect(dialog().getByRole('status')).toHaveTextContent(/warming/i)
    expect(store.getState().tabs.tabs).toHaveLength(0)
  })
})
```

- [ ] **Step 2: Run tests — expect them to pass (integration of finished parts) or reveal real wiring bugs**

```bash
npm run test:vitest -- --config config/vitest/vitest.config.ts test/e2e/resume-button-flow.test.tsx --run
```
Expected: PASS. If the "already open" test fails because `openSessionTab` created a second tab, that is a REAL bug in the Task 8 wiring (e.g. sessionType mismatch in the dedup locator) — fix the wiring, not the test. If store setup fails to render, mirror the `createStore` helper from `test/e2e/sidebar-click-opens-pane.test.tsx` (lines 68–147) for the missing slices and re-run.

- [ ] **Step 3: Commit**

```bash
git add test/e2e/resume-button-flow.test.tsx
git commit -m "$(cat <<'EOF'
test(e2e): resume-button acceptance flows per spec table

🤖 Generated with [Amplifier](https://github.com/microsoft/amplifier)

Co-Authored-By: Amplifier <240397093+microsoft-amplifier@users.noreply.github.com>
EOF
)"
```

---

### Task 10: docs/index.html feature mention + full verification

**Files:**
- Modify: `docs/index.html` (feature list — a new always-visible rail button is a major UI change per repo rules)

**Interfaces:**
- Consumes: the shipped feature set from Tasks 2–9.
- Produces: updated marketing/docs page; final green verification of the whole branch.

- [ ] **Step 1: Add the feature bullet to `docs/index.html`**

Locate the existing feature list (search the file for the sessions/sidebar feature copy, e.g. `grep -n -i "session" docs/index.html | head -20`, then read that section). Add one list item matching the exact sibling markup style (same tag/classes as neighboring feature items), with this copy:

> **Resume any session by ID** — paste a session id (or a whole `codex resume …` command) into the sidebar's Resume button and freshell finds it across Claude Code, Codex, OpenCode, and Amplifier and reopens it in a tab.

Verify the page still renders: open it with `python3 -m http.server 0 --directory docs` is NOT needed — a static check suffices: `node -e "require('fs').readFileSync('docs/index.html','utf8')" && npx --yes html-validate docs/index.html || true` (advisory only; match existing formatting by eye/diff).

- [ ] **Step 2: Full verification sweep**

```bash
npm run test:status        # respect coordinated-run discipline before broad runs
npm run typecheck
npm run lint
npm run test:vitest -- --config config/vitest/vitest.config.ts test/unit/shared/resume-input-parser.test.ts test/unit/client/resume-session-dialog.test.tsx test/unit/client/sidebar-resume-footer.test.tsx test/e2e/resume-button-flow.test.tsx --run
npm run test:vitest -- --config config/vitest/vitest.server.config.ts test/unit/server/coding-cli/session-resolver.test.ts test/unit/server/coding-cli/claude-transcript-locator.test.ts test/unit/server/coding-cli/opencode-by-id-query.test.ts test/unit/server/sessions-resolve-router.test.ts --run
```
Expected: all PASS, no type or lint errors. Then, if `npm run test:status` reports the coordinator is free, run the standard suites for a regression sweep:
```bash
npm run test:balanced
```
Expected: green (matching the base-green baseline). SAFETY: none of these commands may target ports 3001/3002 or any process you did not spawn.

- [ ] **Step 3: Commit**

```bash
git add docs/index.html
git commit -m "$(cat <<'EOF'
docs: add resume-by-id feature to docs/index.html

🤖 Generated with [Amplifier](https://github.com/microsoft/amplifier)

Co-Authored-By: Amplifier <240397093+microsoft-amplifier@users.noreply.github.com>
EOF
)"
```

Do NOT open a PR — prepare the branch and stop (explicit user approval required per AGENTS.md).
