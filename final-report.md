# Worktree Audit: Final Aggregation Report

**Date:** 2026-06-21
**Scope:** Past 4 weeks (May 24 – Jun 21, 2026)
**Repository:** Freshell (`origin/main` SHA `80319767`)
**Total worktrees audited:** 32 (from initial inventory of 91)

---

## Executive Summary

| Metric | Count |
|--------|-------|
| Total worktrees evaluated | 32 |
| **Ready for landing** | **4** |
| **Finish work** (needs rebase/integration) | **2** |
| **Already in main** (throw away) | **19** |
| **Skipped** (plan-only or trivial) | **7** |

The audit covered all worktrees with novel content (non-ancestor of `origin/main`). Each was assessed via automated criteria ([baseline-criteria.md](baseline-criteria.md)), a [first-pass table](first-pass-table.md) review, and — for the 25 with meaningful code changes — an agent-driven deep-dive analysis producing detailed reports in [`deep-dive/`](deep-dive/).

---

## Ready for Landing

These 4 worktrees contain well-tested, verified novel code that is NOT on `origin/main`. They should be pushed, submitted as PRs targeting `main`, and landed.

### 1. `rollback-opencode-sidecars` — Jun 21

| | |
|---|---|
| **Branch** | `rollback/opencode-sidecars` |
| **SHA** | `58714d76` |
| **Ahead/Behind** | 7 ahead, 0 behind |

**Description:** Reverts the multi-cwd-sidecar approach (PR #450) and replaces it with a single shared `opencode serve` process using directory-query routing (`?directory=/path`), global event stream consumption, and cwd validation. Removes ~130 lines of multi-sidecar complexity.

**Deep analysis** ([full report](deep-dive/01-opencode-freshcodex.md#worktree-3-rollback-opencode-sidecars-branch-rollbackopencode-sidecars)):
- 7 commits: revert old approach → plan → implement directory routing → global events → cwd validation → tests → timeout reaping
- 9 files changed, +1074/−1594 (net −520 lines — simplification)
- 205/205 unit tests pass; `npm run typecheck` passes with zero errors
- Architecturally sound: single `running`/`startPromise` instead of maps; no `sessionCwdById`/`cwdByKey`
- Real-provider smoke test cannot run here (needs `opencode` binary), but unit coverage is thorough

**Why ready:** Complete, tested, type-safe, net simplification. Only branch in the repo with 0 behind main. The team consciously pivoted from the cwd-sidecar pattern, and this branch is the culmination.

---

### 2. `fresh-agent-thinking-muted-color` — Jun 18

| | |
|---|---|
| **Branch** | `fix/fresh-agent-thinking-muted-color` |
| **SHA** | `a122fba1` |
| **Ahead/Behind** | 1 ahead, 29 behind |

**Description:** Fixes a visual bug where thinking text in serif transcript style renders in the answer's primary color instead of the muted color. Adds CSS custom property overrides for Tailwind prose typography and an e2e test.

**Deep analysis** ([full report](deep-dive/03-fresh-agent-ui.md#worktree-8-fresh-agent-thinking-muted-color-jun-18)):
- 1 commit: `fix(fresh-agent): render thinking text in muted color across all styles`
- 2 files changed, +168/−0: `src/index.css` and one e2e test
- Not on main: the `--tw-prose-*` override block is absent from `origin/main`'s `src/index.css`; the e2e test for thinking text color is absent
- `git merge origin/main` completes without conflicts
- The `.fresh-agent-thinking-body` container exists on main with `color: var(--fresh-agent-muted-text)`, but Tailwind's prose plugin overrides paragraph/heading colors — the fix adds the necessary `--tw-prose-*` overrides

**Why ready:** Small, focused, well-tested, no merge conflicts, addresses a real visual regression. Push and land.

---

### 3. `opencode-think-normalization` — Jun 17

| | |
|---|---|
| **Branch** | `fix/opencode-think-normalization` |
| **SHA** | `53a34024` |
| **Ahead/Behind** | 1 ahead, 40 behind |

**Description:** Preserves think tag content (`<think>...</think>`) as `thinking` transcript items instead of silently stripping it. Introduces `NormalizedTextSegment` type, segmented think handling, and changes `normalizeOpencodeTurn` return type to `FreshAgentTranscriptItem[]`.

**Deep analysis** ([full report](deep-dive/02-opencode-think-playback.md#1-opencode-think-normalization-fixopencode-think-normalization)):
- 1 commit, 2 files changed, +258/−24
- Not on main: `normalizeBalancedThinkTags`, `itemsFromAssistantTextPart`, `textSummaryFromItems` all absent from main
- Main still has the old `stripThinkTags` which loses reasoning traces
- 18 tests pass (vs 14 on main — 4 new + refactored)
- Handles balanced, malformed, unterminated, and dangling think tags

**Why ready:** Clean functional improvement to transcript quality. No relationship to other worktrees. All tests pass. Should land immediately.

---

### 4. `opencode-refresh-restore-white-page` — Jun 17

| | |
|---|---|
| **Branch** | `fix/opencode-refresh-restore-white-page` |
| **SHA** | `b87f63c2` |
| **Ahead/Behind** | 9 ahead, 40 behind |

**Description:** Fixes a bug where restoring a hidden OpenCode tab after browser refresh results in a blank/white page. Removes `!hiddenRef.current` from `isUnrecoverableOpenCodeViewportHydrate` and adds a `shouldWaitForProviderBehavior` guard to prevent attach race.

**Deep analysis** ([full report](deep-dive/02-opencode-think-playback.md#2-opencode-refresh-restore-white-page-fixopencode-refresh-restore-white-page)):
- 9 commits (4 code + 5 plan docs), 4 files changed, +734/−17
- Not on main: `!hiddenRef.current` is still present at line 3373, blocking the fix
- Not on main: the `shouldWaitForProviderBehavior` guard is absent from the "Create or attach" useEffect
- 126 tests pass (main: 124 — 2 additional tests)
- Bug is reproducible: hidden pane after browser refresh with gap in replay ring → white page on reveal
- E2e test validates real terminal buffer content after repair

**Why ready:** Fixes a real white-page bug that impacts OpenCode users. Thorough unit + e2e test coverage. Verified the bug still exists on `origin/main`. No overlap with other worktrees.

---

## Finish Work

These 2 worktrees contain significant, well-tested work that is not on `origin/main` but needs additional integration effort before landing.

### 5. `freshagent-tool-attribution` — Jun 13

| | |
|---|---|
| **Branch** | `freshagent-tool-attribution` |
| **SHA** | `cce6d98e` |
| **Ahead/Behind** | 2 ahead, 149 behind |

**Description:** Filters Claude skill payload user turns (e.g., fresheyes instructions injected as user-role context) and fixes tool-result attribution so tool results from skills appear as assistant activity rather than separate user turns.

**Deep analysis** ([full report](deep-dive/03-fresh-agent-ui.md#worktree-18-freshagent-tool-attribution-jun-13)):
- 2 commits, 6 files changed, +323/−30
- Nothing from this branch is on main: `isSyntheticToolResultTurn`, `appendTurnItems`, `isSyntheticUserTimelineItem` are all absent
- Real problem: Claude Code injects skill instructions as user-role messages → synthetic "You" turns + misattributed tool results
- Test coverage is excellent (6 test cases)
- **Needs rebase:** Conflicts with transcript contract PR (#452) which rewrote `normalizeClaudeTurn` and `FreshAgentTranscript.tsx`

**What needs to happen:**
1. Rebase onto current `origin/main`
2. Verify the problem still manifests on main (run a Claude session that invokes a skill)
3. Reconcile `normalizeClaudeTurn` return type change (`FreshAgentNormalizedTurn | null`) with contract PR behavior
4. Verify tests pass after rebase

---

### 6. `new-settings-ui` — Jun 12

| | |
|---|---|
| **Branch** | `new-settings-ui` |
| **SHA** | `9696bc0f` |
| **Ahead/Behind** | 2 ahead, 185 behind |

**Description:** Major settings UI refactor that replaces the flat section list with tabbed navigation: Appearance, Coding Agents, Panes, Workspace, Naming, Network, Advanced. Adds 6 new settings component files.

**Deep analysis** ([full report](deep-dive/05-settings-electron-codex.md#worktree-20-new-settings-ui-new-settings-ui)):
- 2 commits (1 plan + 1 code), 42 files changed, +3416/−905
- Not on main: main still uses the old flat settings layout
- 73 tests pass across 4 test files
- **Needs rebase:** 185 commits behind `origin/main`, based on PR #414 merge commit

**What needs to happen:**
1. Rebase onto current `origin/main` and resolve conflicts
2. Verify all tests still pass after rebase
3. Reconcile with any settings-related changes that landed during the 185-commit gap
4. Verify `docs/index.html` settings mockup is in sync

---

## Throw Away — In Main Already

These 19 worktrees have their code fully present on `origin/main`. No novel work remains. Worktrees and branches should be deleted.

### Sorted newest to oldest:

**1. `repro-freshopencode-playwright` — Jun 19**
- Branch: `debug/freshopencode-playwright-repro-dev`
- PR #454 merged `debug/freshopencode-playwright-repro` with identical tree (`da3d834d18f9a7baee1cc74fa8c9908a7c7707cc`). The `-dev` variant and merged `-repro` branch produce byte-identical content. `git diff 80319767 2c5432c6 --stat` produces zero output.
- [Full evidence](deep-dive/01-opencode-freshcodex.md#worktree-1-repro-freshopencode-playwright-branch-debugfreshopencode-playwright-repro-dev)

**2. `debug-freshcodex-cwd` — Jun 19**
- Branch: `debug-freshcodex-cwd`
- PR #450 squash-merged as commit `45efc524`. Tree hash identical (`4ebcc3ac49353d4832c3bc85d0f873b63ff188b2`). `git diff 13cff1f3 45efc524` produces zero output. Squash commit message lists every branch commit.
- [Full evidence](deep-dive/01-opencode-freshcodex.md#worktree-2-debug-freshcodex-cwd-branch-debug-freshcodex-cwd)

**3. `fix-mobile-longpress-menu` — Jun 19**
- Branch: `fix-mobile-longpress-menu`
- PR #451, commit `9383c2fb`. Identical diff: `ContextMenuProvider.tsx` + `ContextMenu.longpress.test.tsx`. `git diff 9383c2fb..75256b25 --stat` produced zero output — byte-identical.
- [Full evidence](deep-dive/03-fresh-agent-ui.md#worktree-4-fix-mobile-longpress-menu-jun-19)

**4. `port-glm-5.2-to-dev` — Jun 18**
- Branch: `port/glm-5.2-to-dev`
- PR #447 landed `6dcb9784` ("Add GLM 5.2 to freshopencode model options"). Same model entry `opencode-go/glm-5.2`. Blob hash of `shared/fresh-agent-models.ts` on main matches worktree HEAD.
- [Full evidence](deep-dive/05-settings-electron-codex.md#worktree-6-port-glm-52-to-dev-portglm-52-to-dev)

**5. `investigate-bouncer` — Jun 18**
- Branch: `investigate-bouncer`
- Plan document only (905-line plan for freshopencode bouncer status fix). Content already on main.

**6. `electron-modifier-link-external` — Jun 17**
- Branch: `electron-modifier-link-external`
- PR #444 landed as `1898a0c4`. All 8 worktree commits are reachable from `origin/main`. Blob hashes for `electron/external-url.ts`, `electron/entry.ts`, and all test files match main. CI commits (`24369354`–`ab07e29d`) also present.
- [Full evidence](deep-dive/05-settings-electron-codex.md#worktree-12-electron-modifier-link-external-electron-modifier-link-external)

**7. `fix-freshagent-user-message-quotes` — Jun 15**
- Branch: `fix/freshagent-user-message-quotes`
- Code present verbatim on main: `stripOpencodeRunArgumentQuoting()` at `normalize.ts:95` with identical JSDoc. All 4 tests present verbatim at lines 160–202 of the test file on main.
- [Full evidence](deep-dive/03-fresh-agent-ui.md#worktree-15-fix-freshagent-user-message-quotes-jun-15)

**8. `opencode-playback-dev-pr` — Jun 15**
- Branch: `test/opencode-playback-coalescing-dev`
- Main commit `c50dd6f8` ("Fix OpenCode replay playback coalescing"). Squashed version of the same fix. `disableWriteCoalescing` absent from `TerminalView.tsx` on both main and worktree.
- [Full evidence](deep-dive/02-opencode-think-playback.md#3-opencode-playback-dev-pr-testopencode-playback-coalescing-dev)

**9. `opencode-playback-coalescing` — Jun 15**
- Branch: `test/opencode-playback-coalescing`
- Main commit `c50dd6f8`. This is the source branch (granular commits + plan docs). Same fix, already landed.
- [Full evidence](deep-dive/02-opencode-think-playback.md#4-opencode-playback-coalescing-testopencode-playback-coalescing)

**10. `freshagent-serif-full-style` — Jun 13**
- Branch: `freshagent-transcript-no-auto-collapse`
- PR #452 (transcript contract) fixed the auto-collapse regression through a different implementation. Key behaviors (no auto-collapse, snapshot merge for in-flight turns, all 10 turns visible) are all present on main. `getSnapshotIdentity()`, `isSnapshotInFlight()`, `mergeSnapshotForDisplay()` all present.
- [Full evidence](deep-dive/03-fresh-agent-ui.md#worktree-19-freshagent-serif-full-style-jun-13)

**11. `proof-terminal-catchup-architecture` — Jun 8**
- Branch: `proof-terminal-catchup-architecture`
- Main commit `6f74e6f7`. Same research dossier content (14 files, +2920). Different author (Codex on main vs danshapiro on worktree). `git diff ad9fbeac66 6f74e6f799` shows zero diff in shared files — only a plan doc added to main after the dossier.
- [Full evidence](deep-dive/04-terminal-catchup.md#23-proof-terminal-catchup-architecture)

**12. `fix-terminal-catchup` — Jun 7**
- Branch: `fix-terminal-catchup`
- PR #396 (`463eff2f` "Speed up terminal replay catch-up"). Write-queue replay coalescing within 32ms budget, mode-aware queuing. Identical stats: 4 files, +225/−14.
- [Full evidence](deep-dive/04-terminal-catchup.md#24-fix-terminal-catchup)

**13. `fix-replay-server-batching` — Jun 7**
- Branch: `fix-replay-server-batching`
- PR #397 (`6faf470e` "Coalesce terminal replay batches server-side"). Server-side batching in `replay-ring.ts`, removal of client-side `replayBudgetMs`. 6 files, +103/−23.
- [Full evidence](deep-dive/04-terminal-catchup.md#25-fix-replay-server-batching)

**14. `fix-electron-launch-chooser` — Jun 10**
- Branch: `fix/electron-launch-chooser-flow`
- PR #377 (launch chooser feature) + PR #379 (hardening fixes). All 24 commits are on main. Blob hashes match for `desktop-provisioning.ts`, `launch-choice-handler.ts`, `startup.ts`, `port-check.ts`, `launch-options.ts`, and all test files.
- [Full evidence](deep-dive/05-settings-electron-codex.md#worktree-21-fix-electron-launch-chooser-fixelectron-launch-chooser-flow)

**15. `codex-interrupt-freeze` — Jun 10**
- Branch: `codex-interrupt-freeze`
- PR #400 (`9f54a96a` "Ack duplicate Codex interrupts after completion"). `MAX_COMPLETED_TURN_KEYS`, `activeTurnKeys`/`completedTurnKeys` sets, `completedTurnInterrupt()` guard — all present on main.
- [Full evidence](deep-dive/05-settings-electron-codex.md#worktree-22-codex-interrupt-freeze-codex-interrupt-freeze)

**16. `tab-status-reliability` — Jun 4**
- Branch: `fix/tab-status-reliability`
- Squash-merged as `6e81e505` ("Improve tab status reliability"). Identical diff stat: 66 files, +3201/−232. Spot-check confirmed `turnCompletionSlice.ts`, `useTurnCompletionNotifications.ts`, `codex-activity-tracker.ts` all match. 22 worktree commits accounted for in the squash.
- [Full evidence](deep-dive/06-tab-status-reliability.md)

**17. `plan-opencode-marker-cache` — Jun 4**
- Branch: `perf/opencode-marker-cache`
- PR #391 (`a24f22bd`). Tree hash identical — `git diff a24f22bd..8f43a9f2 --stat` = zero output. All new core files exist on main with identical line counts (listing-query: 84, listing-runner: 119, listing-worker: 44). 22 tests pass.
- [Full evidence](deep-dive/07-opencode-marker-cache.md)

**18. `warm-tab-delta-replay` — Jun 3**
- Branch: `fix/warm-tab-delta-replay`
- PR #386 (`dd12912b` "fix terminal warm tab replay and backpressure"). 14 files, +918/−80. Broker warm replay logic, replay-ring delta snapshots, terminal-attach-policy, backpressure tests.
- [Full evidence](deep-dive/04-terminal-catchup.md#28-warm-tab-delta-replay)

**19. `fix-mobile-scroll` — Jun 2**
- Branch: `fix/mobile-opencode-touch-scroll`
- PR #383 (`2c8978e8` "fix(terminal): enable touch-scroll in alternate buffer with mouse tracking"). 3 files, +297/−12. Synthetic WheelEvents for mobile touch.
- [Full evidence](deep-dive/04-terminal-catchup.md#30-fix-mobile-scroll)

---

## Skipped — Plan-Only / Trivial

These 7 worktrees were identified during first-pass as containing no substantive code changes. They were not deep-dived.

| # | Worktree | Date | Type | Notes |
|---|----------|------|------|-------|
| 1 | `plan-fresh-agent-transcript-contract` | Jun 19 | Plan-only | 685-line design doc for transcript contract |
| 2 | `find-bug` | Jun 17 | Trivial | 2-line test reorder making shared-state websocket suites sequential |
| 3 | `durable-ws-reconnect` | Jun 15 | Plan-only | WS recovery + legacy codingcli removal plan docs (2200 lines) |
| 4 | `freshagent-user-jump-plan` | Jun 14 | Plan-only | 1908-line plan for user message jump navigation |
| 5 | `disable-superpowers-plugin` | Jun 3 | Trivial | 1-line config change in `.claude/settings.json` (1 insertion, 3 deletions) |
| 6 | `docs-ci-merge-gate` | May 30 | Trivial | 21-line docs update at `docs/development/windows-electron-build.md` |
| 7 | `agent-chat-spec-plan` | May 29 | Plan-only | 823-line design doc for agent chat spec split |

---

## Full Reference Table

All 32 worktrees, sorted newest to oldest.

| # | Worktree | Branch | Date | Verdict | Analysis Summary |
|---|----------|--------|------|---------|-----------------|
| 1 | rollback-opencode-sidecars | `rollback/opencode-sidecars` | Jun 21 | **Ready for landing** | Single-sidecar routing refactor; 205 tests pass; 7 ahead, 0 behind |
| 2 | repro-freshopencode-playwright | `debug/freshopencode-playwright-repro-dev` | Jun 19 | **In main** | PR #454 merged with identical tree |
| 3 | plan-fresh-agent-transcript-contract | `plan/fresh-agent-transcript-contract` | Jun 19 | **Skipped** (plan) | 685-line design doc only |
| 4 | debug-freshcodex-cwd | `debug-freshcodex-cwd` | Jun 19 | **In main** | PR #450 squash-merged (identical tree) |
| 5 | fix-mobile-longpress-menu | `fix-mobile-longpress-menu` | Jun 19 | **In main** | PR #451, byte-identical diff |
| 6 | port-glm-5.2-to-dev | `port/glm-5.2-to-dev` | Jun 18 | **In main** | PR #447 landed same model entry |
| 7 | investigate-bouncer | `investigate-bouncer` | Jun 18 | **In main** | Plan doc already on main |
| 8 | fresh-agent-thinking-muted-color | `fix/fresh-agent-thinking-muted-color` | Jun 18 | **Ready for landing** | Muted thinking text; CSS + e2e test; no conflicts |
| 9 | opencode-think-normalization | `fix/opencode-think-normalization` | Jun 17 | **Ready for landing** | Think tag preservation; 18 tests pass |
| 10 | opencode-refresh-restore-white-page | `fix/opencode-refresh-restore-white-page` | Jun 17 | **Ready for landing** | White page fix; 126 tests pass; bug verified on main |
| 11 | find-bug | `find-bug` | Jun 17 | **Skipped** (trivial) | 2-line test reorder |
| 12 | electron-modifier-link-external | `electron-modifier-link-external` | Jun 17 | **In main** | PR #444; all commits reachable from main |
| 13 | fix-freshagent-user-message-quotes | `fix-freshagent-user-message-quotes` | Jun 15 | **In main** | Code present verbatim on main |
| 14 | opencode-playback-dev-pr | `test/opencode-playback-coalescing-dev` | Jun 15 | **In main** | Main commit c50dd6f8 (squashed) |
| 15 | opencode-playback-coalescing | `test/opencode-playback-coalescing` | Jun 15 | **In main** | Main commit c50dd6f8 (source branch) |
| 16 | durable-ws-reconnect | `plan/durable-ws-reconnect` | Jun 15 | **Skipped** (plan) | WS recovery plan docs only |
| 17 | freshagent-user-jump-plan | `freshagent-user-jump-plan` | Jun 14 | **Skipped** (plan) | 1908-line plan doc only |
| 18 | freshagent-tool-attribution | `freshagent-tool-attribution` | Jun 13 | **Finish work** | Skill filtering; needs rebase onto PR #452 |
| 19 | freshagent-serif-full-style | `freshagent-transcript-no-auto-collapse` | Jun 13 | **In main** | PR #452 fixed regression differently |
| 20 | new-settings-ui | `new-settings-ui` | Jun 12 | **Finish work** | Tab settings refactor; 185 commits behind; needs rebase |
| 21 | fix-electron-launch-chooser | `fix/electron-launch-chooser-flow` | Jun 10 | **In main** | PR #377+#379; blob hashes match |
| 22 | codex-interrupt-freeze | `codex-interrupt-freeze` | Jun 10 | **In main** | PR #400; same code on main |
| 23 | proof-terminal-catchup-architecture | `proof-terminal-catchup-architecture` | Jun 8 | **In main** | Same research content; zero diff in shared files |
| 24 | fix-terminal-catchup | `fix-terminal-catchup` | Jun 7 | **In main** | PR #396; identical stats |
| 25 | fix-replay-server-batching | `fix-replay-server-batching` | Jun 7 | **In main** | PR #397; identical stats |
| 26 | tab-status-reliability | `fix/tab-status-reliability` | Jun 4 | **In main** | Squash-merged as 6e81e505; 66 files match |
| 27 | plan-opencode-marker-cache | `perf/opencode-marker-cache` | Jun 4 | **In main** | PR #391; zero diff tree |
| 28 | warm-tab-delta-replay | `fix/warm-tab-delta-replay` | Jun 3 | **In main** | PR #386; identical stats |
| 29 | disable-superpowers-plugin | `chore/disable-superpowers-plugin` | Jun 3 | **Skipped** (trivial) | 1-line config change |
| 30 | fix-mobile-scroll | `fix/mobile-opencode-touch-scroll` | Jun 2 | **In main** | PR #383; identical stats |
| 31 | docs-ci-merge-gate | `docs/ci-merge-gate-note` | May 30 | **Skipped** (trivial) | 21-line docs update |
| 32 | agent-chat-spec-plan | `codex/agent-chat-spec-plan` | May 29 | **Skipped** (plan) | 823-line design doc only |

---

## Appendix: Deep-Dive Reports

Detailed analysis for all worktrees with meaningful novel work is available in:

| Report | Worktrees Covered |
|--------|-------------------|
| [01-opencode-freshcodex.md](deep-dive/01-opencode-freshcodex.md) | repro-freshopencode-playwright, debug-freshcodex-cwd, rollback-opencode-sidecars |
| [02-opencode-think-playback.md](deep-dive/02-opencode-think-playback.md) | opencode-think-normalization, opencode-refresh-restore-white-page, opencode-playback-dev-pr, opencode-playback-coalescing |
| [03-fresh-agent-ui.md](deep-dive/03-fresh-agent-ui.md) | fix-mobile-longpress-menu, fresh-agent-thinking-muted-color, fix-freshagent-user-message-quotes, freshagent-tool-attribution, freshagent-serif-full-style |
| [04-terminal-catchup.md](deep-dive/04-terminal-catchup.md) | proof-terminal-catchup-architecture, fix-terminal-catchup, fix-replay-server-batching, warm-tab-delta-replay, fix-mobile-scroll |
| [05-settings-electron-codex.md](deep-dive/05-settings-electron-codex.md) | port-glm-5.2-to-dev, electron-modifier-link-external, new-settings-ui, fix-electron-launch-chooser, codex-interrupt-freeze |
| [06-tab-status-reliability.md](deep-dive/06-tab-status-reliability.md) | tab-status-reliability |
| [07-opencode-marker-cache.md](deep-dive/07-opencode-marker-cache.md) | plan-opencode-marker-cache |
