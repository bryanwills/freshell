# Worktree First-Pass Analysis

Generated: 2026-06-21

Key:
- **Ancestor?** — Is HEAD an ancestor of `origin/main`?
- **In Main?** — Does HEAD appear in `origin/main`?
- **Status** — `clean` or summary of uncommitted/untracked files
- **Commits** — Count of commits on `origin/main..HEAD`
- **Meaningful?** — Does the worktree contain novel code/feature work (not just plans/docs/trivial fixes)?

## Table

| # | Worktree | Branch | Date | Ancestor? | In Main? | Status | Commits | Files Δ | Meaningful? | Summary |
|---|----------|--------|------|-----------|----------|--------|---------|---------|-------------|---------|
| 1 | rollback-opencode-sidecars | rollback/opencode-sidecars | 2026-06-21 | no | no | clean | 7 | 9 | **yes** | Reverts opencode cwd sidecars in favor of single-sidecar routing; extensive adapter/serve-manager refactor with tests |
| 2 | repro-freshopencode-playwright | debug/freshopencode-playwright-repro-dev | 2026-06-19 | no | no | clean | 6 | 10 | **yes** | Fixes freshopencode first-send visibility across reload with e2e reproducer and server-side changes |
| 3 | plan-fresh-agent-transcript-contract | plan/fresh-agent-transcript-contract | 2026-06-19 | no | no | clean | 8 | 1 | **no** | Plan document only (685-line design doc for transcript contract) |
| 4 | fix-mobile-longpress-menu | fix-mobile-longpress-menu | 2026-06-19 | no | no | clean | 1 | 2 | **yes** | Fix for mobile long-press context menu with test (46 lines changed) |
| 5 | debug-freshcodex-cwd | debug-freshcodex-cwd | 2026-06-19 | no | no | clean | 10 | 22 | **yes** | Major cwd-scoped serve routing changes across server, client, and tests; 2224 insertions |
| 6 | port-glm-5.2-to-dev | port/glm-5.2-to-dev | 2026-06-18 | no | no | clean | 3 | 2 | **yes** | Adds GLM 5.2 model to freshopencode models (36 lines) |
| 7 | investigate-bouncer | investigate-bouncer | 2026-06-18 | no | no | clean | 3 | 1 | **no** | Plan document only (905-line plan for freshopencode bouncer status fix) |
| 8 | fresh-agent-thinking-muted-color | fix/fresh-agent-thinking-muted-color | 2026-06-18 | no | no | clean | 1 | 2 | **yes** | Renders thinking text in muted color; CSS + e2e tests (168 lines) |
| 9 | opencode-think-normalization | fix/opencode-think-normalization | 2026-06-17 | no | no | clean | 1 | 2 | **yes** | Fixes leaked think normalization in opencode adapter with tests (258 insertions) |
| 10 | opencode-refresh-restore-white-page | fix/opencode-refresh-restore-white-page | 2026-06-17 | no | no | clean | 9 | 4 | **yes** | Fixes white page on opencode refresh; includes plan, e2e tests, and TerminalView changes (734 insertions) |
| 11 | find-bug | find-bug | 2026-06-17 | no | no | clean | 1 | 2 | **no** | Trivial test-only change making shared-state websocket suites sequential (2 lines changed) |
| 12 | electron-modifier-link-external | electron-modifier-link-external | 2026-06-17 | no | no | clean | 8 | 16 | **yes** | Adds ctrl/shift+click external link opening in system browser; electron IPC validation + tests (612 insertions) |
| 13 | opencode-playback-dev-pr | test/opencode-playback-coalescing-dev | 2026-06-15 | no | no | clean | 1 | 6 | **yes** | Fix for OpenCode replay playback coalescing; plan doc + TerminalView changes + e2e tests (1206 insertions) |
| 14 | opencode-playback-coalescing | test/opencode-playback-coalescing | 2026-06-15 | no | no | clean | 10 | 6 | **yes** | Same fix as above with granular planning commits; includes merge from origin/main (1206 insertions) |
| 15 | fix-freshagent-user-message-quotes | fix-freshagent-user-message-quotes | 2026-06-15 | no | no | clean | 2 | 2 | **yes** | Strips surrounding quotes from user message text in OpenCode adapter; normalize + tests (65 insertions) |
| 16 | durable-ws-reconnect | plan/durable-ws-reconnect | 2026-06-15 | no | no | clean | 10 | 2 | **no** | Plan documents only (websocket recovery + legacy codingcli removal, 2200 insertions) |
| 17 | freshagent-user-jump-plan | freshagent-user-jump-plan | 2026-06-14 | no | no | clean | 5 | 1 | **no** | Plan document only (1908-line plan for user message jump navigation) |
| 18 | freshagent-tool-attribution | freshagent-tool-attribution | 2026-06-13 | no | no | clean | 2 | 6 | **yes** | Filters claude skill payload user turns; fixes tool result attribution across server and frontend (323 insertions) |
| 19 | freshagent-serif-full-style | freshagent-transcript-no-auto-collapse | 2026-06-13 | no | no | clean | 1 | 5 | **yes** | Fixes transcript auto-collapse regression; major FreshAgentTranscript/FreshAgentView refactor + tests (621 insertions) |
| 20 | new-settings-ui | new-settings-ui | 2026-06-12 | no | no | clean | 2 | 42 | **yes** | Major settings UI refactor; splits settings into organized tabs (Coding Agents, Devices, Naming, Panes, Runtime) with tests (3416 insertions) |
| 21 | fix-electron-launch-chooser | fix/electron-launch-chooser-flow | 2026-06-10 | no | no | clean | 24 | 49 | **yes** | Fixes electron launch chooser; IPC validation, port collision handling, provisioning hardening, test coverage (1441 insertions) |
| 22 | codex-interrupt-freeze | codex-interrupt-freeze | 2026-06-10 | no | no | clean | 1 | 2 | **yes** | Fixes Codex interrupt freeze by acking duplicate interrupts after completion (120 insertions) |
| 23 | proof-terminal-catchup-architecture | proof-terminal-catchup-architecture | 2026-06-08 | no | no | clean | 1 | 14 | **yes** | Terminal catch-up evidence dossier with metrics probes, browser lifecycle probes, and serialization analysis (2920 insertions) |
| 24 | fix-terminal-catchup | fix-terminal-catchup | 2026-06-07 | no | no | clean | 1 | 4 | **yes** | Speeds up terminal replay catch-up with write-queue changes and tests (225 insertions) |
| 25 | fix-replay-server-batching | fix-replay-server-batching | 2026-06-07 | no | no | clean | 1 | 6 | **yes** | Coalesces terminal replay batches server-side with replay-ring changes and tests (103 insertions) |
| 26 | tab-status-reliability | fix/tab-status-reliability | 2026-06-04 | no | no | clean | 20 | 66 | **yes** | Major tab status reliability feature; green/sound bridge, busy indicators, turn-complete tracking, extensive test coverage (3201 insertions) |
| 27 | plan-opencode-marker-cache | perf/opencode-marker-cache | 2026-06-04 | no | no | clean | 2 | 18 | **yes** | Implements off-thread OpenCode listing worker; includes plan doc, listing query/runner/worker, and tests (2704 insertions) |
| 28 | warm-tab-delta-replay | fix/warm-tab-delta-replay | 2026-06-03 | no | no | clean | 1 | 14 | **yes** | Fixes warm tab replay and backpressure; broker, replay-ring, TerminalView changes, and tests (918 insertions) |
| 29 | disable-superpowers-plugin | chore/disable-superpowers-plugin | 2026-06-03 | no | no | clean | 1 | 1 | **no** | Trivial config change disabling superpowers plugin (1 insertion, 3 deletions in `.claude/settings.json`) |
| 30 | fix-mobile-scroll | fix/mobile-opencode-touch-scroll | 2026-06-02 | no | no | clean | 1 | 3 | **yes** | Enables touch-scroll in alternate buffer with mouse tracking; input policy and tests (297 insertions) |
| 31 | docs-ci-merge-gate | docs/ci-merge-gate-note | 2026-05-30 | no | no | clean | 1 | 1 | **no** | Trivial docs-only change at `docs/development/windows-electron-build.md` (21 lines) |
| 32 | agent-chat-spec-plan | codex/agent-chat-spec-plan | 2026-05-29 | no | no | clean | 4 | 1 | **no** | Plan document only (823-line design doc for agent chat spec split) |
