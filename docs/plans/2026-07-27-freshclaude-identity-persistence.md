# Freshclaude Durable Identity Persistence — Verification, Hazard Guard, and P0.2 Wall Pin Flip Implementation Plan

> **For agentic workers:** This plan is executed task-by-task by the
> workflow's execute stage: a fresh implementer per task, with a spec +
> quality review after each task. Steps use checkbox (`- [ ]`) syntax
> for tracking.

**Goal:** Close the last expected-fail pin (P0.2) in the restore contract wall by correcting the wall's durable-identity READER (`leafDurableIdentity` is `content.sessionId`-first; the contract's durable identity is `sessionRef`), and pin the underlying behavior — a freshclaude/kilroy pane's durable CLI session id already survives browser reload via persisted `content.sessionRef` — with a persistence unit pin, an end-to-end journey spec, and a stale-identity hazard guard.

**Architecture (corrected 2026-07-27 by fresh-eyes review — see the re-grounding addendum):** The client already both LEARNS and PERSISTS the durable id. `cliSessionId` arrives on `freshAgent.session.init`/`.metadata`, lands in `freshAgentSlice`, and `FreshAgentView`'s durable-identity merge effect (`FreshAgentView.tsx:1798-1830`) folds the canonical UUID into pane content as `sessionRef: { provider: 'claude', sessionId }` + `resumeSessionId` via `mergePaneContent`. `persistMiddleware` round-trips `sessionRef` (and strips the live `sessionId` placeholder and `resumeSessionId`, exactly as the durable-session contract requires). Post-reload, the mount create-effect proceeds on `sessionRef` and `buildCreateMessage` derives a create-with-resume. **No production code change is in scope.** The wall leg G is red ONLY because its identity reader compares the ephemeral placeholder across reloads. We fix the reader, flip the pin, and pin the behavior with tests.

**Tech Stack:** React + Redux Toolkit (immer) client, Vitest units (via repo test coordinator), Playwright e2e against per-test `RustServer` instances with the fake claude sidecar fixture.

## Global Constraints

- Worktree: `/home/dan/code/freshell/.worktrees/freshclaude-identity-persistence` (branch `freshclaude-identity-persistence`). ALL work happens here.
- Base: CURRENT `origin/main` — fetch first; `~3f096412` or newer (at plan time `origin/main` is `7508149b`).
- e2e servers: every spec owns its `RustServer` instances on ephemeral ports via `findFreePort()`; **NEVER ports 3001/3002** — the user's LIVE server is on 3002.
- **NEVER restart the user's self-hosted Freshell server. NEVER use broad kill patterns** (no `pkill -f vite`, `pkill node`, etc.) — AGENTS.md Process Safety.
- Broad test runs wait on the shared coordinator gate (3 sibling lanes run concurrently): check `npm run test:status` first; if another agent holds the gate, WAIT. Set `FRESHELL_TEST_SUMMARY="lane D4 freshclaude identity persistence"` on broad runs. Prefix unit runs with `env -u FRESHELL_BIND_HOST`.
- Worktree may need `npm ci` and the tsx symlink: `ln -s ../node_modules/tsx node_modules/tsx` (only if `npm test` complains).
- SCOPE FENCE — may touch ONLY: the wall spec's P0.2 pin block + its identity reader (`test/e2e-browser/specs/restore-contract-wall-rust.spec.ts`), the NEW e2e spec file, `test/e2e-browser/playwright.config.ts` registration lines, and `test/unit/client/store/panesPersistence.test.ts`. **NO production `src/` or `crates/` changes are in scope** — the persistence behavior already shipped; **if a production change seems required at any point, STOP the task and report** (that would mean the corrected diagnosis is wrong somewhere and the plan must be revisited, not improvised around). Do NOT touch: `TerminalView` / `registry.rs` / exited-pane UI (Lane D1); `crates/` freshagent rust code + spawn gate (D2); D3's flake-test regions (double-restart test, remote-proxy, sidebar case-a, pane_ledger tests).
- Wall pin discipline (file doc of the wall spec, lines 12-16): flip = **DELETE the `test.fail(...)` call** and rewrite the pin comment into a HISTORY note. Never widen a pin; never convert to `test.fixme`.
- PR POLICY: **NOT approved.** Push the branch, STOP before `gh pr create`. Final report must include: branch name, the archaeology finding (corrected), red→green proof including the full-wall-green run.
- Test-first honesty: this lane introduces NO new production behavior, so its new tests are **coverage pins of shipped behavior** (expected green on first run) — the lane's red→green story is the wall leg G itself: red under the sessionId-first reader (baselined in Task 1), green after the reader fix (Task 4). State this plainly wherever red/green status is reported; never present a green-on-first-run pin as a TDD red-first driver.
- Conventional, focused commits.
- Long commands (e2e, coordinated suites) can run 10–30+ minutes — use generous timeouts and never kill a coordinator-gated run.

---

## Archaeology: why `persistMiddleware` strips `content.sessionId` — and why the shipped `sessionRef` persistence is contract-compliant

*(Required context for every task. Sourced from `git log --follow`/`git blame` on `src/store/persistMiddleware.ts` and the contract docs; verify with `git show 976d3d48` if needed.)*

**Original rationale.** The strip was introduced by commit `976d3d48` "Repair fresh-agent persistence migrations" (2026-05-08; re-landed through squash `d4c7f5b5`, PR #358 — the squash message notes interim history was lost). The commit implements the written contract `docs/plans/2026-04-19-exact-durable-session-contract.md`:

- §1: "`sessionRef` is the only replay-safe identity written to persisted terminal pane/tab state… live reattach handles… **must never be interpreted as durable restore targets**."
- §2: "Live handles are not replay targets and are never used as durable restore keys."
- §3: live inputs "may exist in memory to finish the current live session, but **they are never persisted as restore targets**."
- Restore-Unavailable Rule: absence must be an explicit `RESTORE_UNAVAILABLE`, never a surviving stale token.

This is the incident-class the strip defends against: **stale persisted live-handle identity causing a wrong-session attach** after the handle was recycled by a different server process. The narrow fresh-agent exception (same commit, same hunk) is a *same-server lease*: it re-admits a bare `sessionId` only when co-persisted with `serverInstanceId`, so another server process can never mistake it for durable identity. There is no `docs/incidents/` entry; the contract doc IS the recorded rationale.

**Why the shipped `sessionRef` persistence does not reintroduce the hazard.**

1. **The live handle is never persisted.** `content.sessionId` (the sidecar-minted nanoid placeholder) is stripped, and `resumeSessionId` is stripped too (`stripTransientSessionFields`, `persistMiddleware.ts:245-268`). What persists is the **durable provider session id** in `content.sessionRef` — the exact field §1 designates as "the only replay-safe identity", round-tripped via `sanitizeSessionRef`.
2. **Canonical gating.** The shipped fold refuses any claude id that is not UUID-shaped: `getCanonicalDurableSessionId` (`src/store/persistControl.ts:87-95`) only returns canonical UUIDs, so a placeholder can never masquerade as durable identity. Rehydration additionally re-checks via `migrateLegacyFreshAgentDurableState({ rejectNonCanonicalClaudeSessionRef: true })` (`persistedState.ts:293-302`).
3. **The §4.2 authority chain makes a persisted client identity a PROPOSAL, not truth.** The chain (campaign plan `docs/plans/2026-07-24-restart-resilience-architecture-analysis.md` §4.2): *in-memory registry (live process truth) → ledger `bound` rows (durable server truth) → client claim (proposal only) → tabs-snapshot (rescue mirror)*. On disagreement "the ledger wins for identity; the client's claim is recorded and answered with a `corrected` verdict… user-visible, never a silent switch." With verdicts live since wave C (`foldFreshAgentVerdict`, `src/lib/pane-reconcile.ts:262-357`), a stale client claim gets `corrected:true` or a loud `dead_session` breadcrumb — never silently honored. Task 3's stale-sessionRef e2e test pins exactly this.

**Validated scope notes (2026-07-27 load-bearing review — evidence in the review ledger; server cites verified against this worktree):**

- The `dead_session` guarantee was verified in server code: an Absent-but-ever-observed claim yields `dead_session{session_not_on_disk}` echoing the claimed ref (`crates/freshell-ws/src/reconcile_freshagent.rs:120-129, 259-265`); `ever_observed` is durable across restart via the ledger; no reconcile arm can bind a *different* session id to a claim (wrong-session attach is structurally impossible). The reconcile snapshot consumes ONLY `pane.session_ref` for fresh-agent panes (`reconcile_freshagent.rs:73-79`) — exactly the field that persists.
- **The guarantee's precondition is the durable binding recorded at `session.init`** — and `claude.rs:1156-1159` skips that binding when the create carried all-default settings (the no-laundering guard, `identity_sink.rs:10-17`). This hole is unreachable from the shipped client: the sole create constructor `buildCreateMessage` (`FreshAgentView.tsx:951-972`) sends `effort:` unconditionally (`:969`, registry fallback `'high'`, `shared/fresh-agent-models.ts:131-152`), so every real freshclaude/kilroy create is non-default and always binds. **Watch note:** this invariant is un-pinned — do NOT refactor `buildCreateMessage` toward "send only user-chosen fields" without adding a pin, or the silent-`fresh` hole reopens.
- **Bound on "never":** ledger retention (~30/90d GC horizon) means an *ancient* stale ref can age out of `ever_observed` and adjudicate `fresh` (silent) instead of `dead_session` — but never a wrong-session attach (the actual incident class), and claude transcript retention (`cleanupPeriodDays` ~30d) makes such refs unresumable anyway. Accepted residual.
- **Real claude resume semantics (verified externally):** default `--resume`/SDK resume REUSES the original session UUID and appends to the same transcript — it does NOT mint a new id per resume; a new id requires explicit `--fork-session`, which our sidecar does not pass (`crates/freshell-claude-sidecar/index.mjs:209`). So the fixture's stable durable id *matches* production default semantics, and a single overwritable `sessionRef` is the correct data model in both id-worlds. **Watch note (future hardening, out of scope):** a cwd-mismatched resume can silently create a fresh session under a new valid UUID which the newest-wins fold would adopt — upstream CLI/SDK behavior, not introduced by this change.

---

## Design overview and file structure

**The corrected diagnosis (one paragraph).** A freshclaude pane's `content.sessionId` is an ephemeral nanoid minted by the Node claude sidecar (`crates/freshell-claude-sidecar/index.mjs:199`); the server broadcasts `freshAgent.created` with `session_ref: None` for claude (`crates/freshell-freshagent/src/claude.rs:500-509`) and never emits `freshAgent.session.materialized` for claude. The durable Claude UUID arrives as `event.cliSessionId` on `freshAgent.session.init`/`.metadata`, is written to `freshAgentSlice.sessions[key].cliSessionId` (`src/store/freshAgentSlice.ts:256`, `:282`) — **and from there it DOES reach persisted pane content**: `FreshAgentView`'s durable-identity merge effect (`FreshAgentView.tsx:1798-1830`) reactively writes `sessionRef: { provider: 'claude', sessionId: <canonical UUID> }` + `resumeSessionId` into pane content via `mergePaneContent`, canonical-gated by `getCanonicalDurableSessionId` (`persistControl.ts:87-95`). `stripTransientSessionFields` (`persistMiddleware.ts:245-268`) keeps `sessionRef` and strips `sessionId`/`resumeSessionId`. Post-reload the pane rehydrates with `sessionRef`; the mount create-effect proceeds (`FreshAgentView.tsx:1200-1207`) and `buildCreateMessage` (`:951-972`) derives `resumeSessionId` from `sessionRef` — a create-with-resume, same conversation. Independent corroboration already in-repo: the parity spec's `liveDurableIdentity` reader + comment (`freshclaude-restart-parity-rust.spec.ts:138-150`, green suite) and the wall's own P0.1 ruler pin narration, which observes the post-reload pane carrying the canonical UUID (`restore-contract-wall-rust.spec.ts:1367-1370`).

**Why P0.2 is red anyway (the actual gap).** The wall's `leafDurableIdentity` (`restore-contract-wall-rust.spec.ts:245-251`) reads `content.sessionId` FIRST. For claude that is the `fc-e2e-*` placeholder forever — a *live handle* that legitimately differs across reload/respawn — so leg G compares placeholder A against placeholder B and fails even though the durable identity survived in `sessionRef`. The repo's own contract says durable identity IS `sessionRef`; the sibling parity spec already documents this exact correction ("Deliberately NOT the donor's leafDurableIdentity: its first fallback arm is content.sessionId, which for a live claude pane is the create-time fc-e2e-* nanoid forever") and reads `sessionRef.sessionId` first. Reordering `leafDurableIdentity` to sessionRef-first is safe for the other fresh-agent legs (E freshcodex, F freshopencode): both compare pre/post through the SAME reader, and where `sessionRef` exists it equals the durable id. The full-wall run in Task 4 proves this.

**Known fragilities of the shipped fold (documented; out of scope — no production change):**

- The fold lives in a component effect, so it requires the pane's `FreshAgentView` to be MOUNTED when the id is learned. Background tabs are fine (`App.tsx:1645` renders every tab; hidden via `visibility:hidden`), but pane ZOOM unmounts sibling leaves (`PaneLayout.tsx:67-68`) — a durable id learned while zoomed-away folds only after unzoom (the effect re-runs on remount).
- The `sessionRef` write rides the effect's "resumeSessionId changed or restoreError present" gate (`FreshAgentView.tsx:1806`) rather than being independently reconciled.
- `buildFreshAgentPersistedIdentityUpdate` (`persistControl.ts:207`) has zero callers — the component effect is the only claude fold writer.

If any of these bites during this lane's verification (Task 3/4 STOP-gates), report it with the captured output — hardening the fold is a production change and belongs to a follow-up lane, not this one.

**The change (all test-side).**

| File | Change |
|---|---|
| `test/unit/client/store/panesPersistence.test.ts` | Persist round-trip pin: `sessionRef` survives, live placeholder + `resumeSessionId` stripped (Task 2). |
| `test/e2e-browser/specs/freshclaude-identity-persistence-rust.spec.ts` | NEW spec pinning shipped behavior: (1) reload → SIGKILL → same-conversation journey; (2) stale-sessionRef → loud `dead_session`, never silent wrong-session attach (Task 3). |
| `test/e2e-browser/playwright.config.ts` | Register the new spec in `RUST_ONLY_SPECS` (~`:89`) and `rust-chromium` `testMatch` (~`:265`) (Task 3). |
| `test/e2e-browser/specs/restore-contract-wall-rust.spec.ts` | Make `leafDurableIdentity` (`:245-251`) sessionRef-first + flip the P0.2 pin (delete `test.fail` at `:1165-1168`, rewrite comment to HISTORY note) (Task 4). |

**Watch items during Task 4's full-wall run:**
- Leg I "THE RULER" (`:1359`, pinned at `:1380`) — its pin text names the P0.2 gap. If closing P0.2 makes it *unexpectedly pass*, Playwright reports that as a hard failure: flip its pin too (documented coupling). If it stays red (remaining P1.x), leave its pin alone.
- Leg M "freshclaude busy-restart" (`:1989`) — exercise care: the reader change makes this leg's identity comparisons look at `sessionRef` too. Re-observe, don't assume.
- Legs J/K (creation-window pins, D3 territory) must NOT flip — if they do, stop and investigate before touching them.

---

### Task 1: Sync worktree to current origin/main, baseline green, pin inventory

**Files:**
- No source changes. (Working dir for every command: `/home/dan/code/freshell/.worktrees/freshclaude-identity-persistence`.)

**Interfaces:**
- Consumes: nothing.
- Produces: a worktree rebased on current `origin/main`, a recorded green baseline, and confirmation the P0.2 pin still exists (all later tasks assume this).

- [ ] **Step 1: Rebase the branch onto current origin/main**

```bash
cd /home/dan/code/freshell/.worktrees/freshclaude-identity-persistence
git fetch origin
git rebase origin/main
git log --oneline -5
```

Expected: clean rebase — the branch carries only this plan document's commits (docs-only), which cannot conflict with source changes on main. If the rebase surprises you with conflicts, STOP and report.

- [ ] **Step 2: Ensure node deps work in this worktree**

```bash
node --version && npm --version
ls node_modules/.bin/vitest >/dev/null 2>&1 || npm ci
```

If a later `npm test` complains about tsx: `ln -s ../node_modules/tsx node_modules/tsx`.

- [ ] **Step 3: Verify the P0.2 pin is still present (STOP-gate)**

```bash
grep -n "test.fail(" test/e2e-browser/specs/restore-contract-wall-rust.spec.ts
grep -n "narrowed 2026-07-26 by reconcile-completion" test/e2e-browser/specs/restore-contract-wall-rust.spec.ts
```

Expected: a `test.fail(` inside the `freshclaude: SIGKILL restore rebinds` test (~line 1165) whose reason string contains "narrowed 2026-07-26 by reconcile-completion". Record the full pin inventory (all `test.fail(` lines) in your notes — Task 4 compares against it. **If the freshclaude pin is gone or its reason changed materially, STOP and report** (another lane may have closed or reshaped P0.2).

- [ ] **Step 4: Baseline unit suite green (coordinator-gated)**

```bash
npm run test:status
FRESHELL_TEST_SUMMARY="lane D4 baseline" env -u FRESHELL_BIND_HOST npm test
```

Expected: PASS. If the coordinator gate is held by a sibling lane, WAIT (poll `npm run test:status`); never kill a foreign holder. If baseline is red, STOP and report — do not build on a red base.

- [ ] **Step 5: Baseline the pinned wall leg (records today's red)**

```bash
npm run test:e2e -- --project=rust-chromium specs/restore-contract-wall-rust.spec.ts -g 'freshclaude: SIGKILL restore rebinds'
```

Expected: 1 **expected-failure** (Playwright reports the `test.fail()`-annotated test as passing-the-suite because it failed as expected). Save the output — this is the red half of the red→green proof for the wall leg. (First e2e invocation also builds client+server via global-setup; allow ~10 min.)

- [ ] **Step 6: Baseline the FULL wall (Task 4's attribution baseline)**

```bash
npm run test:e2e -- --project=rust-chromium specs/restore-contract-wall-rust.spec.ts
```

Expected: all legs green, remaining pins failing-as-expected. Save the full output alongside the Step 3 pin inventory — **Task 4 Step 4's decision protocol compares against exactly this baseline** (3 sibling lanes are churning wall-adjacent territory; without a fresh baseline, a pre-existing red would be mis-attributed to our change, or a foreign flip mis-flipped). If any leg is ALREADY red/flipped in ways the pin inventory doesn't explain, record it and proceed — Task 4 treats those as pre-existing, not ours. (Allow 10–30 min.)

No commit for this task.

---

### Task 2: Persist round-trip pin test (contract documentation)

**Files:**
- Test: `test/unit/client/store/panesPersistence.test.ts` (append)

**Interfaces:**
- Consumes: the persist/reload seams this file already exercises (`loadInitialPanesState` / persisted-layout round-trip helpers — the test at `:46` "persist+restore across refresh" and `:333` "does not persist refreshRequestsByPane" are the structural donors).
- Produces: a pinned invariant later reviewers rely on: *durable `sessionRef` identity survives the persist round-trip; the live placeholder and `resumeSessionId` do not.*

- [ ] **Step 1: Write the test**

Following the round-trip harness of the `:46` test (build store state → trigger flush → re-load via the same seam that test uses):

```ts
it('round-trips a freshclaude durable identity: sessionRef survives, live placeholder and resumeSessionId are stripped', () => {
  const DURABLE = '55555555-5555-4555-8555-555555555555'
  // Arrange: a fresh-agent leaf as the shipped FreshAgentView merge effect
  // (FreshAgentView.tsx:1798-1830) leaves it:
  //   { kind: 'fresh-agent', provider: 'claude', sessionType: 'freshclaude',
  //     sessionId: 'fc-e2e-123', createRequestId: 'req-1', status: 'connected',
  //     sessionRef: { provider: 'claude', sessionId: DURABLE },
  //     resumeSessionId: DURABLE }
  // Act: flush persistence, then re-load panes state (same seam as the :46 test).
  // Assert on the restored fresh-agent leaf content:
  expect(restored.sessionRef).toEqual({ provider: 'claude', sessionId: DURABLE })
  expect(restored.sessionId).toBeUndefined() // live handle never persisted
  expect(restored.resumeSessionId).toBeUndefined() // always stripped; re-derived from sessionRef at create time
})
```

- [ ] **Step 2: Run it**

```bash
npm run test:vitest -- run test/unit/client/store/panesPersistence.test.ts
```

Expected: PASS — and that is the point. **Honest status:** this pins PRE-EXISTING behavior (`stripTransientSessionFields` keeps sanitized `sessionRef`, strips `sessionId` + `resumeSessionId` — `persistMiddleware.ts:245-268`); it is a *contract pin*, not a red-first driver. The lane's red→green driver is the wall leg G (red baselined in Task 1 Step 5; green in Task 4). If this test FAILS, `sanitizeSessionRef` or `normalizeFreshAgentContent` rejects the ref — STOP and investigate before proceeding (the canonical UUID above must pass `rejectNonCanonicalClaudeSessionRef`).

- [ ] **Step 3: Commit**

```bash
git add test/unit/client/store/panesPersistence.test.ts
git commit -m "test(client): pin freshclaude durable sessionRef persist round-trip (P0.2 lane D4)"
```

---

### Task 3: End-to-end coverage pin + stale-identity hazard guard (new e2e spec)

**Files:**
- Create: `test/e2e-browser/specs/freshclaude-identity-persistence-rust.spec.ts`
- Modify: `test/e2e-browser/playwright.config.ts` — add `/freshclaude-identity-persistence-rust\.spec\.ts$/` to `RUST_ONLY_SPECS` (~`:89`) AND to the `rust-chromium` project's `testMatch` (~`:265`)

**Interfaces:**
- Consumes: `RustServer` (`test/e2e-browser/helpers/rust-server.ts:272` — `.start()`, `.restartAbrupt()` `:344`, `.stop()`, `info.homeDir`); `TestHarness` (`helpers/test-harness.ts` — `getState()`, `getPaneLayout(tabId)`, `getActiveTabId()`, `getSentWsMessages()`, `clearSentWsMessages()`); spec-local helper bodies COPIED (per this suite's per-spec-ownership convention, wall spec file doc `:47-51`) from `restore-contract-wall-rust.spec.ts`: `selectShellIfPickerShowing` (`:95` \u2014 the boot-picker settle guard every donor creation path runs before `createFreshclaudePane`; leg G `:1179`, leg M `:2017`), `seedWallConfig` (`:131`), `bootWall` (`:156`), `flushPersistence` (`:118`), `reloadAndReconnect` (`:124`), `waitForWsReady` (`:108`), `findFreshAgentLeaf` (`:233`), `createFreshclaudePane` (`:436` — **returns `Promise<void>`; it does NOT return a tab id**), `sendFreshAgentTurn` (`:371`); IMPORTED (not copied): `openPanePicker` from `helpers/pane-picker.ts` (the copied `createFreshclaudePane` body calls it -- wall imports it at `:29`) and `fileURLToPath` from `node:url` (feeds the copied `FAKE_CLAUDE_SIDECAR_SOURCE` constant, wall `:34,:39`); `Page` type comes from `@playwright/test` (fixtures.ts exports only `test`/`expect`); plus the fake-sidecar env plumbing the wall's leg G uses (fixture `test/e2e-browser/fixtures/fake-claude-sidecar.mjs`; env keys `FRESHELL_CLAUDE_SIDECAR`, `FAKE_CLAUDE_SIDECAR_LOG`; durable UUID constant `44444444-4444-4444-8444-444444444444`; assistant reply text `'Fixture claude turn'`).
- Produces: the e2e evidence Task 5 reports; the stale-identity guard that pins the archaeology safety argument.

- [ ] **Step 1: Write the spec**

Create `test/e2e-browser/specs/freshclaude-identity-persistence-rust.spec.ts`. Skeleton (copy the named helpers verbatim from the wall spec at the cited lines; keep this spec self-contained per convention):

```ts
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
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import type { Page } from '@playwright/test'
import { test, expect } from '../helpers/fixtures.js'
import { RustServer } from '../helpers/rust-server.js'
import { TestHarness } from '../helpers/test-harness.js'
import { openPanePicker } from '../helpers/pane-picker.js'
// NOTE: `Page` comes from '@playwright/test' (fixtures.ts exports only
// `test`/`expect`, as every donor spec does). `openPanePicker` is IMPORTED,
// not copied -- the copied `createFreshclaudePane` body calls it (wall :29,
// :448). `fileURLToPath` feeds the FAKE_CLAUDE_SIDECAR_SOURCE constant below
// (wall :34, :39).

const DURABLE_CLI_SESSION_ID = '44444444-4444-4444-8444-444444444444'

// The contract-correct identity reader: sessionRef IS the durable identity;
// content.sessionId is a live handle (precedent:
// freshclaude-restart-parity-rust.spec.ts:140-148).
const durableIdentity = (leaf: any): string =>
  leaf?.content?.sessionRef?.sessionId ?? leaf?.content?.resumeSessionId ?? ''

// [COPY VERBATIM from restore-contract-wall-rust.spec.ts]
// selectShellIfPickerShowing (:95), waitForWsReady (:108),
// flushPersistence (:118), reloadAndReconnect (:124),
// seedWallConfig (:131), bootWall (:156), findFreshAgentLeaf (:233),
// createFreshclaudePane (:436), sendFreshAgentTurn (:371)
// -- createFreshclaudePane's body calls openPanePicker; that helper is
//    IMPORTED from '../helpers/pane-picker.js' (see import block above),
//    exactly as the wall does at :29 -- do NOT copy its body.
// -- also copy the FAKE_CLAUDE_SIDECAR_SOURCE constant and its
//    fileURLToPath(import.meta.url)-based derivation (wall :34, :39) plus
//    the leg-G env/setupHome wiring for the fake claude sidecar
//    (fixture path + FAKE_CLAUDE_SIDECAR_LOG), including any setupHome
//    seeding leg G performs.

test.describe('Freshclaude identity persistence (P0.2)', () => {
  test.setTimeout(180_000)

  test('durable identity survives browser reload, then SIGKILL restart resumes the SAME conversation', async ({ page, e2eServerKind }) => {
    expect(e2eServerKind).toBe('rust')
    const { server, harness } = await bootWall(page, {
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
      await createFreshclaudePane(page, harness, /* cwd per donor */)
      await sendFreshAgentTurn(page, harness, tabId!, 'first turn before reload')
      await expect(page.locator('[data-context="fresh-agent"]').last()).toContainText('Fixture claude turn', { timeout: 30_000 })

      // THE FOLD (shipped behavior: FreshAgentView.tsx:1798-1830 merge
      // effect): pane content carries the durable ref.
      await expect
        .poll(async () => durableIdentity(findFreshAgentLeaf(await harness.getPaneLayout(tabId!))), { timeout: 15_000 })
        .toBe(DURABLE_CLI_SESSION_ID)

      // RELOAD FIRST (browser-persisted identity alone, no server help).
      await flushPersistence(page)
      await reloadAndReconnect(page, harness)
      const tabIdAfterReload = await harness.getActiveTabId()
      await expect
        .poll(async () => durableIdentity(findFreshAgentLeaf(await harness.getPaneLayout(tabIdAfterReload!))), { timeout: 30_000 })
        .toBe(DURABLE_CLI_SESSION_ID)

      // THEN the SIGKILL restart.
      await harness.clearSentWsMessages()
      await server.restartAbrupt()
      await waitForWsReady(page)

      // Identity held; conversation continues end-to-end.
      await expect
        .poll(async () => durableIdentity(findFreshAgentLeaf(await harness.getPaneLayout(tabIdAfterReload!))), { timeout: 30_000 })
        .toBe(DURABLE_CLI_SESSION_ID)
      await sendFreshAgentTurn(page, harness, tabIdAfterReload!, 'second turn after restart')
      await expect(page.locator('[data-context="fresh-agent"]').last()).toContainText('Fixture claude turn', { timeout: 30_000 })
      await expect(page.locator('[data-context="fresh-agent"]').last()).toContainText('first turn before reload', { timeout: 30_000 })

      // Every create sent after the reload targeted the ORIGINAL session --
      // no identity-losing re-create.
      const sent = await harness.getSentWsMessages()
      for (const create of sent.filter((m: any) => m?.type === 'freshAgent.create') as any[]) {
        expect(create.resumeSessionId ?? create.sessionRef?.sessionId, JSON.stringify(create)).toBe(DURABLE_CLI_SESSION_ID)
      }
      const finalLeaf = findFreshAgentLeaf(await harness.getPaneLayout(tabIdAfterReload!))
      expect(finalLeaf?.content?.status).not.toBe('error')
    } finally {
      await server.stop()
    }
  })

  test('HAZARD GUARD: stale persisted sessionRef yields loud dead_session, never silent wrong-session attach or silent fresh', async ({ page, e2eServerKind }) => {
    expect(e2eServerKind).toBe('rust')
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
      await createFreshclaudePane(page, harness, /* cwd per donor */)
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
      // NEVER silent: identity not swapped, no create fired for this pane.
      expect(leaf?.content?.sessionRef?.sessionId).toBe(DURABLE_CLI_SESSION_ID)
      const sent = await harness.getSentWsMessages()
      expect(sent.filter((m: any) => m?.type === 'freshAgent.create')).toEqual([])
    } finally {
      await server.stop()
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
```

Implementation notes for the copier:
- Match `bootWall`'s option plumbing to how the wall's leg G builds its env/`setupHome` (read leg G's body, `:1168` onward, for the exact fixture wiring — including any transcript/home seeding it performs so history/reconcile see disk truth; `restartAbrupt()` re-runs `setupHome` on every boot, so seeding must be idempotent, wall `:130-153`).
- If `deleteFilesNamed` finds nothing, the fixture stores transcripts under a different name — `find <homeDir> -name '*.jsonl'` (in a debug run) to locate the artifact naming the durable UUID and adjust the basename; the assertion `deleted.length > 0` keeps this honest.
- If the dead-session UI evolves field names, the authoritative sources are `foldFreshAgentVerdict` (`src/lib/pane-reconcile.ts:310-325`: pushes `DeadSessionEntry` + `setPaneRestoreError(buildRestoreError('durable_artifact_missing'))`) and `DeadSessionEntry` (`paneTypes.ts:286-295`).
- **cwd is REQUIRED on `createFreshclaudePane`** (the donor passes it — keep it). Background: the server records the durable identity binding only for non-default create settings (`claude.rs:1156-1159` no-laundering guard); the client always sends `effort` so binding occurs regardless, but the explicit cwd keeps the test aligned with the donor and with real usage.
- **History must be asserted via RENDERED pane content** (the `toContainText` polls), never via the create-response `session.snapshot` event — the fixture's resume snapshot is empty by design; rehydration flows through the server snapshot adapter reading the transcript (`snapshot.rs:134` → `locate_transcript`).
- **Test 2 create/adjudication interplay (validated 2026-07-27, corrected by fresh-eyes review):** the fixture resurrects the transcript on ANY create-with-resume (`fake-claude-sidecar.mjs:95`), after which reconcile sees the session Present and `dead_session` is unreachable — so the test's structure guarantees no create fires before the verdict: the ws-client holds boot-reconcile-claimed creates until the verdict folds (`ws-client.ts:263-284, 687-698`, ~4s bound `ws-client.ts:79`), and a `dead_session` fold sets `restoreError`, which blocks the pane's auto-create. **If the adjudication poll times out, FIRST inspect `getSentWsMessages()` for a pre-adjudication `freshAgent.create`:** if one exists, the sequencing let a create escape the hold (verdict arrived after the ~4s bound) and the run is structurally invalid for this guard — fix the test's timing (e.g. reduce work between reload and the poll; do not add waits before the reload) rather than relaxing ANY assertion. If NO create was sent and the verdict was `fresh` instead of `dead_session`, that is the silent-data-loss hazard itself — STOP and investigate; do not weaken the assertion. There is no fallback that keeps the `dead_session` poll while tolerating a create-with-resume: those two outcomes are mutually exclusive by fixture construction.

- [ ] **Step 2: Register the spec**

In `test/e2e-browser/playwright.config.ts`: add `/freshclaude-identity-persistence-rust\.spec\.ts$/` to the `RUST_ONLY_SPECS` array (~`:89`) AND to the `rust-chromium` project's `testMatch` array (~`:265`). Both are required (wall report convention: rust-only specs are testIgnored by the match-all project and testMatched by rust-chromium).

- [ ] **Step 3: Run the new spec**

```bash
npm run test:e2e -- --project=rust-chromium specs/freshclaude-identity-persistence-rust.spec.ts
```

Expected: 2 PASSED. **Honest status: both tests pin SHIPPED behavior and are expected green on first run** — they are coverage pins, not red-first drivers; the lane's red→green evidence is the wall leg G (Task 1 Step 5 red baseline → Task 4 green). STOP-gates:
- If test 1 fails on the FOLD poll, the shipped merge effect is not engaging under the fixture (first suspects: the effect's gating at `FreshAgentView.tsx:1798-1830`; the canonical gate `persistControl.ts:87-95`) — that contradicts the corrected diagnosis, and any fix would be a production change: **STOP and report.**
- If test 1 fails on the post-reload identity poll, `sessionRef` did not round-trip — re-check against Task 2's unit pin, then **STOP and report.**
- If test 2 fails, follow the create/adjudication interplay note above; a `fresh` verdict is the silent-data-loss hazard — **STOP and investigate.**

- [ ] **Step 4: Flake check (this suite's convention for new e2e)**

```bash
npm run test:e2e -- --project=rust-chromium specs/freshclaude-identity-persistence-rust.spec.ts --repeat-each=2
```

Expected: all green twice.

- [ ] **Step 5: Commit**

```bash
git add test/e2e-browser/specs/freshclaude-identity-persistence-rust.spec.ts test/e2e-browser/playwright.config.ts
git commit -m "test(e2e): pin freshclaude identity persistence across reload+SIGKILL; stale sessionRef dies loud (P0.2 lane D4)"
```

---

### Task 4: Fix the wall's identity reader, flip the P0.2 pin, prove the FULL wall

**Files:**
- Modify: `test/e2e-browser/specs/restore-contract-wall-rust.spec.ts` — `leafDurableIdentity` (`:245-251`) and the P0.2 pin block (`:1144-1168` region)

**Interfaces:**
- Consumes: the pin inventory + full-wall baseline recorded in Task 1; the coverage pins from Tasks 2-3.
- Produces: wall leg G green as a normal expectation; full-wall run evidence for Task 5's report.

- [ ] **Step 1: Make `leafDurableIdentity` sessionRef-first**

At `restore-contract-wall-rust.spec.ts:245-251`, read the CURRENT body first, then reorder so `sessionRef` (the contract's durable identity) wins over the live handle — **only hoist `sessionRef?.sessionId` to the front; preserve the existing fallback arms verbatim, in their current relative order, and keep the function's existing return type exactly as-is** (do not add or remove terminal fallback arms):

```ts
// Durable identity reader: sessionRef IS the durable identity per the
// 2026-04-19 durable-session contract; content.sessionId is a live handle
// (for claude, the create-time fc-e2e-* placeholder forever -- see
// freshclaude-restart-parity-rust.spec.ts:140-148). sessionRef-first keeps
// this reader reload-symmetric for every fresh-agent provider.
const leafDurableIdentity = (leaf: any) =>
  leaf?.content?.sessionRef?.sessionId ?? /* ...the pre-existing arms, unchanged... */
```

- [ ] **Step 2: Flip the pin**

(Order matters: the flip is only valid AFTER Step 1's reader reorder — the pin's 2026-07-26 "server must expose the durable id as the primary handle" theory presumed today's sessionId-first reader; with sessionRef-first, the already-persisted identity is what gets compared. Validated 2026-07-27.)

In the `freshclaude: SIGKILL restore rebinds with history rehydrated and status not wedged` test (`:1144`):
1. **Delete** the entire `test.fail(...)` call (`:1165-1168`, the one whose reason begins `EXPECTED-FAIL WALL PIN (narrowed 2026-07-26 by reconcile-completion)`).
2. **Replace** the pin comment block above it (`:1149-1164`) with a HISTORY note (in-repo precedent: legs F `:1026-1044` and L `:1899-1903`):

```ts
    // HISTORY: the P0.2 pin was FLIPPED 2026-07-27 by lane D4
    // (freshclaude-identity-persistence). Investigation showed the durable
    // identity ALREADY survives reload: FreshAgentView's merge effect folds
    // the canonical claude cliSessionId into content.sessionRef +
    // resumeSessionId (FreshAgentView.tsx mergePaneContent effect), and
    // persistMiddleware round-trips sessionRef -- the 2026-04-19
    // durable-session contract's designated durable identity -- while the
    // live placeholder in content.sessionId stays unpersisted. This leg was
    // red only because leafDurableIdentity read content.sessionId (the
    // fc-e2e-* live handle, legitimately different across reloads) FIRST;
    // the reader is sessionRef-first accordingly. The stale-claim hazard
    // that motivated the original strip is pinned by
    // specs/freshclaude-identity-persistence-rust.spec.ts (dead_session,
    // never silent).
```

3. Leave the leg's assertions otherwise intact — with `leafDurableIdentity` now sessionRef-first, the pre-kill capture and post-reload comparison both read the durable UUID. If any *additional* leg-G assertion still hardcodes `content.sessionId` semantics, fix it to the sessionRef-first reader — never delete an assertion.

- [ ] **Step 3: Run leg G alone — GREEN (STOP-gate)**

```bash
npm run test:e2e -- --project=rust-chromium specs/restore-contract-wall-rust.spec.ts -g 'freshclaude: SIGKILL restore rebinds'
```

Expected: 1 passed (as a NORMAL expectation now). This plus Task 1 Step 5's saved output is the wall leg's red→green proof. **If leg G still fails after the reader fix, the corrected diagnosis is incomplete** — capture the failure output (which assertion, what identity values were compared) and **STOP and report**; any remedy would be a production change, out of this lane's fence.

- [ ] **Step 4: Run the FULL wall — prove no other pin flips unexpectedly**

```bash
npm run test:e2e -- --project=rust-chromium specs/restore-contract-wall-rust.spec.ts
```

Decision protocol against the Task 1 pin inventory + full-wall baseline:
- All green with remaining pins failing-as-expected → done.
- **Leg I "THE RULER" unexpectedly passes** (Playwright hard-fails an unexpected pass): this is the documented coupling — its pin text names the P0.2 gap. Flip its pin too: delete its `test.fail(...)` (`:1380` region) and add a HISTORY note crediting lane D4 for closing the final gap. Re-run the full wall.
- **Legs J/K or any other pin unexpectedly passes**: NOT ours (D3/other-lane territory) — STOP, capture the output, and report it in the final summary instead of flipping.
- **Leg M (busy-restart) or legs E/F regress**: our reader change interacting badly — debug ours (first suspect: the sessionRef-first reader against each leg's pre/post capture points). Do not paper over with pin edits.

Expected end state: full wall green (any remaining `test.fail` pins failing as expected; per the campaign, if this was the last gap, ZERO pins remain and every test passes plainly).

- [ ] **Step 5: Run the sibling parity spec (adjacent coverage)**

```bash
npm run test:e2e -- --project=rust-chromium specs/freshclaude-restart-parity-rust.spec.ts
```

Expected: PASS (untouched by this lane; it already reads sessionRef-first).

- [ ] **Step 6: Commit**

```bash
git add test/e2e-browser/specs/restore-contract-wall-rust.spec.ts
git commit -m "test(e2e): flip P0.2 wall pin -- durable identity reader is sessionRef-first (lane D4)"
```

(If Step 4 flipped the ruler pin as well, include that in this commit and name it in the message body.)

---

### Task 5: Full gates, push, STOP before PR

**Files:**
- No new source changes (fix-ups only if gates fail).

**Interfaces:**
- Consumes: everything above.
- Produces: pushed branch + final report. **NO PR** (policy: not approved — stop before `gh pr create`).

- [ ] **Step 1: Lint clean**

```bash
npm run lint
```

Expected: clean. Fix any findings in OUR files only; re-run.

- [ ] **Step 2: Typecheck + full coordinated suite**

```bash
npm run test:status
FRESHELL_TEST_SUMMARY="lane D4 freshclaude identity persistence" env -u FRESHELL_BIND_HOST npm run check
```

Expected: PASS. Wait on the coordinator gate if held (3 sibling lanes). This lane changed no production code, so unit fallout should be zero; any failure is either pre-existing (check against Task 1 Step 4's baseline) or caused by our test files. Fix root causes in our fenced files; if a failure demands a change OUTSIDE the fence, STOP and report.

- [ ] **Step 3: Final e2e sweep**

```bash
npm run test:e2e -- --project=rust-chromium specs/restore-contract-wall-rust.spec.ts specs/freshclaude-identity-persistence-rust.spec.ts specs/freshclaude-restart-parity-rust.spec.ts
```

Expected: all green. Save the full-wall output — it is the headline evidence.

- [ ] **Step 4: Push the branch — and STOP**

```bash
git log --oneline origin/main..HEAD
git push -u origin freshclaude-identity-persistence
```

**Do NOT run `gh pr create`.** PR is not approved for this lane.

- [ ] **Step 5: Final report (deliverable text, not a commit)**

Report, in this order:
1. Branch: `freshclaude-identity-persistence` (pushed), base commit it sits on.
2. **The archaeology finding (corrected):** the `content.sessionId` strip was introduced by `976d3d48` implementing the 2026-04-19 exact-durable-session contract (live handles are never durable restore targets; hazard: stale identity → wrong-session attach). The contract-compliant durable-identity fold ALREADY SHIPPED before this lane: `FreshAgentView`'s merge effect writes the canonical claude UUID into `content.sessionRef` (+ `resumeSessionId`), and `persistMiddleware` round-trips `sessionRef`. **P0.2 was red because of a test-reader artifact** (`leafDurableIdentity` compared the ephemeral `fc-e2e-*` placeholder across reloads), not because identity was lost. The safety argument (§4.2 authority chain: persisted client identity is a proposal; stale claims get `corrected`/loud `dead_session`) is now pinned by the new hazard-guard e2e.
3. **Red→green proof, honestly framed:** Task 1 Step 5 output (leg G failing-as-expected under the pin) → Task 4 Step 3 output (leg G green, pin deleted) — this reader fix is the lane's sole red→green; Tasks 2-3's tests are green-on-first-run coverage pins of shipped behavior (state this explicitly). Include **the full-wall-green run from Step 3 above**, noting every remaining pin's status (expected: zero pins remaining, or explicitly list any still-pinned legs owned by other lanes).
4. Any observations for sibling lanes (e.g. ruler pin flipped here, unexpected passes left un-flipped per protocol, or any fold-fragility watch note from the design overview that showed up during verification).

---

## Self-Review (re-performed 2026-07-27 after the fresh-eyes re-grounding)

**1. Spec coverage.**
- Archaeology first, rationale + why-safe (§4.2 authority chain answer shape) → plan's Archaeology section + report item 2 (Task 5), both corrected to credit the already-shipped fold. ✔
- Durable identity persists via `sessionRef`; respawn/recovery carries it forward (`triggerRecovery` prefers the durable id via `getCanonicalPaneResumeSessionId`, which reads `pane.sessionRef` first); placeholder kept for genuinely-new panes — all SHIPPED behavior, now pinned by Task 2 (unit round-trip) and Task 3 test 1 (end-to-end journey). ✔
- Flip the pin with a commit note naming the lane; run FULL wall to prove no unexpected flips → Task 4 (+ decision protocol for the coupled ruler pin, against Task 1's recorded baseline). ✔
- Stale-sessionRef guard test (loud dead_session, never silent wrong-session attach) → Task 3 test 2, with the create/adjudication mutual-exclusion analysis replacing the earlier internally-inconsistent fallback. ✔
- e2e with own RustServer on ephemeral ports, never 3001/3002; reload (not just restart) in the journey; SIGKILL restart; full wall green → Tasks 1/3/4; reload happens BEFORE the SIGKILL in test 1, and the wall leg G covers the flush→SIGKILL→reload ordering. ✔
- Repo rules (worktree, baseline green first, coordinator gate, npm ci/tsx, lint, never restart live server, no broad kills, PR policy stop) → Global Constraints + Tasks 1 and 5. ✔

**1b. No silent deferrals.** No production behavior is claimed or deferred: the plan's deliverable is a reader correction + pins, and every claim about shipped behavior is verified by an executable gate (Task 2 unit pin; Task 3 e2e polls; Task 4 leg G green) with STOP-gates where a failure would falsify the diagnosis. The known fold fragilities (zoom-unmount, rider-gated sessionRef write, orphaned `buildFreshAgentPersistedIdentityUpdate`) are explicitly documented as out-of-scope watch items with a STOP-and-report path, not silently assumed away.

**2. Placeholder scan.** The deliberate copy-by-reference instructions remain (spec-local e2e helpers "copy verbatim from restore-contract-wall-rust.spec.ts:<line>"): these point at exact existing code by file:line per the suite's own per-spec-ownership convention — the engineer copies mechanically rather than inventing. The Task 4 reader sketch intentionally elides the pre-existing fallback arms and instructs preserving them verbatim (read-first). No TBD/TODO/"handle edge cases" items.

**3. Type consistency.** The e2e reader `durableIdentity` matches the parity spec's proven expression (`sessionRef.sessionId ?? resumeSessionId`). Task 4's reader change preserves the wall function's existing return type by instruction. `sessionRef` shape `{ provider, sessionId }` matches `SessionLocator` usage throughout. `tabId` is sourced from `harness.getActiveTabId()` in both e2e tests (donor: leg G), never from `createFreshclaudePane` (which returns `Promise<void>`). Consistent throughout.

## Re-grounding addendum (2026-07-27 fresh-eyes review, iteration 1)

An independent cross-model review falsified the plan's original core diagnosis and four executable-plan details. The plan above is the corrected version. Record of what changed and the evidence:

- **Falsified premise:** "the durable UUID is written ONLY to the unpersisted freshAgentSlice; the pane persists with no identity." **Reality (verified in this worktree):** `FreshAgentView.tsx:1798-1830` reactively folds the canonical `cliSessionId` into pane `sessionRef` + `resumeSessionId` via `mergePaneContent` (canonical-gated by `getCanonicalDurableSessionId`, `persistControl.ts:87-95`); `stripTransientSessionFields` (`persistMiddleware.ts:245-268`) persists `sessionRef` (strips `sessionId` + `resumeSessionId`). Corroborated in-repo by the parity spec's reader comment (`freshclaude-restart-parity-rust.spec.ts:138-150`) and the wall's P0.1 ruler pin narration (`restore-contract-wall-rust.spec.ts:1367-1370`), which observes the post-reload pane carrying the canonical UUID.
- **Consequence:** the original Tasks 2-3 (a new `adoptFreshAgentDurableIdentity` panesSlice reducer + ws-transport wiring + created-time catch-up) would have added a second, competing fold path duplicating shipped behavior, and their red-first claims were unachievable. **Removed entirely**; the plan is now test-only (see the scope fence). This also removed the two structural defects the review found inside those tasks (a unit-test step targeting a dispatch-capture harness that does not exist in either ws test file, and a commit step that omitted Step 3b's production files).
- **Fixed executable defects:** both e2e tests now source `tabId` from `harness.getActiveTabId()` (the review caught `const tabId = await createFreshclaudePane(...)` assigning from a `Promise<void>`); test 2's sequencing goes straight from `restartAbrupt()` to `reloadAndReconnect` so the old page can never fire a transcript-resurrecting create-with-resume, and the earlier internally-inconsistent "relaxed zero-creates fallback that keeps the dead_session poll" was replaced with the mutual-exclusion analysis (any pre-adjudication create-with-resume makes `dead_session` unreachable by fixture construction — so the contingency is diagnose-and-fix-sequencing, never relax).
- **Honesty corrections:** Task 1 Step 1 no longer claims the branch has no local commits (it carries the plan-doc commits; the step rebases). All red/green language now states plainly that the new tests are coverage pins of shipped behavior and the lane's only red→green is the wall leg G reader fix. Task 5's report items were rewritten so the delivered archaeology/attribution narrative matches reality.
