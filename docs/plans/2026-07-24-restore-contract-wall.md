# Restore Contract Wall (P0.1) Implementation Plan

> **For agentic workers:** This plan is executed task-by-task by the
> workflow's execute stage: a fresh implementer per task, with a spec +
> quality review after each task. Steps use checkbox (`- [ ]`) syntax
> for tracking.

**Goal:** Build the restart-resilience CONTRACT TEST WALL — a restore-matrix
Playwright e2e spec ("the ruler") that boots the Rust freshell server, creates
every pane type live with fake CLIs, SIGKILLs the server, restarts it, and
asserts each pane's restore contract per §2 of
`docs/plans/2026-07-24-restart-resilience-architecture-analysis.md` — with
today's known-broken contracts pinned as expected failures so the suite stays
CI-green while the wall stays honest.

**Architecture:** One new rust-only spec file
(`test/e2e-browser/specs/restore-contract-wall-rust.spec.ts`) that imports
`RustServer` directly (for `restartAbrupt()`, the SIGKILL primitive), plus two
new checked-in fake fixtures (a fake `claude` terminal CLI and a fake Claude
SDK-bridge sidecar). Per-pane-type contract tests give granular CI signal;
one all-pane-types "ruler" test proves the composed scenario; six named red
tests from plan §5 P0.1 are pinned expected-fail with the plan item that will
flip them. **No production code changes.** Test files + one Playwright config
edit only.

**Tech Stack:** Playwright (`@playwright/test` via the suite's
`helpers/fixtures.js`), `RustServer` fixture (spawns
`target/release/freshell-server`), Node fake-CLI fixtures (`.mjs`), the
in-page `window.__FRESHELL_TEST_HARNESS__` Redux harness.

## Global Constraints

- Work happens in the worktree `/home/dan/code/freshell/.worktrees/restore-contract-wall` (branch `feat/restore-contract-wall`, base `origin/main` @ `a53f185a`). All paths below are relative to that worktree root unless absolute.
- **Do NOT change production code** (`src/`, `crates/`, `server/`, `shared/`, `extensions/`). Only `test/e2e-browser/**` and `test/fixtures/**` files may be created/modified, plus the one config edit in `test/e2e-browser/playwright.config.ts`.
- The source spec for every contract is `/home/dan/code/freshell/docs/plans/2026-07-24-restart-resilience-architecture-analysis.md` (UNTRACKED on main — read it via that absolute path; **never commit it**). §2 defines per-pane contracts; §5 P0.1 names the red tests.
- Fake CLIs only — never real providers. Enablement is per-provider env overrides (`CLAUDE_CMD`, `CODEX_CMD`, `OPENCODE_CMD`, `FRESHELL_CLAUDE_SIDECAR`), never PATH pollution outside the test's mkdtemp root.
- NEVER restart the user's self-hosted freshell server. NEVER use broad kill patterns (`pkill -f ...`). The only kills allowed are `RustServer`'s own ownership-safe group kill of servers this spec spawned (`restartAbrupt()` / `stop()`), which is the established pattern in `compound-restart-rust.spec.ts`.
- The e2e-browser suite is NOT wrapped by the shared test coordinator and does not run in CI on this branch — run it directly with `npx playwright test --config test/e2e-browser/playwright.config.ts --project=rust-chromium <filter>`. If you run any broad coordinated command (`npm test`, `npm run check`), first check `npm run test:status`; if another agent holds the gate, WAIT; set `FRESHELL_TEST_SUMMARY="restore contract wall verification"` for the run.
- `globalSetup` rebuilds the JS client+server on every `playwright test` invocation and the Rust binary is built lazily by `ensureRustServerBuilt()` (cargo release). First run is slow; that is normal.
- Server TS is NodeNext ESM — every relative import in spec/helper files needs a `.js` extension (e.g. `'../helpers/rust-server.js'`).
- Selectors must be role + accessible-name (a11y CI rule). Never add automation-only selectors, and never modify `src/` to add test ids.
- Expected-fail pinning uses Playwright-native `test.fail(condition, description)` naming the campaign plan item, plus a file-level FLIP INSTRUCTION doc comment (canonical example: `test/e2e-browser/specs/settings-persistence-split.spec.ts:73-92`). `test.fixme` is forbidden for wall entries — fixme'd tests never execute and produce no evidence.
- Commits are focused and atomic, one per task, conventional style (`test: ...`).
- **PR creation is NOT approved.** After final verification: commit, push the branch, then STOP and report — do not run `gh pr create`.
- Out of scope, per the task instruction itself: ledger-internal red tests (`corrupt-ledger-boot`, `pending-resolution-collision`, `crash-between-binding-write-and-marker-delete`, `crash-mid-supersession-two-bound-rows`, `client-claims-superseded-ref`, `winner-dies-mid-claim`, `winner-hangs-mid-claim`, `loser-exhausts-then-holder-fails`, `warming-never-completes`, `restart-storm-all-panes-warming`, `rebind-retires-old-row`, `stale-candidate-replay`, `cross-pane-candidate-hijack`, `ledger-write-failure-surfaces-live`) are unit-level and belong to later slices; the opt-in real-provider contract suite (`FRESHELL_RUN_REAL_PROVIDER_CONTRACTS=1`) is excluded because this task mandates "use the suite's existing FAKE CLI harness, never real providers".

---

## Background an implementer must know (read once, it is load-bearing)

**Server fixture.** `test/e2e-browser/helpers/rust-server.ts` exports
`class RustServer implements E2eServerHandle` with
`start(): Promise<TestServerInfo>`, `restart()` (graceful),
`restartAbrupt()` (**SIGKILL** the process group, then reboot on the SAME
home/port/token — this is the wall's kill primitive), `stop()`, `info`.
Options: `{ homeDir?, preserveHomeOnStop?, token?, env?, setupHome?, startTimeoutMs?, verbose? }`.
`restartAbrupt()` exists ONLY on `RustServer` (not on the `E2eServerHandle`
interface), which is why this spec imports `RustServer` directly and must be
registered rust-only. `TestServerInfo` fields used here: `baseUrl`, `wsUrl`,
`token`, `homeDir`, `logsDir`, `port`. `boot()` re-runs `setupHome` on every
restart — seeds must be idempotent overwrites and must never delete runtime
artifacts (e.g. `opencode.db`).

**Isolated HOME.** Every server gets a mkdtemp HOME;
`applyIsolatedHomeEnvironment` forces `HOME`, `CLAUDE_HOME=<home>/.claude`,
`CODEX_HOME=<home>/.codex`, `XDG_DATA_HOME=<home>/.local/share` (→ opencode DB
at `<home>/.local/share/opencode/opencode.db`). The user's real dotfiles are
never touched.

**Reconnect detection.** No bootId in the harness. Live-reconnect pattern:
poll `window.__FRESHELL_TEST_HARNESS__.getWsReadyState() === 'ready'`.
Reload pattern: `page.reload({ waitUntil: 'domcontentloaded' })` then
`harness.waitForHarness(); harness.waitForConnection()`. Always dispatch
`{ type: 'persist/flushNow' }` before any reload.

**Restore signal for terminals.** After a server restart the pane must get a
NEW `terminalId` (PTYs died); after a mere page reload the `terminalId` is
UNCHANGED (reattach). Poll Redux via `harness.getPaneLayout(tabId)`.

**Resume proof (two independent ways).** (1) argv-log delta: every fake CLI
appends `{pid,t,argv}` JSONL to its `FAKE_*_ARGV_LOG`; snapshot the entry
count before the kill and assert the delta contains the resume pair.
(2) xterm buffer marker via `harness.getTerminalBuffer(terminalId)` — ALWAYS
strip newlines (`buffer.replace(/\n/g, '')`) before substring matching
(xterm line-wrap splits markers).

**Resume argv shapes** (from `extensions/*/freshell.json`):
claude `["--resume","<id>"]` (a fresh create pre-allocates
`["--session-id","<uuid>"]` at t=0 **ONLY on the WS/picker create path** —
`crates/freshell-ws/src/terminal.rs:969-982`; the REST `POST /api/tabs`
path never mints its own preallocated id — `LaunchIntent::Resume`,
`crates/freshell-freshagent/src/terminal_tabs.rs:756-768` — so every fresh
claude terminal pane in this plan is created via the picker, never REST);
codex `["resume","<id>"]` (searched anywhere in argv, not position 0);
opencode `["--session","<id>"]`.

**Fresh-agent enablement is two-layered.** The Redux dispatches
(`connection/setAvailableClis`, `settings/previewServerSettingsPatch`) are
client-only; real `freshAgent.create` is gated server-side, so `setupHome`
MUST seed `.freshell/config.json` with `freshAgent: { enabled: true }` and
`codingCli.enabledProviders`.

**Fresh-agent sidecars.**
- freshcodex: server spawns `CODEX_CMD` (whitespace-split) with argv
  `-c features.apps=false app-server --listen ws://127.0.0.1:<port>`; the fake
  is `test/fixtures/coding-cli/codex-app-server/fake-app-server.mjs` (parses
  `--listen` positionally-anywhere; deterministic reply text is the literal
  `'Fixture turn'`; writes a durable rollout under `$CODEX_HOME/sessions/`),
  installed via a generated node wrapper (never a copy — `ws` bare-specifier
  resolution needs the repo's `node_modules` ancestor).
- freshopencode: server spawns `OPENCODE_CMD` (else bare `opencode`) with argv
  `serve --hostname 127.0.0.1 --port <port>`, no whitespace split — must be a
  single executable. The fake is `test/e2e-browser/fixtures/fake-opencode.cjs`
  (real HTTP+SSE server over `opencode.db`; audits to
  `FAKE_OPENCODE_AUDIT_LOG`; ids `ses_*`; reply text
  `Fake OpenCode response: <prompt>`).
- freshclaude: server spawns
  `<FRESHELL_CLAUDE_NODE|"node"> <FRESHELL_CLAUDE_SIDECAR|crates/freshell-claude-sidecar/index.mjs>`
  — **`FRESHELL_CLAUDE_SIDECAR` is a production env seam**, so e2e can point
  it at a fixture. Protocol is newline-JSON over stdio: server sends
  `{"type":"create",requestId,cwd,model,permissionMode,effort,resumeSessionId}`,
  child MUST reply `{"type":"created","sessionId":"<id>"}` (45s budget), then
  emits `sdk.*` event lines which the server renames `sdk.X → freshAgent.X`.
  The CONTROL-FRAME reference is the inline test fake
  `FAKE_CLAUDE_SIDECAR_SOURCE` at `crates/freshell-freshagent/src/claude.rs:861-901`
  and the fixture used by `crates/freshell-ws/tests/freshagent_claude_kill_interrupt.rs`
  — but it emits ONLY `created`, so the authoritative FIELD-shape references
  are the real sidecar `crates/freshell-claude-sidecar/index.mjs:15-30` and
  the client consumer `src/lib/fresh-agent-ws.ts:195-284` (see Task 7).
- **freshclaude restore is expected-red (P0.2) for a LAYERED reason.** The
  FIRST failure is that claude fresh-agent identity is never persisted: the
  server sends no `sessionRef` for claude
  (`crates/freshell-freshagent/src/claude.rs:94-96,247`) and the persist
  middleware strips `sessionId` without a `serverInstanceId`
  (`src/store/persistMiddleware.ts:245-266`), so post-reload the pane sends
  neither attach nor create. Behind that, **`freshAgent.attach` for claude
  is silently swallowed** — `crates/freshell-ws/src/terminal.rs:535-553`
  matches `Codex => …, Opencode => …, _ => {}`; no error frame of any kind —
  and the snapshot route 503s (`snapshot.rs:132-145`); both block rebind
  once identity persistence lands.

**CODEX_CMD / OPENCODE_CMD dual role.** Codex terminal mode uses the SAME
`CODEX_CMD` as the app-server sidecar (terminal spawns via PTY: PATH-style
exec of a single file, NO whitespace split — a dual-role `CODEX_CMD` must be
one executable shim, not `node x.mjs`). Distinguisher: sidecar argv contains
`app-server`. Opencode likewise: same `OPENCODE_CMD` for terminal and serve;
distinguisher: `argv[0] === 'serve'`. Only the ruler test (Task 9) needs the
dual-role shims; per-pane tests each boot their own server with one role.

**Suite conventions.** Helpers are **copied per spec, not imported between
specs** (explicit convention, `compound-restart-rust.spec.ts:44-46`); shared
importables live in `test/e2e-browser/helpers/` (`fixtures.js`,
`rust-server.js`, `test-harness.js`, `pane-picker.js`). Restart specs set a
describe-level `test.setTimeout(180_000)`. Every test owns a mkdtemp
`sharedRoot` (fake binaries + argv logs) and removes it in `finally`.
`expect.poll(...).toBe(...)` resolves to `undefined` — always re-read the
value in `.then()` (documented trap, `restore-matrix.spec.ts:1784-1793`).

**Pin table** (which plan item a red contract pins to):

| Contract | Predicted today | Pin (if red) |
|---|---|---|
| shell terminal | PASS | P1.7 (§4.3) |
| claude terminal resume | PASS | P0.4 (§2.2) |
| codex terminal resume | PASS | P0.3 (§2.3) |
| opencode terminal resume (association settled) | PASS | P1.10 (§2.4) |
| freshcodex SIGKILL restore | PASS (reference impl, §2.6) | P1.8 (§2.6) |
| freshopencode SIGKILL restore | PASS (§2.7 serve DB survives) | P1.8/P1.13 (§2.7) |
| freshclaude SIGKILL restore | **FAIL** | P0.2 (§2.8) |
| browser + editor state | PASS | §2.9 / P1.7 |
| the ruler (all panes at once) | **FAIL** | P0.1 (flips when P0.2–P1.13 land) |
| SIGKILL-within-5s-of-pane-creation | **FAIL** | P1.8+P1.9 (D3) |
| SIGKILL-inside-locator-window | **FAIL** (made deterministic by the fixture's `FAKE_OPENCODE_TERMINAL_ROW_GATE_PATH` row gate, Task 10 Step 2) | P1.8 pending markers (§2.4/§4.2) |
| two-clients-same-sessionRef → 1 PTY | **FAIL** | P1.7 multi-client single-flight (D8/§4.3) |
| freshclaude busy-restart un-wedge | **FAIL** | P0.2 (§2.8.1) |
| double-restart mid-recovery | observe | P1.7 (§4.3) |
| hidden-pane rebind | **FAIL** | P1.11 (F8) |

**Decision rule for every "run it" step:** if a PASS-predicted test comes up
red, first re-check your test against the cited donor spec pattern (the test
may be wrong — fix the test, never production code). If the test is faithful,
the red is real: pin it with `test.fail` naming the plan item from the table
and note the observed failure mode in the pin comment. If a FAIL-predicted
test comes up green, do NOT pin it — leave it green and note the surprise in
the commit message. A `test.fail`-pinned test that later passes becomes a hard
failure (self-retiring wall).

## File Structure

- Create: `test/e2e-browser/fixtures/fake-claude-cli.mjs` — argv-logging fake `claude` terminal CLI (fills the suite's known gap: no claude resume-argv proof exists today).
- Create: `test/e2e-browser/fixtures/fake-claude-sidecar.mjs` — fake Claude SDK-bridge sidecar speaking the newline-JSON stdio protocol (enables LIVE freshclaude panes in e2e for the first time).
- Create: `test/e2e-browser/specs/restore-contract-wall-rust.spec.ts` — the wall: shared helpers + 9 per-pane contract tests + the ruler + 6 named red tests.
- Modify: `test/e2e-browser/playwright.config.ts` — add the spec to `RUST_ONLY_SPECS` (~lines 82-89) and to the `rust-chromium` project's `testMatch` (~lines 138-203).
- Modify: `test/e2e-browser/fixtures/fake-opencode-terminal.mjs` — add the opt-in `FAKE_OPENCODE_TERMINAL_ROW_GATE_PATH` determinism gate (Task 10 Step 2).

Everything else (server, client, extensions) is read-only.

---

### Task 1: Spec scaffold, config registration, shared helpers, and Contract A (shell terminal)

**Files:**
- Create: `test/e2e-browser/specs/restore-contract-wall-rust.spec.ts`
- Modify: `test/e2e-browser/playwright.config.ts`

**Interfaces:**
- Consumes: `RustServer`, `TestServerInfo` from `../helpers/rust-server.js`; `test`, `expect` from `../helpers/fixtures.js`; `TestHarness` from `../helpers/test-harness.js`; `openPanePicker` from `../helpers/pane-picker.js`.
- Produces (used verbatim by Tasks 2–10, all in this same file):
  - `bootWall(page, options?: { env?: Record<string,string>; setupHome?: (homeDir: string) => Promise<void> }): Promise<{ server: RustServer; info: TestServerInfo; harness: TestHarness }>`
  - `waitForWsReady(page: Page, timeoutMs?: number): Promise<void>`
  - `flushPersistence(page: Page): Promise<void>`
  - `reloadAndReconnect(page: Page, harness: TestHarness): Promise<void>`
  - `seedWallConfig(input: { providers: string[]; freshAgent?: boolean }): (homeDir: string) => Promise<void>`
  - `installFakeCli(source: string, binName: string, binDir: string): Promise<string>`
  - `readArgvLog(logPath: string): Promise<Array<{ argv: string[] }>>`
  - `hasResumePair(argv: string[], sessionId: string): boolean` (codex `resume <id>` shape)
  - `hasFlagPair(argv: string[], flag: string, value: string): boolean` (claude `--resume <id>` / opencode `--session <id>` shape)
  - `readServerLogs(logsDir: string): Promise<string>`
  - `selectShellIfPickerShowing(page: Page): Promise<void>`
  - `collectLeaves(node: any): any[]`, `findLeavesByMode(layout: any, mode: string): any[]`, `findFreshAgentLeaf(node: any): any`, `leafDurableIdentity(leaf: any): string | undefined`
  - `restApiHeaders(info: TestServerInfo): Record<string,string>`
  - `createTabViaRest(info: TestServerInfo, body: object): Promise<string>` (returns tabId)

- [ ] **Step 1: Register the spec in the Playwright config**

Open `test/e2e-browser/playwright.config.ts`. Find the `RUST_ONLY_SPECS`
array (~lines 82-89; every match-all project `testIgnore`s these) and the
`rust-chromium` project's `testMatch` array (~lines 138-203). Every existing
entry carries a comment citing its plan doc — match that convention. Add to
**both** arrays:

```ts
  // Restore-resilience contract wall (P0.1 "the ruler") -- imports RustServer
  // directly for restartAbrupt(); see docs/plans/2026-07-24-restore-contract-wall.md
  /restore-contract-wall-rust\.spec\.ts/,
```

- [ ] **Step 2: Create the spec scaffold with the file-level doc comment and all shared helpers**

Create `test/e2e-browser/specs/restore-contract-wall-rust.spec.ts`:

```ts
/**
 * RESTORE CONTRACT WALL -- P0.1 "the ruler" from
 * docs/plans/2026-07-24-restart-resilience-architecture-analysis.md (§5).
 *
 * One spec that creates every pane type live against fake CLIs, SIGKILLs the
 * Rust server (RustServer.restartAbrupt()), restarts it on the same
 * home/port/token, reconnects, and asserts each pane's restore contract per
 * plan §2. Contracts that today's architecture cannot satisfy are pinned with
 * test.fail(<cond>, '<plan item>: <reason>') so the suite is CI-green while
 * the wall stays honest.
 *
 * FLIP INSTRUCTION for whoever lands a pinned plan item: Playwright turns an
 * unexpected PASS of a test.fail()-annotated test into a hard failure -- that
 * is the signal to DELETE the test.fail() line for your item and let the
 * assertion run as a normal (green) expectation. Never widen a pin; never
 * convert a pin to test.fixme (fixme'd tests produce no evidence).
 *
 * Rust-only: registered in RUST_ONLY_SPECS + rust-chromium testMatch, because
 * restartAbrupt() exists only on RustServer.
 *
 * Helpers are copied, not imported, per this suite's per-spec-ownership
 * convention (donors: compound-restart-rust.spec.ts,
 * opencode-terminal-restore-rust.spec.ts, restore-double-restart.spec.ts,
 * freshopencode-restart-recovery.spec.ts).
 */
import { test, expect } from '../helpers/fixtures.js'
import { RustServer, type TestServerInfo } from '../helpers/rust-server.js'
import { TestHarness } from '../helpers/test-harness.js'
import { openPanePicker } from '../helpers/pane-picker.js'
import type { Page } from '@playwright/test'
import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

// ESM project ("type": "module" in package.json): __dirname does not exist in
// ESM modules, so derive it -- same convention as every fixture-referencing
// donor spec (e.g. compound-restart-rust.spec.ts:49-51).
const __filename = fileURLToPath(import.meta.url)
const __dirname = path.dirname(__filename)

const FAKE_CODEX_CLI_SOURCE = path.resolve(__dirname, '../fixtures/fake-codex-cli.mjs')
const FAKE_OPENCODE_TERMINAL_SOURCE = path.resolve(__dirname, '../fixtures/fake-opencode-terminal.mjs')
const FAKE_OPENCODE_SIDECAR_SOURCE = path.resolve(__dirname, '../fixtures/fake-opencode.cjs')
const FAKE_CLAUDE_CLI_SOURCE = path.resolve(__dirname, '../fixtures/fake-claude-cli.mjs')
const FAKE_CLAUDE_SIDECAR_SOURCE = path.resolve(__dirname, '../fixtures/fake-claude-sidecar.mjs')
const FAKE_CODEX_APP_SERVER_SOURCE = path.resolve(
  __dirname,
  '../../fixtures/coding-cli/codex-app-server/fake-app-server.mjs',
)

// ---------------------------------------------------------------------------
// Shared helpers (per-spec copies -- see file doc comment)
// ---------------------------------------------------------------------------

/** Copy a fixture into <binDir>/<binName> and make it executable. */
async function installFakeCli(source: string, binName: string, binDir: string): Promise<string> {
  await fs.mkdir(binDir, { recursive: true })
  const target = path.join(binDir, binName)
  await fs.copyFile(source, target)
  await fs.chmod(target, 0o755)
  return target
}

/** Read a fake CLI's argv-log JSONL (empty array if not yet written). */
async function readArgvLog(logPath: string): Promise<Array<{ argv: string[] }>> {
  const raw = await fs.readFile(logPath, 'utf8').catch(() => '')
  if (!raw) return []
  return raw.trim().split('\n').filter(Boolean).map((line) => JSON.parse(line) as { argv: string[] })
}

/** True when argv contains the adjacent pair `resume <sessionId>` (codex shape). */
function hasResumePair(argv: string[], sessionId: string): boolean {
  const idx = argv.indexOf('resume')
  return idx >= 0 && argv[idx + 1] === sessionId
}

/** True when argv contains the adjacent pair `<flag> <value>` (claude --resume / opencode --session). */
function hasFlagPair(argv: string[], flag: string, value: string): boolean {
  const idx = argv.indexOf(flag)
  return idx >= 0 && argv[idx + 1] === value
}

/** Concatenated content of every server log file in the fixture's logs dir. */
async function readServerLogs(logsDir: string): Promise<string> {
  const names = await fs.readdir(logsDir).catch(() => [] as string[])
  let combined = ''
  for (const name of names) {
    combined += await fs.readFile(path.join(logsDir, name), 'utf8').catch(() => '')
  }
  return combined
}

/** Dismiss the initial pane-type picker by choosing the first visible shell. */
async function selectShellIfPickerShowing(page: Page): Promise<void> {
  const picker = page.getByRole('toolbar', { name: /pane type picker/i }).last()
  if (!(await picker.isVisible().catch(() => false))) return
  for (const name of ['Shell', 'WSL', 'CMD', 'PowerShell', 'Bash']) {
    const option = picker.getByRole('button', { name: new RegExp(`^${name}$`, 'i') })
    if (await option.isVisible().catch(() => false)) {
      await option.click({ force: true })
      return
    }
  }
}

/** Poll the in-page harness until the WS transport reports 'ready'. */
async function waitForWsReady(page: Page, timeoutMs = 60_000): Promise<void> {
  await expect(async () => {
    const status = await page.evaluate(
      () => (window as any).__FRESHELL_TEST_HARNESS__?.getWsReadyState(),
    )
    expect(status).toBe('ready')
  }).toPass({ timeout: timeoutMs })
}

/** Force the persistence middleware to write localStorage NOW (pre-reload). */
async function flushPersistence(page: Page): Promise<void> {
  await page.evaluate(() => {
    ;(window as any).__FRESHELL_TEST_HARNESS__?.dispatch({ type: 'persist/flushNow' })
  })
}

async function reloadAndReconnect(page: Page, harness: TestHarness): Promise<void> {
  await page.reload({ waitUntil: 'domcontentloaded' })
  await harness.waitForHarness()
  await harness.waitForConnection()
}

/** Idempotent .freshell/config.json seed (setupHome re-runs on every boot). */
function seedWallConfig(input: {
  providers: string[]
  freshAgent?: boolean
}): (homeDir: string) => Promise<void> {
  return async (homeDir: string) => {
    const freshellDir = path.join(homeDir, '.freshell')
    await fs.mkdir(freshellDir, { recursive: true })
    await fs.writeFile(
      path.join(freshellDir, 'config.json'),
      JSON.stringify(
        {
          version: 1,
          settings: {
            codingCli: { enabledProviders: input.providers },
            ...(input.freshAgent ? { freshAgent: { enabled: true } } : {}),
          },
        },
        null,
        2,
      ),
    )
  }
}

/** Boot an owned RustServer, navigate, and wait for harness + WS. */
async function bootWall(
  page: Page,
  options: {
    env?: Record<string, string>
    setupHome?: (homeDir: string) => Promise<void>
  } = {},
): Promise<{ server: RustServer; info: TestServerInfo; harness: TestHarness }> {
  const server = new RustServer({ env: options.env, setupHome: options.setupHome })
  const info = await server.start()
  await page.goto(`${info.baseUrl}/?token=${info.token}&e2e=1`)
  const harness = new TestHarness(page)
  await harness.waitForHarness()
  await harness.waitForConnection()
  return { server, info, harness }
}

// --- layout tree walkers (donor: opencode-terminal-restore-rust.spec.ts) ---

function collectLeaves(node: any): any[] {
  if (!node) return []
  if (node.type === 'leaf') return [node]
  if (node.type === 'split') return (node.children ?? []).flatMap(collectLeaves)
  return []
}

function findLeavesByMode(layout: any, mode: string): any[] {
  return collectLeaves(layout).filter((leaf) => leaf?.content?.mode === mode)
}

function findFreshAgentLeaf(node: any): any {
  if (!node) return null
  if (node.type === 'leaf' && node.content?.kind === 'fresh-agent') return node
  if (node.type === 'split') {
    for (const child of node.children ?? []) {
      const found = findFreshAgentLeaf(child)
      if (found) return found
    }
  }
  return null
}

function leafDurableIdentity(leaf: any): string | undefined {
  return (
    leaf?.content?.sessionId ??
    leaf?.content?.sessionRef?.sessionId ??
    leaf?.content?.resumeSessionId
  )
}

// --- REST helpers (donor: continuity-smoke.spec.ts / agent-continuity-matrix) ---

function restApiHeaders(info: TestServerInfo): Record<string, string> {
  return { 'x-auth-token': info.token, 'content-type': 'application/json' }
}

/** POST /api/tabs; returns the created tabId (envelope is {status,data}). */
async function createTabViaRest(info: TestServerInfo, body: object): Promise<string> {
  const res = await fetch(`${info.baseUrl}/api/tabs`, {
    method: 'POST',
    headers: restApiHeaders(info),
    body: JSON.stringify(body),
  })
  const payload = await res.json()
  expect(res.ok, `POST /api/tabs: ${JSON.stringify(payload)}`).toBe(true)
  const tabId = payload?.data?.tabId
  expect(tabId, 'POST /api/tabs envelope data.tabId').toBeTruthy()
  return tabId as string
}

// ---------------------------------------------------------------------------
// The wall
// ---------------------------------------------------------------------------

test.describe('Restore Contract Wall (P0.1)', () => {
  test.setTimeout(180_000)
})
```

- [ ] **Step 3: Add Contract A — shell terminal — inside the describe block**

Per plan §2.1: after SIGKILL+restart a shell pane comes back as a **fresh
shell in its `initialCwd`** (scrollback loss is by design — do not assert on
it). Append inside `test.describe(...)`:

```ts
  test('shell terminal: SIGKILL restore yields a fresh shell in initialCwd', async ({
    page,
    e2eServerKind,
  }) => {
    expect(e2eServerKind).toBe('rust')
    const sharedRoot = await fs.mkdtemp(path.join(os.tmpdir(), 'freshell-wall-shell-'))
    const projectDir = path.join(sharedRoot, 'project')
    await fs.mkdir(projectDir, { recursive: true })

    const { server, harness, info } = await bootWall(page)
    try {
      await selectShellIfPickerShowing(page)

      // Create the shell pane with a KNOWN cwd via REST so the initialCwd
      // contract is assertable.
      const tabCountBefore = await harness.getTabCount()
      const tabId = await createTabViaRest(info, { mode: 'shell', cwd: projectDir })
      await expect(async () => {
        expect(await harness.getTabCount()).toBe(tabCountBefore + 1)
      }).toPass({ timeout: 15_000 })

      const terminalIdBefore: string = await expect
        .poll(async () => (await harness.getPaneLayout(tabId))?.content?.terminalId ?? null, {
          timeout: 20_000,
        })
        .not.toBeNull()
        .then(async () => (await harness.getPaneLayout(tabId))?.content?.terminalId)
      expect(terminalIdBefore).toBeTruthy()

      // Prove the shell is interactive before the kill.
      await page.locator('.xterm').last().click()
      await page.keyboard.type('echo wall-shell-alive')
      await page.keyboard.press('Enter')
      await expect
        .poll(async () => {
          const buffer = await harness.getTerminalBuffer(terminalIdBefore)
          return typeof buffer === 'string' && buffer.replace(/\n/g, '').includes('wall-shell-alive')
        }, { timeout: 15_000 })
        .toBe(true)

      // --- SIGKILL + revive on the same disk state; live client reconnects. ---
      await server.restartAbrupt()
      await waitForWsReady(page)

      // CONTRACT §2.1: pane recreates (new terminalId), never status:error.
      const terminalIdAfter: string = await expect
        .poll(async () => {
          const tid = (await harness.getPaneLayout(tabId))?.content?.terminalId ?? null
          return tid && tid !== terminalIdBefore ? tid : null
        }, { timeout: 30_000 })
        .not.toBeNull()
        .then(async () => (await harness.getPaneLayout(tabId))?.content?.terminalId)
      expect((await harness.getPaneLayout(tabId))?.content?.status).not.toBe('error')

      // CONTRACT §2.1: the fresh shell starts in the pane's opened directory.
      await page.locator('.xterm').last().click()
      await page.keyboard.type('pwd')
      await page.keyboard.press('Enter')
      await expect
        .poll(async () => {
          const buffer = await harness.getTerminalBuffer(terminalIdAfter)
          return typeof buffer === 'string' && buffer.replace(/\n/g, '').includes(projectDir)
        }, { timeout: 15_000 })
        .toBe(true)
    } finally {
      await server.stop()
      await fs.rm(sharedRoot, { recursive: true, force: true })
    }
  })
```

- [ ] **Step 4: Verify project selection without running**

```bash
cd /home/dan/code/freshell/.worktrees/restore-contract-wall
npx playwright test --config test/e2e-browser/playwright.config.ts --project=rust-chromium --list restore-contract-wall
npx playwright test --config test/e2e-browser/playwright.config.ts --project=chromium --list restore-contract-wall || true
```

Expected: the first command lists `shell terminal: SIGKILL restore yields a
fresh shell in initialCwd` under `rust-chromium`; the second lists **no**
tests from this spec (RUST_ONLY_SPECS ignore works). Note: `--list` still
runs `globalSetup` (client+server build) — the first invocation is slow.

- [ ] **Step 5: Run the shell contract test**

```bash
npx playwright test --config test/e2e-browser/playwright.config.ts \
  --project=rust-chromium restore-contract-wall -g "shell terminal"
```

Expected: PASS (predicted green — client-driven shell recreate works today).
If red, apply the decision rule (fix the test first; if faithful, pin per the
table: P1.7).

- [ ] **Step 6: Commit**

```bash
git add test/e2e-browser/specs/restore-contract-wall-rust.spec.ts test/e2e-browser/playwright.config.ts
git commit -m "test(e2e): scaffold restore contract wall with shell terminal SIGKILL contract"
```

---

### Task 2: Fake claude terminal CLI + Contract B (claude terminal resume)

**Files:**
- Create: `test/e2e-browser/fixtures/fake-claude-cli.mjs`
- Modify: `test/e2e-browser/specs/restore-contract-wall-rust.spec.ts` (append test)

**Interfaces:**
- Consumes: `bootWall`, `seedWallConfig`, `installFakeCli`, `readArgvLog`, `hasFlagPair`, `waitForWsReady`, `selectShellIfPickerShowing`, `collectLeaves`, `findLeavesByMode`, `FAKE_CLAUDE_CLI_SOURCE` (Task 1); `openPanePicker` from `../helpers/pane-picker.js`.
- Produces: the fixture env contract `FAKE_CLAUDE_ARGV_LOG` (JSONL `{pid,t,argv}`) and buffer markers `claude: session <id> started` / `claude: resumed session <id>` — reused by Task 9.

- [ ] **Step 1: Create the fixture, mirroring `fake-codex-cli.mjs`**

First open `test/e2e-browser/fixtures/fake-codex-cli.mjs` and mirror its
import style exactly (it is installed as an extensionless executable, so its
module style is proven to work under the repo's Node). Create
`test/e2e-browser/fixtures/fake-claude-cli.mjs`:

```js
#!/usr/bin/env node
// Fake `claude` terminal CLI for e2e. Mirrors fake-codex-cli.mjs: appends
// {pid,t,argv} JSONL to FAKE_CLAUDE_ARGV_LOG on every invocation, prints a
// greppable marker, then stays "running" via stdin.resume().
//
// Real claude launch shapes (extensions/claude-code/freshell.json):
//   fresh:  claude ... --session-id <uuid>   (pre-allocated at t=0 by the
//           WS/picker create path ONLY, crates/freshell-ws/src/terminal.rs:969-982;
//           REST POST /api/tabs never mints one, terminal_tabs.rs:756-768)
//   resume: claude ... --resume <id>
// Flags are searched anywhere in argv (resume args are appended LAST by the
// launch builder), matching fake-codex-cli.mjs's rationale.
import fs from 'node:fs'
import path from 'node:path'

const argv = process.argv.slice(2)

function appendArgvLog() {
  const logPath = process.env.FAKE_CLAUDE_ARGV_LOG
  if (!logPath) return
  fs.mkdirSync(path.dirname(logPath), { recursive: true })
  fs.appendFileSync(logPath, `${JSON.stringify({ pid: process.pid, t: Date.now(), argv })}\n`)
}
appendArgvLog()

const resumeIdx = argv.indexOf('--resume')
const startIdx = argv.indexOf('--session-id')

if (resumeIdx !== -1) {
  process.stdout.write(`claude: resumed session ${argv[resumeIdx + 1] ?? ''}\r\n`)
} else if (startIdx !== -1) {
  process.stdout.write(`claude: session ${argv[startIdx + 1] ?? ''} started\r\n`)
} else {
  process.stdout.write('claude> \r\n')
}
process.stdin.resume()
```

(If `fake-codex-cli.mjs` uses `require()` instead of `import`, mirror that —
the copy is executed extensionless and must match the proven style.)

- [ ] **Step 2: Smoke the fixture standalone (proves extensionless execution)**

```bash
cd /home/dan/code/freshell/.worktrees/restore-contract-wall
TMP=$(mktemp -d)
cp test/e2e-browser/fixtures/fake-claude-cli.mjs "$TMP/claude" && chmod 755 "$TMP/claude"
FAKE_CLAUDE_ARGV_LOG="$TMP/log.jsonl" timeout 2 "$TMP/claude" --resume wall-test-id || true
cat "$TMP/log.jsonl"
rm -rf "$TMP"
```

Expected: stdout shows `claude: resumed session wall-test-id`; the log file
contains one JSONL line with `"argv":["--resume","wall-test-id"]`. If Node
rejects the extensionless ESM file, rewrite the fixture in CJS
(`const fs = require('node:fs')` etc.) and re-run.

- [ ] **Step 3: Append Contract B to the describe block**

Per plan §2.2: fresh create pre-allocates `--session-id <uuid>` at t=0; after
SIGKILL+restart the pane must relaunch with `--resume <that uuid>`. The
creation path MUST be the pane picker (WS `terminal.create`): only the WS
path pre-allocates the fresh-claude session id
(`crates/freshell-ws/src/terminal.rs:969-982`); the REST `POST /api/tabs`
path never mints one (`LaunchIntent::Resume`,
`crates/freshell-freshagent/src/terminal_tabs.rs:756-768`). Poll the
PANE-level `content.sessionRef` — the fold happens in the mounted pane's own
`terminal.created` handler (`src/components/TerminalView.tsx:3729-3742` →
`src/store/panesSlice.ts:1705-1707`), not at tab level. Per-provider
post-kill argv shapes: claude `--resume <id>`, opencode `--session <id>`,
codex `resume <id>`.

```ts
  test('claude terminal: pre-allocated session resumes with --resume after SIGKILL', async ({
    page,
    e2eServerKind,
  }) => {
    expect(e2eServerKind).toBe('rust')
    const sharedRoot = await fs.mkdtemp(path.join(os.tmpdir(), 'freshell-wall-claude-term-'))
    const projectDir = path.join(sharedRoot, 'project')
    await fs.mkdir(projectDir, { recursive: true })
    const argLogPath = path.join(sharedRoot, 'claude-argv.jsonl')
    const fakeClaudePath = await installFakeCli(
      FAKE_CLAUDE_CLI_SOURCE,
      'claude',
      path.join(sharedRoot, 'bin'),
    )

    const { server, harness } = await bootWall(page, {
      env: { CLAUDE_CMD: fakeClaudePath, FAKE_CLAUDE_ARGV_LOG: argLogPath },
      setupHome: seedWallConfig({ providers: ['claude'] }),
    })
    try {
      await selectShellIfPickerShowing(page)
      const tabId = (await harness.getActiveTabId())!

      // Fresh claude pane via the PICKER (WS path) -> server pre-allocates
      // --session-id (terminal.rs:969-982). REST would not (PF1,
      // terminal_tabs.rs:756-768). Candidate dirs can be EMPTY on a clean
      // isolated HOME (crates/freshell-server/src/files.rs:15-26 -- no $HOME
      // fallback), so TYPE the cwd instead of clicking a suggestion (donor:
      // freshopencode-restart-recovery.spec.ts:117-124).
      const beforeIds = new Set(
        findLeavesByMode(await harness.getPaneLayout(tabId), 'claude').map((l) => l.id),
      )
      const picker = await openPanePicker(page)
      await picker.getByRole('button', { name: /^Claude CLI$/i }).click({ force: true })
      const dirInput = page.getByRole('combobox', { name: /Starting directory for Claude/i })
      await expect(dirInput).toBeVisible({ timeout: 15_000 })
      await dirInput.fill(projectDir)
      await dirInput.press('Enter')

      // The new claude pane is a SPLIT in the active tab -- track it by leaf id.
      const claudeLeaf = await expect
        .poll(async () => {
          const layout = await harness.getPaneLayout(tabId)
          const newLeaf = findLeavesByMode(layout, 'claude').find((l) => !beforeIds.has(l.id))
          return newLeaf?.content?.terminalId ? newLeaf : null
        }, { timeout: 20_000 })
        .not.toBeNull()
        .then(async () => {
          const layout = await harness.getPaneLayout(tabId)
          return findLeavesByMode(layout, 'claude').find((l) => !beforeIds.has(l.id))!
        })
      const paneId: string = claudeLeaf.id
      const terminalIdBefore: string = claudeLeaf.content.terminalId
      const claudeContent = async () => {
        const layout = await harness.getPaneLayout(tabId)
        return collectLeaves(layout).find((l) => l.id === paneId)?.content
      }

      // t=0 identity: the FIRST spawn already carries --session-id <uuid>.
      const preallocatedId: string = await expect
        .poll(async () => {
          const entries = await readArgvLog(argLogPath)
          const withId = entries.find((e) => e.argv.includes('--session-id'))
          if (!withId) return null
          return withId.argv[withId.argv.indexOf('--session-id') + 1] ?? null
        }, { timeout: 20_000 })
        .not.toBeNull()
        .then(async () => {
          const entries = await readArgvLog(argLogPath)
          const withId = entries.find((e) => e.argv.includes('--session-id'))!
          return withId.argv[withId.argv.indexOf('--session-id') + 1]!
        })
      expect(preallocatedId).toMatch(/^[0-9a-f-]{36}$/)

      // Client persisted the identity ("restore info set quickly", §2.2).
      // PANE-level content.sessionRef: the fold happens in the mounted pane's
      // own terminal.created handler (TerminalView.tsx:3729-3742 ->
      // panesSlice.ts:1705-1707), so assert on the leaf, never the tab.
      await expect
        .poll(async () => (await claudeContent())?.sessionRef?.sessionId ?? null, {
          timeout: 20_000,
        })
        .toBe(preallocatedId)

      const argvCountBeforeKill = (await readArgvLog(argLogPath)).length

      // --- SIGKILL + revive; live client recovers on its own reconnect. ---
      await server.restartAbrupt()
      await waitForWsReady(page)

      // CONTRACT §2.2: new terminalId, resumed with --resume <preallocatedId>.
      await expect
        .poll(async () => {
          const tid = (await claudeContent())?.terminalId ?? null
          return tid && tid !== terminalIdBefore ? tid : null
        }, { timeout: 30_000 })
        .not.toBeNull()
      await expect
        .poll(async () => {
          const entries = await readArgvLog(argLogPath)
          return entries
            .slice(argvCountBeforeKill)
            .some((e) => hasFlagPair(e.argv, '--resume', preallocatedId))
        }, { timeout: 30_000 })
        .toBe(true)

      const terminalIdAfter = (await claudeContent())?.terminalId
      await expect
        .poll(async () => {
          const buffer = await harness.getTerminalBuffer(terminalIdAfter)
          const unwrapped = typeof buffer === 'string' ? buffer.replace(/\n/g, '') : ''
          return unwrapped.includes(`claude: resumed session ${preallocatedId}`)
        }, { timeout: 20_000 })
        .toBe(true)
      expect((await claudeContent())?.status).not.toBe('error')
      expect((await claudeContent())?.sessionRef?.sessionId).toBe(preallocatedId)
    } finally {
      await server.stop()
      await fs.rm(sharedRoot, { recursive: true, force: true })
    }
  })
```

- [ ] **Step 4: Run it**

```bash
npx playwright test --config test/e2e-browser/playwright.config.ts \
  --project=rust-chromium restore-contract-wall -g "claude terminal"
```

Expected: PASS (client held `sessionRef`; §2.2 says this path works). The
picker/WS path is the ONLY correct creation path here — do NOT "simplify" to
`createTabViaRest`: the REST path never pre-allocates `--session-id`
(`terminal_tabs.rs:756-768`), so the t=0 poll would time out by design. The
picker button's accessible name is the extension registry label verbatim —
`aria-label={option.label}` (`PanePicker.tsx:277`) fed from
`extensions/claude-code/freshell.json` `"label": "Claude CLI"` via
`cliConfigToOption` (`coding-cli-utils.ts:15-24`) — hence the anchored
`/^Claude CLI$/i` (donor: `truly-idle-alerting.spec.ts:117`); `/^Claude$/i`
would never match. If the button never RENDERS at all, then check the
`enabledProviders: ['claude']` seed and `CLAUDE_CMD` detection (the picker
requires availableClis + enabledProviders + not-disabled). If red with a
faithful test, pin P0.4 (§2.2).

- [ ] **Step 5: Commit**

```bash
git add test/e2e-browser/fixtures/fake-claude-cli.mjs test/e2e-browser/specs/restore-contract-wall-rust.spec.ts
git commit -m "test(e2e): fake claude CLI fixture + claude terminal SIGKILL resume contract"
```

---

### Task 3: Contract C (codex terminal resume)

**Files:**
- Modify: `test/e2e-browser/specs/restore-contract-wall-rust.spec.ts` (append helper + test)

**Interfaces:**
- Consumes: Task 1 helpers; `FAKE_CODEX_CLI_SOURCE`.
- Produces: `seedCodexHome(sessionId, sessionTitle, projectDir): (homeDir: string) => Promise<void>` — reused by Tasks 9 and 10.

- [ ] **Step 1: Append the codex seed helper (donor: `compound-restart-rust.spec.ts:104-147` — diff against it and mirror any drift)**

```ts
/** Seed ~/.codex/sessions/<id>.jsonl so the sidebar shows a resumable codex session. */
function seedCodexHome(
  sessionId: string,
  sessionTitle: string,
  projectDir: string,
): (homeDir: string) => Promise<void> {
  return async (homeDir: string) => {
    await seedWallConfig({ providers: ['codex'] })(homeDir)
    const codexSessionsDir = path.join(homeDir, '.codex', 'sessions')
    await fs.mkdir(codexSessionsDir, { recursive: true })
    const lines = [
      JSON.stringify({
        timestamp: '2026-07-21T08:00:00.000Z',
        type: 'session_meta',
        payload: { id: sessionId, cwd: projectDir },
      }),
      JSON.stringify({
        timestamp: '2026-07-21T08:00:01.000Z',
        type: 'response_item',
        payload: {
          type: 'message',
          role: 'user',
          content: [{ type: 'input_text', text: `${sessionTitle} request 1` }],
        },
      }),
      JSON.stringify({
        timestamp: '2026-07-21T08:00:02.000Z',
        type: 'response_item',
        payload: {
          type: 'message',
          role: 'assistant',
          content: [{ type: 'output_text', text: `${sessionTitle} reply 1` }],
        },
      }),
      JSON.stringify({
        timestamp: '2026-07-21T08:00:03.000Z',
        type: 'response_item',
        payload: {
          type: 'message',
          role: 'user',
          content: [{ type: 'input_text', text: `${sessionTitle} request 2` }],
        },
      }),
    ]
    await fs.writeFile(path.join(codexSessionsDir, `${sessionId}.jsonl`), `${lines.join('\n')}\n`)
  }
}
```

- [ ] **Step 2: Append Contract C**

Per plan §2.3: a codex pane whose client still holds `sessionRef` must
relaunch with `resume <id>` after SIGKILL (this is exactly the
compound-restart MODE A scenario — that spec is the donor; this wall entry is
the per-pane ruler line for it).

```ts
  test('codex terminal: sessionRef-bound pane resumes with `resume <id>` after SIGKILL', async ({
    page,
    e2eServerKind,
  }) => {
    expect(e2eServerKind).toBe('rust')
    const CODEX_SESSION_ID = '11111111-2222-4333-8444-555555555555'
    const SESSION_TITLE = 'wall codex session'
    const sharedRoot = await fs.mkdtemp(path.join(os.tmpdir(), 'freshell-wall-codex-term-'))
    const projectDir = path.join(sharedRoot, 'project')
    await fs.mkdir(projectDir, { recursive: true })
    const argLogPath = path.join(sharedRoot, 'codex-argv.jsonl')
    const fakeCodexPath = await installFakeCli(
      FAKE_CODEX_CLI_SOURCE,
      'codex',
      path.join(sharedRoot, 'bin'),
    )

    const { server, harness } = await bootWall(page, {
      env: { CODEX_CMD: fakeCodexPath, FAKE_CODEX_ARGV_LOG: argLogPath },
      setupHome: seedCodexHome(CODEX_SESSION_ID, SESSION_TITLE, projectDir),
    })
    try {
      await selectShellIfPickerShowing(page)

      // Open the seeded historical session from the sidebar (identity lands
      // in content.sessionRef only -- the incident shape).
      const sessionList = page.getByTestId('sidebar-session-list')
      await expect(sessionList).toBeVisible({ timeout: 15_000 })
      const sessionItem = page.getByText(SESSION_TITLE, { exact: false }).first()
      await expect(sessionItem).toBeVisible({ timeout: 15_000 })
      const tabCountBefore = await harness.getTabCount()
      await sessionItem.click()
      await expect(async () => {
        expect(await harness.getTabCount()).toBe(tabCountBefore + 1)
      }).toPass({ timeout: 15_000 })
      const tabId = (await harness.getActiveTabId())!

      const terminalIdBefore: string = await expect
        .poll(async () => (await harness.getPaneLayout(tabId))?.content?.terminalId ?? null, {
          timeout: 20_000,
        })
        .not.toBeNull()
        .then(async () => (await harness.getPaneLayout(tabId))?.content?.terminalId)
      expect((await harness.getPaneLayout(tabId))?.content?.sessionRef?.sessionId).toBe(
        CODEX_SESSION_ID,
      )

      // Create-time resume proof.
      await expect
        .poll(async () => {
          const entries = await readArgvLog(argLogPath)
          return entries.some((e) => hasResumePair(e.argv, CODEX_SESSION_ID))
        }, { timeout: 20_000 })
        .toBe(true)

      const argvCountBeforeKill = (await readArgvLog(argLogPath)).length

      // --- SIGKILL + revive; live client recovers on its own reconnect. ---
      await server.restartAbrupt()
      await waitForWsReady(page)

      // CONTRACT §2.3: new terminalId, re-resumed argv, same sessionRef.
      await expect
        .poll(async () => {
          const tid = (await harness.getPaneLayout(tabId))?.content?.terminalId ?? null
          return tid && tid !== terminalIdBefore ? tid : null
        }, { timeout: 30_000 })
        .not.toBeNull()
      await expect
        .poll(async () => {
          const entries = await readArgvLog(argLogPath)
          return entries
            .slice(argvCountBeforeKill)
            .some((e) => hasResumePair(e.argv, CODEX_SESSION_ID))
        }, { timeout: 30_000 })
        .toBe(true)
      const contentAfter = (await harness.getPaneLayout(tabId))?.content
      expect(contentAfter?.status).not.toBe('error')
      expect(contentAfter?.sessionRef?.sessionId).toBe(CODEX_SESSION_ID)
      const terminalIdAfter = contentAfter?.terminalId
      await expect
        .poll(async () => {
          const buffer = await harness.getTerminalBuffer(terminalIdAfter)
          const unwrapped = typeof buffer === 'string' ? buffer.replace(/\n/g, '') : ''
          return unwrapped.includes(`codex: resumed session ${CODEX_SESSION_ID}`)
        }, { timeout: 20_000 })
        .toBe(true)
    } finally {
      await server.stop()
      await fs.rm(sharedRoot, { recursive: true, force: true })
    }
  })
```

- [ ] **Step 3: Run it**

```bash
npx playwright test --config test/e2e-browser/playwright.config.ts \
  --project=rust-chromium restore-contract-wall -g "codex terminal"
```

Expected: PASS (compound-restart-rust proves this today). If the sidebar row
never appears, diff your `seedCodexHome` against
`compound-restart-rust.spec.ts:104-147` — the seed shape is the usual culprit.
If faithful and red: pin P0.3 (§2.3).

- [ ] **Step 4: Commit**

```bash
git add test/e2e-browser/specs/restore-contract-wall-rust.spec.ts
git commit -m "test(e2e): codex terminal SIGKILL resume contract in the wall"
```

---

### Task 4: Contract D (opencode terminal resume, association settled)

**Files:**
- Modify: `test/e2e-browser/specs/restore-contract-wall-rust.spec.ts` (append helper + test)

**Interfaces:**
- Consumes: Task 1 helpers; `FAKE_OPENCODE_TERMINAL_SOURCE`; `openPanePicker`.
- Produces: `openOpencodePaneAndGetLeaf(page, harness, tabId): Promise<any>` and `findLeafById(harness, tabId, paneId): Promise<any>` — reused by Task 10 (`SIGKILL-inside-locator-window`).

- [ ] **Step 1: Append the opencode pane helpers (donor: `opencode-terminal-restore-rust.spec.ts:104-146`)**

```ts
async function openOpencodePane(page: Page): Promise<void> {
  const picker = await openPanePicker(page)
  await picker.getByRole('button', { name: /^OpenCode$/i }).click({ force: true })
  await page.getByRole('combobox', { name: /Starting directory for OpenCode/i }).press('Enter')
}

async function openOpencodePaneAndGetLeaf(
  page: Page,
  harness: TestHarness,
  tabId: string,
): Promise<any> {
  const before = findLeavesByMode(await harness.getPaneLayout(tabId), 'opencode')
  const beforeIds = new Set(before.map((leaf) => leaf.id))
  await openOpencodePane(page)
  await expect(page.locator('.xterm').last()).toBeVisible({ timeout: 15_000 })
  return expect
    .poll(async () => {
      const layout = await harness.getPaneLayout(tabId)
      const newLeaf = findLeavesByMode(layout, 'opencode').find((leaf) => !beforeIds.has(leaf.id))
      return newLeaf?.content?.terminalId ? newLeaf : null
    }, { timeout: 15_000 })
    .not.toBeNull()
    .then(async () => {
      const layout = await harness.getPaneLayout(tabId)
      return findLeavesByMode(layout, 'opencode').find((leaf) => !beforeIds.has(leaf.id))
    })
}

async function findLeafById(harness: TestHarness, tabId: string, paneId: string): Promise<any> {
  const layout = await harness.getPaneLayout(tabId)
  return collectLeaves(layout).find((leaf) => leaf.id === paneId) ?? null
}
```

- [ ] **Step 2: Append Contract D**

Per plan §2.4: once the locator has resolved the identity, SIGKILL+restart
must relaunch with `--session <ses_id>`.

```ts
  test('opencode terminal: locator-resolved session resumes with --session after SIGKILL', async ({
    page,
    e2eServerKind,
  }) => {
    expect(e2eServerKind).toBe('rust')
    const sharedRoot = await fs.mkdtemp(path.join(os.tmpdir(), 'freshell-wall-opencode-term-'))
    const argLogPath = path.join(sharedRoot, 'opencode-argv.jsonl')
    const fakeOpencodePath = await installFakeCli(
      FAKE_OPENCODE_TERMINAL_SOURCE,
      'opencode',
      path.join(sharedRoot, 'bin'),
    )

    const { server, harness } = await bootWall(page, {
      env: {
        OPENCODE_CMD: fakeOpencodePath,
        FAKE_OPENCODE_TERMINAL_ARGV_LOG: argLogPath,
      },
      // NOTE: seedWallConfig only overwrites config.json -- it never touches
      // <home>/.local/share/opencode/opencode.db, so the runtime-minted DB
      // survives restartAbrupt()'s setupHome re-run. No symlink needed.
      setupHome: seedWallConfig({ providers: ['opencode'] }),
    })
    try {
      await selectShellIfPickerShowing(page)
      const tabId = (await harness.getActiveTabId())!
      const leaf = await openOpencodePaneAndGetLeaf(page, harness, tabId)
      const paneId: string = leaf.id
      const terminalIdBefore: string = leaf.content.terminalId

      // Mint the session: click the pane, type, press Enter (the fake writes
      // the opencode.db row on its first stdin data event).
      await page.locator('.xterm').last().click()
      await page.keyboard.type('hello wall opencode')
      await page.keyboard.press('Enter')

      // Wait for the locator to associate (identity lands in sessionRef).
      const associatedSessionId: string = await expect
        .poll(async () => {
          const l = await findLeafById(harness, tabId, paneId)
          return l?.content?.sessionRef?.sessionId ?? l?.content?.resumeSessionId ?? null
        }, { timeout: 20_000 })
        .not.toBeNull()
        .then(async () => {
          const l = await findLeafById(harness, tabId, paneId)
          return l?.content?.sessionRef?.sessionId ?? l?.content?.resumeSessionId
        })
      expect(associatedSessionId).toMatch(/^ses_e2e_/)

      const argvCountBeforeKill = (await readArgvLog(argLogPath)).length

      // --- SIGKILL + revive; live client recovers on its own reconnect. ---
      await server.restartAbrupt()
      await waitForWsReady(page)

      // CONTRACT §2.4: new terminalId, resumed via --session <id>.
      await expect
        .poll(async () => {
          const l = await findLeafById(harness, tabId, paneId)
          const tid = l?.content?.terminalId ?? null
          return tid && tid !== terminalIdBefore ? tid : null
        }, { timeout: 30_000 })
        .not.toBeNull()
      await expect
        .poll(async () => {
          const entries = await readArgvLog(argLogPath)
          return entries
            .slice(argvCountBeforeKill)
            .some((e) => hasFlagPair(e.argv, '--session', associatedSessionId))
        }, { timeout: 30_000 })
        .toBe(true)
      const leafAfter = await findLeafById(harness, tabId, paneId)
      expect(leafAfter?.content?.status).not.toBe('error')
      await expect
        .poll(async () => {
          const buffer = await harness.getTerminalBuffer(leafAfter?.content?.terminalId)
          const unwrapped = typeof buffer === 'string' ? buffer.replace(/\n/g, '') : ''
          return unwrapped.includes(`opencode: resumed session ${associatedSessionId}`)
        }, { timeout: 20_000 })
        .toBe(true)
    } finally {
      await server.stop()
      await fs.rm(sharedRoot, { recursive: true, force: true })
    }
  })
```

- [ ] **Step 3: Run it**

```bash
npx playwright test --config test/e2e-browser/playwright.config.ts \
  --project=rust-chromium restore-contract-wall -g "opencode terminal"
```

Expected: PASS (association settled before the kill). If the picker button
never shows, the `enabledProviders: ['opencode']` seed or `OPENCODE_CMD`
detection is the culprit (the picker requires availableClis + enabledProviders
+ not-disabled). If faithful and red: pin P1.10 (§2.4).

- [ ] **Step 4: Commit**

```bash
git add test/e2e-browser/specs/restore-contract-wall-rust.spec.ts
git commit -m "test(e2e): opencode terminal SIGKILL resume contract in the wall"
```

---

### Task 5: Contract E (freshcodex fresh-agent SIGKILL restore)

**Files:**
- Modify: `test/e2e-browser/specs/restore-contract-wall-rust.spec.ts` (append helpers + test)

**Interfaces:**
- Consumes: Task 1 helpers; `FAKE_CODEX_APP_SERVER_SOURCE`; `openPanePicker`; `findFreshAgentLeaf`; `leafDurableIdentity`.
- Produces: `installFakeCodexAppServer(destDir): Promise<string>`, `createFreshcodexPane(page, harness): Promise<void>`, `sendFreshAgentTurn(page, harness, tabId, text): Promise<void>` — reused by Task 9.

- [ ] **Step 1: Append the app-server installer (donor: `restore-matrix.spec.ts:62-92`; a WRAPPER, never a copy — `ws` bare-specifier resolution)**

```ts
async function installFakeCodexAppServer(destDir: string): Promise<string> {
  await fs.mkdir(destDir, { recursive: true })
  const dest = path.join(destDir, 'fake-codex-app-server-wrapper.mjs')
  const wrapper = `#!/usr/bin/env node
import { spawnSync } from 'node:child_process'
const target = ${JSON.stringify(FAKE_CODEX_APP_SERVER_SOURCE)}
const result = spawnSync(process.execPath, [target, ...process.argv.slice(2)], { stdio: 'inherit' })
process.exit(result.status ?? 1)
`
  await fs.writeFile(dest, wrapper, 'utf8')
  await fs.chmod(dest, 0o755)
  return dest
}
```

- [ ] **Step 2: Append the freshcodex creation + turn helpers (donor: `restore-double-restart.spec.ts:148-176` and its turn loop)**

```ts
async function createFreshcodexPane(page: Page, harness: TestHarness): Promise<void> {
  // setAvailableClis is client-only AND gets overwritten by the app
  // bootstrap + /api/platform fetch (App.tsx:572,609). Callers reach this
  // helper only after harness.waitForConnection(), which is what makes the
  // dispatch land AFTER those overwrites (donor ordering:
  // freshopencode-restart-recovery.spec.ts:100-115). Keep it that way.
  await page.evaluate(() => {
    ;(window as any).__FRESHELL_TEST_HARNESS__?.dispatch({
      type: 'connection/setAvailableClis',
      payload: { claude: false, codex: true },
    })
  })
  const picker = await openPanePicker(page)
  await picker.getByRole('button', { name: /^Freshcodex$/i }).click({ force: true })
  // "First option" exists here only because selectShellIfPickerShowing
  // already opened a shell whose live-terminal cwd becomes a candidate dir
  // (/api/files/candidate-dirs returns [] on a truly clean boot -- no $HOME
  // fallback, crates/freshell-server/src/files.rs:15-26). This mirrors the
  // donor exactly (restore-double-restart.spec.ts:148-176); if no option
  // renders, switch to the fill+Enter pattern used by
  // createFreshopencodePane/createFreshclaudePane.
  await page.getByRole('option').first().click()
  await expect(page.locator('[data-context="fresh-agent"]').last()).toBeVisible({
    timeout: 15_000,
  })
}

/** Send one chat turn in the last fresh-agent pane and wait for idle. */
async function sendFreshAgentTurn(
  page: Page,
  harness: TestHarness,
  tabId: string,
  text: string,
): Promise<void> {
  const paneRoot = page.locator('[data-context="fresh-agent"]').last()
  await expect
    .poll(async () => findFreshAgentLeaf(await harness.getPaneLayout(tabId))?.content?.status, {
      timeout: 20_000,
    })
    .toBe('idle')
  const composer = paneRoot.getByRole('textbox', { name: 'Chat message input' })
  await composer.fill(text)
  await paneRoot.getByRole('button', { name: 'Send' }).click()
  await expect
    .poll(async () => findFreshAgentLeaf(await harness.getPaneLayout(tabId))?.content?.status, {
      timeout: 30_000,
    })
    .toBe('idle')
}
```

- [ ] **Step 3: Append Contract E**

Per plan §2.6: freshcodex is the reference implementation — after
SIGKILL+restart+reload the pane must rebind to the SAME durable thread with
history rehydrated (`'Fixture turn'` is the fake's deterministic reply) and a
non-wedged status.

```ts
  test('freshcodex: SIGKILL restore rebinds the same thread with history rehydrated', async ({
    page,
    e2eServerKind,
  }) => {
    expect(e2eServerKind).toBe('rust')
    const sharedRoot = await fs.mkdtemp(path.join(os.tmpdir(), 'freshell-wall-freshcodex-'))
    const fakeCodexPath = await installFakeCodexAppServer(path.join(sharedRoot, 'bin'))

    const { server, harness } = await bootWall(page, {
      env: { CODEX_CMD: fakeCodexPath },
      setupHome: seedWallConfig({ providers: ['codex'], freshAgent: true }),
    })
    try {
      await selectShellIfPickerShowing(page)
      const tabId = (await harness.getActiveTabId())!
      await createFreshcodexPane(page, harness)
      await sendFreshAgentTurn(page, harness, tabId, 'wall freshcodex turn')
      await expect(
        page.locator('[data-context="fresh-agent"]').last().getByText('Fixture turn'),
      ).toBeVisible({ timeout: 20_000 })

      const originalSessionId: string = await expect
        .poll(async () => leafDurableIdentity(findFreshAgentLeaf(await harness.getPaneLayout(tabId))) ?? null, {
          timeout: 20_000,
        })
        .not.toBeNull()
        .then(async () =>
          leafDurableIdentity(findFreshAgentLeaf(await harness.getPaneLayout(tabId)))!,
        )

      await flushPersistence(page)
      await harness.clearSentWsMessages()

      // --- SIGKILL + revive, then reload (full client rehydrate). ---
      await server.restartAbrupt()
      await waitForWsReady(page)
      await reloadAndReconnect(page, harness)

      // CONTRACT §2.6: same durable identity, history rehydrated, not wedged,
      // and every post-reload create targets the ORIGINAL thread.
      const rehydratedTabId = (await harness.getActiveTabId())!
      const rehydratedIdentity: string | undefined = await expect
        .poll(async () => leafDurableIdentity(findFreshAgentLeaf(await harness.getPaneLayout(rehydratedTabId))), {
          timeout: 30_000,
        })
        .not.toBeUndefined()
        .then(async () =>
          leafDurableIdentity(findFreshAgentLeaf(await harness.getPaneLayout(rehydratedTabId))),
        )
      expect(rehydratedIdentity).toBe(originalSessionId)
      await expect(
        page.locator('[data-context="fresh-agent"]').last().getByText('Fixture turn'),
      ).toBeVisible({ timeout: 30_000 })
      const finalLeaf = findFreshAgentLeaf(await harness.getPaneLayout(rehydratedTabId))
      expect(finalLeaf?.content?.status).not.toBe('error')

      const sentAfterReload = await harness.getSentWsMessages()
      const createsAfterReload = sentAfterReload.filter((m: any) => m?.type === 'freshAgent.create')
      for (const create of createsAfterReload) {
        const resumeTarget = (create as any).resumeSessionId ?? (create as any).sessionRef?.sessionId
        expect(resumeTarget).toBe(originalSessionId)
      }
    } finally {
      await server.stop()
      await fs.rm(sharedRoot, { recursive: true, force: true })
    }
  })
```

- [ ] **Step 4: Run it**

```bash
npx playwright test --config test/e2e-browser/playwright.config.ts \
  --project=rust-chromium restore-contract-wall -g "freshcodex"
```

Expected: PASS (§2.6 — three resume paths exist; the fake writes a durable
rollout under `$CODEX_HOME` which survives on the same home). If faithful and
red, pin P1.8 (§2.6) with the observed failure mode.

- [ ] **Step 5: Commit**

```bash
git add test/e2e-browser/specs/restore-contract-wall-rust.spec.ts
git commit -m "test(e2e): freshcodex SIGKILL restore contract in the wall"
```

---

### Task 6: Contract F (freshopencode fresh-agent SIGKILL restore)

**Files:**
- Modify: `test/e2e-browser/specs/restore-contract-wall-rust.spec.ts` (append helpers + test)

**Interfaces:**
- Consumes: Task 1 helpers; `FAKE_OPENCODE_SIDECAR_SOURCE`; `openPanePicker`; `findFreshAgentLeaf`; `leafDurableIdentity`.
- Produces: `createFreshopencodePane(page, cwd): Promise<void>`, `enableFreshOpencode(page, enabledProviders?): Promise<void>` (default `['opencode']`; pass the full provider list when later steps still need other providers' picker buttons — the patch REPLACES the array) — reused by Task 9.

- [ ] **Step 1: Append the freshopencode helpers (donor: `freshopencode-restart-recovery.spec.ts:114-206`)**

```ts
async function enableFreshOpencode(
  page: Page,
  enabledProviders: string[] = ['opencode'],
): Promise<void> {
  // These dispatches are client-only and MUST land AFTER the app bootstrap +
  // /api/platform fetch (App.tsx:572,609 overwrite availableClis). Callers
  // reach this helper only after harness.waitForConnection(), which is the
  // donor's ordering (freshopencode-restart-recovery.spec.ts:100-115).
  //
  // CAUTION: mergeServerSettings REPLACES the enabledProviders array when the
  // key is present (shared/settings.ts:1216-1218) -- it does not union. Any
  // test that needs OTHER providers' picker buttons after this call (e.g. the
  // Task 9 ruler, which still has freshclaude to create) MUST pass the full
  // provider list, or those buttons disappear (PanePicker.tsx:125-152 gates
  // fresh-agent options on enabledProviders.includes(<provider>)).
  await page.evaluate((providers) => {
    const harness = (window as any).__FRESHELL_TEST_HARNESS__
    harness?.dispatch({ type: 'connection/setAvailableClis', payload: { opencode: true } })
    harness?.dispatch({
      type: 'settings/previewServerSettingsPatch',
      payload: { codingCli: { enabledProviders: providers }, freshAgent: { enabled: true } },
    })
  }, enabledProviders)
}

async function createFreshopencodePane(page: Page, cwd: string): Promise<void> {
  const picker = await openPanePicker(page)
  await picker.getByRole('button', { name: /^Freshopencode$/i }).click({ force: true })
  const directoryInput = page.getByLabel(/^Starting directory for Freshopencode$/i)
  await expect(directoryInput).toBeVisible({ timeout: 15_000 })
  await directoryInput.fill(cwd)
  await directoryInput.press('Enter')
  await expect(page.locator('[data-context="fresh-agent"]').last()).toBeVisible({
    timeout: 15_000,
  })
}
```

- [ ] **Step 2: Append Contract F**

Per plan §2.7: the serve DB survives; after SIGKILL+restart+reload the pane
must carry the SAME `ses_*` identity, rehydrate prompt+response, and mint NO
new session.

```ts
  test('freshopencode: SIGKILL restore keeps the ses_* identity and rehydrates history', async ({
    page,
    e2eServerKind,
  }) => {
    expect(e2eServerKind).toBe('rust')
    const sharedRoot = await fs.mkdtemp(path.join(os.tmpdir(), 'freshell-wall-freshopencode-'))
    const projectDir = path.join(sharedRoot, 'project')
    await fs.mkdir(projectDir, { recursive: true })
    const auditLogPath = path.join(sharedRoot, 'opencode-audit.jsonl')
    const fakeOpencodePath = await installFakeCli(
      FAKE_OPENCODE_SIDECAR_SOURCE,
      'opencode',
      path.join(sharedRoot, 'bin'),
    )

    const { server, harness } = await bootWall(page, {
      env: { OPENCODE_CMD: fakeOpencodePath, FAKE_OPENCODE_AUDIT_LOG: auditLogPath },
      setupHome: seedWallConfig({ providers: ['opencode'], freshAgent: true }),
    })
    // NOTE: the fixture's /session/:id/abort and /fork routes 404 -- known
    // and out of contract scope (the only production caller is
    // freshAgent.interrupt, whose error is swallowed,
    // crates/freshell-freshagent/src/opencode_ws.rs:562-572). Do not add
    // interrupt-shaped assertions against this fixture.
    try {
      await selectShellIfPickerShowing(page)
      const tabId = (await harness.getActiveTabId())!
      await enableFreshOpencode(page)
      await createFreshopencodePane(page, projectDir)

      const prompt = 'wall freshopencode turn'
      await sendFreshAgentTurn(page, harness, tabId, prompt)
      await expect(page.getByText(`Fake OpenCode response: ${prompt}`)).toBeVisible({
        timeout: 30_000,
      })

      // Materialized ses_* identity.
      const sessionId: string = await expect
        .poll(async () => {
          const id = leafDurableIdentity(findFreshAgentLeaf(await harness.getPaneLayout(tabId)))
          return id && /^ses_/.test(id) ? id : null
        }, { timeout: 30_000 })
        .not.toBeNull()
        .then(async () =>
          leafDurableIdentity(findFreshAgentLeaf(await harness.getPaneLayout(tabId)))!,
        )

      const auditRawBefore = await fs.readFile(auditLogPath, 'utf8').catch(() => '')
      const auditCountBefore = auditRawBefore ? auditRawBefore.trim().split('\n').length : 0

      await flushPersistence(page)

      // --- SIGKILL + revive, then reload. The opencode.db lives under the
      // preserved home (XDG_DATA_HOME) and survives; setupHome only rewrites
      // config.json. ---
      await server.restartAbrupt()
      await waitForWsReady(page)
      await reloadAndReconnect(page, harness)

      // CONTRACT §2.7: same identity, history rehydrated, not wedged.
      const rehydratedTabId = (await harness.getActiveTabId())!
      await expect
        .poll(async () => leafDurableIdentity(findFreshAgentLeaf(await harness.getPaneLayout(rehydratedTabId))), {
          timeout: 30_000,
        })
        .toBe(sessionId)
      await expect(page.getByText(prompt)).toBeVisible({ timeout: 30_000 })
      await expect(page.getByText(`Fake OpenCode response: ${prompt}`)).toBeVisible({
        timeout: 30_000,
      })
      const finalLeaf = findFreshAgentLeaf(await harness.getPaneLayout(rehydratedTabId))
      expect(finalLeaf?.content?.status).not.toBe('error')

      // No NEW durable session was minted by the restore.
      const auditRawAfter = await fs.readFile(auditLogPath, 'utf8').catch(() => '')
      const eventsAfter = auditRawAfter
        .trim()
        .split('\n')
        .filter(Boolean)
        .slice(auditCountBefore)
        .map((line) => JSON.parse(line) as { event?: string })
      expect(
        eventsAfter.filter(
          (event) => event.event === 'session_create_requested' || event.event === 'session_created',
        ),
      ).toEqual([])
    } finally {
      await server.stop()
      await fs.rm(sharedRoot, { recursive: true, force: true })
    }
  })
```

- [ ] **Step 3: Run it**

```bash
npx playwright test --config test/e2e-browser/playwright.config.ts \
  --project=rust-chromium restore-contract-wall -g "freshopencode"
```

Expected: PASS (§2.7 — serve DB survives; attach cold-starts serve). Known
divergence risk: the settings-drop defect (§2.7.b) is deliberately NOT
asserted here — it belongs to P1.13. If faithful and red, pin P1.8/P1.13
(§2.7) with the observed failure mode.

- [ ] **Step 4: Commit**

```bash
git add test/e2e-browser/specs/restore-contract-wall-rust.spec.ts
git commit -m "test(e2e): freshopencode SIGKILL restore contract in the wall"
```

---

### Task 7: Fake Claude sidecar + Contract G (freshclaude SIGKILL restore — expected-fail)

**Files:**
- Create: `test/e2e-browser/fixtures/fake-claude-sidecar.mjs`
- Modify: `test/e2e-browser/specs/restore-contract-wall-rust.spec.ts` (append helper + test)

**Interfaces:**
- Consumes: Task 1 helpers; `openPanePicker`; `findFreshAgentLeaf`; `leafDurableIdentity`; `sendFreshAgentTurn` (Task 5).
- Produces: `createFreshclaudePane(page, harness, cwd): Promise<void>`; fixture env contract `FAKE_CLAUDE_SIDECAR_HOLD_TURN=1` (a send never completes — pane stays `running`) — reused by Tasks 9 and 10.

- [ ] **Step 1: Write the sidecar fixture from the canonical protocol, then verify field names**

The CONTROL-FRAME reference (create/created/shutdown framing) is the inline
test fake `FAKE_CLAUDE_SIDECAR_SOURCE` at
`crates/freshell-freshagent/src/claude.rs:861-901` (cfg(test)-only, so it
must be transcribed, not reused) — but it emits ONLY `created`, so it
**cannot catch field-shape bugs**. The authoritative FIELD-shape references
are the real sidecar `crates/freshell-claude-sidecar/index.mjs:15-30` (its
doc comment enumerates every emitted frame) and the client consumer
`src/lib/fresh-agent-ws.ts:195-284` (what the client actually reads). Create
`test/e2e-browser/fixtures/fake-claude-sidecar.mjs` with the following, then
**diff every emitted `type` string and field name against index.mjs +
fresh-agent-ws.ts and correct any mismatch** (the server forwards only known
`sdk.*` types and passes payload fields through verbatim; the client
silently drops mis-shaped fields):

```js
#!/usr/bin/env node
// Fake Claude SDK-bridge sidecar for e2e (freshclaude). Enabled via the
// production env seam: FRESHELL_CLAUDE_SIDECAR=<this file>. Speaks the
// newline-JSON stdio protocol from crates/freshell-freshagent/src/claude.rs:
//   in : {"type":"create",requestId,cwd,model,permissionMode,effort,resumeSessionId}
//        {"type":"send",sessionId,text} {"type":"interrupt",sessionId} {"type":"shutdown"}
//   out: {"type":"created","sessionId"} FIRST (any earlier sdk.* line is
//        DISCARDED by read_created, claude.rs:551; 45s budget claude.rs:71),
//        then sdk.* event lines (renamed sdk.X -> freshAgent.X server-side).
// FIELD shapes come from the REAL sidecar (crates/freshell-claude-sidecar/
// index.mjs:15-30) + the client consumer (src/lib/fresh-agent-ws.ts:195-284):
//   - sdk.assistant content MUST be an ARRAY of blocks (fresh-agent-ws.ts:260-265);
//   - sdk.turn.complete MUST carry a numeric `at` (fresh-agent-ws.ts:233-240);
//   - sdk.session.init cliSessionId MUST be a canonical UUID
//     (shared/session-contract.ts:34) or no durable sessionRef ever lands.
// The process MUST stay alive (no EOF) until shutdown/kill -- an early exit
// stops the server's consumer.
// FAKE_CLAUDE_SIDECAR_HOLD_TURN=1 -> a send starts running and never
// completes (busy-restart wedge scenario).
import readline from 'node:readline'

const HOLD_TURN = process.env.FAKE_CLAUDE_SIDECAR_HOLD_TURN === '1'
const CLI_SESSION_ID =
  process.env.FAKE_CLAUDE_SIDECAR_CLI_SESSION_ID ?? '44444444-4444-4444-8444-444444444444'

function emit(obj) {
  process.stdout.write(`${JSON.stringify(obj)}\n`)
}

const rl = readline.createInterface({ input: process.stdin })
rl.on('line', (line) => {
  let msg
  try {
    msg = JSON.parse(line)
  } catch {
    return
  }
  if (msg.type === 'create') {
    const sessionId = msg.resumeSessionId ?? `fc-e2e-${process.pid}-${Date.now()}`
    // `created` FIRST -- pre-created sdk.* lines are discarded (claude.rs:551).
    emit({ type: 'created', requestId: msg.requestId, sessionId })
    // cliSessionId MUST match the canonical Claude UUID regex
    // (shared/session-contract.ts:34) or the client never derives a durable
    // sessionRef/resumeSessionId for the pane.
    emit({
      type: 'sdk.session.init',
      sessionId,
      cliSessionId: CLI_SESSION_ID,
      model: msg.model ?? 'claude-opus-4-6',
      cwd: msg.cwd ?? process.cwd(),
      tools: [],
    })
    if (msg.resumeSessionId) {
      // Resume creates set expectsHistoryHydration (fresh-agent-ws.ts:86-107,
      // freshAgentSlice.ts:230-231) -- emit a snapshot so the restored pane
      // (Tasks 7/9/10 restore halves) leaves isRestoring.
      emit({ type: 'sdk.session.snapshot', sessionId, messages: [] })
    }
    // Required for pane status 'idle' (created alone only yields 'connected').
    emit({ type: 'sdk.status', sessionId, status: 'idle' })
  } else if (msg.type === 'send') {
    emit({ type: 'sdk.status', sessionId: msg.sessionId, status: 'running' })
    if (!HOLD_TURN) {
      // content MUST be an ARRAY of blocks: the client renders assistant
      // messages only from event.content arrays (fresh-agent-ws.ts:260-265);
      // a bare `text` field is silently dropped.
      emit({
        type: 'sdk.assistant',
        sessionId: msg.sessionId,
        content: [{ type: 'text', text: 'Fixture claude turn' }],
        model: 'claude-opus-4-6',
      })
      // turn.complete without a NUMERIC `at` is dropped by the client
      // (fresh-agent-ws.ts:233-240).
      emit({
        type: 'sdk.turn.complete',
        sessionId: msg.sessionId,
        subtype: 'success',
        at: Date.now(),
      })
      emit({ type: 'sdk.status', sessionId: msg.sessionId, status: 'idle' })
    }
  } else if (msg.type === 'shutdown') {
    process.exit(0)
  }
})
```

- [ ] **Step 2: Smoke the fixture standalone**

```bash
printf '%s\n' '{"type":"create","requestId":"r1","cwd":"/tmp"}' '{"type":"send","sessionId":"X","text":"hi"}' \
  | timeout 2 node test/e2e-browser/fixtures/fake-claude-sidecar.mjs || true
```

Expected: seven JSON lines, in order: `created`, `sdk.session.init`,
`sdk.status` (idle), `sdk.status` (running), `sdk.assistant`,
`sdk.turn.complete`, `sdk.status` (idle) — the FIRST line must be
`{"type":"created",...}`. Verify the `sdk.assistant` line carries a
`content` ARRAY of blocks (not a `text` field) and the `sdk.turn.complete`
line carries a numeric `at`. (sessionId for the create is minted, so the
send's `X` mismatch is fine for the smoke.)

- [ ] **Step 3: Append the freshclaude creation helper**

```ts
async function createFreshclaudePane(page: Page, harness: TestHarness, cwd: string): Promise<void> {
  // setAvailableClis is client-only AND gets overwritten by the app
  // bootstrap + /api/platform fetch (App.tsx:572,609). Callers reach this
  // helper only after harness.waitForConnection(), which is what makes the
  // dispatch land AFTER those overwrites (donor ordering:
  // freshopencode-restart-recovery.spec.ts:100-115). Keep it that way.
  await page.evaluate(() => {
    ;(window as any).__FRESHELL_TEST_HARNESS__?.dispatch({
      type: 'connection/setAvailableClis',
      payload: { claude: true, codex: false },
    })
  })
  const picker = await openPanePicker(page)
  await picker.getByRole('button', { name: /^Freshclaude$/i }).click({ force: true })
  // /api/files/candidate-dirs returns [] on a clean isolated HOME (no $HOME
  // fallback, crates/freshell-server/src/files.rs:15-26), so a "first
  // option" may not exist -- TYPE the cwd and press Enter instead (donor:
  // freshopencode-restart-recovery.spec.ts:117-124).
  const directoryInput = page.getByLabel(/^Starting directory for Freshclaude$/i)
  await expect(directoryInput).toBeVisible({ timeout: 15_000 })
  await directoryInput.fill(cwd)
  await directoryInput.press('Enter')
  await expect(page.locator('[data-context="fresh-agent"]').last()).toBeVisible({
    timeout: 15_000,
  })
  // NOTE: once the canonical-UUID cliSessionId lands, the client fetches the
  // thread snapshot and the Rust router has NO claude adapter -> 503
  // FRESH_AGENT_RUNTIME_UNAVAILABLE (crates/freshell-freshagent/src/
  // snapshot.rs:133-146), which can surface a history-load-error banner on a
  // perfectly healthy fresh pane. Assert pane state via the harness (Redux),
  // tolerate the banner -- never assert error-free UI chrome for freshclaude.
}
```

- [ ] **Step 4: Append Contract G — pinned expected-fail**

Per plan §2.8, freshclaude is "not restart-resilient at all": attach is
swallowed, snapshot 503s. The CONTRACT asserted is the target state (rebound
with history rehydrated, status not wedged); the pin records today's reality.

```ts
  test('freshclaude: SIGKILL restore rebinds with history rehydrated and status not wedged', async ({
    page,
    e2eServerKind,
  }) => {
    expect(e2eServerKind).toBe('rust')
    // EXPECTED-FAIL WALL PIN -- P0.2 (§2.8): the FIRST failure today is that
    // claude fresh-agent identity is never persisted -- the server sends no
    // sessionRef for claude (crates/freshell-freshagent/src/claude.rs:94-96,247)
    // and the persist middleware strips sessionId without a serverInstanceId
    // (src/store/persistMiddleware.ts:245-266) -- so post-reload the pane
    // sends NEITHER attach nor create, and the identity poll below times out.
    // Attach-swallow (crates/freshell-ws/src/terminal.rs:535-553 `_ => {}`)
    // and snapshot-503 (crates/freshell-freshagent/src/snapshot.rs:132-145)
    // are real and block rebind + rehydration AFTER identity persistence
    // lands. FLIP only when claude identity survives reload AND the attach
    // arm + snapshot adapter land (P0.2 slices). See file doc comment.
    test.fail(
      e2eServerKind === 'rust',
      'P0.2 (§2.8): claude identity never persisted; attach swallow + missing snapshot adapter block rebind behind it',
    )

    const sharedRoot = await fs.mkdtemp(path.join(os.tmpdir(), 'freshell-wall-freshclaude-'))
    const projectDir = path.join(sharedRoot, 'project')
    await fs.mkdir(projectDir, { recursive: true })

    const { server, harness } = await bootWall(page, {
      env: { FRESHELL_CLAUDE_SIDECAR: FAKE_CLAUDE_SIDECAR_SOURCE },
      setupHome: seedWallConfig({ providers: ['claude'], freshAgent: true }),
    })
    try {
      await selectShellIfPickerShowing(page)
      const tabId = (await harness.getActiveTabId())!
      await createFreshclaudePane(page, harness, projectDir)
      await sendFreshAgentTurn(page, harness, tabId, 'wall freshclaude turn')
      await expect(
        page.locator('[data-context="fresh-agent"]').last().getByText('Fixture claude turn'),
      ).toBeVisible({ timeout: 20_000 })

      const originalSessionId: string = await expect
        .poll(async () => leafDurableIdentity(findFreshAgentLeaf(await harness.getPaneLayout(tabId))) ?? null, {
          timeout: 20_000,
        })
        .not.toBeNull()
        .then(async () =>
          leafDurableIdentity(findFreshAgentLeaf(await harness.getPaneLayout(tabId)))!,
        )

      await flushPersistence(page)

      // --- SIGKILL + revive, then reload. ---
      await server.restartAbrupt()
      await waitForWsReady(page)
      await reloadAndReconnect(page, harness)

      // CONTRACT §2.8 target: same identity, history rehydrated, not wedged.
      const rehydratedTabId = (await harness.getActiveTabId())!
      await expect
        .poll(async () => leafDurableIdentity(findFreshAgentLeaf(await harness.getPaneLayout(rehydratedTabId))), {
          timeout: 30_000,
        })
        .toBe(originalSessionId)
      await expect(
        page.locator('[data-context="fresh-agent"]').last().getByText('Fixture claude turn'),
      ).toBeVisible({ timeout: 30_000 })
      const finalLeaf = findFreshAgentLeaf(await harness.getPaneLayout(rehydratedTabId))
      expect(finalLeaf?.content?.status).not.toBe('error')
      expect(finalLeaf?.content?.status).not.toBe('creating')
    } finally {
      await server.stop()
      await fs.rm(sharedRoot, { recursive: true, force: true })
    }
  })
```

- [ ] **Step 5: Run it — verify the PRE-restart half works and the pin holds**

```bash
npx playwright test --config test/e2e-browser/playwright.config.ts \
  --project=rust-chromium restore-contract-wall -g "freshclaude: SIGKILL"
```

Expected: reported as **expected failure** (suite green). Critically, verify
from the trace/output that the failure happens AT OR AFTER the restore
assertions — if creation or the first turn fails (before the kill), the fake
sidecar protocol is wrong: re-check Step 1's field names against
`crates/freshell-claude-sidecar/index.mjs:15-30` and
`src/lib/fresh-agent-ws.ts:195-284` (exact event type strings, `content`
block array, numeric `at`, canonical-UUID `cliSessionId`) and fix the
fixture until the pre-kill half passes, because a wall entry that fails
during setup measures nothing. Expected first failure: the post-restart
IDENTITY poll times out (claude identity is never persisted — see the pin
comment), not the transcript assertion. If the whole test unexpectedly
PASSES, remove the pin (and celebrate — but verify a restore message really
went out by checking `harness.getSentWsMessages()` for a `freshAgent.attach`
or resume-bearing `freshAgent.create` with the original id).

- [ ] **Step 6: Commit**

```bash
git add test/e2e-browser/fixtures/fake-claude-sidecar.mjs test/e2e-browser/specs/restore-contract-wall-rust.spec.ts
git commit -m "test(e2e): fake claude sidecar + freshclaude SIGKILL restore contract (expected-fail, P0.2)"
```

---

### Task 8: Contract H (browser + editor panes)

**Files:**
- Modify: `test/e2e-browser/specs/restore-contract-wall-rust.spec.ts` (append test)

**Interfaces:**
- Consumes: Task 1 helpers only.
- Produces: `createBrowserPaneInPage(page): Promise<void>` — reused by Task 9.

- [ ] **Step 1: Append the browser-pane helper (donor: `browser-pane.spec.ts:8`) and the contract test**

Per plan §2.9: browser/editor panes are pure client state — after
SIGKILL+restart+reload the browser `url` and the editor `filePath`+`viewMode`
must be intact. (First-ever reload/restart coverage for these pane kinds.)
IMPORTANT: editor `content.content` is BLANKED at every persist flush AND
load BY DESIGN (`stripEditorContent`,
`src/store/persistMiddleware.ts:236-243,581`); visible content
re-materializes ONLY via file re-fetch on mount when `filePath && !content`,
gated on connection ready (`src/components/panes/EditorPane.tsx:450-463`).
So the editor half is FILE-BACKED: write a real file inside the test's tmp
dir, open it in the editor pane, and post-restart assert (a)
`filePath`+`viewMode` round-trip in Redux and (b) visible content
re-materializes via the re-fetch. Never assert `content.content` survives
persistence — that is falsified-by-design, not a product red.

```ts
async function createBrowserPaneInPage(page: Page): Promise<void> {
  const termContainer = page.locator('.xterm').first()
  await termContainer.click({ button: 'right' })
  await page.getByRole('menuitem', { name: /split horizontally/i }).click()
  const browserButton = page.getByRole('button', { name: /^Browser$/i })
  await expect(browserButton).toBeVisible({ timeout: 10_000 })
  await browserButton.click()
  await expect(page.getByPlaceholder('Enter URL...')).toBeVisible({ timeout: 10_000 })
}

  test('browser and editor panes: state intact after SIGKILL restart', async ({
    page,
    e2eServerKind,
  }) => {
    expect(e2eServerKind).toBe('rust')
    const sharedRoot = await fs.mkdtemp(path.join(os.tmpdir(), 'freshell-wall-broweditor-'))
    // FILE-BACKED editor pane: content.content never survives persistence
    // (stripEditorContent blanks it at flush AND load, persistMiddleware.ts:
    // 236-243,581 -- BY DESIGN); visible content re-materializes only via
    // file re-fetch on mount (EditorPane.tsx:450-463, connection-gated).
    const editorMarker = `wall-editor-${Math.random().toString(36).slice(2, 8)}`
    const editorFilePath = path.join(sharedRoot, 'wall-editor.md')
    await fs.writeFile(editorFilePath, `# wall\n\n${editorMarker}\n`)
    const { server, harness, info } = await bootWall(page)
    try {
      await selectShellIfPickerShowing(page)
      const tabId = (await harness.getActiveTabId())!

      // Browser pane with a concrete URL.
      await createBrowserPaneInPage(page)
      const urlInput = page.getByPlaceholder('Enter URL...')
      await urlInput.fill(`${info.baseUrl}/api/health`)
      await urlInput.press('Enter')
      const iframe = page.locator('iframe[title="Browser content"]')
      await iframe.waitFor({ state: 'attached', timeout: 10_000 })

      // Editor pane via Redux dispatch, file-backed: empty content + filePath
      // makes EditorPane auto-fetch the file on mount (EditorPane.tsx:450-463).
      await page.evaluate(
        ({ currentTabId, filePath }) => {
          const harnessApi = (window as any).__FRESHELL_TEST_HARNESS__
          const state = harnessApi?.getState()
          const paneId = state?.panes?.activePane?.[currentTabId]
          harnessApi?.dispatch({
            type: 'panes/splitPane',
            payload: {
              tabId: currentTabId,
              paneId,
              direction: 'horizontal',
              newPaneId: 'pane-wall-editor',
              newContent: {
                kind: 'editor',
                filePath,
                language: 'markdown',
                content: '',
                readOnly: false,
                viewMode: 'source',
              },
            },
          })
        },
        { currentTabId: tabId, filePath: editorFilePath },
      )
      await expect(page.locator('.monaco-editor').getByText(editorMarker)).toBeVisible({
        timeout: 20_000,
      })

      await flushPersistence(page)

      // --- SIGKILL + revive, then reload. ---
      await server.restartAbrupt()
      await waitForWsReady(page)
      await reloadAndReconnect(page, harness)

      // CONTRACT §2.9: browser url intact; editor filePath+viewMode intact
      // and visible content re-materialized from the FILE. Do NOT assert
      // content.content in Redux -- it is '' by design (stripEditorContent,
      // persistMiddleware.ts:236-243,581): a red there would be a TEST BUG,
      // never a product regression to pin.
      const rehydratedTabId = (await harness.getActiveTabId())!
      await expect
        .poll(async () => {
          const layout = await harness.getPaneLayout(rehydratedTabId)
          const browserLeaf = collectLeaves(layout).find((l) => l?.content?.kind === 'browser')
          return browserLeaf?.content?.url ?? null
        }, { timeout: 30_000 })
        .toContain('/api/health')
      const layout = await harness.getPaneLayout(rehydratedTabId)
      const editorLeaf = collectLeaves(layout).find((l) => l?.content?.kind === 'editor')
      expect(editorLeaf?.content?.viewMode).toBe('source')
      expect(editorLeaf?.content?.filePath).toBe(editorFilePath)
      // Re-fetch is ASYNC and gated on connection ready (EditorPane.tsx:
      // 454-457) -- poll for the visible content, never assert immediately.
      await expect(page.locator('.monaco-editor').getByText(editorMarker)).toBeVisible({
        timeout: 30_000,
      })
      await expect(page.getByPlaceholder('Enter URL...')).toHaveValue(/\/api\/health/, {
        timeout: 15_000,
      })
    } finally {
      await server.stop()
      await fs.rm(sharedRoot, { recursive: true, force: true })
    }
  })
```

- [ ] **Step 2: Run it**

```bash
npx playwright test --config test/e2e-browser/playwright.config.ts \
  --project=rust-chromium restore-contract-wall -g "browser and editor"
```

Expected: PASS (pure client state). If the editor `splitPane` dispatch shape
is rejected, mirror the exact payload from `editor-pane.spec.ts:161/207`. If
faithful and red: pin §2.9/P1.7.

- [ ] **Step 3: Commit**

```bash
git add test/e2e-browser/specs/restore-contract-wall-rust.spec.ts
git commit -m "test(e2e): browser + editor pane SIGKILL restore contract in the wall"
```

---

### Task 9: The ruler — all pane types live, one SIGKILL (expected-fail)

**Files:**
- Modify: `test/e2e-browser/specs/restore-contract-wall-rust.spec.ts` (append helpers + test)

**Interfaces:**
- Consumes: everything produced by Tasks 1–8.
- Produces: `installDualRoleCodex(binDir, argLogPath): Promise<string>`, `installDualRoleOpencode(binDir, argLogPath, auditLogPath): Promise<string>` (single-executable CJS shims — codex terminal spawns via PTY exec, no whitespace split).

- [ ] **Step 1: Append the dual-role shims**

`CODEX_CMD` serves BOTH the codex terminal CLI and the freshcodex app-server;
`OPENCODE_CMD` serves BOTH the opencode terminal CLI and the freshopencode
`serve` sidecar. Dispatch on argv. CJS bodies (extensionless executables
default to CJS — no ESM-detection dependence):

```ts
/** Single-executable `codex` shim: app-server argv -> fake app-server; else terminal fake. */
async function installDualRoleCodex(binDir: string, argLogPath: string): Promise<string> {
  await fs.mkdir(binDir, { recursive: true })
  const target = path.join(binDir, 'codex')
  const script = `#!/usr/bin/env node
const { spawnSync } = require('node:child_process')
const fs = require('node:fs')
const path = require('node:path')
const argv = process.argv.slice(2)
if (argv.includes('app-server')) {
  const result = spawnSync(process.execPath, [${JSON.stringify(FAKE_CODEX_APP_SERVER_SOURCE)}, ...argv], { stdio: 'inherit', env: process.env })
  process.exit(result.status ?? 1)
}
const logPath = ${JSON.stringify(argLogPath)}
fs.mkdirSync(path.dirname(logPath), { recursive: true })
fs.appendFileSync(logPath, JSON.stringify({ pid: process.pid, t: Date.now(), argv }) + '\\n')
const resumeIndex = argv.indexOf('resume')
if (resumeIndex !== -1) {
  process.stdout.write('codex: resumed session ' + (argv[resumeIndex + 1] ?? '') + '\\r\\n')
} else {
  process.stdout.write('codex> \\r\\n')
}
process.stdin.resume()
`
  await fs.writeFile(target, script, 'utf8')
  await fs.chmod(target, 0o755)
  return target
}

/** Single-executable `opencode` shim: `serve` argv -> fake sidecar; else terminal fake. */
async function installDualRoleOpencode(
  binDir: string,
  argLogPath: string,
  auditLogPath: string,
): Promise<string> {
  await fs.mkdir(binDir, { recursive: true })
  const target = path.join(binDir, 'opencode')
  const script = `#!/usr/bin/env node
const { spawnSync } = require('node:child_process')
const argv = process.argv.slice(2)
const source = argv[0] === 'serve' || argv[0] === '--version'
  ? ${JSON.stringify(FAKE_OPENCODE_SIDECAR_SOURCE)}
  : ${JSON.stringify(FAKE_OPENCODE_TERMINAL_SOURCE)}
const env = { ...process.env, FAKE_OPENCODE_AUDIT_LOG: ${JSON.stringify(auditLogPath)}, FAKE_OPENCODE_TERMINAL_ARGV_LOG: ${JSON.stringify(argLogPath)} }
const result = spawnSync(process.execPath, [source, ...argv], { stdio: 'inherit', env })
process.exit(result.status ?? 1)
`
  await fs.writeFile(target, script, 'utf8')
  await fs.chmod(target, 0o755)
  return target
}
```

- [ ] **Step 2: Append the ruler test**

```ts
  test('THE RULER: all pane types live, one SIGKILL, every §2 contract holds', async ({
    page,
    e2eServerKind,
  }) => {
    expect(e2eServerKind).toBe('rust')
    test.setTimeout(300_000)
    // EXPECTED-FAIL WALL PIN -- P0.1: this is the composed ruler; it flips
    // green only when every per-pane contract above is green un-pinned
    // (P0.2..P1.13). Today the FIRST red comes from the hidden-tab legs
    // (F8/P1.11): post-reload only the LAST tab is active, so the hidden
    // tabs' shell-recreate / claude-argv polls fail first. The freshclaude
    // leg is ADDITIONALLY red (identity never persisted, P0.2), while the
    // freshcodex/freshopencode identity polls can pass VACUOUSLY from
    // persisted state. FLIP: delete this pin when the last per-pane pin is
    // retired.
    test.fail(
      e2eServerKind === 'rust',
      'P0.1: composed all-pane ruler; red until F8 (P1.11) + P0.2..P1.13 land',
    )

    const CODEX_SESSION_ID = '99999999-8888-4777-8666-555555555555'
    const SESSION_TITLE = 'ruler codex session'
    const sharedRoot = await fs.mkdtemp(path.join(os.tmpdir(), 'freshell-wall-ruler-'))
    const projectDir = path.join(sharedRoot, 'project')
    await fs.mkdir(projectDir, { recursive: true })
    const claudeArgLog = path.join(sharedRoot, 'claude-argv.jsonl')
    const codexArgLog = path.join(sharedRoot, 'codex-argv.jsonl')
    const opencodeArgLog = path.join(sharedRoot, 'opencode-argv.jsonl')
    const opencodeAuditLog = path.join(sharedRoot, 'opencode-audit.jsonl')
    const binDir = path.join(sharedRoot, 'bin')
    const fakeClaudePath = await installFakeCli(FAKE_CLAUDE_CLI_SOURCE, 'claude', binDir)
    const fakeCodexPath = await installDualRoleCodex(binDir, codexArgLog)
    const fakeOpencodePath = await installDualRoleOpencode(binDir, opencodeArgLog, opencodeAuditLog)

    const { server, harness, info } = await bootWall(page, {
      env: {
        CLAUDE_CMD: fakeClaudePath,
        FAKE_CLAUDE_ARGV_LOG: claudeArgLog,
        CODEX_CMD: fakeCodexPath,
        FAKE_CODEX_ARGV_LOG: codexArgLog,
        OPENCODE_CMD: fakeOpencodePath,
        FRESHELL_CLAUDE_SIDECAR: FAKE_CLAUDE_SIDECAR_SOURCE,
      },
      setupHome: async (homeDir) => {
        await seedWallConfig({ providers: ['claude', 'codex', 'opencode'], freshAgent: true })(
          homeDir,
        )
        await seedCodexHome(CODEX_SESSION_ID, SESSION_TITLE, projectDir)(homeDir)
        // seedCodexHome overwrote config.json with codex-only providers; the
        // second write below restores the full set (both are idempotent).
        await seedWallConfig({ providers: ['claude', 'codex', 'opencode'], freshAgent: true })(
          homeDir,
        )
      },
    })
    try {
      await selectShellIfPickerShowing(page)
      const tab1 = (await harness.getActiveTabId())!

      // --- TAB 1: shell (already there) + browser + editor. ---
      await createBrowserPaneInPage(page)
      const urlInput = page.getByPlaceholder('Enter URL...')
      await urlInput.fill(`${info.baseUrl}/api/health`)
      await urlInput.press('Enter')
      // FILE-BACKED editor pane (editor content.content never survives
      // persistence -- stripEditorContent, persistMiddleware.ts:236-243,581).
      const editorMarker = 'ruler-editor-marker'
      const editorFilePath = path.join(projectDir, 'ruler-editor.md')
      await fs.writeFile(editorFilePath, `# ruler\n\n${editorMarker}\n`)
      await page.evaluate(
        ({ currentTabId, filePath }) => {
          const harnessApi = (window as any).__FRESHELL_TEST_HARNESS__
          const state = harnessApi?.getState()
          const paneId = state?.panes?.activePane?.[currentTabId]
          harnessApi?.dispatch({
            type: 'panes/splitPane',
            payload: {
              tabId: currentTabId,
              paneId,
              direction: 'horizontal',
              newPaneId: 'pane-ruler-editor',
              newContent: {
                kind: 'editor',
                filePath,
                language: 'markdown',
                content: '',
                readOnly: false,
                viewMode: 'source',
              },
            },
          })
        },
        { currentTabId: tab1, filePath: editorFilePath },
      )

      // --- TAB 2 (picker/WS path): claude terminal, fresh -> pre-allocated
      // id. REST POST /api/tabs never pre-allocates --session-id
      // (terminal_tabs.rs:756-768); only the WS-path terminal.create does
      // (terminal.rs:969-982), so create the tab via tab-add and pick Claude
      // in the new tab's own pane-type picker. Type the cwd -- candidate
      // dirs may be empty on a clean HOME (files.rs:15-26).
      await page.locator('[data-context="tab-add"]').click()
      await harness.waitForTabCount(2)
      const claudeTabId = (await harness.getActiveTabId())!
      const claudePicker = page.getByRole('toolbar', { name: /pane type picker/i }).last()
      await claudePicker.getByRole('button', { name: /^Claude CLI$/i }).click({ force: true })
      const claudeDirInput = page.getByRole('combobox', {
        name: /Starting directory for Claude/i,
      })
      await expect(claudeDirInput).toBeVisible({ timeout: 15_000 })
      await claudeDirInput.fill(projectDir)
      await claudeDirInput.press('Enter')
      const claudePreallocatedId: string = await expect
        .poll(async () => {
          const entries = await readArgvLog(claudeArgLog)
          const withId = entries.find((e) => e.argv.includes('--session-id'))
          return withId ? withId.argv[withId.argv.indexOf('--session-id') + 1] ?? null : null
        }, { timeout: 20_000 })
        .not.toBeNull()
        .then(async () => {
          const entries = await readArgvLog(claudeArgLog)
          const withId = entries.find((e) => e.argv.includes('--session-id'))!
          return withId.argv[withId.argv.indexOf('--session-id') + 1]!
        })

      // --- TAB 3 (sidebar): codex terminal on the seeded session. ---
      const codexItem = page.getByText(SESSION_TITLE, { exact: false }).first()
      await expect(codexItem).toBeVisible({ timeout: 15_000 })
      await codexItem.click()
      const codexTabId = (await harness.getActiveTabId())!
      await expect
        .poll(async () => (await harness.getPaneLayout(codexTabId))?.content?.terminalId ?? null, {
          timeout: 20_000,
        })
        .not.toBeNull()

      // --- TAB 3 split: opencode terminal, minted via Enter. ---
      const opencodeLeaf = await openOpencodePaneAndGetLeaf(page, harness, codexTabId)
      await page.locator('.xterm').last().click()
      await page.keyboard.type('hello ruler opencode')
      await page.keyboard.press('Enter')
      const opencodeSessionId: string = await expect
        .poll(async () => {
          const l = await findLeafById(harness, codexTabId, opencodeLeaf.id)
          return l?.content?.sessionRef?.sessionId ?? l?.content?.resumeSessionId ?? null
        }, { timeout: 20_000 })
        .not.toBeNull()
        .then(async () => {
          const l = await findLeafById(harness, codexTabId, opencodeLeaf.id)
          return l?.content?.sessionRef?.sessionId ?? l?.content?.resumeSessionId
        })

      // --- TAB 4: freshcodex; TAB 5: freshopencode; TAB 6: freshclaude. ---
      // (Tab count so far: tab1 + claude picker tab + codex sidebar tab = 3;
      // the opencode pane is a SPLIT inside the codex tab, not a tab.)
      await page.locator('[data-context="tab-add"]').click()
      await harness.waitForTabCount(4)
      const freshcodexTabId = (await harness.getActiveTabId())!
      await selectShellIfPickerShowing(page)
      await createFreshcodexPane(page, harness)
      await sendFreshAgentTurn(page, harness, freshcodexTabId, 'ruler freshcodex turn')
      const freshcodexId = leafDurableIdentity(
        findFreshAgentLeaf(await harness.getPaneLayout(freshcodexTabId)),
      )!

      await page.locator('[data-context="tab-add"]').click()
      await harness.waitForTabCount(5)
      const freshopencodeTabId = (await harness.getActiveTabId())!
      await selectShellIfPickerShowing(page)
      // MUST pass the FULL provider list: enableFreshOpencode's
      // previewServerSettingsPatch REPLACES enabledProviders
      // (mergeServerSettings, shared/settings.ts:1216-1218). The default
      // ['opencode'] would hide the Freshclaude button needed for tab 6
      // (PanePicker.tsx:125-152 gates on enabledProviders.includes('claude')).
      await enableFreshOpencode(page, ['claude', 'codex', 'opencode'])
      await createFreshopencodePane(page, projectDir)
      await sendFreshAgentTurn(page, harness, freshopencodeTabId, 'ruler freshopencode turn')
      const freshopencodeId = leafDurableIdentity(
        findFreshAgentLeaf(await harness.getPaneLayout(freshopencodeTabId)),
      )!

      await page.locator('[data-context="tab-add"]').click()
      await harness.waitForTabCount(6)
      const freshclaudeTabId = (await harness.getActiveTabId())!
      await selectShellIfPickerShowing(page)
      await createFreshclaudePane(page, harness, projectDir)
      await sendFreshAgentTurn(page, harness, freshclaudeTabId, 'ruler freshclaude turn')
      const freshclaudeId = leafDurableIdentity(
        findFreshAgentLeaf(await harness.getPaneLayout(freshclaudeTabId)),
      )!

      const tabCountBefore = await harness.getTabCount()
      const claudeArgvBefore = (await readArgvLog(claudeArgLog)).length
      const codexArgvBefore = (await readArgvLog(codexArgLog)).length
      const opencodeArgvBefore = (await readArgvLog(opencodeArgLog)).length
      await flushPersistence(page)

      // ===================== THE SIGKILL ====================
      await server.restartAbrupt()
      await waitForWsReady(page)
      await reloadAndReconnect(page, harness)
      // ======================================================

      expect(await harness.getTabCount()).toBe(tabCountBefore)

      // Shell (§2.1): recreated, not error.
      await expect
        .poll(async () => {
          const layout = await harness.getPaneLayout(tab1)
          const shellLeaf = collectLeaves(layout).find(
            (l) => l?.content?.kind === 'terminal' && (l?.content?.mode ?? 'shell') === 'shell',
          )
          return shellLeaf?.content?.terminalId ?? null
        }, { timeout: 30_000 })
        .not.toBeNull()

      // Browser + editor (§2.9): durable state intact. Editor content.content
      // is '' by design after persistence (stripEditorContent,
      // persistMiddleware.ts:236-243,581) -- assert the durable fields
      // (filePath/viewMode) instead; tab1 is HIDDEN post-reload, so no
      // visible-content re-fetch assertion here (the file-backed re-fetch is
      // covered by Contract H on a visible pane).
      const tab1Layout = await harness.getPaneLayout(tab1)
      expect(
        collectLeaves(tab1Layout).find((l) => l?.content?.kind === 'browser')?.content?.url,
      ).toContain('/api/health')
      const rulerEditorLeaf = collectLeaves(tab1Layout).find((l) => l?.content?.kind === 'editor')
      expect(rulerEditorLeaf?.content?.viewMode).toBe('source')
      expect(rulerEditorLeaf?.content?.filePath).toBe(editorFilePath)

      // Claude terminal (§2.2): resumed with the pre-allocated id.
      await expect
        .poll(async () => {
          const entries = await readArgvLog(claudeArgLog)
          return entries
            .slice(claudeArgvBefore)
            .some((e) => hasFlagPair(e.argv, '--resume', claudePreallocatedId))
        }, { timeout: 45_000 })
        .toBe(true)

      // Codex terminal (§2.3): resumed.
      await expect
        .poll(async () => {
          const entries = await readArgvLog(codexArgLog)
          return entries
            .slice(codexArgvBefore)
            .some((e) => hasResumePair(e.argv, CODEX_SESSION_ID))
        }, { timeout: 45_000 })
        .toBe(true)

      // Opencode terminal (§2.4): resumed.
      await expect
        .poll(async () => {
          const entries = await readArgvLog(opencodeArgLog)
          return entries
            .slice(opencodeArgvBefore)
            .some((e) => hasFlagPair(e.argv, '--session', opencodeSessionId))
        }, { timeout: 45_000 })
        .toBe(true)

      // Fresh agents (§2.6/§2.7/§2.8): identities survive, status not wedged.
      for (const [tabIdX, expectedId] of [
        [freshcodexTabId, freshcodexId],
        [freshopencodeTabId, freshopencodeId],
        [freshclaudeTabId, freshclaudeId],
      ] as const) {
        await expect
          .poll(async () => leafDurableIdentity(findFreshAgentLeaf(await harness.getPaneLayout(tabIdX))), {
            timeout: 45_000,
          })
          .toBe(expectedId)
        const leafX = findFreshAgentLeaf(await harness.getPaneLayout(tabIdX))
        expect(leafX?.content?.status).not.toBe('error')
        expect(leafX?.content?.status).not.toBe('creating')
      }

      // Quiet client: no alerts, no noisy error text (donor: restore-sync05).
      // CAVEAT: freshclaude creates get a snapshot 503
      // (FRESH_AGENT_RUNTIME_UNAVAILABLE, snapshot.rs:133-146) which can
      // surface a history-load-error banner; if this count is nonzero ONLY
      // because of that banner, scope the locator to exclude the freshclaude
      // pane (or drop this line) rather than treating it as a new product
      // red -- the pane-state assertions above are the real contract.
      await expect(page.getByRole('alert')).toHaveCount(0)
    } finally {
      await server.stop()
      await fs.rm(sharedRoot, { recursive: true, force: true })
    }
  })
```

Notes for the implementer:
- If `[data-context="tab-add"]` is not the add-tab anchor in this build, use
  `page.getByRole('button', { name: /add tab/i })` — check
  `browser-pane.spec.ts` / `restore-matrix.spec.ts` for the current idiom.
- Hidden-tab panes may not re-create until revealed (that is F8, pinned in
  Task 10) — post-reload only the LAST tab is active, so expect the ruler's
  FIRST red to be a hidden-tab leg (the tab1 shell-recreate poll or the
  claude/codex/opencode resume-argv polls), NOT the freshclaude leg. The
  freshclaude identity poll is additionally red (identity never persisted,
  P0.2 — see Task 7's pin comment), while the freshcodex/freshopencode
  identity polls can pass VACUOUSLY from persisted state (they measure
  persistence, not restore). The ruler's per-tab polls run against Redux
  layout state, and if a hidden tab never re-creates, the poll fails inside
  an already-pinned test. That is honest: the ruler is red until F8 (P1.11)
  and P0.2 land too. (Which hidden-tab assertion fails FIRST depends on
  whether the dead-terminal census reaches hidden tabs' layouts — runtime-
  dependent, verdict-neutral: both candidates are post-restart contract
  assertions.)

- [ ] **Step 3: Run it**

```bash
npx playwright test --config test/e2e-browser/playwright.config.ts \
  --project=rust-chromium restore-contract-wall -g "THE RULER"
```

Expected: reported as **expected failure**. Verify the setup half (all 9 panes
created, all pre-kill identities captured) succeeds — a ruler that dies during
setup measures nothing; fix creation issues until the first failing assertion
is a genuine post-restart contract assertion.

- [ ] **Step 4: Commit**

```bash
git add test/e2e-browser/specs/restore-contract-wall-rust.spec.ts
git commit -m "test(e2e): the ruler -- all-pane-types SIGKILL restore matrix (expected-fail, P0.1)"
```

---

### Task 10: The six named red tests from plan §5 P0.1

**Files:**
- Modify: `test/e2e-browser/specs/restore-contract-wall-rust.spec.ts` (append 6 tests)
- Modify: `test/e2e-browser/fixtures/fake-opencode-terminal.mjs` (Step 2: opt-in `FAKE_OPENCODE_TERMINAL_ROW_GATE_PATH` determinism gate — a TEST fixture, allowed by this plan's constraints)

**Interfaces:**
- Consumes: all helpers from Tasks 1–7; additionally the Playwright `browser` fixture (for the two-clients test).
- Produces: fixture env contract `FAKE_OPENCODE_TERMINAL_ROW_GATE_PATH` (when set, the fake polls ~50ms for the gate file before writing its session row; unset = unchanged behavior).

Append each test inside the describe block. Each carries its pin naming the
plan item; apply the decision rule after each run.

- [ ] **Step 1: `SIGKILL-within-5s-of-pane-creation` (pin P1.8+P1.9, D3)**

A pane created <5s before SIGKILL whose browser also lost its state is
unrecoverable today (no server-side record). Target contract (§4.2): the
server's ledger makes it recoverable; the client offers/executes recovery.

```ts
  test('SIGKILL-within-5s-of-pane-creation: identity survives without client state', async ({
    page,
    e2eServerKind,
  }) => {
    expect(e2eServerKind).toBe('rust')
    // EXPECTED-FAIL WALL PIN -- P1.8+P1.9 (D3, §4.2): no server-side durable
    // pane-identity record exists; with localStorage gone the binding is lost
    // even though the pre-allocated claude session id was server-minted.
    // FLIP when the pane-identity ledger + "recover my panes" surface land.
    test.fail(
      e2eServerKind === 'rust',
      'P1.8+P1.9 (D3): pane created <5s before SIGKILL is unrecoverable after browser loss',
    )
    const sharedRoot = await fs.mkdtemp(path.join(os.tmpdir(), 'freshell-wall-5s-'))
    const projectDir = path.join(sharedRoot, 'project')
    await fs.mkdir(projectDir, { recursive: true })
    const argLogPath = path.join(sharedRoot, 'claude-argv.jsonl')
    const fakeClaudePath = await installFakeCli(
      FAKE_CLAUDE_CLI_SOURCE,
      'claude',
      path.join(sharedRoot, 'bin'),
    )
    const { server, harness } = await bootWall(page, {
      env: { CLAUDE_CMD: fakeClaudePath, FAKE_CLAUDE_ARGV_LOG: argLogPath },
      setupHome: seedWallConfig({ providers: ['claude'] }),
    })
    try {
      await selectShellIfPickerShowing(page)
      // Fresh claude pane via the picker/WS path -- REST POST /api/tabs never
      // pre-allocates --session-id (terminal_tabs.rs:756-768); only the
      // WS-path terminal.create does (terminal.rs:969-982). Type the cwd --
      // candidate dirs may be empty on a clean HOME (files.rs:15-26).
      const picker = await openPanePicker(page)
      await picker.getByRole('button', { name: /^Claude CLI$/i }).click({ force: true })
      const dirInput = page.getByRole('combobox', { name: /Starting directory for Claude/i })
      await expect(dirInput).toBeVisible({ timeout: 15_000 })
      await dirInput.fill(projectDir)
      await dirInput.press('Enter')

      // Server-minted identity exists the moment the CLI spawns: the fake
      // appends its argv line synchronously at spawn, so the first
      // --session-id entry marks the pane's t=0. UI creation is slower than
      // a REST call, so the poll gets a UI-scale timeout; the "within 5s of
      // creation" premise is anchored on the SPAWN instead -- SIGKILL is
      // issued immediately after the entry appears (moments after spawn,
      // well inside any snapshot cadence).
      const preallocatedId: string = await expect
        .poll(async () => {
          const entries = await readArgvLog(argLogPath)
          const withId = entries.find((e) => e.argv.includes('--session-id'))
          return withId ? withId.argv[withId.argv.indexOf('--session-id') + 1] ?? null : null
        }, { timeout: 20_000 })
        .not.toBeNull()
        .then(async () => {
          const entries = await readArgvLog(argLogPath)
          const withId = entries.find((e) => e.argv.includes('--session-id'))!
          return withId.argv[withId.argv.indexOf('--session-id') + 1]!
        })

      // ...and the SIGKILL lands immediately after the spawn -- before any
      // snapshot cadence could have persisted the binding. Then the browser
      // loses its state.
      await server.restartAbrupt()
      await page.evaluate(() => localStorage.clear())
      await reloadAndReconnect(page, harness)

      // TARGET CONTRACT (§4.2/§4.4): the server still knows the binding --
      // some pane resuming <preallocatedId> becomes reachable (auto-restored
      // or offered via "recover my panes").
      await expect
        .poll(async () => {
          const state = await harness.getState()
          const layouts = state?.panes?.layouts ?? {}
          for (const layout of Object.values(layouts)) {
            const hit = collectLeaves(layout).find(
              (l) => l?.content?.sessionRef?.sessionId === preallocatedId,
            )
            if (hit) return true
          }
          const recoverOffer = await page
            .getByText(/recover .*pane/i)
            .first()
            .isVisible()
            .catch(() => false)
          return recoverOffer
        }, { timeout: 30_000 })
        .toBe(true)
    } finally {
      await server.stop()
      await fs.rm(sharedRoot, { recursive: true, force: true })
    }
  })
```

- [ ] **Step 2: `SIGKILL-inside-locator-window` (pin P1.8 pending markers, §2.4/§4.2) — with a determinism gate in the fixture**

Without a control this pin is a RACE, not a deterministic red: the locator
sweep runs every 150ms (`crates/freshell-server/src/main.rs:1112,543-546`)
and the fixture writes its session row synchronously on first stdin
(`test/e2e-browser/fixtures/fake-opencode-terminal.mjs:108-122`), so the
association can beat the SIGKILL a few percent of runs — and an unexpected
PASS of a `test.fail` pin is a HARD suite failure. The existing
`FAKE_OPENCODE_SESSION_EVENT_GATE_PATH` seam lives in the SIDECAR fixture
and gates SSE emission only (`fake-opencode.cjs:390,636-652`) — unsuitable
here. So FIRST edit `test/e2e-browser/fixtures/fake-opencode-terminal.mjs`
(a TEST fixture — allowed by this plan's constraints): replace its fresh-
launch `else` branch (currently lines ~103-124) with the version below,
adding an opt-in row gate that mirrors the sidecar's poll-for-gate-file
pattern. Unset, behavior is byte-identical for the existing restore specs.

```js
} else {
  process.stdout.write('opencode> \r\n')

  // Opt-in determinism gate for the restore-contract wall
  // (SIGKILL-inside-locator-window): when FAKE_OPENCODE_TERMINAL_ROW_GATE_PATH
  // is set, poll ~50ms for the gate file before writing the session row
  // (mirrors FAKE_OPENCODE_SESSION_EVENT_GATE_PATH in fake-opencode.cjs:636-652,
  // which gates SSE emission only and cannot gate this row write). The wall
  // test sets the env and never creates the file pre-kill, so the identity
  // deterministically never lands before the SIGKILL.
  const rowGatePath = process.env.FAKE_OPENCODE_TERMINAL_ROW_GATE_PATH

  function commitSessionRow(sessionId, cwd) {
    writeSessionRow(sessionId, cwd)
    process.stdout.write(`opencode: session ${sessionId} started\r\n`)
  }

  let sessionCreated = false
  process.stdin.setEncoding('utf8')
  process.stdin.on('data', () => {
    // Any input at all counts as "the first submit" for this fixture's
    // purposes -- the real locator arms only on Enter-shaped WS input, and
    // the pty's own cooked-mode line discipline already withholds bytes
    // from this process until the user presses Enter, so the first `data`
    // event this process ever sees IS that submit.
    if (sessionCreated) return
    sessionCreated = true

    const cwd = process.cwd()
    const sessionId = `ses_e2e_${Date.now()}_${process.pid}`
    if (!rowGatePath) {
      commitSessionRow(sessionId, cwd)
      return
    }
    const interval = setInterval(() => {
      if (!fs.existsSync(rowGatePath)) return
      clearInterval(interval)
      commitSessionRow(sessionId, cwd)
    }, 50)
    interval.unref?.()
  })
  process.stdin.resume()
}
```

Re-run the existing `opencode-terminal-restore-rust` spec after this edit to
prove the unset-env path is unchanged. Then append the wall test:

```ts
  test('SIGKILL-inside-locator-window: never silently fresh', async ({
    page,
    e2eServerKind,
  }) => {
    expect(e2eServerKind).toBe('rust')
    // EXPECTED-FAIL WALL PIN -- P1.8 (§2.4/§4.2 pending markers): killing the
    // server inside the opencode locator's ~2s correlation window loses the
    // minted identity permanently, and the pane restores SILENTLY FRESH --
    // no resume, no breadcrumb. FLIP when ledger pending markers land
    // (fresh-by-race must be visible) or the identity is captured in time.
    // DETERMINISM: the fake's session-row write is held behind
    // FAKE_OPENCODE_TERMINAL_ROW_GATE_PATH and this test NEVER creates the
    // gate file before the kill, so the identity provably cannot land
    // pre-kill. Without the gate, the 150ms locator sweep (main.rs:1112)
    // can beat the SIGKILL a few percent of runs -> unexpected PASS of this
    // pin -> hard suite failure.
    test.fail(
      e2eServerKind === 'rust',
      'P1.8 (§2.4): SIGKILL inside locator window yields silent fresh, no breadcrumb',
    )
    const sharedRoot = await fs.mkdtemp(path.join(os.tmpdir(), 'freshell-wall-locwin-'))
    const argLogPath = path.join(sharedRoot, 'opencode-argv.jsonl')
    // Deliberately never created -- see the DETERMINISM note above.
    const rowGatePath = path.join(sharedRoot, 'row-gate-never-created')
    const fakeOpencodePath = await installFakeCli(
      FAKE_OPENCODE_TERMINAL_SOURCE,
      'opencode',
      path.join(sharedRoot, 'bin'),
    )
    const { server, harness } = await bootWall(page, {
      env: {
        OPENCODE_CMD: fakeOpencodePath,
        FAKE_OPENCODE_TERMINAL_ARGV_LOG: argLogPath,
        FAKE_OPENCODE_TERMINAL_ROW_GATE_PATH: rowGatePath,
      },
      setupHome: seedWallConfig({ providers: ['opencode'] }),
    })
    try {
      await selectShellIfPickerShowing(page)
      const tabId = (await harness.getActiveTabId())!
      const leaf = await openOpencodePaneAndGetLeaf(page, harness, tabId)

      // Mint the session and kill IMMEDIATELY -- inside the locator window,
      // before terminal.session.associated can land.
      await page.locator('.xterm').last().click()
      await page.keyboard.type('hello locator window')
      await page.keyboard.press('Enter')
      const argvCountBeforeKill = (await readArgvLog(argLogPath)).length
      await server.restartAbrupt()
      await waitForWsReady(page)

      // Wait for the pane to settle post-restart.
      await expect
        .poll(async () => {
          const l = await findLeafById(harness, tabId, leaf.id)
          const tid = l?.content?.terminalId ?? null
          return tid && tid !== leaf.content.terminalId ? tid : null
        }, { timeout: 30_000 })
        .not.toBeNull()

      // TARGET CONTRACT (§2.4/§4.2): EITHER resumed with a ses_ id, OR a
      // visible fresh-by-race breadcrumb. Silent fresh is the failure.
      const resumed = (await readArgvLog(argLogPath))
        .slice(argvCountBeforeKill)
        .some((e) => e.argv.includes('--session'))
      const breadcrumbVisible = await page
        .getByText(/couldn't be resumed|could not be resumed|fresh session/i)
        .first()
        .isVisible()
        .catch(() => false)
      expect(resumed || breadcrumbVisible).toBe(true)
    } finally {
      await server.stop()
      await fs.rm(sharedRoot, { recursive: true, force: true })
    }
  })
```

- [ ] **Step 3: `two-clients-same-sessionRef` (pin P1.7, D8)**

```ts
  test('two-clients-same-sessionRef: duplicate respawn must yield exactly 1 PTY', async ({
    page,
    browser,
    e2eServerKind,
  }) => {
    expect(e2eServerKind).toBe('rust')
    // EXPECTED-FAIL WALL PIN -- P1.7 (D8, §4.3): dedupe keys on
    // createRequestId only; two clients holding the same sessionRef carry
    // different createRequestIds, so BOTH respawn -> two PTYs -> two JSONL
    // writers on one session file. FLIP when sessionRef-level single-flight
    // lands (Phase 3 multi-client spec).
    test.fail(
      e2eServerKind === 'rust',
      'P1.7 (D8): two clients on one sessionRef respawn two PTYs after SIGKILL',
    )
    const CODEX_SESSION_ID = '77777777-6666-4555-8444-333333333333'
    const SESSION_TITLE = 'two-client codex session'
    const sharedRoot = await fs.mkdtemp(path.join(os.tmpdir(), 'freshell-wall-twoclient-'))
    const projectDir = path.join(sharedRoot, 'project')
    await fs.mkdir(projectDir, { recursive: true })
    const argLogPath = path.join(sharedRoot, 'codex-argv.jsonl')
    const fakeCodexPath = await installFakeCli(
      FAKE_CODEX_CLI_SOURCE,
      'codex',
      path.join(sharedRoot, 'bin'),
    )
    const { server, harness, info } = await bootWall(page, {
      env: { CODEX_CMD: fakeCodexPath, FAKE_CODEX_ARGV_LOG: argLogPath },
      setupHome: seedCodexHome(CODEX_SESSION_ID, SESSION_TITLE, projectDir),
    })
    const contextB = await browser.newContext()
    const pageB = await contextB.newPage()
    try {
      // Client A opens the seeded session from the sidebar.
      await selectShellIfPickerShowing(page)
      await page.getByText(SESSION_TITLE, { exact: false }).first().click()
      const tabIdA = (await harness.getActiveTabId())!
      await expect
        .poll(async () => (await harness.getPaneLayout(tabIdA))?.content?.sessionRef?.sessionId ?? null, {
          timeout: 20_000,
        })
        .toBe(CODEX_SESSION_ID)

      // Client B (separate context = separate localStorage) does the same.
      await pageB.goto(`${info.baseUrl}/?token=${info.token}&e2e=1`)
      const harnessB = new TestHarness(pageB)
      await harnessB.waitForHarness()
      await harnessB.waitForConnection()
      await selectShellIfPickerShowing(pageB)
      await pageB.getByText(SESSION_TITLE, { exact: false }).first().click()
      const tabIdB = (await harnessB.getActiveTabId())!
      await expect
        .poll(async () => (await harnessB.getPaneLayout(tabIdB))?.content?.sessionRef?.sessionId ?? null, {
          timeout: 20_000,
        })
        .toBe(CODEX_SESSION_ID)

      const argvCountBeforeKill = (await readArgvLog(argLogPath)).length

      // --- SIGKILL; both live clients race to respawn the same sessionRef. ---
      await server.restartAbrupt()
      await waitForWsReady(page)
      await waitForWsReady(pageB)

      // Let both recovery rounds fully settle before counting.
      const countRespawns = async () =>
        (await readArgvLog(argLogPath))
          .slice(argvCountBeforeKill)
          .filter((e) => hasResumePair(e.argv, CODEX_SESSION_ID)).length
      await expect
        .poll(countRespawns, { timeout: 45_000 })
        .toBeGreaterThan(0)
      // STABLE-COUNT settle (not a fixed sleep): accept only when two samples
      // >=5s apart agree, so a tail-latency straggler cannot make the count
      // read 1 spuriously -- an unexpected PASS of this pin is a hard suite
      // failure. Under today's bug the stable count is 2 (pin holds); when
      // sessionRef single-flight lands it is 1 (pin flips loudly).
      await expect
        .poll(
          async () => {
            const first = await countRespawns()
            await page.waitForTimeout(5_000)
            const second = await countRespawns()
            return second === first ? second : null
          },
          { timeout: 60_000 },
        )
        .not.toBeNull()

      // TARGET CONTRACT (§4.3 multi-client single-flight): EXACTLY 1 PTY.
      const respawns = (await readArgvLog(argLogPath))
        .slice(argvCountBeforeKill)
        .filter((e) => hasResumePair(e.argv, CODEX_SESSION_ID))
      expect(respawns.length).toBe(1)
    } finally {
      await contextB.close()
      await server.stop()
      await fs.rm(sharedRoot, { recursive: true, force: true })
    }
  })
```

- [ ] **Step 4: `freshclaude busy-restart un-wedge` (pin P0.2, §2.8.1)**

```ts
  test('freshclaude busy-restart: a pane that was BUSY at SIGKILL must not wedge BUSY', async ({
    page,
    e2eServerKind,
  }) => {
    expect(e2eServerKind).toBe('rust')
    // EXPECTED-FAIL WALL PIN -- P0.2 (§2.8.1): freshAgent.attach for claude is
    // silently swallowed; the client never receives a lost frame, so a pane
    // that was BUSY at restart wedges BUSY forever. FLIP when the attach arm
    // emits INVALID_SESSION_ID/lost frames (P0.2 slice 1 -- this test is the
    // named kill-server-while-busy proof from §2.8.1).
    test.fail(
      e2eServerKind === 'rust',
      'P0.2 (§2.8.1): busy freshclaude pane wedges BUSY after SIGKILL (attach swallowed)',
    )
    const sharedRoot = await fs.mkdtemp(path.join(os.tmpdir(), 'freshell-wall-fcbusy-'))
    const projectDir = path.join(sharedRoot, 'project')
    await fs.mkdir(projectDir, { recursive: true })
    const { server, harness } = await bootWall(page, {
      env: {
        FRESHELL_CLAUDE_SIDECAR: FAKE_CLAUDE_SIDECAR_SOURCE,
        FAKE_CLAUDE_SIDECAR_HOLD_TURN: '1',
      },
      setupHome: seedWallConfig({ providers: ['claude'], freshAgent: true }),
    })
    try {
      await selectShellIfPickerShowing(page)
      const tabId = (await harness.getActiveTabId())!
      await createFreshclaudePane(page, harness, projectDir)

      // Send a turn that NEVER completes (HOLD_TURN) -> status running.
      const paneRoot = page.locator('[data-context="fresh-agent"]').last()
      await expect
        .poll(async () => findFreshAgentLeaf(await harness.getPaneLayout(tabId))?.content?.status, {
          timeout: 20_000,
        })
        .toBe('idle')
      await paneRoot.getByRole('textbox', { name: 'Chat message input' }).fill('busy turn')
      await paneRoot.getByRole('button', { name: 'Send' }).click()
      await expect
        .poll(async () => findFreshAgentLeaf(await harness.getPaneLayout(tabId))?.content?.status, {
          timeout: 20_000,
        })
        .toBe('running')

      await flushPersistence(page)

      // --- SIGKILL while BUSY, revive, reload (client re-attaches). ---
      await server.restartAbrupt()
      await waitForWsReady(page)
      await reloadAndReconnect(page, harness)

      // TARGET CONTRACT (§2.8.1): within 45s the pane must LEAVE 'running' --
      // any surfaced terminal state (lost/error/idle) is acceptable; a
      // forever-running status is the wedge.
      const rehydratedTabId = (await harness.getActiveTabId())!
      await expect
        .poll(
          async () =>
            findFreshAgentLeaf(await harness.getPaneLayout(rehydratedTabId))?.content?.status ?? null,
          { timeout: 45_000 },
        )
        .not.toBe('running')
    } finally {
      await server.stop()
      await fs.rm(sharedRoot, { recursive: true, force: true })
    }
  })
```

- [ ] **Step 5: `double-restart mid-recovery` (observe; pin P1.7 if red)**

```ts
  test('double-restart mid-recovery: a second SIGKILL during recovery must not duplicate or wedge', async ({
    page,
    e2eServerKind,
  }) => {
    expect(e2eServerKind).toBe('rust')
    // OBSERVE-THEN-PIN -- if red, pin P1.7 (§4.3): respawn caps and keyed-
    // create dedupe are dormant (paneReconcileV1 never sent), so a restart
    // landing mid-recovery can double-create or dead-end panes (F9).
    const CODEX_SESSION_ID = '55555555-4444-4333-8222-111111111111'
    const SESSION_TITLE = 'double-restart codex session'
    const sharedRoot = await fs.mkdtemp(path.join(os.tmpdir(), 'freshell-wall-dblrestart-'))
    const projectDir = path.join(sharedRoot, 'project')
    await fs.mkdir(projectDir, { recursive: true })
    const argLogPath = path.join(sharedRoot, 'codex-argv.jsonl')
    const fakeCodexPath = await installFakeCli(
      FAKE_CODEX_CLI_SOURCE,
      'codex',
      path.join(sharedRoot, 'bin'),
    )
    const { server, harness } = await bootWall(page, {
      env: { CODEX_CMD: fakeCodexPath, FAKE_CODEX_ARGV_LOG: argLogPath },
      setupHome: seedCodexHome(CODEX_SESSION_ID, SESSION_TITLE, projectDir),
    })
    try {
      await selectShellIfPickerShowing(page)
      await page.getByText(SESSION_TITLE, { exact: false }).first().click()
      const tabId = (await harness.getActiveTabId())!
      await expect
        .poll(async () => (await harness.getPaneLayout(tabId))?.content?.terminalId ?? null, {
          timeout: 20_000,
        })
        .not.toBeNull()
      const tabCountBefore = await harness.getTabCount()
      const argvCountBeforeKill = (await readArgvLog(argLogPath)).length

      // First SIGKILL; wait until recovery is IN FLIGHT (a new spawn hit the
      // argv log), then SIGKILL again mid-recovery.
      await server.restartAbrupt()
      await expect
        .poll(async () => (await readArgvLog(argLogPath)).length, { timeout: 45_000 })
        .toBeGreaterThan(argvCountBeforeKill)
      await server.restartAbrupt()
      await waitForWsReady(page)

      // CONTRACT: the pane settles resumed on the same session -- exactly one
      // pane, same tab count, not status:error, resumed argv in the final
      // recovery round.
      await expect(async () => {
        expect(await harness.getTabCount()).toBe(tabCountBefore)
        const content = (await harness.getPaneLayout(tabId))?.content
        expect(content?.status).not.toBe('error')
        expect(content?.sessionRef?.sessionId).toBe(CODEX_SESSION_ID)
        expect(content?.terminalId).toBeTruthy()
      }).toPass({ timeout: 60_000 })
      // No duplicate codex panes anywhere.
      const state = await harness.getState()
      const layouts = state?.panes?.layouts ?? {}
      let codexLeaves = 0
      for (const layout of Object.values(layouts)) {
        codexLeaves += collectLeaves(layout).filter(
          (l) => l?.content?.sessionRef?.sessionId === CODEX_SESSION_ID,
        ).length
      }
      expect(codexLeaves).toBe(1)
    } finally {
      await server.stop()
      await fs.rm(sharedRoot, { recursive: true, force: true })
    }
  })
```

- [ ] **Step 6: `hidden-pane rebind` (pin P1.11, F8)**

```ts
  test('hidden-pane rebind: a background tab pane must rebind without being revealed', async ({
    page,
    e2eServerKind,
  }) => {
    expect(e2eServerKind).toBe('rust')
    // EXPECTED-FAIL WALL PIN -- P1.11 (F8): hidden panes never send
    // create/attach on reconnect; background tabs do not rebind until
    // revealed. FLIP when F8 lands (hidden panes rebind on reconnect).
    test.fail(
      e2eServerKind === 'rust',
      'P1.11 (F8): hidden pane in a background tab does not rebind after SIGKILL',
    )
    const { server, harness, info } = await bootWall(page)
    try {
      await selectShellIfPickerShowing(page)
      const hiddenTabId = (await harness.getActiveTabId())!
      const hiddenTerminalIdBefore: string = await expect
        .poll(async () => (await harness.getPaneLayout(hiddenTabId))?.content?.terminalId ?? null, {
          timeout: 20_000,
        })
        .not.toBeNull()
        .then(async () => (await harness.getPaneLayout(hiddenTabId))?.content?.terminalId)

      // Second tab becomes active; the first is now hidden.
      await createTabViaRest(info, { mode: 'shell', cwd: os.tmpdir() })
      await harness.waitForTabCount(2)
      await expect
        .poll(async () => harness.getActiveTabId(), { timeout: 15_000 })
        .not.toBe(hiddenTabId)

      // --- SIGKILL + revive; do NOT touch the hidden tab. ---
      await server.restartAbrupt()
      await waitForWsReady(page)

      // TARGET CONTRACT (F8): the HIDDEN pane rebinds (new terminalId) within
      // 30s without being revealed.
      await expect
        .poll(async () => {
          const tid = (await harness.getPaneLayout(hiddenTabId))?.content?.terminalId ?? null
          return tid && tid !== hiddenTerminalIdBefore ? tid : null
        }, { timeout: 30_000 })
        .not.toBeNull()
    } finally {
      await server.stop()
    }
  })
```

- [ ] **Step 7: Run all six**

```bash
npx playwright test --config test/e2e-browser/playwright.config.ts \
  --project=rust-chromium restore-contract-wall \
  -g "SIGKILL-within-5s|SIGKILL-inside-locator|two-clients|busy-restart|double-restart|hidden-pane"
```

Expected: the four hard-pinned tests report as expected failures;
`double-restart mid-recovery` either passes (leave green, note in commit) or
gets pinned P1.7 per the decision rule. For every pinned test, confirm from
the output that the failure is the post-restart contract assertion, not setup.

- [ ] **Step 8: Commit**

```bash
git add test/e2e-browser/specs/restore-contract-wall-rust.spec.ts test/e2e-browser/fixtures/fake-opencode-terminal.mjs
git commit -m "test(e2e): six named §5 P0.1 red tests pinned expected-fail in the wall"
```

---

### Task 11: Full verification, push, and STOP (no PR)

**Files:** none (verification only).

- [ ] **Step 1: Run the entire wall spec end-to-end**

```bash
cd /home/dan/code/freshell/.worktrees/restore-contract-wall
npx playwright test --config test/e2e-browser/playwright.config.ts \
  --project=rust-chromium restore-contract-wall
```

Expected: **all tests green** — passes for the working contracts, "expected
failures" for the pinned ones, zero unexpected failures, zero unexpected
passes. If an expected-fail test unexpectedly passes, apply the FLIP
instruction (remove that pin) and re-run.

- [ ] **Step 2: Prove the wall did not break spec selection for the rest of the suite**

```bash
npx playwright test --config test/e2e-browser/playwright.config.ts --project=chromium --list | tail -5
npx playwright test --config test/e2e-browser/playwright.config.ts \
  --project=rust-chromium compound-restart-rust
```

Expected: `--list` succeeds (config parses; wall spec absent from `chromium`);
`compound-restart-rust` still passes (the config edit is additive).

- [ ] **Step 3: Coordinated sanity gate (only test files changed, but run it — the repo rule for broad runs)**

```bash
npm run test:status
# If another agent holds the gate: WAIT until idle, then:
FRESHELL_TEST_SUMMARY="restore contract wall: new e2e spec + fixtures, no production changes" npm test
```

Expected: green (no `src/`/`crates/` changes were made — verify with
`git diff origin/main --stat` that only `test/e2e-browser/**` and
`docs/plans/2026-07-24-restore-contract-wall.md` changed).

- [ ] **Step 4: Push the branch and STOP**

```bash
git push -u origin feat/restore-contract-wall
```

Then STOP — **do not run `gh pr create`** (not user-approved). Report:
- branch name `feat/restore-contract-wall`;
- what the wall covers: 8 per-pane SIGKILL restore contracts (shell,
  claude/codex/opencode terminals, freshcodex, freshopencode, freshclaude,
  browser+editor), the all-pane ruler, and 6 named §5 P0.1 red tests;
- which entries are expected-fail and their pins (freshclaude restore → P0.2;
  the ruler → P0.1; SIGKILL-within-5s → P1.8+P1.9; SIGKILL-inside-locator-window
  → P1.8; two-clients-same-sessionRef → P1.7; freshclaude busy-restart → P0.2;
  hidden-pane rebind → P1.11; double-restart → as observed);
- any predicted-PASS contracts that had to be pinned (with observed failure
  modes) — these are new findings for the campaign.
