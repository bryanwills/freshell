# Deep Dive: Uncommitted Work Analysis (09)

**Date:** 2026-06-23
**Scope:** Second-pass analysis of 2 worktrees whose HEAD is an ancestor of `origin/main` but have uncommitted changes that may contain lost work.

---

## 1. `freshagent-header-bar` — Ready for landing (with test gap)

**Branch:** `freshagent-header-bar`
**HEAD:** `74a9313d` — "fix(fresh-agent): remove decorative prose backticks around inline code in assistant messages (#435)"
**HEAD on main?** Yes — PR #435 landed on `origin/main`.

### Uncommitted changes (3 files, +100/-33)

#### `src/components/panes/PaneContainer.tsx` (+42/-29)
Renames `resolveFreshClaudeRuntimeMeta` → `resolveFreshAgentRuntimeMeta` and **generalizes it from Claude-only to all fresh-agent providers**:
- Removes the `if (content.provider !== 'claude') return undefined` guard.
- When an indexed session is found (for any provider), returns the same metadata as before (cwd, checkoutRoot, repoRoot, branch, isDirty, tokenUsage).
- When no indexed session is found, falls back to session snapshot data (cwd, branch from `session.snapshot.worktrees[0].branch`, tokenUsage from `session.snapshot?.tokenUsage`) — previously returned `undefined`.
- Simplifies the call site from a nested ternary (`provider === 'claude' ? resolveFreshClaudeRuntimeMeta(...) : undefined`) to a direct `resolveFreshAgentRuntimeMeta(...)` call.

#### `src/components/panes/PaneHeader.tsx` (+42/-2)
Adds a new `FreshAgentToolIcons` component that:
- Reads `tools` from `state.freshAgent.sessions[sessionKey].tools` via `useAppSelector`.
- Renders up to 8 tool icons (Bash→Terminal, Read→FileText, Write/Edit→FilePen, Glob/Grep→FileSearch, WebFetch/WebSearch→Globe) in the pane header.
- Uses `makeFreshAgentSessionKey` from `@shared/fresh-agent` (exists on main at `shared/fresh-agent.ts:126`).
- Includes a `title` attribute listing all tool names for accessibility.
- Inserted into the header before the agent identity span.

#### `test/unit/client/components/panes/PaneHeader.test.tsx` (+18)
Adds icon mocks for the 6 new lucide-react icons (Terminal, FileSearch, Globe, FilePen, FileText, SquareTerminal) to the existing `vi.mock('lucide-react', ...)` block.

### Is this work on main?

**No.** Verified:
- Main still has `resolveFreshClaudeRuntimeMeta` (Claude-only) at `src/components/panes/PaneContainer.tsx:142,436`.
- Main does NOT have `FreshAgentToolIcons` or `FRESH_AGENT_TOOL_ICONS` in `PaneHeader.tsx`.
- Main DOES have the `tools` state in `freshAgentSlice.ts` (lines 244, 251, 262, 279) — so the worktree consumes existing Redux state that was already landed.

### Merge feasibility
Main has diverged minimally on these files since the worktree's base:
- `PaneContainer.tsx`: 4 insertions, 1 deletion on main since worktree HEAD.
- `PaneHeader.tsx`: no changes on main since worktree HEAD.
- `PaneHeader.test.tsx`: no changes on main since worktree HEAD.

The uncommitted changes should apply with minimal conflict.

### Caveat — test gap
The test file adds icon mocks but **does not add any new test assertions** for `FreshAgentToolIcons` behavior. There are 46 tests in the file (same count on main and worktree base). At minimum, a test should be added that:
1. Renders `PaneHeader` with a fresh-agent pane that has `tools` in Redux state.
2. Verifies the tool icons render.
3. Verifies nothing renders when `tools` is empty/undefined.

### Verdict: **Ready for landing** (add the missing test assertion first)

The work is a clean, idiomatic generalization that consumes existing Redux state. The `resolveFreshAgentRuntimeMeta` rename removes a Claude-specific guard and provides a sensible fallback for non-Claude providers. The `FreshAgentToolIcons` feature is a useful UX enhancement. Add the missing test, then commit and land.

---

## 2. `fix-codex-sidecar-build` — Throw away (superseded by main)

**Branch:** `fix/codex-sidecar-build`
**HEAD:** `8cd9ccf1` — "Merge pull request #349 from danshapiro/replacement/chunk-error-recovery-main-20260518"
**HEAD on main?** Yes — PR #349 landed on `origin/main`.

### Uncommitted changes (4 modified + 4 new files)

#### Modified files — ALL changes already on main

**`server/terminal-registry.ts`** (+2):
Adds `OPENCODE_SERVER_USERNAME` and `OPENCODE_SERVER_PASSWORD` to the env passthrough destructuring in `buildSpawnSpec`.
- **On main?** Yes — `origin/main:server/terminal-registry.ts:997-998` has both vars.
- **Conclusion:** Duplicate of work already landed.

**`src/components/TerminalView.tsx`** (+1/-1):
Changes `restoreError: buildRestoreError('dead_live_handle')` → `restoreError: undefined` in the fresh-recovery `status: 'creating'` path (worktree line 2374).
- **On main?** Yes — main has `restoreError: undefined` in all `status: 'creating'` paths (lines 2797, 2907, 4086). Main only retains `buildRestoreError('dead_live_handle')` in the `status: 'error'` path with `launchAttempt?.recoveryIntent` (line 3959).
- **Conclusion:** Duplicate of work already landed (likely via commit `63944cfd "Add Codex session lifecycle observability (#318)"`).

**`test/e2e/codex-refresh-rehydrate-flow.test.tsx`** (+1/-4):
Renames test from "surfaces restore-unavailable while starting explicit fresh recovery..." to "starts explicit fresh recovery without retaining a restore error..." and changes expectation from `restoreError: { code: 'RESTORE_UNAVAILABLE', reason: 'dead_live_handle' }` to `restoreError: undefined`.
- **On main?** The specific test ("live-only terminal is gone" with `dead_live_handle`) **does not exist on main**. Main's test file was entirely reworked around the durability-based approach. Main's remaining `RESTORE_UNAVAILABLE` expectations (lines 541, 552) use `reason: 'durable_artifact_missing'`, not `dead_live_handle`.
- **Conclusion:** The test scenario was replaced by main's durability implementation. The worktree's change is moot.

**`test/server/ws-session-observability.test.ts`** (+1):
Adds `cwd: '/home/user/project'` to one `waitForLifecycleEvent` call (the `restore_unavailable_fresh_fallback` event at ~line 372).
- **On main?** Main has `cwd: '/home/user/project'` in 5 other `waitForLifecycleEvent` calls (lines 273, 286, 298, 322, 361) but NOT in this specific one. Main's test suite passes without it, meaning the schema doesn't require `cwd` for this event type.
- **Conclusion:** Unnecessary — main's test passes without this change.

#### New files — superseded by main's different implementation

**`server/coding-cli/codex-app-server/durable-rollout-tracker.ts`** (250 lines):
A `CodexDurableRolloutTracker` class that tracks codex rollout files by:
- Watching the rollout path and its parent directory via `watchPath`/`subscribeToFsChanged`.
- Polling with exponential backoff (250ms → 5s max) via `pathExists`.
- Promoting the thread when the rollout file becomes durable (exists on disk).
- Handling thread replacement, cleanup, and disposal.

**`server/coding-cli/codex-app-server/sidecar.ts`** (218 lines):
A `CodexTerminalSidecar` class that wraps `CodexAppServerRuntime` with:
- Terminal attachment lifecycle (`attachTerminal`, `onDurableSession`, `onThreadLifecycle`, `onFatal`).
- Pending lifecycle event buffering (max 10 events) for terminals not yet attached.
- Durable session promotion forwarding.
- Ownership metadata updates.
- Orphan reaping via `reapOrphanedSidecars()`.

**`test/unit/server/coding-cli/codex-app-server/durable-rollout-tracker.test.ts`** (154 lines, 4 tests):
Tests fs/changed event handling, late durability, watch registration fallback, and thread replacement.

**`test/unit/server/coding-cli/codex-app-server/sidecar.test.ts`** (423 lines, 7 tests):
Tests orphan reaping (process identity matching), durable rollout tracking forwarding, lifecycle event replay ordering, and shutdown ordering.

### Is this work on main?

**Main took a completely different approach to codex durability:**

| Worktree approach | Main approach |
|---|---|
| `durable-rollout-tracker.ts` — fs watch + poll for rollout file existence | `durability-proof.ts` — proof rollout file by reading first JSONL record and validating thread ID |
| `sidecar.ts` — sidecar lifecycle wrapper with pending event buffering | `durability-store.ts` — persist durability records to `~/.freshell/codex-durability/` |
| `CodexTerminalSidecar` class | No equivalent class (different architecture) |

Main's durability implementation was landed through multiple commits:
- `eeb42f6a` "Add Codex durability proof and store"
- `996f48b9` "feat: rehydrate codex tabs through exact app-server sessions"
- `d965725c` "refactor: move codex terminals to sidecar-owned durability"
- `3e69fc97` "fix(codex): track durable rollouts by exact path"
- `ff837b45` "Launch fresh Codex without pre-durable resume"
- `1c2ff3b4` "Stabilize Codex durable session restore"
- Plus several review/harden commits

Main's `server/coding-cli/codex-app-server/` directory has **7 files** (`client.ts`, `durability-proof.ts`, `durability-store.ts`, `launch-planner.ts`, `launch-retry.ts`, `protocol.ts`, `recovery-policy.ts`, `remote-proxy.ts`, `remote-tui-failure-detector.ts`, `restore-decision.ts`, `runtime.ts`) — none of which are `durable-rollout-tracker.ts` or `sidecar.ts`.

### Verdict: **Throw away — superseded by main**

The worktree's approach (fs-watch + poll tracker + sidecar wrapper) was replaced by main's approach (durability proof via JSONL parsing + durability store via disk persistence). The modified-file changes are all already on main. The new files implement functionality that main provides through different files with a different architecture. Keeping this work would create duplicate, conflicting implementations of the same feature.

---

## Summary

| Worktree | Verdict | Reason |
|---|---|---|
| `freshagent-header-bar` | **Ready for landing** | Generalizes header bar to all providers + adds tool icons; consumes existing Redux state; not on main; add missing test assertion first |
| `fix-codex-sidecar-build` | **Throw away — superseded by main** | All modified-file changes already on main; new files implement a different approach to codex durability that main replaced with `durability-proof.ts`/`durability-store.ts` |
