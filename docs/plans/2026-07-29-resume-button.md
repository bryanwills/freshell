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
- Repo: TDD red-green-refactor for every unit of new behavior: each test file is WRITTEN and RUN RED before its implementation exists (Tasks 2–8 order their steps this way — do not reorder), and every implementation task ends with an explicit REFACTOR pass (with tests green, review the new code for duplication/naming/structure, apply safe improvements, re-run that task's tests) before its commit step. Task 9's acceptance flows are integration gates over already-TDD'd units and are expected green. Run tests via the coordinated wrapper `npm run test:vitest -- --config <config> <files> --run` (check `npm run test:status` before broad runs; broad runs WAIT for the coordinator gate — never skip, never kill a foreign holder).
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
- Modify: `server/coding-cli/session-indexer.ts` (add public methods to `CodingCliSessionIndexer`, class starts ~line 442; `initialized` is an existing private field)
- Modify: `server/coding-cli/providers/claude.ts`, `server/coding-cli/providers/codex.ts`, `server/coding-cli/providers/amplifier.ts` (root-level scan failures must PROPAGATE — see Step 4; without this the provider-health channel cannot observe real filesystem failures for these three providers, because they currently swallow ALL errors into empty results)
- Create: `server/coding-cli/session-resolver.ts`
- Test: `test/unit/server/coding-cli/session-resolver.test.ts`
- Test: modify `test/unit/server/coding-cli/session-indexer.test.ts` (existing file — readiness + scan-failure cases)
- Test: root-failure cases for the three providers (add to their existing test files if present — check `ls test/unit/server/coding-cli` — else create `test/unit/server/coding-cli/provider-root-failures.test.ts`)

**Interfaces:**
- Consumes: `ProjectGroup`, `CodingCliSession`, `CodingCliProviderName` from `server/coding-cli/types.js`; indexer snapshot shape `getProjects(): ProjectGroup[]` (already public — see `SessionsRouterDeps.codingCliIndexer`).
- Produces (used by Tasks 4–6):
  - `CodingCliSessionIndexer.isReady(): boolean` — true once at least one full `refresh()` has completed (Step 4 sets `initialized` at the end of `refresh()`, so this is observable without `start()`)
  - `CodingCliSessionIndexer.getScanFailures(): CodingCliProviderName[]` — currently-ENABLED providers whose most recent listing attempt FAILED (the indexer currently swallows these into empty lists and still reports itself initialized; the resolve route must surface them as degraded, never as "not found"; a DISABLED provider is pruned from this set — it is UNSEARCHED, not failed)
  - `CodingCliSessionIndexer.requestRefresh(): void` — public fire-and-forget wrapper around the private `scheduleRefresh()`; the resolve route calls it on a degraded response so a user "Retry" converges once the failed provider recovers
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

  it('an EXACT id finds a subagent/child session (spec: scan ALL sessions)', async () => {
    const snapshot = projects([
      session({ provider: 'claude', sessionId: CLAUDE_V4, isSubagent: true }),
    ])
    const { matches } = await resolveSessionCandidates([CLAUDE_V4], { getProjects: () => snapshot })
    expect(matches).toHaveLength(1)
    expect(matches[0].sessionId).toBe(CLAUDE_V4)
  })

  it('prefix DISCOVERY does not surface subagent sessions', async () => {
    const snapshot = projects([
      session({ provider: 'claude', sessionId: CLAUDE_V4, isSubagent: true }),
    ])
    const { matches } = await resolveSessionCandidates(['ed2afda6'], { getProjects: () => snapshot })
    expect(matches).toHaveLength(0)
  })

  it('an exact FALLBACK hit beats an indexed PREFIX match of the same token', async () => {
    // Token exactly equals an unindexed session id AND is a prefix of an
    // indexed one: exact must win or the wrong session gets resumed.
    const indexedPrefix = projects([
      session({ provider: 'amplifier', sessionId: `${CLAUDE_V4}9999`, lastActivityAt: 999 }),
    ])
    const fallbackMatch: ResolveMatch = {
      provider: 'claude', sessionId: CLAUDE_V4, cwd: '/tmp/exact', projectPath: '/tmp/exact',
      sessionType: 'claude', lastActivityAt: 1, matchType: 'exact', matchedToken: CLAUDE_V4,
    }
    const { matches } = await resolveSessionCandidates([CLAUDE_V4], {
      getProjects: () => indexedPrefix,
      fallbacks: { claudeTranscriptById: async (id) => (id === CLAUDE_V4 ? fallbackMatch : null) },
    })
    expect(matches).toEqual([fallbackMatch])
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

  it('a failed exact-id fallback does NOT hide a later lower-priority match — BOTH the match and the provider error are returned', async () => {
    // First token: full claude UUID, index miss, claude fallback THROWS.
    // Second token: prefix that matches an indexed amplifier session.
    // The caller (route) needs the surviving match AND the error so it can
    // report 'degraded' and the client can refuse to auto-resume.
    const MISSING_V4 = 'ed2afda6-a340-443e-ba60-024a1b3554b9'
    const snapshot = projects([session({ provider: 'amplifier', sessionId: '417e8345-90ab-4cde-8f01-234567890abc' })])
    const { matches, providerErrors } = await resolveSessionCandidates([MISSING_V4, '417e8345'], {
      getProjects: () => snapshot,
      fallbacks: { claudeTranscriptById: async () => { throw new Error('EACCES') } },
    })
    expect(matches).toHaveLength(1)
    expect(matches[0]).toMatchObject({ provider: 'amplifier', matchType: 'prefix' })
    expect(providerErrors).toEqual(['claude'])
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
```

- [ ] **Step 4: Add `isReady()` and scan-failure tracking to the indexer**

In `server/coding-cli/session-indexer.ts`, inside `class CodingCliSessionIndexer`, directly ABOVE the existing `onUpdate(...)` method (~line 687), add:

```typescript
  /** True once at least one refresh has completed (resolve endpoint: 'warming' until then). */
  isReady(): boolean {
    return this.initialized
  }

  /**
   * Providers whose MOST RECENT listing attempt failed. The scan path swallows
   * these failures into empty lists and still reports the index initialized,
   * so without this the resolve endpoint would report 'ready' + zero matches
   * (i.e. "not found") during a provider outage — spec violation.
   */
  getScanFailures(): CodingCliProviderName[] {
    return [...this.scanFailures]
  }

  /**
   * Public fire-and-forget refresh request (debounced scheduleRefresh). The
   * resolve route calls this on a degraded response so the user's "Retry"
   * converges after a failed provider recovers — otherwise a recorded scan
   * failure could outlive the outage until the next watcher event/periodic
   * full scan.
   */
  requestRefresh(): void {
    this.scheduleRefresh()
  }
```

Make readiness observable without `start()`: at the END of a successful `refresh()` pass (after the snapshot swap), set `this.initialized = true`. `start()` already sets it after its initial refresh — this is idempotent, and it makes `isReady()` mean "at least one refresh completed", which is exactly what the resolve endpoint needs. (Tests then use `refresh()` directly — do NOT use `start()` in unit tests: it installs watchers and timers that need cleanup.)

Add the backing field next to the existing `private initialized = false` (~line 463):

```typescript
  private scanFailures = new Set<CodingCliProviderName>()
```

Then wire per-provider success/failure recording into EVERY place the refresh path converts a provider listing failure into an empty result. Verified: the file-based catch is `provider.listSessionFiles().catch((err) => { logger.warn(...); return [] })` at ~line 1432 — change it to also `this.scanFailures.add(provider.name)` in the catch, and `this.scanFailures.delete(provider.name)` on success. Audit and instrument the SAME way: the equivalent listing catch(es) inside `lightweightScan` (cold-start path) and any failure handling in `refreshDirectProvider` (direct-listing providers). Record per attempt (add on failure, delete on success) — do NOT bulk-clear the set per refresh, or an untouched failed provider would be wrongly reported healthy after an incremental refresh.

DISABLED-provider lifecycle (must not trap the user): at the end of each refresh pass, PRUNE `scanFailures` entries for providers that are no longer in the enabled/scanned provider set (the same `enabledProviders` filter the scan already applies, ~line 1385). A disabled provider is UNSEARCHED, not failed — the resolve route reports it via `unsearchedProviders`; leaving its stale failure in `scanFailures` would keep responses `degraded` forever with a Retry that can never succeed.

MAKE THE FAILURES OBSERVABLE — provider root-error propagation (without this, the instrumentation above records nothing for three of the four providers): the indexer's catches can only fire if `listSessionFiles()` actually REJECTS, but today `claude.ts` (~lines 541–563), `codex.ts` (`walkJsonlFiles`, ~lines 433–439) and `amplifier.ts` (`walkMetadataFiles`, ~lines 142–148) swallow ALL directory/stat errors into empty results, so EACCES/EIO/EMFILE outages read as successful empty scans → false "not found". Fix each provider the same way:
- Distinguish absence from failure: treat `ENOENT`/`ENOTDIR` on the provider's ROOT directory (claude: `<home>/projects`; codex/amplifier: the walker's top-level root) as a legitimate empty result; RETHROW any other error from the root-level enumeration so `listSessionFiles()` rejects and the indexer records the scan failure. For the recursive walkers, add the root-level guard in `listSessionFiles` itself (readdir the root outside the walker, absence → `[]`, other errors → throw), keeping the recursive descent best-effort.
- Per-subdirectory/per-file errors deeper in the tree remain best-effort skips (partial results beat none) — document this as an accepted limitation in a code comment at each site.
- (The opencode provider's listing already runs through the worker runner whose failures reject — verify, do not change.)

Tests:
- Add to the EXISTING `test/unit/server/coding-cli/session-indexer.test.ts` (reuse its harness/provider stubs): (a) a provider whose `listSessionFiles` rejects → after `refresh()`, `getScanFailures()` contains that provider's name and `isReady()` is true (readiness comes from the completed refresh — see above); (b) after the stub is repaired and `refresh()` runs again, `getScanFailures()` is empty; (c) a provider fails, then is REMOVED from the enabled provider set → after the next `refresh()`, `getScanFailures()` no longer contains it (pruned, not trapped); (d) `requestRefresh()` schedules a refresh (spy on the scheduled path or observe a subsequent snapshot change with fake timers).
- Provider root-failure tests (real fs, throwaway tmp dirs only — never a real HOME): for each of claude/codex/amplifier, (a) missing root → `listSessionFiles()` resolves `[]`; (b) root with mode `0o000` → `listSessionFiles()` REJECTS with `EACCES` (guard with `it.skipIf(process.getuid?.() === 0)` since root bypasses permissions; restore the mode in `afterEach` so cleanup works).

- [ ] **Step 5: Run tests to verify they pass**

```bash
npm run test:vitest -- --config config/vitest/vitest.server.config.ts test/unit/server/coding-cli/session-resolver.test.ts test/unit/server/coding-cli/session-indexer.test.ts --run
npm run typecheck:server
```
Expected: PASS, clean typecheck. (Also run the provider root-failure test file(s) from Step 4 with the same wrapper — include whichever file(s) you added them to.)

- [ ] **Step 6: Refactor pass, then commit**

Refactor (TDD's third step): with tests green, re-read the new resolver, the indexer additions, and the provider root-guard changes for duplication/naming/structure (e.g. the three identical root-guard blocks may deserve a tiny shared helper); apply safe improvements and re-run the Step 5 commands.

```bash
git add server/coding-cli/session-resolver.ts server/coding-cli/session-indexer.ts server/coding-cli/providers/claude.ts server/coding-cli/providers/codex.ts server/coding-cli/providers/amplifier.ts test/unit/server/coding-cli/session-resolver.test.ts test/unit/server/coding-cli/session-indexer.test.ts
# ALSO add the provider root-failure test file(s) touched/created in Step 4.
git commit -m "$(cat <<'EOF'
feat(server): cross-provider session resolver + indexer readiness/scan-failure channel

- session-resolver over the index snapshot (exact > fallback > prefix, provider
  errors surface alongside surviving matches)
- indexer: isReady() (set on refresh completion), getScanFailures() with
  disabled-provider pruning, requestRefresh()
- claude/codex/amplifier providers: root-level scan failures now REJECT instead
  of reading as empty results, so the health channel covers all four providers

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

  it('PROPAGATES non-absence failures (EACCES) instead of swallowing them as a miss', async () => {
    // Provider failure ≠ not found: an unreadable root must reject so the
    // resolver records providerErrors → 'degraded', never "no matching session".
    // chmod-based: CI/dev runs unprivileged (root would bypass the mode bits).
    const lockedRoot = path.join(root, 'locked')
    await fsp.mkdir(lockedRoot, { recursive: true })
    await fsp.chmod(lockedRoot, 0o000)
    try {
      await expect(locateClaudeTranscriptById(SESSION_ID, [lockedRoot])).rejects.toThrow()
    } finally {
      await fsp.chmod(lockedRoot, 0o700)
    }
  })

  it('still returns the hit when no line has a cwd', async () => {
    await writeTranscript('-x', SESSION_ID, [JSON.stringify({ type: 'summary' })])
    const hit = await locateClaudeTranscriptById(SESSION_ID, [root])
    expect(hit).not.toBeNull()
    expect(hit!.cwd).toBeUndefined()
  })

  it('finds a SUBAGENT transcript at <project>/<parent>/subagents/<id>.jsonl (index-missed child session)', async () => {
    // Claude also stores child-session transcripts one level deeper (see
    // claude.ts listSessionFiles): the exact-id contract covers them too.
    const PARENT_ID = 'aaaaaaaa-a340-443e-ba60-024a1b3554b4'
    const dir = path.join(root, '-home-u-proj', PARENT_ID, 'subagents')
    await fsp.mkdir(dir, { recursive: true })
    await fsp.writeFile(path.join(dir, `${SESSION_ID}.jsonl`), JSON.stringify({ cwd: '/home/u/proj' }) + '\n')
    const hit = await locateClaudeTranscriptById(SESSION_ID, [root])
    expect(hit).not.toBeNull()
    expect(hit!.cwd).toBe('/home/u/proj')
    expect(hit!.filePath.endsWith(path.join('subagents', `${SESSION_ID}.jsonl`))).toBe(true)
  })

  it('prefers the direct layout when both layouts contain the id (pass ordering)', async () => {
    await writeTranscript('-x', SESSION_ID, [JSON.stringify({ cwd: '/direct' })])
    const dir = path.join(root, '-x', 'aaaaaaaa-a340-443e-ba60-024a1b3554b4', 'subagents')
    await fsp.mkdir(dir, { recursive: true })
    await fsp.writeFile(path.join(dir, `${SESSION_ID}.jsonl`), JSON.stringify({ cwd: '/sub' }) + '\n')
    const hit = await locateClaudeTranscriptById(SESSION_ID, [root])
    expect(hit!.cwd).toBe('/direct')
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

/** Expected-absence errors are misses; EVERYTHING else is a provider failure. */
function isAbsenceError(err: unknown): boolean {
  const code = (err as NodeJS.ErrnoException | null)?.code
  return code === 'ENOENT' || code === 'ENOTDIR'
}

/**
 * Exact-id fallback for claude sessions the index missed. Claude stores
 * transcripts in TWO layouts (both indexed by the provider — see claude.ts
 * listSessionFiles ~line 571):
 *   1. direct:   <root>/<project-dir>/<sessionId>.jsonl
 *   2. subagent: <root>/<project-dir>/<parent-session>/subagents/<sessionId>.jsonl
 * An exact pasted id must resolve for BOTH — child sessions included — so the
 * locator probes the cheap direct layout first (one readdir per root + one
 * stat per project dir) and only on a total miss falls back to the subagent
 * layout (one readdir per project dir + one stat per session subdirectory).
 *
 * ERROR CONTRACT: expected absence (ENOENT/ENOTDIR — missing root, missing
 * transcript, non-directory entries probed as directories) is a miss (null).
 * Any OTHER failure (EACCES, EMFILE, EIO, …) PROPAGATES: the resolver records
 * it as a provider error and the route reports 'degraded' — a provider
 * failure must never read as "not found".
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
  // PASS 1 — direct layout.
  for (const root of roots) {
    for (const dir of await readdirOrEmpty(root)) {
      const hit = await probeTranscript(path.join(root, dir, `${id}.jsonl`), id)
      if (hit) return hit
    }
  }
  // PASS 2 — subagent layout (only when the direct layout missed everywhere).
  for (const root of roots) {
    for (const dir of await readdirOrEmpty(root)) {
      const projectDir = path.join(root, dir)
      for (const entry of await readdirOrEmpty(projectDir)) {
        const hit = await probeTranscript(path.join(projectDir, entry, 'subagents', `${id}.jsonl`), id)
        if (hit) return hit
      }
    }
  }
  return null
}

/** readdir treating absence as empty; anything else PROPAGATES (provider failure). */
async function readdirOrEmpty(dir: string): Promise<string[]> {
  try {
    return await fsp.readdir(dir)
  } catch (err) {
    if (isAbsenceError(err)) return []
    throw err
  }
}

/** stat + cwd-read for one candidate; absence = null, anything else PROPAGATES. */
async function probeTranscript(candidate: string, id: string): Promise<ClaudeTranscriptHit | null> {
  let stat
  try {
    stat = await fsp.stat(candidate)
  } catch (err) {
    if (isAbsenceError(err)) return null
    throw err
  }
  const cwd = await readCwdFromTranscriptHead(candidate)
  return { sessionId: id, cwd, filePath: candidate, lastActivityAt: stat.mtimeMs }
}

async function readCwdFromTranscriptHead(filePath: string): Promise<string | undefined> {
  let handle
  try {
    handle = await fsp.open(filePath, 'r')
  } catch (err) {
    // The file existed a moment ago (stat succeeded): absence = raced
    // deletion (miss the cwd only); anything else is a provider failure.
    if (isAbsenceError(err)) return undefined
    throw err
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
- Create: `server/coding-cli/providers/opencode-by-id.worker.ts`
- Create: `server/coding-cli/providers/opencode-by-id-runner.ts`
- Test: `test/unit/server/coding-cli/opencode-by-id-query.test.ts`
- Test: `test/unit/server/coding-cli/opencode-by-id-runner.test.ts`

**Interfaces:**
- Consumes: `node:sqlite` (lazy `await import` — same pattern and reason as `opencode-listing-query.ts`); `OpencodeSessionRow` type from `./opencode-listing-query.js`; the worker/runner pattern from `./opencode-listing-runner.ts` + `./opencode-listing.worker.ts`.
- Produces:
  - `runOpencodeSessionByIdQuery(dbPath: string, sessionId: string): Promise<OpencodeSessionRow | null>` — WORKER-SIDE synchronous-sqlite implementation. Production code must never call it on the main thread: `DatabaseSync` blocks the event loop (the listing query was moved to a worker for exactly this reason at ~180 ms; a locked DB would block for the full busy timeout).
  - `runOpencodeSessionByIdOffThread(dbPath: string, sessionId: string): Promise<OpencodeSessionRow | null>` from the runner — what Task 6 wraps into an `ExactIdFallback` using `OpencodeProvider.getDatabasePath()` (`<opencode-data>/opencode.db`). Worker/spawn failures REJECT (provider unavailable ≠ not found).

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
 * a locked DB must fail fast (the failure is surfaced as provider-unavailable,
 * NOT "not found"). This synchronous function runs INSIDE THE WORKER THREAD
 * (opencode-by-id.worker.ts) — same rule as the listing query, which was moved
 * off the event loop even at ~180 ms. Never call it on the main thread in
 * production: `DatabaseSync` blocks whatever thread runs it for up to this
 * timeout when the DB is locked.
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

- [ ] **Step 4: Off-thread worker + runner (event-loop safety) — tests FIRST (red), then implement (green)**

**Step 4a (RED):** Create `test/unit/server/coding-cli/opencode-by-id-runner.test.ts` FIRST, by mirroring the existing `test/unit/server/coding-cli/opencode-listing-runner.test.ts` (same injectable-spawn harness): ok-row message resolves; ok-null resolves null; err message rejects; malformed/truncated message rejects; timeout terminates the worker and rejects. Add ONE integration case that runs the REAL worker against the Step 1 tmp-DB fixture and resolves the root session (proves the off-thread wiring end to end; tmp DB only — never a real HOME, session safety rule). Then run it and verify it FAILS:

```bash
npm run test:vitest -- --config config/vitest/vitest.server.config.ts test/unit/server/coding-cli/opencode-by-id-runner.test.ts --run
```
Expected: FAIL — runner module not found.

**Step 4b (GREEN):** Create `server/coding-cli/providers/opencode-by-id.worker.ts` and `server/coding-cli/providers/opencode-by-id-runner.ts` by MIRRORING the existing pair `opencode-listing.worker.ts` / `opencode-listing-runner.ts` in the same directory (read both first — they are the repo's canonical off-thread sqlite pattern). Keep every load-bearing detail of that pattern:
- sentinel-guarded auto-run in the worker module (importing it on the main thread must never spawn/post anything),
- `SELF_EXT` sibling resolution (`.ts` in dev/test via tsx, `.js` in compiled dist),
- `execArgv` APPENDED to `process.execArgv` with `--disable-warning=ExperimentalWarning`,
- FULL-shape message validation (a truncated `{ ok: true }` must reject, not resolve garbage),
- hard per-query timeout (listing uses 15 s; keep the same default) with worker termination,
- injectable `spawn` for unit tests.

The worker calls `runOpencodeSessionByIdQuery(dbPath, sessionId)` (Step 3) and posts `{ ok: true, row: OpencodeSessionRow | null }`; errors post `{ ok: false, error: { name, message } }`. The runner exports:

```typescript
export function createWorkerByIdRunner(options?: CreateWorkerByIdRunnerOptions): (input: { dbPath: string; sessionId: string }) => Promise<OpencodeSessionRow | null>
/** Default production runner: one short-lived worker per lookup, hard timeout. */
export async function runOpencodeSessionByIdOffThread(dbPath: string, sessionId: string): Promise<OpencodeSessionRow | null>
```

Worker/spawn/timeout failures REJECT (provider unavailable ≠ not found — Task 6's fallback lets that propagate). Per-request cost is bounded upstream (shape gate + `FALLBACK_BUDGET_PER_REQUEST`), so worst case is two short-lived workers per request — and the EVENT LOOP stays free even when the DB is locked for the full 500 ms busy timeout.

- [ ] **Step 5: Run tests to verify they pass, then refactor pass**

Refactor (TDD's third step): with tests green, compare the new worker/runner pair against the listing pair for drift and factor out any safe shared helpers; re-run the commands below after any change.

```bash
npm run test:vitest -- --config config/vitest/vitest.server.config.ts test/unit/server/coding-cli/opencode-by-id-query.test.ts test/unit/server/coding-cli/opencode-by-id-runner.test.ts --run
npm run typecheck:server
```
Expected: PASS, clean typecheck.

- [ ] **Step 6: Commit**

```bash
git add server/coding-cli/providers/opencode-by-id-query.ts server/coding-cli/providers/opencode-by-id.worker.ts server/coding-cli/providers/opencode-by-id-runner.ts test/unit/server/coding-cli/opencode-by-id-query.test.ts test/unit/server/coding-cli/opencode-by-id-runner.test.ts
git commit -m "$(cat <<'EOF'
feat(server): opencode session-by-id DB query + off-thread worker runner for resolve fallback

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
- Test: `test/unit/server/coding-cli/resolve-fallbacks.test.ts`
- Test: `test/unit/server/sessions-resolve-router.test.ts`

**Interfaces:**
- Consumes: `parseResumeInput` from `../shared/resume-input-parser.js` (Task 2); `resolveSessionCandidates`, `ResolveMatch`, `ExactIdFallback` from `./coding-cli/session-resolver.js` (Task 3); `locateClaudeTranscriptById` (Task 4); `runOpencodeSessionByIdOffThread` (Task 5); `CodingCliProvider` interface (`getSessionRoots()`, `name`); `OpencodeProvider.getDatabasePath()`; the EXISTING `SessionsRouterDeps.sessionMetadataStore` (`SessionMetadataStore.getAll()` returns entries keyed `provider:sessionId`, each optionally carrying `sessionType` — the same source of truth the indexer uses to route freshclaude/freshopencode/kilroy sessions to the right pane runtime).
- Produces: HTTP contract used by the client (Task 7):
  - Request: `POST /api/sessions/resolve` body `{ input: string }` (1–20000 chars; 400 on invalid body; candidate tokens are already capped by `MAX_RESUME_CANDIDATES` in the parser).
  - Response 200: `{ indexState: 'ready' | 'warming' | 'degraded', tokens: string[], agentHint: { provider, source } | null, homeDir: string, providerErrors: CodingCliProviderName[], unsearchedProviders: CodingCliProviderName[], matches: ResolveMatch[] }`.
    - `providerErrors` = ENABLED providers that could NOT be searched because something FAILED: an exact-id fallback threw, or the provider's most recent index scan failed (`indexer.getScanFailures()`, filtered to enabled providers — a DISABLED provider belongs in `unsearchedProviders`, never here, or a failed-then-disabled provider would trap the user in a permanent degraded state).
    - `degraded` = `providerErrors` non-empty, EVEN WHEN matches exist: a failed provider means a higher-priority exact match may have been missed, so a surviving lower-priority/prefix match must NOT auto-resume (client rule) — and zero matches must show retry, never "no matching session". A degraded response also fire-and-forgets `indexer.requestRefresh()` so the user's Retry converges once the provider recovers.
    - `unsearchedProviders` = providers DISABLED in settings. Verified: `session-indexer.ts` filters the scan by `settings.codingCli.enabledProviders` (~line 1385), so a disabled provider's sessions are ABSENT from `getProjects()` and cannot be found — the client must say so instead of implying the id does not exist. (The claude/opencode exact-id fallbacks do not consult the index, so exact pasted ids for those two resolve even while disabled.)
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
const CODEX_V7 = '019fac27-69d7-78a0-b972-b339d551042e'
const OPENCODE_ID = 'ses_root0000000000000000000000'

// ALL FOUR providers — the endpoint's core claim is one scan answers every
// agent, so the fixture must be able to falsify a provider-specific miss.
function snapshot(): ProjectGroup[] {
  return [{
    projectPath: '/home/u/proj',
    sessions: [
      {
        provider: 'claude', sessionId: CLAUDE_V4, projectPath: '/home/u/proj',
        lastActivityAt: 111, cwd: '/home/u/proj', title: 'claude one', sessionType: 'claude',
      },
      {
        provider: 'codex', sessionId: CODEX_V7, projectPath: '/home/u/proj',
        lastActivityAt: 333, cwd: '/home/u/proj', title: 'codex one', sessionType: 'codex',
      },
      {
        provider: 'opencode', sessionId: OPENCODE_ID, projectPath: '/home/u/proj',
        lastActivityAt: 444, cwd: '/home/u/proj', title: 'oc one', sessionType: 'opencode',
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

  it('resolves an exact CODEX UUID from the snapshot', async () => {
    const res = await request(makeApp()).post('/api/sessions/resolve').send({ input: CODEX_V7 })
    expect(res.body.matches).toHaveLength(1)
    expect(res.body.matches[0]).toMatchObject({
      provider: 'codex', sessionId: CODEX_V7, sessionType: 'codex', matchType: 'exact',
    })
  })

  it('resolves an exact OPENCODE ses_ id from the snapshot', async () => {
    const res = await request(makeApp()).post('/api/sessions/resolve').send({ input: OPENCODE_ID })
    expect(res.body.matches).toHaveLength(1)
    expect(res.body.matches[0]).toMatchObject({
      provider: 'opencode', sessionId: OPENCODE_ID, sessionType: 'opencode', matchType: 'exact',
    })
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

  it('bounds per-request fallback work to FALLBACK_BUDGET_PER_REQUEST — and wrong-shape tokens do NOT consume it', async () => {
    const fallback = vi.fn().mockResolvedValue(null)
    await request(makeApp({
      codingCliIndexer: { getProjects: () => [], refresh: async () => {}, isReady: () => true },
      resolveFallbacks: { claudeTranscriptById: fallback },
    })).post('/api/sessions/resolve').send({
      // Parser emits prefixed ids BEFORE UUIDs: if no-op wrong-shape calls
      // counted against the claude budget, the two ses_ tokens would exhaust
      // it and the valid claude UUIDs would never be probed (false negative).
      input: [
        'ses_aaaaaaaaaaaaaaaaaaaaaaaaaa',
        'ses_bbbbbbbbbbbbbbbbbbbbbbbbbb',
        'ed2afda6-a340-443e-ba60-024a1b3554b1',
        'ed2afda6-a340-443e-ba60-024a1b3554b2',
        'ed2afda6-a340-443e-ba60-024a1b3554b3',
      ].join(' '),
    })
    // Exactly the first FALLBACK_BUDGET_PER_REQUEST UUID-shaped tokens reach
    // the claude fallback; ses_ tokens are shape-gated out before the budget.
    expect(fallback.mock.calls.map((c) => c[0])).toEqual([
      'ed2afda6-a340-443e-ba60-024a1b3554b1',
      'ed2afda6-a340-443e-ba60-024a1b3554b2',
    ].slice(0, FALLBACK_BUDGET_PER_REQUEST))
  })

  it('reports degraded when a provider SCAN failed (indexer.getScanFailures), even with no fallback error', async () => {
    const res = await request(makeApp({
      codingCliIndexer: {
        getProjects: () => [], refresh: async () => {}, isReady: () => true,
        getScanFailures: () => ['codex'],
      },
    })).post('/api/sessions/resolve').send({ input: CODEX_V7 })
    expect(res.status).toBe(200)
    expect(res.body.indexState).toBe('degraded')
    expect(res.body.providerErrors).toEqual(['codex'])
    expect(res.body.matches).toEqual([])
  })

  it('reports degraded EVEN WITH matches when a higher-priority exact-id fallback failed (client must not auto-resume a lower-priority match)', async () => {
    // Unindexed full claude UUID (fallback throws) + a second token that
    // prefix-matches the indexed amplifier session: the match survives, but
    // the response must be degraded so the client refuses to auto-resume.
    const MISSING_V4 = 'ed2afda6-a340-443e-ba60-024a1b3554b9'
    const res = await request(makeApp({
      resolveFallbacks: { claudeTranscriptById: async () => { throw new Error('EACCES') } },
    })).post('/api/sessions/resolve').send({ input: `${MISSING_V4} 417e8345` })
    expect(res.status).toBe(200)
    expect(res.body.indexState).toBe('degraded')
    expect(res.body.providerErrors).toEqual(['claude'])
    expect(res.body.matches).toHaveLength(1)
    expect(res.body.matches[0].provider).toBe('amplifier')
  })

  it('a degraded response fire-and-forgets indexer.requestRefresh() so Retry can converge', async () => {
    const requestRefresh = vi.fn()
    await request(makeApp({
      codingCliIndexer: {
        getProjects: () => [], refresh: async () => {}, isReady: () => true,
        getScanFailures: () => ['codex'], requestRefresh,
      },
    })).post('/api/sessions/resolve').send({ input: CODEX_V7 })
    expect(requestRefresh).toHaveBeenCalled()
  })

  it('a scan failure for a DISABLED provider is excluded from providerErrors (unsearched, not degraded)', async () => {
    const res = await request(makeApp({
      configStore: {
        getSettings: async () => ({ codingCli: { enabledProviders: ['claude'] } }),
        patchSessionOverride: async () => ({}),
        deleteSession: async () => {},
      },
      codingCliIndexer: {
        getProjects: () => snapshot(), refresh: async () => {}, isReady: () => true,
        getScanFailures: () => ['codex'],
      },
    })).post('/api/sessions/resolve').send({ input: CLAUDE_V4 })
    expect(res.body.providerErrors).toEqual([])
    expect(res.body.unsearchedProviders).toEqual(['codex', 'opencode', 'amplifier'])
    expect(res.body.indexState).toBe('ready')
  })

  it('reports DISABLED providers as unsearched, never silently as absence', async () => {
    const res = await request(makeApp({
      configStore: {
        getSettings: async () => ({ codingCli: { enabledProviders: ['claude', 'opencode'] } }),
        patchSessionOverride: async () => ({}),
        deleteSession: async () => {},
      },
    })).post('/api/sessions/resolve').send({ input: CLAUDE_V4 })
    expect(res.body.unsearchedProviders).toEqual(['codex', 'amplifier'])
  })

  it('400s on a missing/invalid body', async () => {
    const res = await request(makeApp()).post('/api/sessions/resolve').send({})
    expect(res.status).toBe(400)
  })
})
```

ALSO create `test/unit/server/coding-cli/resolve-fallbacks.test.ts` NOW, before any implementation exists (TDD red — its full case list is specified in Step 5, which later verifies it green): both test files must fail before Step 3.

- [ ] **Step 2: Run tests to verify they fail**

```bash
npm run test:vitest -- --config config/vitest/vitest.server.config.ts test/unit/server/sessions-resolve-router.test.ts test/unit/server/coding-cli/resolve-fallbacks.test.ts --run
```
Expected: FAIL — TS error on unknown deps (`homeDir`, `resolveFallbacks`, `isReady`) and/or 404 on the route; `resolve-fallbacks` module not found.

- [ ] **Step 3: Implement the default fallback wiring**

Create `server/coding-cli/resolve-fallbacks.ts`:

```typescript
import type { CodingCliProvider } from './provider.js'
import type { ExactIdFallback, ResolveMatch } from './session-resolver.js'
import type { SessionMetadataEntry } from '../session-metadata-store.js'
import { locateClaudeTranscriptById } from './claude-transcript-locator.js'
import { runOpencodeSessionByIdOffThread } from './providers/opencode-by-id-runner.js'
import type { OpencodeSessionRow } from './providers/opencode-listing-query.js'

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
 * Combined with MAX_RESUME_CANDIDATES and the shape gates this bounds the
 * fallback work (FS probes, worker spawns) one request can trigger.
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

  const claudeTranscriptById: ExactIdFallback | undefined = claude
    ? async (id): Promise<ResolveMatch | null> => {
        // Non-absence FS failures PROPAGATE from the locator (provider
        // unavailable ≠ not found — resolver records providerErrors).
        const hit = await locateClaudeTranscriptById(id, claude.getSessionRoots())
        if (!hit) return null
        return {
          provider: 'claude',
          sessionId: hit.sessionId,
          // cwd may legitimately be missing here — the CLIENT must then ask
          // for a working directory instead of auto-opening (Task 7).
          cwd: hit.cwd,
          projectPath: hit.cwd ?? '',
          sessionType: await sessionTypeFor('claude', hit.sessionId),
          lastActivityAt: hit.lastActivityAt,
          matchType: 'exact',
          matchedToken: id,
        }
      }
    : undefined

  const opencodeSessionById: ExactIdFallback | undefined = opencode?.getDatabasePath
    ? async (id): Promise<ResolveMatch | null> => {
        // NO catch here: a locked/corrupt DB or worker failure must PROPAGATE
        // so the resolver records provider-unavailable (degraded), never
        // "not found". Runs OFF the event loop (Task 5 worker runner).
        const row = await runById(opencode.getDatabasePath!(), id)
        if (!row) return null
        return {
          provider: 'opencode',
          sessionId: row.sessionId,
          cwd: row.cwd || undefined,
          projectPath: row.projectPath ?? row.cwd ?? '',
          sessionType: await sessionTypeFor('opencode', row.sessionId),
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

Extend `SessionsRouterDeps`: inside the existing `codingCliIndexer` object type add `isReady?: () => boolean` and `getScanFailures?: () => CodingCliProviderName[]`, and add two new optional top-level deps (`sessionMetadataStore` ALREADY exists on the interface — reuse it):

```typescript
  codingCliIndexer: {
    getProjects: () => any[]
    refresh: () => Promise<void>
    isReady?: () => boolean
    getScanFailures?: () => CodingCliProviderName[]
    requestRefresh?: () => void
  }
  /** Test override for the exact-id fallbacks (defaults to buildResolveFallbacks(...)). */
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
  const resolveFallbacks = deps.resolveFallbacks
    ?? buildResolveFallbacks(deps.codingCliProviders, { sessionMetadataStore: deps.sessionMetadataStore })
  const KNOWN_RESUME_PROVIDERS = ['claude', 'codex', 'opencode', 'amplifier'] as const
  router.post('/sessions/resolve', async (req, res) => {
    const parsed = ResolveBodySchema.safeParse(req.body)
    if (!parsed.success) {
      res.status(400).json({ error: 'body must be { input: string } (1-20000 chars)' })
      return
    }
    try {
      // The indexer scans ONLY settings-enabled providers (verified:
      // session-indexer.ts filters by enabledProviders, ~line 1385). A
      // disabled provider's sessions cannot be found — report it as
      // UNSEARCHED so the client never implies the id does not exist.
      const settings = await deps.configStore.getSettings().catch(() => ({}))
      const enabled = new Set<string>(
        settings?.codingCli?.enabledProviders ?? KNOWN_RESUME_PROVIDERS,
      )
      const unsearchedProviders = KNOWN_RESUME_PROVIDERS.filter((n) => !enabled.has(n))
      const { candidates, agentHint } = parseResumeInput(parsed.data.input)
      const { matches, providerErrors: fallbackErrors } = await resolveSessionCandidates(candidates, {
        getProjects: () => deps.codingCliIndexer.getProjects(),
        // Fresh budget per request: bounds fallback work; shape gates run
        // before the budget inside withRequestBudget.
        fallbacks: withRequestBudget(resolveFallbacks),
      })
      // A provider whose last index scan FAILED was not searched either —
      // the indexer swallows listing failures and still reports ready. A
      // DISABLED provider is UNSEARCHED (reported above), never a provider
      // error: without this filter a failed-then-disabled provider would
      // keep responses degraded forever (no successful scan can ever clear
      // it) and trap the user in a Retry loop.
      const scanFailures = (deps.codingCliIndexer.getScanFailures?.() ?? [])
        .filter((name) => enabled.has(name))
      const providerErrors = [...new Set([...fallbackErrors, ...scanFailures])]
      // degraded = something FAILED — even when matches exist: a failed
      // provider means a HIGHER-priority exact match may have been missed,
      // so the client must not auto-resume a surviving lower-priority match
      // (and with zero matches it shows retry, never "no matching session").
      const indexState: 'ready' | 'warming' | 'degraded' =
        deps.codingCliIndexer.isReady?.() === false ? 'warming'
        : providerErrors.length > 0 ? 'degraded'
        : 'ready'
      // Fire-and-forget: give the user's Retry a chance to converge once a
      // failed provider recovers (scan failures only clear on a new scan).
      if (indexState === 'degraded') deps.codingCliIndexer.requestRefresh?.()
      res.json({
        indexState,
        tokens: candidates,
        agentHint: agentHint ?? null,
        homeDir: deps.homeDir ?? os.homedir(),
        providerErrors,
        unsearchedProviders,
        matches,
      })
    } catch (err) {
      log.warn({ err }, 'sessions/resolve failed')
      res.status(500).json({ error: 'resolve failed' })
    }
  })
```

- [ ] **Step 5: Verify the PRODUCTION fallback-composition tests (`buildResolveFallbacks` + `withRequestBudget`) — written RED in Step 1 — now pass**

The router tests above inject fallbacks, so they never exercise the production composition. `test/unit/server/coding-cli/resolve-fallbacks.test.ts` (node environment; created in Step 1 before any implementation existed) covers, at minimum:

- `withRequestBudget` shape-before-budget: a wrong-shape token returns null WITHOUT calling the inner fallback and WITHOUT consuming budget; the (budget+1)-th valid-shape token returns null without calling the inner fallback.
- `buildResolveFallbacks` claude wiring: stub provider `{ name: 'claude', getSessionRoots: () => [tmpRoot] }` with a real transcript fixture (reuse Task 4's `writeTranscript` shape) → returns the full match tuple; with a metadata-store stub whose `getAll()` returns `{ ['claude:' + id]: { sessionType: 'freshclaude' } }` → `sessionType` is `'freshclaude'`; with no metadata → `'claude'`; metadata-store `getAll()` REJECTING → still resolves with `'claude'` (metadata failure must not become a provider error).
- `buildResolveFallbacks` opencode wiring: stub provider `{ name: 'opencode', getDatabasePath: () => '/tmp/x.db' }` with injected `runOpencodeById` returning a row → full match tuple (including metadata-driven `sessionType`); `runOpencodeById` REJECTING → the fallback REJECTS (error propagation, not null).
- No claude/opencode provider in the set → the corresponding fallback is `undefined`.

```bash
npm run test:vitest -- --config config/vitest/vitest.server.config.ts test/unit/server/coding-cli/resolve-fallbacks.test.ts --run
```
Expected: PASS.

- [ ] **Step 6: Run tests to verify they pass**

```bash
npm run test:vitest -- --config config/vitest/vitest.server.config.ts test/unit/server/sessions-resolve-router.test.ts --run
npm run typecheck:server
```
Expected: PASS, clean typecheck. Also confirm the wiring assumption: open `server/index.ts` around line 748 and verify `createSessionsRouter({ ... codingCliIndexer ... })` receives the indexer INSTANCE (then `isReady()`/`getScanFailures()` from Task 3 are picked up automatically); if it is an object literal, add `isReady: () => codingCliIndexer.isReady()` and `getScanFailures: () => codingCliIndexer.getScanFailures()`. Verify `sessionMetadataStore` is already passed (it is an existing dep used by the `/session-metadata` route).

- [ ] **Step 7: Run the pre-existing sessions-router + indexer test files to catch regressions**

```bash
npm run test:vitest -- --config config/vitest/vitest.server.config.ts test/unit/server --run
```
Expected: PASS (no regressions from the deps change).

- [ ] **Step 8: Refactor pass, then commit**

Refactor (TDD's third step): with tests green, re-read the route handler and `resolve-fallbacks.ts` for duplication/clarity (e.g. the enabled-provider set logic); apply safe improvements and re-run Steps 5–7's commands.

```bash
git add server/sessions-router.ts server/coding-cli/resolve-fallbacks.ts server/coding-cli/providers/opencode.ts test/unit/server/sessions-resolve-router.test.ts test/unit/server/coding-cli/resolve-fallbacks.test.ts
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
    unsearchedProviders: [],
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

  it('single match WITHOUT a cwd does NOT auto-resume: asks for a working directory instead', async () => {
    // Exact-id fallback hits can lack a recorded cwd; the spec requires a
    // concrete working directory before opening — never auto-open without one.
    mockResolve.mockResolvedValue(response({ matches: [match({ cwd: undefined, projectPath: '' })] }))
    const { onResume } = renderDialog()
    await pasteAndResolve(CLAUDE_V4)
    expect(onResume).not.toHaveBeenCalled()
    const cwdInput = screen.getByLabelText(/working directory/i)
    expect(cwdInput).toHaveValue('/home/testuser')
    fireEvent.change(cwdInput, { target: { value: '/home/u/somewhere' } })
    fireEvent.click(screen.getByRole('button', { name: /resume claude code session/i }))
    expect(onResume).toHaveBeenCalledWith(expect.objectContaining({
      provider: 'claude', sessionId: CLAUDE_V4, cwd: '/home/u/somewhere',
    }))
  })

  it('EDITING the input invalidates the previous result: stale resume-anyway/matches cannot act', async () => {
    // Without this, resolve(A) -> "not found" -> replace text with B ->
    // "Resume anyway" would resume STALE id A via result.tokens[0].
    mockResolve.mockResolvedValue(response({ matches: [] }))
    renderDialog()
    await pasteAndResolve(CLAUDE_V4)
    expect(screen.getByRole('button', { name: /resume anyway/i })).toBeInTheDocument()
    fireEvent.change(screen.getByLabelText(/resume string/i), { target: { value: 'ses_other0000000000000000000000' } })
    expect(screen.queryByRole('button', { name: /resume anyway/i })).toBeNull()
    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('names DISABLED (unsearched) providers in the no-match message', async () => {
    mockResolve.mockResolvedValue(response({ matches: [], unsearchedProviders: ['codex', 'amplifier'] }))
    renderDialog()
    await pasteAndResolve(CLAUDE_V4)
    expect(screen.getByRole('alert')).toHaveTextContent(/not searched \(disabled\): codex, amplifier/i)
    // Resume-anyway stays available.
    expect(screen.getByRole('button', { name: /resume anyway/i })).toBeInTheDocument()
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

  it('DEGRADED single match with cwd: NO auto-resume — match listed with degraded notice + Retry', async () => {
    // A failed provider means a higher-priority exact match may have been
    // missed: auto-opening the surviving match could open the WRONG session.
    mockResolve.mockResolvedValue(response({
      indexState: 'degraded', providerErrors: ['claude'], matches: [match()],
    }))
    const { onResume } = renderDialog()
    await pasteAndResolve(CLAUDE_V4)
    expect(onResume).not.toHaveBeenCalled()
    expect(screen.getByRole('status')).toHaveTextContent(/could not be searched/i)
    expect(screen.getByRole('button', { name: /retry/i })).toBeInTheDocument()
    expect(screen.getByRole('list', { name: /matching sessions/i })).toBeInTheDocument()
  })

  it('cwd-less match with a BLANK working-directory field: confirm is blocked with an inline error, no resume', async () => {
    mockResolve.mockResolvedValue(response({ matches: [{ ...match(), cwd: undefined }] }))
    const { onResume } = renderDialog()
    await pasteAndResolve(CLAUDE_V4)
    fireEvent.change(screen.getByLabelText(/working directory/i), { target: { value: '   ' } })
    fireEvent.click(screen.getByRole('button', { name: /resume claude session/i }))
    expect(onResume).not.toHaveBeenCalled()
    expect(screen.getByRole('alert')).toHaveTextContent(/working directory/i)
  })

  it('"Resume anyway" is DISABLED while the working-directory field is blank (never launch a cwd-less tuple)', async () => {
    mockResolve.mockResolvedValue(response({ matches: [] }))
    const { onResume } = renderDialog()
    await pasteAndResolve(CLAUDE_V4)
    fireEvent.change(screen.getByLabelText(/working directory/i), { target: { value: '  ' } })
    const anyway = screen.getByRole('button', { name: /resume anyway/i })
    expect(anyway).toBeDisabled()
    fireEvent.click(anyway)
    expect(onResume).not.toHaveBeenCalled()
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
  /** 'degraded' = zero matches AND a provider could not be searched — retry, NOT "not found". */
  indexState: 'ready' | 'warming' | 'degraded'
  tokens: string[]
  agentHint: { provider: ResumeResolveMatch['provider']; source: 'command' | 'word' | 'id-format' } | null
  homeDir: string
  /** Providers whose search FAILED (fallback threw, or last index scan failed). */
  providerErrors: Array<ResumeResolveMatch['provider']>
  /** Providers DISABLED in settings — not scanned at all; absence claims must name them. */
  unsearchedProviders: Array<ResumeResolveMatch['provider']>
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
    // A match without a recorded cwd never auto-resumes (see runResolve);
    // when the user confirms it from the match list, the editable
    // working-directory field below supplies the concrete cwd (spec). A
    // session must NEVER open without a concrete cwd — an empty/whitespace
    // field blocks the confirm with an inline error instead of silently
    // launching a cwd-less tuple.
    const cwd = m.cwd ?? anywayCwd.trim()
    if (!cwd) {
      setErrorText('Enter a working directory to open this session.')
      return
    }
    onResume({
      provider: m.provider,
      sessionId: m.sessionId,
      sessionType: m.sessionType,
      cwd,
      title: m.title,
      firstUserMessage: m.firstUserMessage,
    })
    setNote(`Found in ${PROVIDER_LABELS[m.provider] ?? m.provider}`)
    closeTimerRef.current = setTimeout(onClose, CLOSE_AFTER_RESUME_MS)
  }, [anywayCwd, onClose, onResume])

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
    // Auto-resume ONLY on a fully-healthy response: a 'degraded' response
    // means some provider FAILED, so a higher-priority exact match may have
    // been missed — auto-opening the surviving match could open the WRONG
    // session (hard requirement). Degraded matches render in the list below
    // with the degraded notice + Retry instead.
    if (response.matches.length === 1 && response.matches[0].cwd && response.indexState === 'ready') {
      finishResume(response.matches[0])
      return
    }
    // A single match WITHOUT a recorded cwd (exact-id fallback hit) must NOT
    // auto-open — the spec requires a concrete working directory. It renders
    // in the match list below alongside an editable working-directory field.
    if (response.matches.length === 0 && response.indexState === 'ready') {
      // Absence claims must name what was NOT searched (disabled providers) —
      // otherwise "not found" implies the id does not exist anywhere.
      setErrorText(response.unsearchedProviders.length > 0
        ? `No matching session found. Not searched (disabled): ${response.unsearchedProviders.join(', ')}.`
        : 'No matching session found in any agent.')
    }
  }, [finishResume, pickerTouched])

  const handleResumeAnyway = useCallback(() => {
    // CURRENT input first: result is cleared on edit, but never act on a
    // stale token when the user has typed something new.
    const token = parseResumeInput(inputValue).candidates[0] ?? result?.tokens[0]
    // A concrete cwd is REQUIRED (spec: never open without one) — the button
    // below is disabled when the field is blank; this guard is the backstop.
    const cwd = anywayCwd.trim()
    if (!token || !cwd) return
    onResume({ provider: picker, sessionId: token, sessionType: picker, cwd })
    onClose()
  }, [anywayCwd, inputValue, onClose, onResume, picker, result])

  if (!open) return null

  // warming AND degraded are retry states — NEITHER is "not found" (spec:
  // absence needs evidence; provider unavailable gets loading/retry). The
  // retry notice shows for EVERY warming/degraded response — with OR without
  // matches — because degraded matches may be incomplete/lower-priority.
  const retryState = result !== null && result.tokens.length > 0
    && (result.indexState === 'warming' || result.indexState === 'degraded')
    ? result.indexState
    : null
  const matchesToShow = result?.matches ?? []
  // >1 = disambiguation; ==1 without cwd = needs-working-directory
  // confirmation; ==1 on a DEGRADED response = manual confirmation required
  // (a lone match WITH cwd on a READY response auto-resumed in runResolve
  // and never reaches here).
  const showMatchList = matchesToShow.length > 1
    || (matchesToShow.length === 1 && (!matchesToShow[0].cwd || result?.indexState === 'degraded'))
  const showResumeAnyway = errorText !== null && errorText.startsWith('No matching session')
  // Editable working directory: shown for resume-anyway AND for listed matches
  // lacking a recorded cwd (spec: never open without a concrete cwd).
  const showCwdInput = showResumeAnyway || (showMatchList && matchesToShow.some((m) => !m.cwd))

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
            onChange={(e) => {
              setInputValue(e.target.value)
              // EDITING invalidates everything derived from the previous
              // text: bump the sequence so in-flight responses go stale, and
              // clear result/error/note so stale "Resume anyway" or
              // disambiguation actions can never act on old tokens.
              resolveSeqRef.current += 1
              setResolving(false); setResult(null); setErrorText(null); setNote(null)
            }}
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
              : matchesToShow.length > 0
                ? 'Some agents could not be searched right now — the matches below may be incomplete. Confirm one manually or retry.'
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

        {showMatchList && (
          <ul aria-label="Matching sessions" className="flex flex-col gap-1 max-h-64 overflow-y-auto">
            {matchesToShow.map((m) => (
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

        {showCwdInput && (
          <div className="flex flex-col gap-2 border-t border-border pt-2">
            <div className="flex flex-col gap-1">
              <label htmlFor="resume-anyway-cwd" className="text-xs text-muted-foreground">
                {showResumeAnyway
                  ? 'Working directory'
                  : 'Working directory (this session has no recorded one — required to open it)'}
              </label>
              <input
                id="resume-anyway-cwd"
                className="bg-background border border-border rounded px-2 py-1.5 text-sm font-mono"
                value={anywayCwd}
                onChange={(e) => setAnywayCwd(e.target.value)}
              />
            </div>
            {showResumeAnyway && (
              <button
                type="button"
                className="self-start rounded border border-border px-3 py-1.5 text-sm hover:bg-muted/50 disabled:opacity-50 disabled:cursor-not-allowed"
                onClick={handleResumeAnyway}
                disabled={!anywayCwd.trim()}
              >
                Resume anyway with {PROVIDER_LABELS[picker]}
              </button>
            )}
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
- Stale-response guard is MANDATORY: `resolveSeqRef` invalidates in-flight resolves on every new resolve, on EVERY INPUT EDIT, and on close; a stale response (including a stale single-match auto-resume) must be ignored, and editing also clears `result`/`errorText`/`note` so stale "Resume anyway"/disambiguation actions cannot act on old tokens. The Step 1 reversed-ordering and editing-invalidation tests prove both.
- Never auto-open without a concrete cwd: a single match WITHOUT `cwd` (exact-id fallback hit) renders in the match list with the editable working-directory field (prefilled from `homeDir`) instead of auto-resuming; `finishResume` fills `cwd` from that field for such matches — and REFUSES (inline error, no `onResume`) when the field is blank/whitespace, while "Resume anyway" is disabled until the field is non-blank. No code path may call `onResume` with an undefined/empty `cwd`. The Step 1 missing-cwd, blank-cwd, and disabled-resume-anyway tests prove it.
- Never auto-resume on a DEGRADED response: `runResolve` auto-resumes only when `indexState === 'ready'`; a degraded single match renders in the match list under the degraded notice + Retry (a failed provider means a higher-priority exact match may have been missed — auto-opening could resume the WRONG session). The Step 1 degraded-no-auto-resume test proves it.
- Absence claims must name unsearched providers: when zero matches and `unsearchedProviders` is non-empty, the no-match message lists them (they are DISABLED in settings — the server never scanned them).

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
  return { indexState: 'ready', tokens: [CLAUDE_V4], agentHint: null, homeDir: '/home/t', providerErrors: [], unsearchedProviders: [], matches: [], ...overrides }
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
// that always opened a claude pane would otherwise pass every flow). The tuple
// includes sessionType: dropping/replacing the resolved sessionType would make
// openSessionTab create the wrong terminal runtime instead of a fresh-agent
// pane (src/lib/session-type-utils.ts buildPaneInput), so every flow asserts
// it. Verify the exact field placement against tabsSlice.openSessionTab
// (~lines 585-615: `resolvedSessionType = sessionType || provider` flows into
// the tab's session metadata and the pane content) and mirror how
// test/e2e/sidebar-click-opens-pane.test.tsx asserts pane/tab shape — if the
// sessionType lands on the PANE CONTENT rather than the tab, assert it there.
function expectResumedTab(
  store: ReturnType<typeof makeStore>,
  expected: { provider: string; sessionId: string; cwd?: string; sessionType?: string },
) {
  const tabs = store.getState().tabs.tabs
  expect(tabs).toHaveLength(1)
  expect(tabs[0]).toMatchObject({
    codingCliProvider: expected.provider,
    mode: expected.provider,
    ...(expected.cwd !== undefined ? { initialCwd: expected.cwd } : {}),
    sessionRef: { provider: expected.provider, sessionId: expected.sessionId },
  })
  expectTabSessionType(store, expected.sessionType ?? expected.provider)
}

// Assert the resolved sessionType survived into the opened tab/pane content
// (see comment above for where it lives — adjust the accessor, NOT the
// expectation, if the store shape differs).
function expectTabSessionType(store: ReturnType<typeof makeStore>, sessionType: string) {
  const state = store.getState()
  const tab = state.tabs.tabs[0]
  const paneContentTypes = Object.values(state.panes?.panesByTab?.[tab.id]?.panes ?? {})
    .map((p: any) => p?.content?.sessionType)
  expect([tab.sessionType, ...paneContentTypes]).toContain(sessionType)
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

  it('quoted claude --resume with a TRUNCATED id (spec: ed2afda6-…) and picker set to codex → prefix evidence wins → CLAUDE tab with the FULL id + note', async () => {
    // The spec's acceptance row pastes a truncated id — the flow must prove
    // prefix evidence resolves to the full session AND overrides the codex
    // picker. Using the full UUID here would prove neither.
    const TRUNCATED = 'ed2afda6-'
    mockResolve.mockResolvedValue(response({
      tokens: ['ed2afda6'],
      agentHint: { provider: 'claude', source: 'command' },
      matches: [{ ...claudeMatch(), matchType: 'prefix' as const, matchedToken: 'ed2afda6' }],
    }))
    const { store } = renderApp()
    fireEvent.click(screen.getByTestId('sidebar-resume-button'))
    fireEvent.change(screen.getByLabelText(/agent/i), { target: { value: 'codex' } })
    const input = screen.getByLabelText(/resume string/i)
    fireEvent.change(input, { target: { value: `"claude --resume ${TRUNCATED}"` } })
    fireEvent.keyDown(input, { key: 'Enter' })
    await act(async () => { await Promise.resolve(); await Promise.resolve() })
    expect(mockResolve).toHaveBeenCalledWith(`"claude --resume ${TRUNCATED}"`)
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

  it('a FRESH-AGENT sessionType survives resume end to end: freshclaude match opens a freshclaude pane, not a bare claude terminal', async () => {
    // Hard requirement: sessions opened via freshclaude/freshopencode/kilroy
    // must reopen through that runtime. Every other flow has
    // sessionType === provider, so ONLY this test would catch an
    // implementation that drops or replaces the resolved sessionType
    // (openSessionTab would then build the wrong pane runtime — see
    // src/lib/session-type-utils.ts buildPaneInput).
    mockResolve.mockResolvedValue(response({
      matches: [{ ...claudeMatch(), sessionType: 'freshclaude' }],
    }))
    const { store } = renderApp()
    await openDialogAndResolve(CLAUDE_V4)
    expectResumedTab(store, {
      provider: 'claude', sessionId: CLAUDE_V4, cwd: '/home/u/proj', sessionType: 'freshclaude',
    })
    // Belt-and-braces: the fresh-agent runtime, not a plain terminal pane.
    expectTabSessionType(store, 'freshclaude')
  })

  it('DEGRADED response with a single full match: NO auto-resume, NO tab — match listed for manual confirmation', async () => {
    // A failed provider means a higher-priority exact match may have been
    // missed; auto-resuming the surviving match could open the WRONG session.
    mockResolve.mockResolvedValue(response({
      indexState: 'degraded', providerErrors: ['claude'], matches: [claudeMatch()],
    }))
    const { store } = renderApp()
    await openDialogAndResolve(CLAUDE_V4)
    expect(store.getState().tabs.tabs).toHaveLength(0)
    expect(dialog().getByRole('status')).toHaveTextContent(/could not be searched/i)
    expect(dialog().getByRole('list', { name: /matching sessions/i })).toBeInTheDocument()
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

These flows are the plan's INTEGRATION GATES over units that were each built red-green in Tasks 2–8 (that is where this feature's TDD red phases live); a first-run failure here is not a broken test to rewrite — it is evidence of a real wiring bug to fix.

```bash
npm run test:vitest -- --config config/vitest/vitest.config.ts test/e2e/resume-button-flow.test.tsx --run
```
Expected: PASS. If the "already open" test fails because `openSessionTab` created a second tab, that is a REAL bug in the Task 8 wiring (e.g. sessionType mismatch in the dedup locator) — fix the wiring, not the test. If the freshclaude flow fails on the store-shape accessor in `expectTabSessionType`, verify the actual field placement against `tabsSlice.openSessionTab` and fix the ACCESSOR (the expectation that `'freshclaude'` survives is non-negotiable). If store setup fails to render, mirror the `createStore` helper from `test/e2e/sidebar-click-opens-pane.test.tsx` (lines 68–147) for the missing slices and re-run.

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

### Task 10: docs/index.html demo-mock update + full verification

**Files:**
- Modify: `docs/index.html` (the static demo mock — a new always-visible sidebar control is a major UI change per repo rules, and the mock's sidebar must show it)

**Interfaces:**
- Consumes: the shipped feature set from Tasks 2–9.
- Produces: updated demo mock page; final green verification of the whole branch.

- [ ] **Step 1: Show the pinned Resume control in the `docs/index.html` sidebar mock**

`docs/index.html` is a self-contained static DEMO MOCK of the app (it has NO feature list — do not invent one). Its sidebar mock is the relevant structure (VERIFIED): a scrollable `<div class="sb-list">` (~line 633) immediately followed by `<div class="sb-footer">…Star on GitHub…</div>` (~line 642), with the matching CSS rules `.sb-list { flex: 1; overflow-y: auto; … }` (~line 191) and `.sb-footer { padding: 12px 16px; border-top: …; }` (~line 207).

Mirror the app's new pinned footer: BETWEEN the closing `</div>` of `.sb-list` and the `.sb-footer` div, insert:

```html
      <div class="sb-resume"><button type="button" title="Resume any session by ID — paste an id or a whole 'codex resume …' command and freshell finds it across Claude Code, Codex, OpenCode, and Amplifier">⟲ Resume</button></div>
```

And next to the `.sb-footer` CSS rules (~line 207), add matching styles in the same formatting style as their neighbors:

```css
.sb-resume { padding: 8px; border-top: 1px solid hsl(var(--border)); }
.sb-resume button { width: 100%; padding: 6px 8px; border: 0; border-radius: 6px; background: transparent; color: hsl(var(--muted-foreground)); font: inherit; font-size: 13px; cursor: pointer; }
.sb-resume button:hover { color: hsl(var(--foreground)); background: hsl(var(--muted) / .5); }
```

The mock button is intentionally non-functional (the page is a static demo); the `title` tooltip carries the feature copy. Static check (advisory only; match existing formatting by eye/diff): `node -e "require('fs').readFileSync('docs/index.html','utf8')" && npx --yes html-validate docs/index.html || true`.

- [ ] **Step 2: Full verification sweep**

```bash
npm run test:status        # respect coordinated-run discipline before broad runs
npm run typecheck
npm run lint
npm run test:vitest -- --config config/vitest/vitest.config.ts test/unit/shared/resume-input-parser.test.ts test/unit/client/resume-session-dialog.test.tsx test/unit/client/sidebar-resume-footer.test.tsx test/e2e/resume-button-flow.test.tsx --run
npm run test:vitest -- --config config/vitest/vitest.server.config.ts test/unit/server/coding-cli/session-resolver.test.ts test/unit/server/coding-cli/session-indexer.test.ts test/unit/server/coding-cli/claude-transcript-locator.test.ts test/unit/server/coding-cli/opencode-by-id-query.test.ts test/unit/server/coding-cli/opencode-by-id-runner.test.ts test/unit/server/coding-cli/resolve-fallbacks.test.ts test/unit/server/sessions-resolve-router.test.ts --run
```
Expected: all PASS, no type or lint errors. Then run the full regression sweep THROUGH THE COORDINATOR GATE — this sweep is MANDATORY, not conditional: per AGENTS.md, broad runs WAIT for the shared coordinator gate when another agent holds it (never skip the sweep, never kill a foreign holder). Do NOT use `npm run test:balanced` here — it launches Vitest directly (`scripts/run-standard-tests.ts`) without acquiring the coordinator gate, so it can race a concurrent holder even right after `test:status`. Use the coordinated workload commands, which acquire the gate (and block until it frees):

```bash
FRESHELL_TEST_SUMMARY='resume-button final regression sweep' npm run test:unit
FRESHELL_TEST_SUMMARY='resume-button final regression sweep' npm run test:integration
```
Expected: green (matching the base-green baseline; `npm run test:status` earlier is advisory — it shows the current holder but is NOT the gate itself). SAFETY: none of these commands may target ports 3001/3002 or any process you did not spawn.

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

## Verification log

- Target backend confirmed (Task 1): `npm run dev` → `tsx watch server/index.ts`; `npm run start` → `node dist/server/index.js`. The Node server (`server/index.ts`) serves the sidebar in the default dev/start path; the feature's API is implemented there per spec.
- Deployment gap recorded (Task 1): the canonical self-hosted production is the RUST server (AGENTS.md, `scripts/launch-rust.sh`, port 3002) serving the same `dist/client`; it has no `/api/sessions/resolve`. The client degrades gracefully on resolve 404 (explicit "this server build does not support resume-by-id" message — implemented and tested in Task 7). FOLLOW-UP: Rust-server `/api/sessions/resolve` parity is required before the Resume button is fully functional on the canonical production deployment.
