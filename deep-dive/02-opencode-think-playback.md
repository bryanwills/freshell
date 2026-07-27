# Deep Dive: OpenCode Think/Playback Worktrees

## 1. opencode-think-normalization (fix/opencode-think-normalization)

**Verdict: Ready for landing**

**Commits (1):**
```
53a34024 Fix OpenCode leaked think normalization
```

**Diff summary: 2 files, +258/-24**
- `server/fresh-agent/adapters/opencode/normalize.ts` (+163/-24)
- `test/unit/server/fresh-agent/opencode-normalize.test.ts` (+119/-1)

**Evidence:**
- NOT on main: `normalizeBalancedThinkTags`, `itemsFromAssistantTextPart`, `textSummaryFromItems`, `computeToolAfterByPartIndex` all absent from main's codebase.
- Main still has the old `stripThinkTags` (line 103) which strips think content entirely, losing reasoning traces.
- Main's `itemFromPart` returns `FreshAgentTranscriptItem | undefined`; worktree changes it to return `FreshAgentTranscriptItem[]`.
- Main has no `NormalizedTextSegment` type, no segmented think handling.
- All 18 tests pass on the worktree (vs 14 on main — 4 new tests plus test refactoring).
- Significant improvement: instead of silently deleting think tag content, it's preserved as `thinking` items visible in the transcript.
- Handles balanced, malformed, unterminated, and dangling think tags with proper categorization.
- Changes the `normalizeOpencodeTurn` return type from `FreshAgentTranscriptItem | undefined` to `FreshAgentTranscriptItem[]` (removes the `!` assertions in tests).

**Recommendation:** This is clean, well-tested, and improves OpenCode transcript quality. No relationship to other worktrees. Should land.

---

## 2. opencode-refresh-restore-white-page (fix/opencode-refresh-restore-white-page)

**Verdict: Ready for landing**

**Commits (9):**
```
b87f63c2 Fix hidden OpenCode refresh hydration race
2905d0f5 test(e2e): harden opencode hidden refresh replay coverage
0289309c Fix hidden OpenCode replay gap repair
a415a4a1 docs: seed opencode e2e scrollback budget
592168ce docs: harden opencode refresh e2e plan
b5c75674 docs: fix opencode refresh plan review issues
9adb0f5e docs: fix opencode plan test command
41e70ccb docs: update opencode refresh restore plan
7a7f81da docs: plan opencode refresh restore fix
```

**Diff summary: 4 files, +734/-17**
- `src/components/TerminalView.tsx` (+3/-1) — two changes:
  1. Removes `!hiddenRef.current` from `isUnrecoverableOpenCodeViewportHydrate` (line 3373 on main), allowing hidden OpenCode viewport hydrations with replay gaps to enter the replacement path.
  2. Adds `if (shouldWaitForProviderBehavior) return` guard to the "Create or attach to backend terminal" useEffect (line 2666 on main), preventing attach race before extensions registry loads.
- `test/unit/client/components/TerminalView.lifecycle.test.tsx` (+203/-0) — 3 new tests:
  - "arms hidden OpenCode viewport hydration after provider registry readiness"
  - "recreates a hidden restored OpenCode pane when background viewport hydration cannot replay startup output"
  - Also imports `getHydrationQueue` and `resetEnsureExtensionsRegistryCacheForTests`
- `test/e2e-browser/specs/opencode-restart-recovery.spec.ts` (+151/-17)
- `docs/superpowers/plans/2026-06-17-opencode-refresh-restore-white-page.md` (new, 394 lines)

**Evidence:**
- NOT on main: `!hiddenRef.current` is still present at line 3373 on main, blocking the fix.
- NOT on main: The `shouldWaitForProviderBehavior` guard is absent from the "Create or attach" useEffect on main (only present in xterm init at line 1652 and renderer effect at line 2076).
- All 126 tests pass on the worktree (main: 124 — 2 additional tests).
- Real bug: When a hidden OpenCode tab is restored after browser refresh and the replay ring can't cover startup output, the hidden pane is left with stale terminal state. After reveal, the terminal shows blank/white. This fix lets the hidden pane enter the same durable OpenCode replacement flow that already works for visible panes.
- The e2e test validates real terminal buffer content after repair, not just session IDs.
- Does not conflict with think-normalization (different files) or playback-coalescing (different section of TerminalView.tsx).

**Recommendation:** Fixes a real white-page bug for OpenCode restore. Tests are thorough (unit + e2e). No overlap with other worktrees. Should land.

---

## 3. opencode-playback-dev-pr (test/opencode-playback-coalescing-dev)

**Verdict: Throw away - in main already**

**Commits (1, squashed):**
```
35372288 Fix OpenCode replay playback coalescing
```

**Diff summary: 6 files, +1206/-6** (same as item 4)

**Evidence:**
- The exact same fix is already on `origin/main` as commit `c50dd6f8 Fix OpenCode replay playback coalescing`.
- The worktree is a squashed version of the real branch.
- `disableWriteCoalescing` parameter is absent from `TerminalView.tsx` on both main and the worktree (the fix already applied).
- Both playback worktrees have identical TerminalView.tsx content (confirmed via `git diff` of the files).

**Recommendation:** Already landed as part of `c50dd6f8`. No new code to land.

---

## 4. opencode-playback-coalescing (test/opencode-playback-coalescing)

**Verdict: Throw away - in main already**

**Commits (10):**
```
9174c25f Merge remote-tracking branch 'origin/main' into test/opencode-playback-coalescing
dc3c1347 test: harden replay write progression recorder
f4ee6982 test: cover OpenCode replay write progression in browser
b73cb61f fix: coalesce parser-barrier terminal replay writes
036819f1 test: expect parser barrier replay writes to coalesce
b143ffe5 plan: fix deferred replay test gates
66b20eb8 plan: align replay test semantics
f031ed1a plan: fix replay verification gates
7d903762 plan: add replay validation coverage
847e4774 plan: outline OpenCode replay coalescing fix
7895df4a test: cover barrier-heavy replay coalescing
```

**Diff summary: 6 files, +1206/-6**
- `src/components/TerminalView.tsx` — removes `disableWriteCoalescing`, adds harness write recording
- `src/lib/test-harness.ts` — adds `TerminalWriteEvent` type and storage
- `test/e2e-browser/helpers/test-harness.ts` — adds `getTerminalWriteEvents`, `clearTerminalWriteEvents`
- `test/e2e-browser/specs/opencode-replay-write-progression.spec.ts` (new, 107 lines)
- `test/unit/client/components/TerminalView.lifecycle.test.tsx` (+227/-6)
- `docs/superpowers/plans/2026-06-15-opencode-replay-playback-coalescing.md` (new, 802 lines)

**Evidence:**
- `c50dd6f8 Fix OpenCode replay playback coalescing` on `origin/main` is the same fix (squashed into 1 commit).
- `disableWriteCoalescing` is absent from TerminalView.tsx on main and both worktrees.
- This is the source branch from which `c50dd6f8` was derived (the granular version with plan commits).

**Recommendation:** Already landed as `c50dd6f8`. No new code to land.
