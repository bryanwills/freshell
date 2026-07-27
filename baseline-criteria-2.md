# Baseline Criteria — Audit #2 (Jun 23, 2026)

## Scope
All worktrees in `.worktrees/`. No date filter — full sweep.

## Summary
- **Total worktrees:** 67 (including main + analysis)
- **Ancestor=YES (already on main):** 61
- **Ancestor=NO (novel work):** 5
- **Dirty ancestor=YES worktrees:** 9 (uncommitted changes, mostly minor)

## Novel Worktrees (ancestor=NO) — Category C: Deep-Dive Required

| # | Worktree | Branch | Date | Ahead | Behind | Dirty | Notes |
|---|---|---|---|---|---|---|---|
| 1 | fresh-agent-parity-audit | feat/fresh-agent-parity-audit | Jun 23 | 1 | 0 | 0 | 0 behind — likely ready |
| 2 | fresh-agent-turn-complete | fix/fresh-agent-server-authoritative-completion | Jun 23 | 7 | 3 | 0 | Active WIP, fresheyes rounds |
| 3 | fresh-agent-rehydration-fix | plan/fresh-agent-rehydration-fix | Jun 22 | 3 | 33 | 0 | Plan only (2559 lines docs) |
| 4 | fresh-agent-progressive-hydration | feature/fresh-agent-progressive-hydration | Jun 21 | 6 | 47 | 0 | Landed as PR #468, reverted via PR #470 |
| 5 | dev-stack-main-trial | integration/main-to-tested-dev-stack-20260520 | May 20 | 1 | 508 | 0 | Ancient relic, 508 behind |

## Dirty Ancestor=YES Worktrees — Category B: First-Pass Check

| Worktree | Dirty | What |
|---|---|---|
| freshagent-header-bar | 3 | Modified PaneContainer.tsx, PaneHeader.tsx, test — uncommitted code |
| fix-codex-sidecar-build | 5 | Modified terminal-registry, TerminalView, tests + new durable-rollout-tracker.ts |
| electron-windows-native | 64 | Many modified assets/configs — likely build artifacts |
| build-new-favicon | 16 | Modified icon assets — likely uncommitted binaries |
| fix-freshagent-ui-details | 1 | Untracked docs/design/ dir |
| deflake-terminal-refresh | 1 | Untracked plan doc |
| rebuild | 1 | Modified electron-builder.yml |
| origin-main-smoke | 1 | Modified test file |

## Auto-Skip (ancestor=YES, clean, 0 ahead) — Category A
52 worktrees already on main with clean working trees. Safe to delete.
