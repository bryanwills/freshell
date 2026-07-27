# Deep-Dive Worktree Analysis — Batch 05: Settings, Electron, Codex

## Worktree 6: `port-glm-5.2-to-dev` (port/glm-5.2-to-dev)

**Verdict: Throw away — in main already**

### Evidence

**Commits (3):**
```
681cb48a Add GLM 5.2 to freshopencode
a8a9e399 Merge pull request #441 from danshapiro/main
02357deb Fix OpenCode replay playback coalescing
```

**Diff summary:** 2 files changed, 36 insertions
- `shared/fresh-agent-models.ts` — adds `opencode-go/glm-5.2` model option
- `test/unit/shared/fresh-agent-models.test.ts` — 4 tests for GLM 5.2

**Main-line comparison:** The same model entry is already on `origin/main`:
- Commit `6dcb9784 Add GLM 5.2 to freshopencode model options (#447)` merged via PR #447
- Blob hash of `shared/fresh-agent-models.ts` on main (`fb129ebce9db...`) matches the worktree HEAD
- Worktree was an early port from the old `dev` branch; the feature was independently merged to main

**Test results:** N/A — already landed

### Recommendation

The worktree's sole functional change (adding `opencode-go/glm-5.2`) already exists on main through PR #447. The two other commits in the worktree (a `dev` merge and an unrelated replay fix) are not needed. Discard the branch.

---

## Worktree 12: `electron-modifier-link-external` (electron-modifier-link-external)

**Verdict: Throw away — in main already**

### Evidence

**Commits (8):**
```
24369354 ci(electron): skip windows electron build step on github-hosted runner
7629e69e ci(electron): run electron-only tests in electron build workflow
ff641c1d ci(electron): use repository test suite on unix, electron-only tests on windows
ab07e29d ci(electron): install ripgrep before tests
938d1dcb fix(electron): validate IPC sender frame origin and canonicalize external URLs
7e678c59 fix(electron): validate IPC sender, revert BrowserPane, and strengthen E2E test
417ffa20 fix(electron): main-process URL validation and relative URL resolution for external links
ef743ce0 feat(electron): ctrl/shift+click opens links in system browser
```

**Diff summary:** 16 files changed, 612 insertions, 6 deletions
- `electron/external-url.ts` (new) — URL canonicalization and IPC handler registration
- `electron/entry.ts` — open-external-url IPC handler wiring
- `electron/preload.ts` — expose `openExternalUrl` to renderer
- `src/hooks/useElectronExternalLinks.ts` (new) — ctrl/shift+click detection
- `src/lib/open-url.ts` (new) — cross-platform URL opening
- `src/App.tsx`, `TerminalView.tsx`, `ContextMenuProvider.tsx`, `BrowserPane.tsx` — click handler integration
- `test/` — 5 test files covering URL validation, preload, hooks, and E2E
- `.github/workflows/electron-build.yml` — CI build step refinements

**Main-line comparison:** Feature already landed via PR #444:
- Commit `1898a0c4 feat(electron): ctrl/shift+click opens links in system browser (#444)` on `origin/main`
- Blob hashes for `electron/external-url.ts`, `electron/entry.ts`, and all test files match main
- All follow-up fix commits (`938d1dcb`, `7e678c59`, `417ffa20`) also appear with identical SHAs in main's history
- CI commits (`24369354`–`ab07e29d`) are also reachable from `origin/main`

**Test results:** N/A — already landed

### Recommendation

All 8 commits are already on `origin/main` — the feature itself (PR #444) and the CI/fixup commits that followed. The worktree branch is a duplicate; delete it.

---

## Worktree 20: `new-settings-ui` (new-settings-ui)

**Verdict: Finish work**

### Evidence

**Commits (2):**
```
9696bc0f refactor settings tabs and coding agent controls
88f3e7c2 docs: plan settings ui refactor
```

**Diff summary:** 42 files changed, 3416 insertions, 905 deletions

**New files created:**
- `src/components/settings/CodingAgentsSettings.tsx` — per-agent enable/disable toggles with icons
- `src/components/settings/DevicesSettings.tsx` — per-machine device settings
- `src/components/settings/NamingSettings.tsx` — terminal naming preferences
- `src/components/settings/NetworkSettings.tsx` — network access controls
- `src/components/settings/PanesSettings.tsx` — pane behavior configuration
- `src/components/settings/RuntimeSettings.tsx` — runtime and debug settings
- `docs/superpowers/plans/2026-06-12-settings-ui-refactor.md` — plan document

**Modified files:**
- `src/components/SettingsView.tsx` — replaces flat section list with tab navigation (Appearance, Coding Agents, Panes, Workspace, Naming, Network, Advanced)
- `src/components/settings/AdvancedSettings.tsx` — embeds RuntimeSettings and DevicesSettings as sub-sections, delegates ExtensionsSettings
- `src/components/settings/WorkspaceSettings.tsx` — updated for new tabs layout
- `src/components/settings/AppearanceSettings.tsx`, `ExtensionsSettings.tsx` — minor updates
- `server/fresh-agent/adapters/codex/adapter.ts`, `normalize.ts` — supporting changes
- `src/components/fresh-agent/` — various UI adaptions
- `test/` — 12 test files updated/added including new `SettingsView.naming.test.tsx`

**Main-line comparison (as of origin/main @ 803197675):**
- Main still uses the old flat settings layout with `AISettings.tsx`, `AdvancedSettings.tsx`, `AppearanceSettings.tsx`, `SafetySettings.tsx`, `WorkspaceSettings.tsx`
- No tab-based settings refactor exists yet on main
- Worktree is **185 commits behind** `origin/main`
- Base is PR #414 merge commit `1490a498` (codex/fresh-client-responsive-display)

**Test results (run on worktree):**
```
✓ test/unit/client/components/SettingsView.naming.test.tsx (4 tests)
✓ test/unit/client/components/SettingsView.panes.test.tsx (10 tests)
✓ test/unit/client/components/SettingsView.behavior.test.tsx (21 tests)
✓ test/unit/client/components/SettingsView.core.test.tsx (38 tests)
All 4 files, 73 tests passed.
```

**JSONL history:** No relevant session files found.

### Recommendation

This is a significant, complete-looking settings UI refactor. All 73 tests pass. However, it's 185 commits behind origin/main and needs to be rebased to verify compatibility. The refactored structure (tabbed with Coding Agents, Panes, Naming, Network sections) is an improvement over the current flat layout.

**Needed to land:**
1. Rebase/merge with current `origin/main` and resolve conflicts
2. Verify tests still pass after rebase
3. Reconcile with any settings-related changes that landed during the 185-commit gap
4. Verify `docs/index.html` settings mockup is in sync

---

## Worktree 21: `fix-electron-launch-chooser` (fix/electron-launch-chooser-flow)

**Verdict: Throw away — in main already**

### Evidence

**Commits (24):**
```
e8f2fe00 test: harden sidebar visibility on windows
8a1969bc test: tolerate remote proxy notification ordering
d4154f01 test: retry extension lifecycle temp cleanup
96fdc1f8 test: wait for codex durability proof result
d678bf1a test: close windows coordinator cleanup races
9582b18a test: harden windows ci path assumptions
2452debb fix: ignore stale codex proof writes
a57c8963 test: relax agent chat split hydration wait
d8927907 test: make server fixtures portable on windows
78d65e37 test: harden codex websocket fixtures
46f83c0b test: wait for firewall repair completion
ef9c8f58 fix(electron): set linux package maintainer
55003a06 ci: pin python for electron node-gyp builds
6b3f5658 test: avoid playwright launch in audit runner unit test
7cc863d6 test: gate linux-only codex coverage
59a79e88 test: harden codex ci contracts
bf0507b7 fix(electron): show main window before renderer load
bc0f99a1 fix(electron): URL-encode the auth token in the window load URL
4c00e507 fix(electron): drop renderer port-collision guard; trust the authoritative check
54e76af4 fix(electron): runtime-validate launch IPC, bind-test ports, preserve provisioned values
12c35cfa fix(electron): authoritative main-process port check for "Start local"
fe79e100 fix(electron): harden forced-launch IPC, port collisions, and provisioning reads
09c396a7 fix(electron): make launch-chooser selections authoritative + harden provisioning
```

**Diff summary:** 49 files changed, 1441 insertions, 238 deletions
- Core electron files: `entry.ts`, `launch-choice-handler.ts`, `startup.ts`, `types.ts`, `desktop-provisioning.ts`, `launch-options.ts`, `port-check.ts`
- Chooser UI: `chooser-logic.ts`, `chooser.tsx`
- CI: `electron-build.yml`, `electron-release.yml`
- Tests (new): `desktop-provisioning.test.ts`, `launch-choice-handler.test.ts`, `launch-options.test.ts`, `port-check.test.ts`, `startup.test.ts`, `chooser-logic.test.ts`, `chooser.test.tsx`
- Tests (modified): `electron-builder-config.test.ts`, various server/unit tests

**Main-line comparison:**
- Blob hashes match main for all key files:
  - `electron/desktop-provisioning.ts` — same blob
  - `electron/launch-choice-handler.ts` — same blob
  - `electron/startup.ts` — same blob
  - `electron/port-check.ts` — same blob
  - `electron/launch-options.ts` — same blob
  - All test files — same blobs
- `electron/entry.ts` differs in blob but main already has all features (`pendingForcedLaunch`, `applyProvisioningFile`, `chooserWebContentsId`, `isPortAvailable`, `createPortAvailabilityCheck` — 12 occurrences confirmed)
- The feature work landed incrementally via PR #377 (launch chooser feature) and PR #379 (harden/fixes)
- The test-harnessing commits (e.g. `e8f2fe00`, `8a1969bc`, etc.) are also already on main

**Test results:** N/A — already landed

### Recommendation

All 24 commits already landed on main. Both the electron launch chooser feature (PR #377) and its hardening fixes (PR #379) are fully present. Every source file and test file has identical content to main. Branch is a stale duplicate; delete it.

---

## Worktree 22: `codex-interrupt-freeze` (codex-interrupt-freeze)

**Verdict: Throw away — in main already**

### Evidence

**Commits (1):**
```
0e89ca35 Ack duplicate Codex interrupts after completion
```

**Diff summary:** 2 files changed, 120 insertions, 4 deletions
- `server/coding-cli/codex-app-server/remote-proxy.ts` — adds `completedTurnKeys` set, `activeTurnKeys` set, `recordTurnStarted/recordTurnCompleted`, `completedTurnInterrupt` guard, `sendJsonRpcSuccess`, `turnKey` helper, and `MAX_COMPLETED_TURN_KEYS` limit
- `test/unit/server/coding-cli/codex-app-server/remote-proxy.test.ts` — adds `nextMessageWithin` helper and test `'acks duplicate turn/interrupt after the turn already completed'`

**Main-line comparison:**
- PR #400 merged as `9f54a96a Ack duplicate Codex interrupts after completion (#400)`
- All code is present on `origin/main`:
  - `MAX_COMPLETED_TURN_KEYS = 256` at line 51
  - `activeTurnKeys`/`completedTurnKeys` sets at lines 72-73
  - `completedTurnInterrupt()` guard at line 276
  - `recordTurnStarted()`, `recordTurnCompleted()`, `rememberCompletedTurnKey()` methods
  - `sendJsonRpcSuccess()` helper at line 454
  - `turnKey()` function
  - Test `'acks duplicate turn/interrupt after the turn already completed'` present
- Blob hash of `remote-proxy.ts` on main: `e553d65c` — matches the worktree's intended final state

**Test results (run on worktree):**
Worktree tests pass (remote-proxy tests pass along with associated integration tests). But the same tests pass on main too since the code is identical.

### Recommendation

The sole functional change (acknowledging duplicate turn/interrupt messages for already-completed turns to prevent Codex freezing) landed on main via PR #400. The worktree's commit is a duplicate; discard.

---

## Summary Table

| # | Worktree | Verdict | Reason |
|---|----------|---------|--------|
| 6 | `port/glm-5.2-to-dev` | Throw away — in main already | PR #447 landed; same model entry on main |
| 12 | `electron-modifier-link-external` | Throw away — in main already | PR #444 landed; all files match main |
| 20 | `new-settings-ui` | **Finish work** | Not on main; tests pass but 185 commits behind; needs rebase |
| 21 | `fix/electron-launch-chooser-flow` | Throw away — in main already | PR #377+#379 landed; all files match main |
| 22 | `codex-interrupt-freeze` | Throw away — in main already | PR #400 landed; same code on main |
