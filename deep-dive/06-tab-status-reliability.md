# Worktree Deep Dive: tab-status-reliability

## Branch Info
- Branch: `fix/tab-status-reliability`
- Location: `.worktrees/tab-status-reliability`
- HEAD: `744f2e74` (committed Jun 4)
- Base: `a24f22bd` (PR #391 — `perf(opencode): run session listing off the event loop in a worker`)
- Commits: 22 (20 code + 2 plan docs)
- Files changed: 66, +3201/−232

## Verdict: **Throw away - in main already**

## Evidence

### 1. Squash merge already landed on main

Commit `6e81e505` ("Improve tab status reliability") is a **squash merge** of the entire worktree's changes and is an ancestor of current `origin/main`:

```
git merge-base --is-ancestor 6e81e505 origin/main  →  YES
```

**Identical diff stat:**
- Worktree: `66 files changed, 3201 insertions(+), 232 deletions(-)`
- Squash merge `6e81e505`: `66 files changed, 3201 insertions(+), 232 deletions(-)`

**Identical file content** (spot-checked 3 key files across server and client):
- `src/store/turnCompletionSlice.ts` — MATCH
- `src/hooks/useTurnCompletionNotifications.ts` — MATCH
- `server/coding-cli/codex-activity-tracker.ts` — MATCH

### 2. Main has continued to evolve

The worktree is based on an old base (`a24f22bd`). Main has 315 additional commits. Post-merge evolution of the same files includes cleanups like:

- `37c9097c` — Remove legacy sdk websocket surface
- `67c14784` — Clean up fresh-agent websocket contract names  
- `72112fd6` — Remove live agent-chat pane type

This matches the AGENTS.md description of the current architecture that uses `fresh-agent` naming exclusively. The worktree branch still uses `agent-chat` naming patterns.

### 3. Key files already identical between worktree and main

Files that are identical as of main HEAD (`Jun 16`):
- `server/terminal-stream/registry-events.ts`
- `src/components/TabItem.tsx`
- `src/hooks/useTurnCompletionNotifications.ts`
- `src/lib/turn-complete-signal.ts`
- `src/store/turnCompletionSlice.ts`
- `src/store/turnCompletionThunks.ts`
- `src/lib/tab-codex-activity.ts` — deleted on main too
- All server activity trackers and wiring files

Files that differ do so because main has been refactored further (e.g., `agent-chat` → `fresh-agent` rename), not because the status-reliability changes are absent.

### 4. Commit list

All 22 worktree commits are accounted for in the squash merge:

| SHA | Message |
|-----|---------|
| 0056031e | docs(plan): tab-status reliability implementation plan |
| 8aee4912 | docs(plan): harden tab-status plan with load-bearing findings |
| ea6558af | docs(plan): apply Fresh Eyes round 1 corrections |
| bb3a5e30 | docs(plan): apply Fresh Eyes round 2 corrections |
| 4e014646 | docs(plan): apply Fresh Eyes round 3 corrections |
| bff1566c | refactor(status): monotonic turn-complete dedupe |
| a4382182 | fix(status): make codex turn-complete server-authoritative |
| 09b53ec1 | feat(status): green+sound bridge for fresh-agent & agent-chat |
| 01bae7b5 | refactor(status): de-advertise gemini/kimi |
| fa36cc27 | chore(status): reset opencode overlay; delete dead tab-codex-activity |
| 187b6af3 | fix(status): robust green clearing on any real engagement |
| ed57e523 | fix(status): claude resilient bind/create ordering |
| 76aaf73c | fix(status): clear stuck SDK blue on stream-end and error |
| 79e39594 | test(status): align clearing tests with server turn-complete |
| c77ead9f | feat(status): render codex pending as blue |
| 66b838ae | test(status): align activity indicators with pending blue |
| 04f2f051 | fix(status): opencode busy deadman and read-stall watchdog |
| a818026d | fix(status): opencode snapshot completion via association |
| 7f172dd0 | fix(status): feed codex turn events into activity tracker |
| d0fed3fb | fix(status): durable replay-safe turn completion |
| f99d6357 | docs(status): reflect status indicator behavior |
| 744f2e74 | test(status): align codex wiring completion sequence |

## Recommendation

Delete this worktree. The entire body of work has been squash-merged into `origin/main` via commit `6e81e505`. No code from this branch needs landing. The worktree is stale (315 commits behind main) and uses pre-refactored naming conventions (`agent-chat` instead of `fresh-agent`).

## Test Results

Not applicable — the code is already on main and active CI. Running tests against the worktree would test already-landed code against an outdated base.
