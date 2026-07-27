# Worktree Audit #2: Final Aggregation Report

**Date:** Jun 23, 2026
**Scope:** Full sweep — all worktrees in `.worktrees/`
**Repository:** Freshell (`origin/main` SHA `84e049b6`)
**Total worktrees audited:** 67 (including main + analysis)

---

## Executive Summary

| Metric | Count |
|--------|-------|
| Total worktrees evaluated | 67 |
| Ancestor=NO (novel work) | 5 |
| Ancestor=YES, dirty | 9 |
| Ancestor=YES, clean, 0 ahead (bulk cleanup) | 52 |
| **Ready for landing** | **3** |
| **Throw away — superseded** | **2** |
| **Skipped — plan only / obsolete** | **2** |
| **Bulk cleanup — already on main** | **52** |

This audit swept all 67 worktrees (including main and the analysis worktree itself). Five had novel work (ancestor=NO) and nine had uncommitted changes on ancestor=YES heads. Seven received deep-dive analysis across two reports ([08-fresh-agent-audit2.md](deep-dive/08-fresh-agent-audit2.md) and [09-uncommitted-work.md](deep-dive/09-uncommitted-work.md)). The remaining 52 are clean, already-on-main worktrees safe for bulk deletion.

---

## Ready for Landing

These 3 worktrees contain verified novel code that is NOT on `origin/main`. They should be committed (if uncommitted), pushed, and submitted as PRs targeting `main`.

### 1. `fresh-agent-parity-audit` — Jun 23

| | |
|---|---|
| **Branch** | `feat/fresh-agent-parity-audit` |
| **Commits** | 1 (`4e9a51dd`) |
| **Ahead/Behind** | 1 ahead, 0 behind, clean |
| **Diff** | 8 files, +168 lines |

**Description:** Closes four parity gaps across the fresh-agent adapter layer: (1) Codex `onTurnCompleted` snapshot emit, (2) OpenCode idle session absent from status map, (3) `OPENCODE_CMD` env var support, (4) runtime manager subscribe materialization race. Each fix is independently valuable and surgical.

**Tests:** All 299 tests pass across 12 test files.

**Risk:** Low. Small, surgical changes. No merge conflicts (0 behind). The `requireOrRecoverySession` change reuses an existing, well-tested method.

**Deep-dive:** [08-fresh-agent-audit2.md §1](deep-dive/08-fresh-agent-audit2.md#1-fresh-agent-parity-audit-featfresh-agent-parity-audit)

---

### 2. `fresh-agent-turn-complete` — Jun 23

| | |
|---|---|
| **Branch** | `fix/fresh-agent-server-authoritative-completion` |
| **Commits** | 7 (initial implementation + 6 fresheyes review rounds) |
| **Ahead/Behind** | 7 ahead, 3 behind, clean |
| **Diff** | 23 files, +1289/−63 lines |

**Description:** Moves fresh-agent (freshclaude/freshcodex/freshopencode) turn-completion from client-derived to server-authoritative, matching the pattern already used for terminal Claude/Codex (`terminal.turn.complete`). Adds `turn-complete-clock.ts`, `sdk-events.ts` turn-complete event type, SDK bridge, and client-side completion thunk. Client hook simplified to handle only the waiting-for-approval edge.

**Tests:** All 203 relevant tests pass (173 server + 30 client).

**Maturity:** 6 fresheyes review rounds. No merge conflicts (merge-tree clean against `origin/main`). 3 behind needs rebase but semantically compatible.

**Deep-dive:** [08-fresh-agent-audit2.md §2](deep-dive/08-fresh-agent-audit2.md#2-fresh-agent-turn-complete-fixfresh-agent-server-authoritative-completion)

---

### 3. `freshagent-header-bar` — Jun 15

| | |
|---|---|
| **Branch** | `freshagent-header-bar` |
| **Commits** | 0 (uncommitted changes) |
| **Ahead/Behind** | 0 ahead, 0 behind |
| **Diff** | 3 files modified, +100/−33 (uncommitted) |

**Description:** Generalizes the fresh-agent runtime meta resolution from Claude-only to all providers (`resolveFreshClaudeRuntimeMeta` → `resolveFreshAgentRuntimeMeta`). Adds a new `FreshAgentToolIcons` component in `PaneHeader.tsx` that renders up to 8 tool icons (Bash, Read, Write/Edit, Glob/Grep, WebFetch/WebSearch) in the pane header, consuming existing Redux state.

**Test gap:** The test file adds icon mocks but does not add assertions for `FreshAgentToolIcons` behavior. A test must be added that renders `PaneHeader` with a fresh-agent pane having `tools` in Redux state and verifies the icons render (and nothing renders when `tools` is empty/undefined).

**Risk:** Low. Clean, idiomatic generalization consuming existing Redux state. Minimal divergence on main.

**Deep-dive:** [09-uncommitted-work.md §1](deep-dive/09-uncommitted-work.md#1-freshagent-header-bar--ready-for-landing-with-test-gap)

---

## Throw Away — Superseded

These 2 worktrees contain work that was either reverted or replaced by a different approach on main. They should be deleted to avoid confusion.

### 4. `fresh-agent-progressive-hydration` — Jun 21

| | |
|---|---|
| **Branch** | `feature/fresh-agent-progressive-hydration` |
| **Commits** | 6 |
| **Ahead/Behind** | 6 ahead, 47 behind, clean |
| **Diff** | 47 files, +3165/−2477 lines |

**What happened:** PR #468 (`d9bdc212`) merged progressive fresh-agent hydration. PR #470 (`4e88560a`) fully reverted it due to a "body-heavy restart regression." The revert is an exact inverse (47 files, +2477/−3165).

**Why throw away:** The work was deliberately removed from main. A replacement plan exists (`fresh-agent-rehydration-fix`) that re-lands the same goal with a fundamentally different, safer architecture (metadata-only snapshots, bounded transcript loading). Re-landing the original worktree would reintroduce the regression.

**Deep-dive:** [08-fresh-agent-audit2.md §3](deep-dive/08-fresh-agent-audit2.md#3-fresh-agent-progressive-hydration-featurefresh-agent-progressive-hydration)

---

### 5. `fix-codex-sidecar-build` — May 18

| | |
|---|---|
| **Branch** | `fix/codex-sidecar-build` |
| **Commits** | 0 (uncommitted changes) |
| **Ahead/Behind** | 0 ahead, 0 behind |
| **Diff** | 4 modified + 4 new files (uncommitted) |

**What happened:** Built a codex sidecar abstraction with `durable-rollout-tracker.ts` (250 lines, fs-watch + poll) and `sidecar.ts` (218 lines, lifecycle wrapper) plus 577 lines of tests. All 4 modified-file changes are already on main. Main replaced the entire approach with `durability-proof.ts` (JSONL parsing) and `durability-store.ts` (disk persistence) — a completely different architecture.

**Why throw away:** All modified-file changes duplicate work already landed. The new files implement functionality that main provides through different files with a different architecture. Keeping this work would create duplicate, conflicting implementations.

**Deep-dive:** [09-uncommitted-work.md §2](deep-dive/09-uncommitted-work.md#2-fix-codex-sidecar-build--throw-away-superseded-by-main)

---

## Skipped — Plan Only / Obsolete

These 2 worktrees contain no actionable implementation work.

### 6. `fresh-agent-rehydration-fix` — Jun 22

| | |
|---|---|
| **Branch** | `plan/fresh-agent-rehydration-fix` |
| **Commits** | 3 (all `docs:` commits) |
| **Ahead/Behind** | 3 ahead, 33 behind, clean |
| **Diff** | docs only (2559 lines planning document) |

**Description:** A detailed replacement plan for the reverted progressive-hydration work. The plan treats snapshots as metadata-only, uses a dedicated turns API endpoint for transcript loading, loads one visible page with bounded bodies, and warms older history through a strict background budget. Baseline is `origin/main` after rollback PR #470.

**Why skipped:** Plan only — zero implementation. The plan is valuable as a reference but the worktree itself contains no code to land. Implementation should proceed from a fresh worktree based on `origin/main`.

---

### 7. `dev-stack-main-trial` — May 20

| | |
|---|---|
| **Branch** | `integration/main-to-tested-dev-stack-20260520` |
| **Commits** | 1 |
| **Ahead/Behind** | 1 ahead, 508 behind, clean |
| **Diff** | 282 files, +44769/−8187 lines |

**Description:** A historical integration snapshot bringing main to a tested dev integration stack. The enormous diff (44K+ lines) is a snapshot of a prior integration state.

**Why skipped:** Ancient relic — 508 commits behind main. This integration snapshot is fully superseded by main's evolution. No novel work remains.

---

## Bulk Cleanup — Already on Main

These 52 worktrees are ancestor=YES, clean, and 0 ahead. Their HEAD commits are already on `origin/main`. They contain no novel work and no uncommitted changes. **Safe to delete via `git worktree remove`.**

| # | Worktree | Branch | HEAD Date |
|---|----------|--------|-----------|
| 1 | agent-chat-spec-impl | `codex/agent-chat-spec-split` | 2026-05-29 |
| 2 | allowed-origins-bootstrap-cleanup | `codex/allowed-origins-bootstrap-cleanup` | 2026-05-21 |
| 3 | autokill-15 | `fix/autokill-default-15min` | 2026-05-29 |
| 4 | claude-resume-robustness | `claude-resume-robustness` | 2026-06-11 |
| 5 | claude-status-notification-robustness | `claude-status-notification-robustness` | 2026-05-29 |
| 6 | codex-cli-upgrade-hang | `fix/codex-cli-upgrade-hang` | 2026-06-17 |
| 7 | codex-runtime-flake | `fix/codex-runtime-startup-flake` | 2026-05-30 |
| 8 | deterministic-codex-durability | `fix/deterministic-codex-durability` | 2026-06-15 |
| 9 | dev | `dev` | 2026-06-22 |
| 10 | feat-new-favicon-landing | `feat/new-favicon` | 2026-05-24 |
| 11 | fix-awaitConfig-timeout | `fix/awaitConfig-timeout` | 2026-05-25 |
| 12 | fix-codex-clean-exit-recovery | `fix-codex-clean-exit-recovery` | 2026-05-24 |
| 13 | fix-codex-update-skip | `fix-codex-update-skip` | 2026-06-18 |
| 14 | fix-editor-complete-root | `fix/editor-complete-root` | 2026-06-11 |
| 15 | fix-freshopencode-bouncer | `fix/freshopencode-bouncer` | 2026-06-19 |
| 16 | fix-launch-planner-shutdown | `fix-launch-planner-shutdown` | 2026-06-07 |
| 17 | fix-pr-415-freshopencode | `fix/pr-415-freshopencode` | 2026-06-12 |
| 18 | fix-setup-wizard-dismiss | `fix-setup-wizard-dismiss` | 2026-05-24 |
| 19 | fresh-agent-font-size | `feat/fresh-agent-font-size` | 2026-06-11 |
| 20 | fresh-agent-import-cleanup | `codex/fresh-agent-import-cleanup` | 2026-05-29 |
| 21 | fresh-agent-mono-style | `feat/fresh-agent-mono-style` | 2026-06-17 |
| 22 | fresh-agent-opacity-whitespace | `codex/fresh-agent-opacity-whitespace` | 2026-06-11 |
| 23 | fresh-agent-transcript-contract | `feature/fresh-agent-transcript-contract` | 2026-06-19 |
| 24 | fresh-client-responsive-display | `codex/fresh-client-responsive-display` | 2026-06-12 |
| 25 | fresh-clients-toggle | `codex/fresh-clients-toggle` | 2026-05-29 |
| 26 | fresh-pane | `feat/fresh-pane-clients` | 2026-06-10 |
| 27 | freshagent-body-reopen-context | `fix/freshagent-body-reopen-context` | 2026-06-14 |
| 28 | freshagent-qa-fixes | `freshagent-qa-fixes` | 2026-06-13 |
| 29 | freshagent-style-settings | `freshagent-style-settings` | 2026-06-12 |
| 30 | freshagent-ux-restoration | `freshagent-ux-restoration` | 2026-05-24 |
| 31 | freshcodex-improvements | `freshcodex-improvements` | 2026-05-24 |
| 32 | freshopencode-db-history | `freshopencode-db-history` | 2026-06-12 |
| 33 | freshopencode-parity | `freshopencode-parity` | 2026-05-24 |
| 34 | kata-freshopencode-materialization-race | `kata/freshopencode-materialization-race` | 2026-06-21 |
| 35 | main-test-server-2 | `fix/freshclaude-main-server-validation` | 2026-06-11 |
| 36 | materialization-subscription-leak-plan | `docs/materialization-subscription-leak-plan` | 2026-06-14 |
| 37 | new-settings-ui | `new-settings-ui` | 2026-06-21 |
| 38 | opencode-browser-refresh-restore | `opencode-browser-refresh-restore` | 2026-06-01 |
| 39 | opencode-refresh-tab-survival | `fix/opencode-refresh-tab-survival` | 2026-06-01 |
| 40 | opencode-replay-gap-lease | `fix/opencode-replay-gap-lease` | 2026-06-15 |
| 41 | pane-settings-input-ux | `codex/pane-settings-input-ux` | 2026-06-11 |
| 42 | plan-codex-stale-resume-recovery | `plan/codex-stale-resume-recovery` | 2026-06-10 |
| 43 | plan-terminal-catchup-architecture | `plan-terminal-catchup-architecture` | 2026-06-08 |
| 44 | plan-windows-wsl-paths | `plan-windows-wsl-paths` | 2026-06-07 |
| 45 | reopen-session-flavor-plan | `plan/reopen-session-flavor` | 2026-06-13 |
| 46 | replay-logging-summaries | `fix/replay-logging-summaries` | 2026-06-15 |
| 47 | repro-c3-sidecar-missing | *(detached HEAD)* | 2026-05-18 |
| 48 | rollback-fresh-agent-progressive-hydration | `rollback-fresh-agent-progressive-hydration` | 2026-06-21 |
| 49 | settings-show-switches-plan | `plan/settings-show-switches` | 2026-06-22 |
| 50 | stdout-stderr-errors-only | `codex/stdout-stderr-errors-only` | 2026-05-24 |
| 51 | terminal-catchup-stream-safety | `terminal-catchup-stream-safety` | 2026-06-10 |
| 52 | zrrj-freshopencode-recovery | `fix/zrrj-freshopencode-recovery` | 2026-06-22 |

---

## Methodology

1. **Baseline classification** ([baseline-criteria-2.md](baseline-criteria-2.md)): Automated sweep of all 67 worktrees checking ancestor status, dirty state, and ahead/behind counts.
2. **First-pass table** ([first-pass-table-2.md](first-pass-table-2.md)): Manual review of 5 ancestor=NO and 2 dirty ancestor=YES worktrees to assess meaningfulness.
3. **Deep-dive analysis**: Two reports covering 7 worktrees — [08-fresh-agent-audit2.md](deep-dive/08-fresh-agent-audit2.md) (3 fresh-agent worktrees) and [09-uncommitted-work.md](deep-dive/09-uncommitted-work.md) (2 uncommitted-work worktrees). Plus 2 plan/obsolete worktrees assessed from the first-pass table.
4. **Bulk classification**: Remaining 52 worktrees verified as ancestor=YES, clean, 0 ahead via automated script.

---

## Recommendations

1. **Land** `fresh-agent-parity-audit` — push, get approval, open PR. Small, low-risk, high-value.
2. **Land** `fresh-agent-turn-complete` — push, get approval, open PR. Most significant feature; 6 fresheyes rounds; no conflicts.
3. **Finish** `freshagent-header-bar` — add the missing test assertion for `FreshAgentToolIcons`, commit, push, get approval, open PR.
4. **Delete** `fresh-agent-progressive-hydration` — superseded by rehydration-fix plan.
5. **Delete** `fix-codex-sidecar-build` — superseded by main's durability approach.
6. **Keep or delete** `fresh-agent-rehydration-fix` — plan-only; valuable as reference but no code to land.
7. **Delete** `dev-stack-main-trial` — ancient relic, 508 behind.
8. **Bulk delete** the 52 clean ancestor=YES worktrees via `git worktree remove`.
