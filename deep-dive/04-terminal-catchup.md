# Deep Dive: Terminal Catch-Up Worktrees

## 23. proof-terminal-catchup-architecture

**Verdict: Throw away — in main already**

- **Branch:** proof-terminal-catchup-architecture
- **Commit:** `ad9fbeac6` — "Add terminal catch-up proof dossier"
- **Head SHA:** ad9fbeac66a75ca6f751d06da6a47948940d42d2
- **Merge-base test:** `git merge-base --is-ancestor ad9fbeac66 origin/main` → NOT IN MAIN (different SHA)

**Evidence:**

| Worktree | Main (PR) |
|---|---|
| `ad9fbeac6` | `6f74e6f79` |
| Author: danshapiro | Author: Codex |
| 14 files, +2920 | 14 files, +2920 |
| Same files, same content | Same files, same content |

`git diff ad9fbeac66 6f74e6f799` shows **zero diff in shared files** — the only delta is a 2610-line planning doc (`docs/superpowers/plans/2026-06-08-terminal-catchup-stream-safety.md`) that was added to main *after* the dossier commit. This is the exact same research artifact content.

**Content:** Pure research/evidence — probe scripts, JSON metrics artifacts, and an architecture dossier analyzing terminal catch-up behavior, browser lifecycle, PTY metrics, and xterm write disposal. Contains no production code. The 416-line dossier systematically proves/disproves assumptions and produces a detailed implementation plan with 11 tasks for terminal catch-up stream safety.

The research was valuable and directly informed subsequent work:
- 4 related PRs (#383, #386, #396, #397) from sibling worktrees
- Numerous main commits (30+) hardening terminal replay, batching, retention, backpressure, and observability

---

## 24. fix-terminal-catchup

**Verdict: Throw away — in main already**

- **Branch:** fix-terminal-catchup
- **Commit:** `d5e33daeb` — "Speed up terminal replay catch-up"
- **Merged as:** `463eff2f6` — PR #396
- **Files:** 4 files: `TerminalView.tsx`, `terminal-write-queue.ts`, `terminal-create-attach-ordering.test.tsx`, `terminal-write-queue.test.ts`
- **Stats:** +225/-14 — identical to main

**Evidence:** `git merge-base --is-ancestor d5e33daeb origin/main` → NOT IN MAIN (SHA differs). But `git log origin/main --oneline --all --grep="Speed up terminal replay"` finds `463eff2f Speed up terminal replay catch-up (#396)` with identical stats and same files.

The terminal-write-queue changes (coalescing replay writes within a 32ms budget, mode-aware queuing) are present on main.

---

## 25. fix-replay-server-batching

**Verdict: Throw away — in main already**

- **Branch:** fix-replay-server-batching
- **Commit:** `fbc9291b2` — "Coalesce terminal replay batches server-side"
- **Merged as:** `6faf470e3` — PR #397
- **Files:** 6 files: `replay-ring.ts`, `terminal-write-queue.ts`, `ws-terminal-stream-v2-replay.test.ts`, `terminal-write-queue.test.ts`, `replay-ring.test.ts`, `ws-handler-backpressure.test.ts`
- **Stats:** +103/-23 — identical to main

**Evidence:** Matching PR commit with identical files and line counts. Server-side batching in `replay-ring.ts` and removal of the client-side `replayBudgetMs` from the write queue are on main.

---

## 28. warm-tab-delta-replay

**Verdict: Throw away — in main already**

- **Branch:** fix/warm-tab-delta-replay
- **Commit:** `1c2632add` — "fix terminal warm tab replay and backpressure"
- **Merged as:** `dd12912b0` — PR #386
- **Files:** 14 files: broker.ts, constants.ts, replay-ring.ts, types.ts, ws-handler.ts, ws-protocol.ts, TerminalView.tsx, hydration-queue.ts, terminal-attach-policy.ts, plus 5 test files and a markdown plan
- **Stats:** +918/-80 — identical to main

**Evidence:** Matching PR commit with identical stats. Major architectural changes including broker warm replay logic, replay-ring delta snapshots, terminal-attach-policy, and extensive backpressure tests.

---

## 30. fix-mobile-scroll

**Verdict: Throw away — in main already**

- **Branch:** fix/mobile-opencode-touch-scroll
- **Commit:** `34e293c3d` — "fix(terminal): enable touch-scroll in alternate buffer with mouse tracking"
- **Merged as:** `2c8978e83` — PR #383
- **Files:** 3 files: `TerminalView.tsx`, `opencode-touch-scroll-input-policy.test.tsx`, `TerminalView.touch-scroll-input-policy.test.tsx`
- **Stats:** +297/-12 — identical to main

**Evidence:** Matching PR commit with identical stats. Touch-scroll in alternate buffer with mouse tracking — synthetic WheelEvents dispatched for mobile touch when native scrollInputPolicy is active.

---

## Summary

| # | Worktree | Verdict | Main PR/Commit | Notes |
|---|---|---|---|---|
| 23 | proof-terminal-catchup-architecture | Throw away — in main already | `6f74e6f7` | Research dossier already merged (different author, same content) |
| 24 | fix-terminal-catchup | Throw away — in main already | #396 (`463eff2f`) | Write-queue replay coalescing |
| 25 | fix-replay-server-batching | Throw away — in main already | #397 (`6faf470e`) | Server-side batch coalescing |
| 28 | warm-tab-delta-replay | Throw away — in main already | #386 (`dd12912b`) | Warm replay + backpressure fix |
| 30 | fix-mobile-scroll | Throw away — in main already | #383 (`2c8978e8`) | Touch-scroll in alt buffer |

All 5 worktrees have their changes already integrated on `origin/main` through PRs. The research dossier from #23 and the 4 tactical fixes from #24-30 are all present. The 11-task architectural plan from the dossier was never executed as a single branch, but its recommendations were incrementally implemented across ~30+ subsequent main commits (barrier-aware batching, serialized byte budgets, structured observability, replay safety hardening, etc.).
