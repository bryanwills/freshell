# Deep-Dive Worktree Analysis: Fresh-Agent Audit 2

**Date:** 2026-06-23
**Analyst:** opencode (umans-glm-5.2)
**Scope:** 3 fresh-agent worktrees in `/home/dan/code/freshell/.worktrees/`

---

## Summary Verdicts

| # | Worktree | Branch | Verdict |
|---|----------|--------|---------|
| 1 | `fresh-agent-parity-audit` | `feat/fresh-agent-parity-audit` | **Ready for landing** |
| 2 | `fresh-agent-turn-complete` | `fix/fresh-agent-server-authoritative-completion` | **Ready for landing** |
| 3 | `fresh-agent-progressive-hydration` | `feature/fresh-agent-progressive-hydration` | **Throw away — superseded** |

---

## 1. fresh-agent-parity-audit (`feat/fresh-agent-parity-audit`)

### Verdict: **Ready for landing**

### Metadata
- **Commits:** 1 (`4e9a51dd fix: close fresh-agent parity gaps across freshclaude/freshcodex/freshopencode`)
- **Ahead/Behind:** 1 ahead, 0 behind, clean
- **Diff:** 8 files, +168 lines
- **PR:** None open
- **Date:** Jun 23, 2026

### What it does
Closes four parity gaps across the fresh-agent adapter layer:

1. **Codex adapter — `onTurnCompleted` snapshot emit** (`server/fresh-agent/adapters/codex/adapter.ts`): The Codex app-server's `thread_status_changed(idle)` event fires *before* the completed assistant turn is committed to thread history, leaving the client with an empty transcript. This adds an `onTurnCompleted` subscription that fires *after* the turn is committed, emitting another idle snapshot so the client re-fetches the committed transcript. This achieves parity with freshopencode's post-idle emit.

2. **OpenCode adapter — idle session absent from status map** (`server/fresh-agent/adapters/opencode/adapter.ts`): The opencode `/session/status` map only reports active (busy/retry) sessions; an idle session returns `undefined`. Main treats this as malformed and logs a warning. This fix adds an early `if (status == null) return` (treating absence as idle), matching the serve manager's `onceIdle` semantics.

3. **OpenCode serve manager — `OPENCODE_CMD` env var** (`server/fresh-agent/adapters/opencode/serve-manager.ts`): Adds support for the `OPENCODE_CMD` environment variable to override the serve binary path, achieving parity with `CODEX_CMD` and `CLAUDE_CMD`. Also reorders constructor initialization so `this.env` is set before `this.command` (since `this.command` now reads `this.env.OPENCODE_CMD`).

4. **Runtime manager — subscribe materialization race** (`server/fresh-agent/runtime-manager.ts`): Changes `subscribe()` from `requireSession` (which throws "is not tracked" if the session isn't registered yet) to `requireOrRecoverSession` (which recovers via attach). This fixes the materialization race where the real session ID isn't yet registered (adapter.send hasn't resolved) when subscribe is called for the materialized real ID. The `requireOrRecoverSession` method already exists and is used by other mutation methods; `subscribe` was the only one still using the throwing variant.

### Tests
All 299 tests pass across 12 test files:
- `test/unit/server/fresh-agent/codex-adapter.test.ts` (34 tests) — includes new test for `onTurnCompleted` snapshot emission
- `test/unit/server/fresh-agent/opencode-serve-adapter.test.ts` (44 tests) — includes new test for absent-session-as-idle
- `test/unit/server/fresh-agent/opencode-serve-manager.test.ts` (49 tests) — includes new tests for `OPENCODE_CMD` env var
- `test/unit/server/fresh-agent/runtime-manager.test.ts` (23 tests) — includes new test for subscribe materialization race recovery

### Parity gaps still present on main
Confirmed all four fixes are absent from `origin/main`:
- `onTurnCompleted`: Not in main's `CodexRuntimePort` type; main's `subscribe` returns `runtime.onThreadLifecycle(...)` directly with no turn-completion handler.
- `status == null`: Main still uses `if (!status || typeof status !== 'object' ...)` — treats undefined as malformed.
- `OPENCODE_CMD`: Main uses `this.command = options.command ?? 'opencode'` — no env var support.
- `requireOrRecoverSession` in subscribe: Main uses `this.requireSession(locator)` — throws on materialization race.

### Risk assessment
- **Low risk.** Small, surgical changes. Each fix is independently valuable. All tests pass. No merge conflicts (0 behind). The `requireOrRecoverSession` change reuses an existing, well-tested method already used by other code paths.
- The `onTurnCompleted` handler is optional (`runtime.onTurnCompleted?.(...)`) — safely no-ops if the runtime doesn't support it.

---

## 2. fresh-agent-turn-complete (`fix/fresh-agent-server-authoritative-completion`)

### Verdict: **Ready for landing**

### Metadata
- **Commits:** 7 (initial implementation + 6 fresheyes review rounds)
- **Ahead/Behind:** 7 ahead, 3 behind, clean
- **Diff:** 23 files, +1289/-63 lines
- **PR:** None open
- **Date:** Jun 23, 2026
- **Plan doc:** `docs/plans/2026-02-08-fresh-agent-server-authoritative-turn-complete.md` (193 lines)

### What it does
Moves fresh-agent (freshclaude/freshcodex/freshopencode) turn-completion from **client-derived** to **server-authoritative**, matching the pattern already used for terminal Claude/Codex (`terminal.turn.complete`).

**Current (main) approach:** `useAgentSessionTurnCompletion.ts` watches each SDK pane's busy/pending edges and fires `recordTurnComplete` on a busy→idle transition. This client-side derivation was the source of premature (flicker), missed (fast-turn), and stale-color chimes.

**New (worktree) approach:** The server pushes discrete `freshAgent.turn.complete` events through the SDK bridge. The client hook is simplified to only handle the "waiting-for-approval" edge (0→≥1 pending permissions/questions), since turn completion is now server-authoritative. The waiting-for-approval edge uses a `#waiting` suffix on the synthetic terminalId to keep it in a separate dedupe namespace from the server turn-complete, so an approval can never suppress a real completion via the monotonic `at` guard.

### Key components added
- `server/fresh-agent/turn-complete-clock.ts` — per-thread turn-complete clock that survives reconnects; resets on shutdown
- `server/fresh-agent/sdk-events.ts` — adds `freshAgent.turn.complete` event type
- `server/sdk-bridge.ts` — bridges SDK turn-completion signals into the WS protocol
- `server/coding-cli/codex-app-server/protocol.ts` — adds `onTurnCompleted` to the runtime port
- `server/fresh-agent/adapters/codex/adapter.ts` — emits completion events via the clock
- `server/fresh-agent/adapters/opencode/adapter.ts` — emits completion edge for `/compact` and normal turns
- `src/store/turnCompletionThunks.ts` — new thunk for `applyFreshAgentCompletion`
- `src/lib/fresh-agent-ws.ts` — handles `freshAgent.turn.complete` WS messages
- `src/App.tsx` — wires the completion thunk
- Client tests: notification, hook, lib, slice

### Tests
All 203 relevant tests pass:
- **Server (173 tests, 5 files):**
  - `test/unit/server/fresh-agent/turn-complete-clock.test.ts` (4 tests)
  - `test/unit/server/fresh-agent/sdk-events.test.ts` (2 tests)
  - `test/unit/server/fresh-agent/codex-adapter.test.ts` (38 tests)
  - `test/unit/server/fresh-agent/opencode-serve-adapter.test.ts` (50 tests)
  - `test/unit/server/sdk-bridge.test.ts` (79 tests)
- **Client (30 tests, 3 files):**
  - `test/unit/client/hooks/useAgentSessionTurnCompletion.test.tsx` (4 tests)
  - `test/unit/client/lib/fresh-agent-turn-complete.test.ts` (4 tests)
  - `test/unit/client/store/turnCompletionSlice.test.ts` (22 tests)

### On main?
**No.** Confirmed:
- `server/fresh-agent/turn-complete-clock.ts` does not exist on main
- `server/sdk-bridge.ts` on main has no turn-complete references
- `server/fresh-agent/sdk-events.ts` on main has no turn-complete references
- Main's `useAgentSessionTurnCompletion.ts` (107 lines) uses client-derived busy→idle transitions; the worktree's version (98 lines) removes busy derivation and relies on server-authoritative events

The grep for `turn.complete|server.authoritative` on main returned results for **terminal** Claude turn-complete (`0c3b504c feat: server-owned Claude turn.complete`), which is a different feature (terminal BEL-based, not fresh-agent SDK-based).

### Merge compatibility
- **No git conflicts** (merge-tree check clean against `origin/main`)
- **3 behind:** PR #474 (freshagent-tool-attribution) — unrelated to turn-complete; no conflicts
- Main has evolved `useAgentSessionTurnCompletion.ts` slightly (107 vs 98 lines), but the worktree's changes are semantically compatible: it intentionally *removes* the busy-derivation path and replaces it with the server-authoritative event

### Maturity
- **6 fresheyes review rounds** — extensively reviewed and iterated
- Clean working tree
- No open questions or TODOs in the diff

### Risk assessment
- **Low-medium risk.** The architectural shift (client-derived → server-authoritative) is significant but well-tested and mature. The key risk is semantic compatibility with main's evolved `useAgentSessionTurnCompletion.ts`, but the worktree's approach is a clean replacement (removes busy derivation, adds server event handling). No merge conflicts. 6 fresheyes rounds inspire confidence.

---

## 3. fresh-agent-progressive-hydration (`feature/fresh-agent-progressive-hydration`)

### Verdict: **Throw away — superseded**

### Metadata
- **Commits:** 6 (plan + implementation + 4 review/cleanup rounds)
- **Ahead/Behind:** 6 ahead, 47 behind, clean
- **Diff:** 47 files, +3165/-2477 lines
- **PR:** #468 (merged), then **reverted** via PR #470 (commit `4e88560a`)
- **Date:** Jun 21, 2026

### What happened
1. PR #468 (`d9bdc212`) merged progressive fresh-agent hydration — a large refactor (+3165/-2477) that changed how fresh-agent snapshots/transcripts are loaded and displayed
2. PR #470 (`4e88560a`) **fully reverted** it: "This reverts d9bdc212... while preserving later unrelated work from #467 and #469."
3. The revert is an **exact inverse**: 47 files, +2477/-3165 (vs original +3165/-2477)

### Revert reason
Per the follow-up plan doc (`fresh-agent-rehydration-fix`), the revert was due to a **"body-heavy restart regression"** — the progressive hydration approach caused excessive payload sizes on restart/reload.

### Follow-up plan exists
The `fresh-agent-rehydration-fix` worktree (branch `plan/fresh-agent-rehydration-fix`) contains a detailed replacement plan:
- **File:** `docs/superpowers/plans/2026-06-22-fresh-agent-rehydration-fix.md`
- **Status:** Plan only (3 commits, all `docs:` commits)
- **Approach:** Fundamentally different from the reverted work:
  - Treats snapshots as **metadata-only** (status, capabilities, pending requests, usage totals) — contract rejects non-empty turn arrays
  - Uses `/api/fresh-agent/threads/:sessionType/:provider/:threadId/turns` as the **only transcript loading path**
  - Client loads one visible page with bounded bodies when a pane is actually visible
  - Warms older history for hidden/inactive idle panes through a strict background budget
  - Adds structured server logs for served turn pages
  - Baseline is `origin/main` after rollback PR #470

### Why throw away
- The work was **fully reverted** — it's not on main and was deliberately removed
- A **new plan exists** (`fresh-agent-rehydration-fix`) that re-lands the same goal with a fundamentally different, safer architecture
- The worktree is **47 commits behind** main — extremely stale
- The original approach caused a regression that the new plan explicitly avoids ("Do not revive PR #468's restart-time automatic deep backfill behavior")
- Re-landing the original worktree would reintroduce the regression

### Risk assessment
- **N/A — should not be landed.** The work is superseded by a better-planned replacement. The worktree should be deleted to avoid confusion.

---

## Recommendations

1. **fresh-agent-parity-audit**: Push branch, get approval, open PR targeting `main`. Small, low-risk, high-value parity fixes.

2. **fresh-agent-turn-complete**: Push branch, get approval, open PR targeting `main`. Mature (6 fresheyes rounds), well-tested (203 tests), no conflicts. This is the most significant of the three — it completes the server-authoritative turn-complete pattern across all fresh-agent providers.

3. **fresh-agent-progressive-hydration**: Delete the worktree. The follow-up work should proceed from the `fresh-agent-rehydration-fix` plan, not from this reverted branch.

---

## Methodology

- Reviewed actual diffs against `origin/main` for all three worktrees
- Ran the relevant test suites in-context (from each worktree)
- Verified whether the fixes/features are present on `origin/main` by inspecting main's version of the affected files
- Checked for open PRs via `gh pr list`
- Checked merge compatibility via `git merge-tree`
- Read the revert commit message and follow-up plan doc for progressive-hydration
- Verified no broad kill patterns or process safety violations were needed
