# Worktree Audit: Baseline Criteria

> Generated: 2026-06-21
> Purpose: Define repeatable rules for categorizing Freshell worktrees during an audit.
> Use: Each worktree is classified as **A (auto-skip/safe-to-delete)**, **B (first-pass only)**, or **C (deep-dive required)**.

---

## 0. Repository Baseline State

| Property | Value |
|---|---|
| `origin/main` SHA | `80319767` |
| `main` <> `origin/main` | 0 ahead, 0 behind (fully synced) |
| Active worktrees | 91 (under `.worktrees/`) |
| Local-only branches | 69 |
| Remote-only branches (in inventory) | 37 |
| Worktrees with ANCESTOR=YES | ~58 |
| Worktrees with ANCESTOR=NO | ~33 |
| Dirty worktrees | 8 |

**Key invariant:** The local `main` branch tracks `origin/main` exactly. Any worktree whose HEAD is an ancestor of `origin/main` contains *zero* novel commits.

---

## 1. Primary Dimension: Ancestry (HEAD vs `origin/main`)

### 1.1 ANCESTOR == YES -- HEAD is an ancestor of `origin/main`

**Meaning:** Every commit on this branch already exists in the mainline history. The worktree may still have *uncommitted* work.

**Recommendation matrix:**

| Working tree | Upstream branch exists on origin? | Rating | Rationale |
|---|---|---|---|
| Clean | Yes (pushed) | **A** -- auto-skip | No novel code at all. The worktree is a stale checkout of already-merged history. |
| Clean | No (local only) | **B** -- first-pass | Branch may hold local-only commits that are ancestors of main (odd but possible; check `git log origin/main..HEAD` is empty). If truly empty, downgrade to A. |
| Dirty | Any | **B** -- first-pass | Uncommitted changes could be novel. Quick inspection of `git status --porcelain` and `git diff` needed. |

**Exception:** If the dirty files are only:
- `assets/electron/*` (icons)
- `docs/` (documentation)
- `*.md` files (readme/docs)
- untracked files in a `docs/` or `assets/` directory

...and no source code is touched, downgrade to **A** (asset/doc-only changes are not behavioral).

### 1.2 ANCESTOR == NO -- HEAD is NOT an ancestor of `origin/main`

**Meaning:** There is at least one commit on this branch that does NOT exist in `origin/main`. This is the primary pool of potentially novel, unmerged work.

**All such worktrees start at minimum as B (first-pass).** Upgrade to C (deep-dive) based on secondary criteria below.

---

## 2. Secondary Dimensions

### 2.1 Ahead / Behind Counts

Use `git rev-list --count --left-right HEAD...origin/main`.

| Ahead | Behind | Rating | Rationale |
|---|---|---|---|
| 0 | any | See ancestry dimension above | No novel work (by definition). |
| 1 | >= 100 | **B** -- first-pass | Single trivial commit that was never merged and is now very stale. Quickly inspect `git log -1 --stat` -- if it's a chore/markup/doc-only commit, downgrade to A. |
| 1-5 | < 100 | **B** -- first-pass | Small delta, manageable. Assess topic. |
| 6-20 | any | **C** -- deep-dive | Non-trivial amount of potentially novel work. |
| > 20 | any | **C** -- deep-dive | Significant amount of work that may have been superseded or is genuinely novel. |
| any | 0 | **C** -- deep-dive | Branch has zero behind-main but is not an ancestor -- this means it contains truly novel work that *diverged* from a clean baseline. **Rare and high-value.** |

**Special case** -- `rollback/opencode-sidecars`: 7 ahead, 0 behind. This is the only branch in this category. Requires deep-dive -- it has 7 unmerged commits about opencode sidecar routing and the worktree is clean.

### 2.2 Branch Naming Convention

| Pattern | Default | Rationale |
|---|---|---|
| `backup/*` | **A** -- auto-skip | Local-only backup snapshots. If clean and ancestor, safe to delete. |
| `analysis/*` | **A** -- auto-skip | Analysis/audit branches, not code. |
| `plan/*`, `plan-*` | **B** -- first-pass | Planning branches. Quick check if they contain only `.plans/`, docs, or markdown. If code diff is empty, downgrade to A. |
| `docs/*`, `docs-*` | **B** -- first-pass | Documentation. Same as plan. |
| `proof-*` | **B** -- first-pass | Proof-of-concept. May contain experimental code. |
| `port/*` | **B** -- first-pass | Port (cherry-pick) from one branch to another. Usually redundant. |
| `debug/*`, `debug-*` | **B** -- first-pass | Debug/investigation. Small, targeted. |
| `trycycle-*` | **C** -- deep-dive | Trial cycles that may contain substantive work. |
| `replacement/*` | **C** -- deep-dive | Replacement branches for prior approaches. Often supersede other branches. |
| `revert/*` | **B** -- first-pass | Revert commits. Usually 1 commit. Check if revert has itself been reverted. |
| `integration/*` | **C** -- deep-dive | Integration/merge testing branches. |
| `feat/*`, `feature/*` | **C** -- deep-dive | Feature work. Presumed novel. |
| `fix/*`, `fix-*` | **C** -- deep-dive | Bugfix work. Presumed novel. |
| `codex/*` | **C** -- deep-dive | Codex-driven branches. Contain agent-authored code changes. |
| `freshagent-*`, `freshcodex-*`, `freshopencode-*` | **C** -- deep-dive | Fresh-agent subsystem work. |
| `opencode-*`, `codex-*` | **C** -- deep-dive | OpenCode/Codex subsystem work. |
| `chore/*` | **B** -- first-pass | Usually small non-functional changes. |
| `perf/*` | **C** -- deep-dive | Performance work, potentially important. |
| `rollback/*` | **C** -- deep-dive | Rollback preparation. May contain the logic being rolled back to. |
| `test/*` | **B** -- first-pass | Test-only branches. If no source changes, downgrade to A. |
| `claude-*`, `claude/*` | **B** -- first-pass | Single-agent exploration branches. |
| `origin-main-*`, `main-*` | **A** -- auto-skip | Smoke-test or main-sync branches. |
| `rebuild` | **B** -- first-pass | Build config changes only. |
| `materialization-*` | **C** -- deep-dive | Data-flow / architecture work. |
| `new-*` | **C** -- deep-dive | New feature work. |
| `electron-*` | **C** -- deep-dive | Electron desktop work. |

**Important:** Naming is not definitive. Even `plan/*` branches are upgraded to C if ahead count > 20 or if source code is present. Use naming as a *triage hint*, not a hard rule.

### 2.3 Supersedence (from `branch-inventory.json`)

The `superseded_by` field in the branch inventory identifies branches that have been explicitly replaced.

- If a branch is marked as **superseded** AND its `contained_dev` is false AND ahead count is low (<=5): **A** -- auto-skip.
- Example: `replacement/fresh-agent-foundation-main-20260518` (2 ahead, superseded by two later branches).

### 2.4 Contained in `dev` (from `branch-inventory.json`)

The `contained_dev` field indicates the branch's commits are already in the `dev` integration branch.

| `contained_dev` | Rating | Rationale |
|---|---|---|
| `true` | **B** -- first-pass | Work is already in dev, but may not be in main yet. Quick check if dev has been merged to main. |
| `false` | Per other criteria | Work may or may not be anywhere. |

Note: As of this analysis, only 3 inventory branches have `contained_dev=true`:
- `origin/codex/opencode-focus-activation-repro-20260517`
- `origin/docs/mcp-split-editor-instructions`
- `origin/freshcodex-contract-foundation`

These should be **B** at minimum. They may need C if their code is not yet in `origin/main`.

### 2.5 Dirty Working Tree

A dirty working tree means uncommitted (staged, unstaged, or untracked) changes exist.

- If dirty AND **ANCESTOR=YES**: **B** -- first-pass (may have work-in-progress).
- If dirty AND **ANCESTOR=NO**: **C** -- deep-dive (novel commits + in-flight changes = highest risk of lost work).

**Current dirty worktrees (8 total):**

| Worktree | Branch | ANCESTOR | Notes |
|---|---|---|---|
| `build-new-favicon` | `feat/new-favicon-build` | YES | Icon assets only -- potentially A |
| `deflake-terminal-refresh` | `fix/terminal-directory-refresh-unhandled-rejection` | YES | Untracked doc only -- potentially A |
| `electron-windows-native` | `fix/electron-windows-native` | YES | AGENTS.md + icon assets -- potentially A |
| `fix-codex-sidecar-build` | `fix/codex-sidecar-build` | YES | Source code changes -- B/C |
| `fix-freshagent-ui-details` | `fix/freshagent-ui-details` | YES | Untracked docs/design/ -- potentially A |
| `freshagent-header-bar` | `freshagent-header-bar` | YES | Source + test changes -- B/C |
| `origin-main-smoke` | `fix/real-claude-binary-path` | YES | Test file modified -- B |
| `rebuild` | `rebuild` | YES | `electron-builder.yml` -- potentially A |

First 5 (asset/doc-only) can be downgraded to A. `fix-codex-sidecar-build`, `freshagent-header-bar`, and `origin-main-smoke` need first-pass.

### 2.6 Local-Only Branches

**69 local branches have no remote counterpart.** These are higher risk (work cannot be recovered from origin).

| Condition | Rating |
|---|---|
| ANCESTOR=YES + clean | **A** -- auto-skip |
| ANCESTOR=YES + dirty | **B** -- first-pass |
| ANCESTOR=NO + ahead <= 5 | **B** -- first-pass |
| ANCESTOR=NO + ahead > 5 | **C** -- deep-dive |

---

## 3. Consolidated Criteria Summary

### Category A -- Auto-Skip / Safe to Delete

All of the following must be true:

1. `git merge-base --is-ancestor HEAD origin/main` exits 0 (ANCESTOR=YES), AND
2. Working tree is clean (`git status --porcelain` is empty), OR dirty files are only non-code assets (icons, docs, markdown), AND
3. Branch is not in the `superseded_by` chain of an active branch (if it is, it might be needed as a parent)
4. OR branch matches: `backup/*`, `analysis/*`, `origin-main-*`, `pr/###` (merged PR refs, not local worktrees)
5. OR branch is in `branch-inventory.json` with `superseded_by: [non-empty]` AND `ahead_main <= 5` AND `contained_dev == false`
6. OR branch name indicates it is a plan/doc-only branch AND `git log --name-only origin/main..HEAD` shows only markdown/docs files

**Expected size:** ~40-50 worktrees.

### Category B -- First-Pass Only

Any of the following:

1. ANCESTOR=YES + dirty working tree (quick `git diff` review)
2. ANCESTOR=NO + ahead <= 5 + behind >= 100 (stale trivial branch)
3. ANCESTOR=NO + naming matches `plan/*`, `docs/*`, `proof-*`, `port/*`, `debug/*`, `test/*`, `chore/*` AND ahead <= 20
4. `contained_dev=true` in inventory (check if dev has been merged to main)
5. `revert/*` branches (1 commit, quick check)
6. `rebuild` branch (build config)

**Expected size:** ~20-25 worktrees.

### Category C -- Deep-Dive Required

Any of the following:

1. ANCESTOR=NO + ahead > 5 (unless naming falls into a trivial pattern and inspection confirms triviality)
2. ANCESTOR=NO + ahead >= 1 + behind == 0 (truly novel, no mainline catch-up needed -- e.g., `rollback/opencode-sidecars`)
3. ANCESTOR=NO + dirty working tree
4. ANCESTOR=NO + local-only branch with ahead > 5
5. Naming matches: `feat/*`, `feature/*`, `fix/*`, `codex/*`, `freshagent-*`, `freshcodex-*`, `freshopencode-*`, `opencode-*`, `codex-*`, `trycycle-*`, `replacement/*`, `integration/*`, `perf/*`, `rollback/*`, `materialization-*`, `new-*`, `electron-*`
6. Branch is in `branch-inventory.json` with `ahead_main > 20`

**Expected size:** ~15-25 worktrees.

---

## 4. Quick Reference: Decision Flowchart

```
Is HEAD ancestor of origin/main?
  +-- YES ------------------------------------------------+
  |   Is working tree clean?                              |
  |   +-- YES -> A (auto-skip)                            |
  |   +-- NO                                              |
  |       Are dirty files only docs/assets?               |
  |       +-- YES -> A (auto-skip)                        |
  |       +-- NO -> B (first-pass)                        |
  +-- NO -------------------------------------------------+
      Is ahead > 0?                                       |
      +-- NO -> A (impossible but safe)                    |
      +-- YES                                             |
          Is behind == 0?                                 |
          +-- YES -> C (deep-dive, novel work)             |
          +-- NO                                          |
              Is ahead <= 5?                              |
              +-- YES -> check naming                     |
              |   +-- plan/docs/proof/port/               |
              |   |   debug/test/chore -> B               |
              |   +-- other/fix/feat/codex -> C           |
              +-- NO -> C (deep-dive)                     |
```

---

## 5. Tooling

For automated inspection of each worktree during the audit, use:

```bash
# Per-worktree checks
wt_dir=".worktrees/<name>"
branch=$(git -C "$wt_dir" rev-parse --abbrev-ref HEAD)
head=$(git -C "$wt_dir" rev-parse HEAD)
ancestor=$(git merge-base --is-ancestor "$head" origin/main && echo YES || echo NO)
status=$(git -C "$wt_dir" status --porcelain)
counts=$(git rev-list --count --left-right "$head"...origin/main 2>/dev/null)
ahead=$(echo "$counts" | cut -f1)
behind=$(echo "$counts" | cut -f2)
```

---

## 6. Edge Cases and Caveats

1. **Detached HEAD worktrees** (e.g., `repro-c3-sidecar-missing`): Treat as ANCESTOR=YES if merge-base matches. No branch to save, but verify commits are reachable from origin/main before deleting.

2. **`fresh-pane-staging`**: Currently on `main` itself (0 ahead, 0 behind). This is a staging checkout of main -- safe to delete if not actively running a server.

3. **`dev` worktree**: Tracks the local `dev` branch, currently at origin/main. If dev is no longer used per AGENTS.md, this is A.

4. **`freshopencode-new-hang`**: Also on main commit. A.

5. **`worktree-analysis`**: This very worktree. Keep it during the audit, delete afterward.

6. **Branch inventory has branches with NO local worktree**: 37 remote-only branches from the inventory are not checked out locally. They still represent unmerged work on origin and should be evaluated separately, but are outside scope of a *worktree* audit.

7. **Misleading ahead/behind counts**: `repro-c3-sidecar-missing` and `debug/freshopencode-playwright-repro-dev` show 6 ahead / 3 behind in counts, but their commits are all ancestors of origin/main (ANCESTOR=YES). The counts reflect ancestry divergence from a fork point, not novel work. Verify with `git log origin/main..HEAD` -- if empty, downgrade to A.

---

## Appendix: Current Worktree Inventory by Category

### Candidate A (Ancestor + Clean -- auto-skip candidates)

These worktrees are ANCESTOR=YES, clean (`git status --porcelain` empty), and contain zero novel commits. They are stale checkouts of already-merged history. Safe to delete unless actively running a server.

Worktrees in this category (47 total):
`agent-chat-spec-impl`, `allowed-origins-bootstrap-cleanup`, `autokill-15`, `claude-resume-robustness`, `claude-status-notification-robustness`, `codex-cli-upgrade-hang`, `codex-runtime-flake`, `deterministic-codex-durability`, `dev`, `feat-new-favicon-landing`, `fix-awaitConfig-timeout`, `fix-codex-clean-exit-recovery`, `fix-codex-update-skip`, `fix-editor-complete-root`, `fix-freshopencode-bouncer`, `fix-launch-planner-shutdown`, `fix-pr-415-freshopencode`, `fix-setup-wizard-dismiss`, `fresh-agent-font-size`, `fresh-agent-import-cleanup`, `fresh-agent-mono-style`, `fresh-agent-opacity-whitespace`, `fresh-agent-transcript-contract`, `fresh-client-responsive-display`, `fresh-clients-toggle`, `fresh-pane`, `fresh-pane-staging`, `freshagent-body-reopen-context`, `freshagent-qa-fixes`, `freshagent-style-settings`, `freshagent-ux-restoration`, `freshcodex-improvements`, `freshopencode-db-history`, `freshopencode-new-hang`, `freshopencode-parity`, `main-test-server-2`, `materialization-subscription-leak-plan`, `opencode-browser-refresh-restore`, `opencode-refresh-tab-survival`, `opencode-replay-gap-lease`, `pane-settings-input-ux`, `plan-codex-stale-resume-recovery`, `plan-terminal-catchup-architecture`, `plan-windows-wsl-paths`, `reopen-session-flavor-plan`, `replay-logging-summaries`, `stdout-stderr-errors-only`, `terminal-catchup-stream-safety`

### Candidate C (Non-ancestor or significant ahead -- deep-dive candidates)

These start at C and may be downgraded after first-pass inspection:

| Worktree | Branch | Ahead | Behind | Key Risk |
|---|---|---|---|---|
| `agent-chat-spec-plan` | `codex/agent-chat-spec-plan` | 4 | 390 | Non-ancestor, codex topic |
| `codex-interrupt-freeze` | `codex-interrupt-freeze` | 1 | 303 | Non-ancestor |
| `debug-freshcodex-cwd` | `debug-freshcodex-cwd` | 11 | 28 | 11 ahead |
| `dev-stack-main-trial` | `integration/main-to-tested-dev-stack-20260520` | 1 | 425 | Integration branch |
| `disable-superpowers-plugin` | `chore/disable-superpowers-plugin` | 1 | 322 | Non-ancestor |
| `docs-ci-merge-gate` | `docs/ci-merge-gate-note` | 1 | 339 | Non-ancestor |
| `durable-ws-reconnect` | `plan/durable-ws-reconnect` | 10 | 91 | 10 ahead, plan branch |
| `electron-modifier-link-external` | `electron-modifier-link-external` | 8 | 40 | Non-ancestor |
| `find-bug` | `find-bug` | 1 | 40 | Non-ancestor |
| `fix-electron-launch-chooser` | `fix/electron-launch-chooser-flow` | 23 | 219 | 23 ahead |
| `fix-freshagent-user-message-quotes` | `fix-freshagent-user-message-quotes` | 2 | 75 | Non-ancestor |
| `fix-mobile-longpress-menu` | `fix-mobile-longpress-menu` | 1 | 29 | Non-ancestor |
| `fix-mobile-scroll` | `fix/mobile-opencode-touch-scroll` | 1 | 323 | Non-ancestor |
| `fix-replay-server-batching` | `fix-replay-server-batching` | 1 | 305 | Non-ancestor |
| `fix-terminal-catchup` | `fix-terminal-catchup` | 1 | 306 | Non-ancestor |
| `fresh-agent-thinking-muted-color` | `fix/fresh-agent-thinking-muted-color` | 1 | 29 | Non-ancestor |
| `freshagent-header-bar` | `freshagent-header-bar` | 0 | 75 | **DIRTY** (source + tests) |
| `freshagent-serif-full-style` | `freshagent-transcript-no-auto-collapse` | 1 | 150 | Non-ancestor |
| `freshagent-tool-attribution` | `freshagent-tool-attribution` | 2 | 149 | Non-ancestor |
| `freshagent-user-jump-plan` | `freshagent-user-jump-plan` | 5 | 149 | Non-ancestor |
| `investigate-bouncer` | `investigate-bouncer` | 3 | 29 | Non-ancestor |
| `new-settings-ui` | `new-settings-ui` | 2 | 185 | Non-ancestor |
| `opencode-playback-coalescing` | `test/opencode-playback-coalescing` | 11 | 77 | 11 ahead |
| `opencode-playback-dev-pr` | `test/opencode-playback-coalescing-dev` | 1 | 89 | Non-ancestor |
| `opencode-refresh-restore-white-page` | `fix/opencode-refresh-restore-white-page` | 9 | 40 | Non-ancestor |
| `opencode-think-normalization` | `fix/opencode-think-normalization` | 1 | 40 | Non-ancestor |
| `plan-fresh-agent-transcript-contract` | `plan/fresh-agent-transcript-contract` | 8 | 29 | 8 ahead, plan branch |
| `plan-opencode-marker-cache` | `perf/opencode-marker-cache` | 2 | 316 | Non-ancestor, perf |
| `port-glm-5.2-to-dev` | `port/glm-5.2-to-dev` | 3 | 40 | Non-ancestor |
| `proof-terminal-catchup-architecture` | `proof-terminal-catchup-architecture` | 1 | 303 | Non-ancestor |
| `repro-freshopencode-playwright` | `debug/freshopencode-playwright-repro-dev` | 6 | 3 | 6 ahead (verify ancestor) |
| `rollback-opencode-sidecars` | `rollback/opencode-sidecars` | 7 | 0 | **Highest priority: 7 ahead, 0 behind** |
| `tab-status-reliability` | `fix/tab-status-reliability` | 22 | 315 | 22 ahead |
| `warm-tab-delta-replay` | `fix/warm-tab-delta-replay` | 1 | 321 | Non-ancestor |
| `fix-codex-sidecar-build` | `fix/codex-sidecar-build` | 0 | 425 | **DIRTY** (source code) |
