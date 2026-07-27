# Deep Dive: 03-fresh-agent-ui

Analysis of 5 worktrees related to Fresh Agent UI concerns.

---

## Worktree 4: fix-mobile-longpress-menu (Jun 19)

**Branch:** `fix-mobile-longpress-menu`
**Verdict: Throw away - in main already**

### Commits
```
75256b25 Fix mobile long-press menu release
```

### Diff Summary
```
ContextMenuProvider.tsx           | 19 ++++++++++---
ContextMenu.longpress.test.tsx    | 31 ++++++++++++++++++++++
2 files changed, 46 insertions(+), 4 deletions(-)
```

### Evidence
- Commit `9383c2fb` on `origin/main` ("Fix mobile long-press menu release") has the exact same diff — identical changes to `ContextMenuProvider.tsx` and `ContextMenu.longpress.test.tsx`.
- `git diff 9383c2fb..75256b25 --stat` produced zero output, confirming the diffs are byte-identical.
- Different SHA due to different parent commit (worktree based on a slightly older base).

### Recommendation
Delete the worktree and branch. The fix already landed on `origin/main` as commit `9383c2fb` (part of PR #451).

---

## Worktree 8: fresh-agent-thinking-muted-color (Jun 18)

**Branch:** `fix/fresh-agent-thinking-muted-color`
**Verdict: Ready for landing**

### Commits
```
a122fba1 fix(fresh-agent): render thinking text in muted color across all styles
```

### Diff Summary
```
src/index.css                              |  21 +++++
test/e2e-browser/specs/fresh-agent.spec.ts | 147 +++++++++++++++++++++
2 files changed, 168 insertions(+)
```

### Evidence
- **Not on main:** `grep` for `.fresh-agent-thinking-body [data-markdown-body]` in `src/index.css` returns zero results on `origin/main`. The `--tw-prose-*` override block (10 CSS custom properties overriding Tailwind prose colors to `--fresh-agent-muted-text`) is absent.
- **Not on main:** The `:where(p,li,blockquote)` and `:where(h1,h2,h3,h4)` color rules inside `.fresh-agent-style-serif .fresh-agent-thinking-body [data-markdown-body]` are absent.
- **Not on main:** The e2e test `"thinking text renders lighter than the final answer across sans, serif, and mono styles"` is absent (no match for `"thinking-color-thread"` or `"thinking.*color"` in the e2e spec).
- **Clean merge:** `git merge origin/main` completes without conflicts, confirming no overlap with subsequent work.
- **The bug:** The `.fresh-agent-thinking-body` container class exists on main with `color: var(--fresh-agent-muted-text)`, but Tailwind's prose plugin overrides paragraph/heading colors via `--tw-prose-*` variables at the `[data-markdown-body]` level. Without the worktree's overrides, thinking text in serif style renders in the answer's primary color instead of the muted color — a clear visual bug.
- **Tests:** The e2e test is thorough — it mocks a 5-turn freshcodex session with thinking items, probes computed `color` values across serif/sans/mono styles, and asserts thinking body prose != answer color.
- **History:** `git merge-base --is-ancestor a122fba1 origin/main` confirms the commit is not an ancestor of main.

### Recommendation
Push the branch, open a PR targeting `main`, get it landed. The fix is small, focused, well-tested, and addresses a real visual regression in the serif style.

---

## Worktree 15: fix-freshagent-user-message-quotes (Jun 15)

**Branch:** `fix/freshagent-user-message-quotes`
**Verdict: Throw away - in main already**

### Commits
```
49bcda45 refactor(fresh-agent): document that user quote stripping is scoped to opencode run argv
18e8a4bf fix(fresh-agent): strip surrounding quotes from user message text in OpenCode adapter
```

### Diff Summary
```
server/fresh-agent/adapters/opencode/normalize.ts   | 27 +++++++++++---
test/unit/server/fresh-agent/opencode-normalize.test.ts | 42 ++++++++++++++
2 files changed, 65 insertions(+), 4 deletions(-)
```

### Evidence
- **Fully on main:** `stripOpencodeRunArgumentQuoting()` is present at `server/fresh-agent/adapters/opencode/normalize.ts:95` with identical implementation and JSDoc.
- **On main usage:** Called at line 117 within `itemFromPart()` as an additional filter after `stripThinkTags()` (the main version is slightly more advanced — it also strips think/thinking tags).
- **All 4 tests on main:** "strips surrounding quotes from user text parts added by the OpenCode CLI", "leaves unquoted user text parts unchanged", "does not strip surrounding quotes from assistant text parts", "strips only one pair of surrounding quotes from user text parts" — all present verbatim at lines 160–202 of the test file on main.
- Merge conflicts occur because main added `stripThinkTags()` in the same function, but the quote-stripping logic is identical and fully functional.

### Recommendation
Delete the worktree and branch. The fix was independently reimplemented (or cherry-picked) as part of another change that also added think-tag stripping. The quote-stripping behavior is live on `origin/main`.

---

## Worktree 18: freshagent-tool-attribution (Jun 13)

**Branch:** `freshagent-tool-attribution`
**Verdict: Finish work**

### Commits
```
cce6d98e Filter Claude skill payload user turns
05593ce1 Fix fresh-agent tool result attribution
```

### Diff Summary
```
server/coding-cli/utils.ts                         |   2 +
server/fresh-agent/adapters/claude/normalize.ts    | 112 +++++++++-----
src/components/fresh-agent/FreshAgentTranscript.tsx |  25 ++++
FreshAgentTranscript.test.tsx                       | 122 +++++++++++++++
test/unit/server/coding-cli/utils.test.ts           |  20 +++
claude-normalize.test.ts                            |  72 ++++++++++
6 files changed, 323 insertions(+), 30 deletions(-)
```

### Evidence
- **Nothing from this worktree is on main:**
  - `isSyntheticToolResultTurn()`, `appendTurnItems()`, `isSyntheticUserTimelineItem()` — zero results across `src/` and `server/`.
  - `normalizeClaudeTurn()` on main returns `FreshAgentNormalizedTurn` (not `FreshAgentNormalizedTurn | null` as the worktree changes it to). The `null` return for empty user turns was never adopted.
  - `isSystemContext()` check for `"Base directory for this skill:"` — not present in `server/coding-cli/utils.ts` on main.
  - `extractUserAuthoredText()` is not used anywhere in `server/fresh-agent/adapters/claude/normalize.ts` on main.
  - None of the 3 new test files/sections exist on main.
- **Merge conflicts:** `server/fresh-agent/adapters/claude/normalize.ts` and `FreshAgentTranscript.tsx` and `FreshAgentTranscript.test.tsx` all conflict with main due to the transcript contract PR (#452), which rewrote significant portions of the same files.
- **The problem is real:** Claude Code injects skill instructions (e.g., fresheyes) as user-role context messages. Without filtering, these appear as synthetic "You" turns in the transcript. Tool results from these skills appear as separate user turns (with `kind: 'tool_result'`) instead of being attributed to the assistant.
- **Test coverage is excellent:** 6 test cases covering skill payload dropping, user-role tool result folding into assistant activity, adjacent tool-use/result coalescing, and `isSystemContext` detection of skill instruction payloads.
- **Needs rebasing work:** The transcript contract PR changed how turns are normalized at the server level. The tool-attribution approach (filtering in `normalizeClaudeTurn` + appending in frontend `coalesceActivityOnlyTurns`) may need to be reconciled with the contract PR's approach. A fresh subagent should assess whether the problems still manifest on main (by running a Claude session that invokes a skill and checking the transcript), then rebase accordingly.

### Recommendation
This is significant, well-tested work that solves a real problem. It needs to be rebased onto the current `origin/main` and reconciled with the transcript contract PR's changes. After rebase, verify:
1. That Claude skill payloads still appear as "You" turns on main (likely yes — the contract PR didn't add this filtering)
2. That tool results are still misattributed (likely yes — `isSyntheticToolResultTurn` is unique to this branch)
3. That the `normalizeClaudeTurn` signature change (returning `null`) doesn't break any contract PR behavior

Assign this as **Finish work** — the implementation is sound but needs integration effort.

---

## Worktree 19: freshagent-serif-full-style (Jun 13)

**Branch (actual):** `freshagent-transcript-no-auto-collapse`
**Verdict: Throw away - in main already**

### Commits
```
9a5cea4e Fix fresh agent transcript auto-collapse regression
```

### Diff Summary
```
FreshAgentTranscript.tsx           | 235 ++++++++--------
FreshAgentView.tsx                 |  69 ++++-
test/e2e-browser/specs/fresh-agent.spec.ts |   3 +-
FreshAgentTranscript.test.tsx      | 157 +++++++++-
FreshAgentView.test.tsx            | 294 +++++++++++++++++++++
5 files changed, 621 insertions(+), 137 deletions(-)
```

### Evidence
- **Core refactors partially on main:**
  - `getSnapshotIdentity()`, `isSnapshotInFlight()`, `mergeSnapshotForDisplay()` — all present on main at `FreshAgentView.tsx:60-137`.
  - `snapshotRef` pattern (tracking snapshot across re-renders in a ref + committing via `commitSnapshot` callback) — present on main at `FreshAgentView.tsx:410`.
  - The `FreshAgentView` tests "preserves loaded transcript history...", "replaces prior history...", "ignores an older same-session revision..." — all present on main in `FreshAgentView.test.tsx`.
- **Auto-collapse removal on main:**
  - `CollapsedFreshAgentTurn` component, `collapsedCutoff` logic — NOT present on main (the collapse feature was removed).
  - Test "keeps completed long transcripts expanded instead of replacing older turns with summary rows" — on main at `FreshAgentTranscript.test.tsx:543`.
  - Test name changed from "strips system reminders and **collapses** older turns" to "strips system reminders **without collapsing** older turns" — the latter is on main at line 727.
  - `CoalesceActivityOnlyTurns()`, `mergeActivityOnlyTurns()`, `isActivityOnlyTurn()` — not on main; the transcript contract PR used a different approach for turn display.
- **Heavy merge conflicts:** `FreshAgentTranscript.tsx`, `FreshAgentView.tsx`, and `FreshAgentTranscript.test.tsx` all conflict.
- **Transcript contract PR (#452)** landed the fix for the auto-collapse regression through a different implementation. The key behaviors (no auto-collapse, snapshot merge for in-flight turns, all 10 turns always visible) are all present on main.

### Recommendation
Delete the worktree and branch. The regression was fixed by the transcript contract PR (#452) which landed on `origin/main` on Jun 17. The worktree's implementation was a valid approach but has been superseded by a more comprehensive refactor. All testing evidence confirms the behaviors this branch aimed to fix are now correct on main.
