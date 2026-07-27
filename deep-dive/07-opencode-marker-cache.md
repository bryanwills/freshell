# Worktree Deep Dive: plan-opencode-marker-cache (perf/opencode-marker-cache)

## Branch Info
- **Worktree path:** `.worktrees/plan-opencode-marker-cache`
- **Branch:** `perf/opencode-marker-cache`
- **Created:** Jun 4 (last activity)
- **Commits:** 2 commits, 18 files changed, 2704 insertions
- **Worktree HEAD:** `8f43a9f2` (`perf(opencode): run session listing off the event loop in a worker`)
- **Parent commit:** `cc58518a` (`docs(plan): off-thread OpenCode listing worker plan`)

## Verdict: **Throw away - in main already**

## Evidence

### 1. Git Analysis — Commits
```
8f43a9f2 perf(opencode): run session listing off the event loop in a worker
cc58518a docs(plan): off-thread OpenCode listing worker plan
```

### 2. Implementation Already on `origin/main`
Three independent checks confirm the worktree's code is already landed on main:

**a) PR #391 merge commit on origin/main:**
```
a24f22bd perf(opencode): run session listing off the event loop in a worker (#391)
```
This merge commit (`Author: danshapiro, Date: Jun 4 10:00:30 2026`) is reachable from `origin/main`. It includes the same co-author attribution: `Co-Authored-By: Claude Opus 4.8`.

**b) Zero diff between worktree HEAD and merged PR:**
```bash
$ git diff a24f22bd..8f43a9f2 --stat | wc -l
0
```
The tree at `8f43a9f2` (worktree HEAD) is byte-for-byte identical to the merged commit `a24f22bd` on `origin/main`.

**c) All new core files exist on `origin/main` with identical line counts:**
| File | Worktree | `origin/main` | Match |
|------|----------|---------------|-------|
| `server/coding-cli/providers/opencode-listing-query.ts` | 84 lines | 84 lines | ✓ |
| `server/coding-cli/providers/opencode-listing-runner.ts` | 119 lines | 119 lines | ✓ |
| `server/coding-cli/providers/opencode-listing.worker.ts` | 44 lines | 44 lines | ✓ |
| `server/coding-cli/providers/opencode.ts` | Same content | Same content | ✓ (`diff` = 0 lines) |
| `server/coding-cli/session-indexer.ts` | Diff only from unrelated later PRs | — | Later PRs moved past this |

### 3. Plan Document Supersession
The plan document `docs/superpowers/plans/2026-06-04-opencode-listing-offthread-worker.md` (1417 lines, in the worktree) explicitly states:
> "This plan replaces the superseded `2026-06-03-opencode-marker-cache-eventloop.md` (cache/gate approaches were falsified)."

The earlier plan `2026-06-03-opencode-marker-cache-eventloop.md` (308 lines, in the worktree) is marked:
> **STATUS: SUPERSEDED — do not execute as-is.**

Both plan documents reference kata `xe4t`/`wab5` as the root cause. The off-thread worker plan was the final solution, and it has been fully implemented and merged.

### 4. One Minor Diff — Session Indexer (Unrelated)
The 24-line diff in `server/coding-cli/session-indexer.ts` is NOT related to the off-thread worker. It's about `extractFromIdeContext`/`isSystemContext` helper imports — a completely different feature that landed in a subsequent PR. The worktree is simply stale relative to `origin/main`'s recent commits (PRs #450–#454).

### 5. Test Results — All Passing
```
 ✓ test/unit/server/coding-cli/opencode-listing-query.test.ts      (6 tests)  ✓
 ✓ test/unit/server/coding-cli/opencode-listing-runner.test.ts    (12 tests) ✓
 ✓ test/unit/server/coding-cli/opencode-listing-worker.test.ts     (2 tests) ✓
 ✓ test/integration/server/opencode-listing-offthread.test.ts      (2 tests) ✓
```
20 unit tests + 2 integration tests all pass. The off-thread worker works correctly using real `worker_threads`.

### 6. JSONL History
No relevant JSONL history found in `~/.claude/projects/freshell/sessions/` or `~/.config/opencode/`. The kata/plan workflow appears to have been entirely code-driven.

## Recommendation Narrative

This is a clean case. The worktree `plan-opencode-marker-cache` was created to implement an off-thread OpenCode session listing worker. The implementation was fully developed, tested, and then merged to `main` via PR #391 as commit `a24f22bd`. The worktree branch's two commits (`cc58518a` plan doc + `8f43a9f2` implementation) have identical content to what's now on `origin/main`.

The worktree is stale — `origin/main` has moved ~20 commits past the merge point with unrelated features (fresh opencode fixes, Playwright repros, GLM model additions). The worktree itself has no unique or unmerged value.

**Action:** Delete the worktree branch and worktree directory. No salvageable work remains.
