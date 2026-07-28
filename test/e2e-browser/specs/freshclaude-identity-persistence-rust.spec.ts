/**
 * FRESHCLAUDE CLIENT IDENTITY PERSISTENCE -- P0.2 close-out (lane D4).
 * Pins SHIPPED behavior end-to-end (coverage pins, not red-first TDD --
 * the red->green story for this lane is the wall's leg G reader fix):
 *   1. converse -> RELOAD (identity survives via the browser's persisted
 *      sessionRef alone) -> server SIGKILL restart -> the SAME conversation
 *      resumes.
 *   2. HAZARD GUARD (the reason persistMiddleware stripped sessionId in the
 *      first place -- 2026-04-19 durable-session contract): a STALE persisted
 *      sessionRef (transcripts deleted server-side) yields the LOUD
 *      dead_session adjudication flow -- never a silent wrong-session attach
 *      and never a silent fresh.
 * Rust-only: registered in RUST_ONLY_SPECS + rust-chromium testMatch.
 * Helpers copied, not imported, per this suite's per-spec-ownership
 * convention (donor: restore-contract-wall-rust.spec.ts).
 */
import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import type { Page } from '@playwright/test'
import { test, expect } from '../helpers/fixtures.js'
import { RustServer, type TestServerInfo } from '../helpers/rust-server.js'
import { TestHarness } from '../helpers/test-harness.js'
import { openPanePicker } from '../helpers/pane-picker.js'
// NOTE: `Page` comes from '@playwright/test' (fixtures.ts exports only
// `test`/`expect`, as every donor spec does). `openPanePicker` is IMPORTED,
// not copied -- the copied `createFreshclaudePane` body calls it (wall :29,
// :448). `fileURLToPath` feeds the FAKE_CLAUDE_SIDECAR_SOURCE constant below
// (wall :34, :39).

// ESM project ("type": "module" in package.json): __dirname does not exist in
// ESM modules, so derive it -- same convention as every fixture-referencing
// donor spec (e.g. compound-restart-rust.spec.ts:49-51).
const __filename = fileURLToPath(import.meta.url)
const __dirname = path.dirname(__filename)

const FAKE_CLAUDE_SIDECAR_SOURCE = path.resolve(__dirname, '../fixtures/fake-claude-sidecar.mjs')

const DURABLE_CLI_SESSION_ID = '44444444-4444-4444-8444-444444444444'

// The contract-correct identity reader: sessionRef IS the durable identity;
// content.sessionId is a live handle (precedent:
// freshclaude-restart-parity-rust.spec.ts:140-148).
const durableIdentity = (leaf: any): string =>
  leaf?.content?.sessionRef?.sessionId ?? leaf?.content?.resumeSessionId ?? ''

// ---------------------------------------------------------------------------
// Shared helpers (per-spec copies -- donor: restore-contract-wall-rust.spec.ts)
// ---------------------------------------------------------------------------

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

// --- freshclaude fresh-agent helper (fixture: fake-claude-sidecar.mjs via the
// production env seam FRESHELL_CLAUDE_SIDECAR) ---

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
  // NOTE (corrected, council fix round -- the Rust router HAS shipped a
  // claude snapshot adapter since this comment was written: crates/
  // freshell-freshagent/src/snapshot.rs:133-146 routes freshclaude/kilroy +
  // claude through get_claude_snapshot(), a disk+env adapter over the CLI's
  // own transcript store; FRESH_AGENT_RUNTIME_UNAVAILABLE now only fires for
  // session types with NO adapter registered at all). A transient
  // history-load-error banner may still appear on a freshly-created pane
  // (snapshot fetch racing pane creation), so this suite still asserts pane
  // state via the harness (Redux) rather than error-free UI chrome for
  // freshclaude -- but the reason is a fetch-timing race, not a missing
  // adapter.
}

test.describe('Freshclaude identity persistence (P0.2)', () => {
  test.setTimeout(180_000)

  test('durable identity survives browser reload, then SIGKILL restart resumes the SAME conversation', async ({ page, e2eServerKind }) => {
    expect(e2eServerKind).toBe('rust')
    const sharedRoot = await fs.mkdtemp(path.join(os.tmpdir(), 'freshell-identity-freshclaude-'))
    const projectDir = path.join(sharedRoot, 'project')
    await fs.mkdir(projectDir, { recursive: true })
    const { server, info, harness } = await bootWall(page, {
      // EXACT leg-G options (wall :1174-1177):
      env: { FRESHELL_CLAUDE_SIDECAR: FAKE_CLAUDE_SIDECAR_SOURCE },
      setupHome: seedWallConfig({ providers: ['claude'], freshAgent: true }),
    })
    try {
      // Donor creation sequence, copied from leg G :1179-1187 (leg M
      // :2017-2022 is identical): settle the boot picker, THEN read the tab
      // id, THEN the boot-picker fade-out guard (.xterm visible), THEN
      // create. Skipping the guard makes openPanePicker race the boot
      // picker's fade-out and the Freshclaude click is swallowed (donor
      // comment at leg G :1181-1185).
      await selectShellIfPickerShowing(page)
      const tabId = (await harness.getActiveTabId())!
      expect(tabId).toBeTruthy()
      await expect(page.locator('.xterm').first()).toBeVisible({ timeout: 30_000 })
      // createFreshclaudePane returns void (:436); the tab id comes from the
      // harness, exactly as the wall's leg G does (:1180).
      await createFreshclaudePane(page, harness, projectDir)
      await sendFreshAgentTurn(page, harness, tabId!, 'first turn before reload')
      await expect(page.locator('[data-context="fresh-agent"]').last()).toContainText('Fixture claude turn', { timeout: 30_000 })

      // THE FOLD (shipped behavior: FreshAgentView.tsx's durable-identity
      // merge effect -- cited at plan-time base 7508149b as :1798-1830; on
      // current main the same mergePaneContent effect sits around
      // :1976-2015, line numbers having drifted as unrelated work landed
      // around it. See the plan doc's status banner.): pane content carries
      // the durable ref. Captured (not
      // hardcoded) so the round-trip checks below assert against what this
      // RUN actually minted -- same discipline as freshclaude-restart-parity
      // -rust.spec.ts's `originalDurable` capture (:236-243).
      await expect
        .poll(async () => durableIdentity(findFreshAgentLeaf(await harness.getPaneLayout(tabId!))), { timeout: 15_000 })
        .toBe(DURABLE_CLI_SESSION_ID)
      const originalDurable: string = durableIdentity(findFreshAgentLeaf(await harness.getPaneLayout(tabId!)))

      // Council fix round (B2): clearSentWsMessages() moved to BEFORE the
      // reload (not just before the SIGKILL restart). The fixture mints the
      // SAME static id for every resume-less create, so a regressed
      // persistMiddleware that silently drops sessionRef would make the
      // reload fire a bare (no-resumeSessionId) freshAgent.create that gets
      // re-stamped with this identical id -- the durableIdentity poll above
      // would stay green even though a NEW session was actually minted
      // (colliding onto the SAME transcript file). Auditing the FULL
      // reload+restart window (not just the post-reload slice) is what makes
      // that regression visible below: any bare create in this window fails
      // the resumeSessionId assertion instead of being silently discarded by
      // an audit window that opened too late.
      await harness.clearSentWsMessages()

      // RELOAD FIRST (browser-persisted identity alone, no server help).
      await flushPersistence(page)
      await reloadAndReconnect(page, harness)
      const tabIdAfterReload = await harness.getActiveTabId()
      await expect
        .poll(async () => durableIdentity(findFreshAgentLeaf(await harness.getPaneLayout(tabIdAfterReload!))), { timeout: 30_000 })
        .toBe(originalDurable)

      // THEN the SIGKILL restart.
      await server.restartAbrupt()
      await waitForWsReady(page)

      // Identity held; conversation continues end-to-end.
      await expect
        .poll(async () => durableIdentity(findFreshAgentLeaf(await harness.getPaneLayout(tabIdAfterReload!))), { timeout: 30_000 })
        .toBe(originalDurable)
      await sendFreshAgentTurn(page, harness, tabIdAfterReload!, 'second turn after restart')
      await expect(page.locator('[data-context="fresh-agent"]').last()).toContainText('Fixture claude turn', { timeout: 30_000 })
      await expect(page.locator('[data-context="fresh-agent"]').last()).toContainText('first turn before reload', { timeout: 30_000 })

      // Every create sent across the reload+restart window targeted the
      // ORIGINAL session -- no identity-losing re-create. A create IS
      // expected here: the post-SIGKILL reconcile verdict re-drives exactly
      // one RESPAWN freshAgent.create carrying resumeSessionId (verified
      // empirically for this exact flow: terminal.detach -> terminal.create
      // -> freshAgent.create -> freshAgent.attach). Asserting the count is
      // non-zero proves this audit is exercising a real code path, not
      // vacuously passing over an empty array.
      const sent = await harness.getSentWsMessages()
      const creates = sent.filter((m: any) => m?.type === 'freshAgent.create') as any[]
      expect(creates.length, JSON.stringify(sent.map((m: any) => m?.type))).toBeGreaterThan(0)
      for (const create of creates) {
        expect(create.resumeSessionId ?? create.sessionRef?.sessionId, JSON.stringify(create)).toBe(originalDurable)
      }
      const finalLeaf = findFreshAgentLeaf(await harness.getPaneLayout(tabIdAfterReload!))
      expect(finalLeaf?.content?.status).not.toBe('error')

      // Positive turn-2 proof from disk (not pre-SIGKILL DOM): the fake
      // sidecar's transcript file is the ground truth for what the fixture
      // actually received post-restart. Both turns' user text must be
      // present as SEPARATE lines and there must be two assistant replies --
      // this cannot be satisfied by leftover pre-restart DOM state.
      const transcriptFile = path.join(info.homeDir, '.claude', 'projects', '-fixture', `${originalDurable}.jsonl`)
      const transcriptLines = (await fs.readFile(transcriptFile, 'utf-8'))
        .trim()
        .split('\n')
        .filter(Boolean)
        .map((line) => JSON.parse(line))
      const assistantTurnCount = transcriptLines.filter((l: any) => l.type === 'assistant').length
      expect(assistantTurnCount, JSON.stringify(transcriptLines)).toBeGreaterThanOrEqual(2)
      const userTexts = transcriptLines
        .filter((l: any) => l.type === 'user')
        .map((l: any) => l.message?.content?.[0]?.text)
      expect(userTexts).toEqual(expect.arrayContaining(['first turn before reload', 'second turn after restart']))
    } finally {
      await server.stop()
      await fs.rm(sharedRoot, { recursive: true, force: true })
    }
  })

  test('HAZARD GUARD: stale persisted sessionRef yields loud dead_session, never silent wrong-session attach or silent fresh', async ({ page, e2eServerKind }) => {
    expect(e2eServerKind).toBe('rust')
    const sharedRoot = await fs.mkdtemp(path.join(os.tmpdir(), 'freshell-identity-stale-'))
    const projectDir = path.join(sharedRoot, 'project')
    await fs.mkdir(projectDir, { recursive: true })
    const { server, info, harness } = await bootWall(page, {
      // Same exact leg-G options as test 1 (env: fake sidecar via
      // FAKE_CLAUDE_SIDECAR_SOURCE; setupHome: seedWallConfig claude+freshAgent).
      env: { FRESHELL_CLAUDE_SIDECAR: FAKE_CLAUDE_SIDECAR_SOURCE },
      setupHome: seedWallConfig({ providers: ['claude'], freshAgent: true }),
    })
    try {
      // Same donor creation sequence as test 1 (leg G :1179-1187): settle
      // guard, tab id, .xterm fade-out guard, then create.
      await selectShellIfPickerShowing(page)
      const tabId = (await harness.getActiveTabId())!
      expect(tabId).toBeTruthy()
      await expect(page.locator('.xterm').first()).toBeVisible({ timeout: 30_000 })
      await createFreshclaudePane(page, harness, projectDir)
      await sendFreshAgentTurn(page, harness, tabId!, 'turn that will become stale')
      await expect
        .poll(async () => durableIdentity(findFreshAgentLeaf(await harness.getPaneLayout(tabId!))), { timeout: 15_000 })
        .toBe(DURABLE_CLI_SESSION_ID)
      await flushPersistence(page)

      // Make the persisted identity STALE: delete every server-side artifact
      // naming the durable session (transcripts under the isolated HOME).
      const deleted = await deleteFilesNamed(info.homeDir, `${DURABLE_CLI_SESSION_ID}.jsonl`)
      expect(deleted.length, `expected transcript artifacts for ${DURABLE_CLI_SESSION_ID} under ${info.homeDir}`).toBeGreaterThan(0)

      // SIGKILL, then reload IMMEDIATELY -- the OLD page must never
      // reconnect and fire a recovery create-with-resume: the fake sidecar
      // re-creates the transcript on ANY create carrying resumeSessionId
      // (fake-claude-sidecar.mjs:95, fs.openSync(..., 'a')), after which the
      // session is Present again and dead_session can never surface. No
      // waitForWsReady on the old page here, by design.
      await harness.clearSentWsMessages()
      await server.restartAbrupt()
      await reloadAndReconnect(page, harness)
      const tabIdAfter = await harness.getActiveTabId()

      // LOUD: the dead-session adjudication surfaces the stale claim.
      await expect
        .poll(async () => {
          const state = await harness.getState()
          const entries = state?.panes?.deadSessionAdjudication ?? []
          return entries.some((e: any) => e?.kind === 'fresh-agent' && e?.sessionRef?.sessionId === DURABLE_CLI_SESSION_ID)
        }, { timeout: 30_000 })
        .toBe(true)
      const leaf = findFreshAgentLeaf(await harness.getPaneLayout(tabIdAfter!))
      expect(leaf?.content?.restoreError?.reason).toBe('durable_artifact_missing')
      // Batched dead-session adjudication is a real modal dialog
      // (DeadSessionPanel.tsx:20-24, role="dialog" aria-label="Dead sessions"),
      // not just Redux state -- assert it's actually presented to the user.
      await expect(page.getByRole('dialog', { name: 'Dead sessions' })).toBeVisible({ timeout: 10_000 })
      // Hazard-guard hardening (council fix round): give any async
      // create-firing code a settle window BEFORE snapshotting sent
      // messages, so this "never fires" assertion can't pass merely because
      // we checked too early for a delayed create to have gone out yet.
      // FORWARD-LOOKING HAZARD (follow-up from the PR #562 council review):
      // this settle window is a fixed 1s. If freshell-ws auto-resume
      // supervision ever extends to fresh-agent drivers (today it only
      // covers terminal panes), a respawn create arriving with a backoff
      // longer than 1s would land AFTER this snapshot and be invisible to
      // the assertion below -- silently defeating this hazard guard. Any PR
      // extending auto-resume supervision to fresh-agent drivers must widen
      // this window (or poll) accordingly.
      await page.waitForTimeout(1_000)
      // Re-fetch the leaf AFTER the settle window: the `leaf` captured above
      // (before the settle) only proves the initial dead_session
      // adjudication fired -- it is a stale snapshot with respect to any
      // create that might land during the settle window, so asserting on it
      // here would not actually prove "never silent" (follow-up from the PR
      // #562 council review).
      const leafAfterSettle = findFreshAgentLeaf(await harness.getPaneLayout(tabIdAfter!))
      // NEVER silent: identity not swapped, no create fired for this pane.
      expect(leafAfterSettle?.content?.sessionRef?.sessionId).toBe(DURABLE_CLI_SESSION_ID)
      const sent = await harness.getSentWsMessages()
      expect(sent.filter((m: any) => m?.type === 'freshAgent.create')).toEqual([])
    } finally {
      await server.stop()
      await fs.rm(sharedRoot, { recursive: true, force: true })
    }
  })
})

/** Recursively delete files with the given basename under root; returns deleted paths. */
async function deleteFilesNamed(root: string, basename: string): Promise<string[]> {
  const hits: string[] = []
  async function walk(dir: string): Promise<void> {
    let entries
    try {
      entries = await fs.readdir(dir, { withFileTypes: true })
    } catch {
      return
    }
    for (const entry of entries) {
      const p = path.join(dir, entry.name)
      if (entry.isDirectory()) await walk(p)
      else if (entry.name === basename) {
        await fs.rm(p)
        hits.push(p)
      }
    }
  }
  await walk(root)
  return hits
}
