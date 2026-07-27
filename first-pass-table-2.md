# Worktree First-Pass Audit — Batch 2

Generated: 2026-06-23

## Ancestor=NO Worktrees (5)

| # | Name | Branch | Date | Ahead/Behind | Meaningful? | Summary |
|---|------|--------|------|--------------|-------------|---------|
| 1 | fresh-agent-parity-audit | feat/fresh-agent-parity-audit | Jun 23 | 1 / 0 | **YES** | 1 commit closing fresh-agent parity gaps (codex/opencode adapters) with +168/-5 across 8 files (mostly tests); clean tree, 0 behind — likely ready to land. |
| 2 | fresh-agent-turn-complete | fix/fresh-agent-server-authoritative-completion | Jun 23 | 7 / 3 | **YES** | 7 commits implementing server-authoritative turn completion for fresh-agent (codex+opencode); +1289/-63 across 23 files with comprehensive tests; 3 behind needs rebase but substantial, near-complete feature. |
| 3 | fresh-agent-rehydration-fix | plan/fresh-agent-rehydration-fix | Jun 22 | 3 / 33 | **NO** | 3 commits, all docs-only — a 2559-line planning document with zero implementation; 33 behind and stale. |
| 4 | fresh-agent-progressive-hydration | feature/fresh-agent-progressive-hydration | Jun 21 | 6 / 47 | **YES** | 6 commits, +3165/-2477 across 47 files; was merged as PR #468 (d9bdc212) then reverted via PR #470 (4e88560a "Revert fresh-agent progressive hydration") — implementation exists here but is NOT live on main; 47 behind and needs investigation of revert rationale. |
| 5 | dev-stack-main-trial | integration/main-to-tested-dev-stack-20260520 | May 20 | 1 / 508 | **NO** | 1 commit bringing main to a tested dev integration stack; +44769/-8187 across 282 files but 508 commits behind — this is a historical integration snapshot fully superseded by main's evolution. |

## Ancestor=YES Dirty Worktrees (2)

| # | Name | Branch | Date | Ahead | Working Tree | Meaningful? | Summary |
|---|------|--------|------|-------|--------------|-------------|---------|
| 6 | freshagent-header-bar | freshagent-header-bar | — | 0 | 3 modified (uncommitted) | **YES** | Uncommitted work generalizing fresh-agent runtime meta to all providers (not just claude) in PaneContainer, plus new FreshAgentToolIcons header showing active tool icons; ~80 lines of source + test updates — lost if deleted. |
| 7 | fix-codex-sidecar-build | fix/codex-sidecar-build | — | 0 | 4 modified + 4 untracked (uncommitted) | **YES** | Uncommitted work building a new codex sidecar abstraction: 2 new source files (sidecar.ts 218 lines, durable-rollout-tracker.ts 250 lines) + 2 new test files (577 lines) + 4 modifications (env passthrough, restore-error removal); ~1045 lines of new code/tests — substantial lost work if deleted. |

## Revert Verification: fresh-agent-progressive-hydration

The progressive hydration work WAS reverted on `origin/main`:
- **Merged:** `d9bdc212` — "Fresh-agent progressive hydration (#468)" (PR #468)
- **Reverted:** `4e88560a` — "Revert fresh-agent progressive hydration" (via PR #470, merge `a4c75e14`)

Net effect: the feature is NOT live on main. The full implementation (6 commits, 47 files) exists only in this worktree.
