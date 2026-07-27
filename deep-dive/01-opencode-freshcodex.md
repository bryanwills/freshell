# Worktree Deep-Dive Analysis: OpenCode & FreshCodex Branches

---

## Worktree 1: `repro-freshopencode-playwright` (branch `debug/freshopencode-playwright-repro-dev`)

**Verdict: Throw away - in main already**

### Evidence

**Commits (vs origin/main):**
```
489c2918 fix: keep freshopencode first send visible across reload
88297dc1 test: repro freshopencode first-send reload loss
```

**Diff stat (origin/main..HEAD):** 10 files changed, 550 insertions(+), 42 deletions(-)

**Test results:** N/A (worktree content identical to origin/main)

**Main-line comparison:**
- Main contains PR #454 (`80319767` merge of `debug/freshopencode-playwright-repro`) with commits `ea7265f4` + `26bbd22e`
- The `-dev` branch and the `-repro` branch produce identical trees (`da3d834d18f9a7baee1cc74fa8c9908a7c7707cc`)
- Both `489c2918` (dev) and `ea7265f4` (repro) have the same diff stat: 9 files, 305 insertions, 43 deletions
- `git diff 80319767 2c5432c6 --stat` produces zero output (same tree)

**JSONL findings:** No JSONL history found in `~/.claude/` or `~/.config/opencode/`

### Recommendation Narrative

Two parallel branches were created to fix the same bug — `debug/freshopencode-playwright-repro` and `debug/freshopencode-playwright-repro-dev`. Both PRs were opened (#454 and #455) with the same tree content. PR #454 was merged to `origin/main`; PR #455's merge commit (`2c5432c6`) shares the same tree but was never incorporated into the main line ancestry. The worktree is checked out at the `-dev` branch HEAD (`489c2918`) which is NOT an ancestor of `origin/main`. However, every byte of its content is already on main via PR #454. This worktree should be deleted; the branch can be force-pushed to retire it on the remote.

---

## Worktree 2: `debug-freshcodex-cwd` (branch `debug-freshcodex-cwd`)

**Verdict: Throw away - in main already**

### Evidence

**Commits (vs origin/main):**
```
13cff1f3 fix: preserve fork cwd fallbacks
d50146d9 test: cover freshopencode real cwd materialization
a5297713 fix: route freshopencode operations with pane cwd
de533de4 fix: scope opencode serve processes by cwd
aa565760 test: tighten freshopencode serve cwd contract
86453cb5 test: pin freshopencode serve cwd contract
6f8a883d docs: reconcile freshopencode plan snippets
ebbed5ee docs: address freshopencode plan review
fdc3d3de docs: harden freshopencode cwd plan
eca9391a docs: plan freshopencode cwd-scoped serve
212b2d7a Fix freshcodex restored cwd handling
```

**Diff stat (origin/main..HEAD):** 22 files changed, 2224 insertions(+), 145 deletions(-)

**Main-line comparison:**
- Main commit `45efc524` ("Fix fresh agent cwd routing (#450)") is a squash merge containing ALL these changes
- `git diff 13cff1f3 45efc524` produces zero output — identical tree (`4ebcc3ac49353d4832c3bc85d0f873b63ff188b2`)
- Commit `45efc524` IS an ancestor of `origin/main`; `13cff1f3` is NOT
- The squash commit message literally lists every commit message from the branch

**JSONL findings:** None found

### Recommendation Narrative

This branch introduced cwd-scoped serve routing — spawning separate `opencode serve` sidecar processes per directory and routing operations with `{ directory: cwd }` — across 22 files with full test coverage. It was squashed and merged to main as commit `45efc524` in PR #450 on Jun 19. The worktree's HEAD (`13cff1f3`) shares an identical tree with that squash commit, but its unique branch commits are not ancestors of `origin/main`. The work is fully landed on main; this worktree and branch are obsolete. Notably, the `rollback/opencode-sidecars` branch (see below) later supersedes this architectural approach entirely by reverting to a single sidecar with query-based routing.

---

## Worktree 3: `rollback-opencode-sidecars` (branch `rollback/opencode-sidecars`)

**Verdict: Ready for landing**

### Evidence

**Commits (vs origin/main):**
```
58714d76 fix: reap opencode sidecar on message timeout
4dadad4c test: cover opencode routed cwd smoke
6f0051ed fix: validate freshopencode cwd before materialization
ed25fed7 fix: consume opencode global event stream
e32bedbb fix: route opencode requests by directory query
073b2e61 plan: opencode single sidecar routing
f6b060fa revert: remove opencode cwd sidecars
```

**Diff stat (origin/main..HEAD):** 9 files changed, 1074 insertions(+), 1594 deletions(-)

**Test results:**
- `test/unit/server/fresh-agent/opencode-serve-manager.test.ts` — all tests pass
- `test/unit/server/fresh-agent/opencode-serve-adapter.test.ts` — all tests pass
- `test/unit/server/fresh-agent/opencode-serve-events.test.ts` — all tests pass
- 11 test files, 205 tests total — PASS
- `npm run typecheck` — PASS (no errors)

**Main-line comparison:**
- NOT on `origin/main` (merge-base check confirms `58714d76` is not an ancestor)
- No superseding work found — this is the latest iteration of the opencode routing architecture
- Other opencode-related work on main (PR #450, #451, #452, #454) are prior phases that this branch builds on top of

**Architectural change:**
The branch reverts the multi-sidecar approach introduced in PR #450 and replaces it with:
1. **Single shared `opencode serve` process** instead of one per cwd
2. **Directory query routing** (`?directory=/path` appended to serve API URLs) instead of spawning separate processes per project
3. **Global event stream consumption** (`/global/event`) instead of per-sidecar streams
4. **Cwd validation** before materializing sessions
5. **Simpler lifecycle** — one `running`/`startPromise` instead of maps; no `sessionCwdById`/`cwdByKey` mappings

### Recommendation Narrative

This is the most mature of the three worktrees — it represents a conscious architectural pivot from the cwd-sidecar pattern (PR #450) to a single-sidecar model. The commit narrative is clear and logical: revert the old approach, add a plan, implement directory-query routing, consume global events, validate cwds, add comprehensive tests, and handle message timeout reaping.

The codebase changes are well-structured:
- `serve-manager.ts` loses ~130 lines of multi-sidecar complexity (maps, per-cwd lifecycle, cwd-to-id bookkeeping)
- `serve-events.ts` properly unwraps `/global/event` payloads and filters server control frames
- `adapter.ts` uses `cwdRoute()` to tag all serve API calls with directory context; adds `validateCwd` hook
- Tests are comprehensively updated: removed tests that asserted multi-sidecar behavior, added tests for single-sidecar routing, URL encoding, global event consumption, and cwd validation

All 205 unit tests pass and the type checker passes with zero errors. The work appears complete, well-tested, and architecturally sound.

**One caution:** The real-provider smoke test (`opencode-serve-real-provider-smoke.test.ts`) cannot be run in this environment (requires actual `opencode` binary on PATH). The test coverage in the unit tests is thorough, but the integration test would need a real opencode serve to validate the global event stream consumption and session routing end-to-end.

If the user agrees, this branch is ready to push, submit as a PR targeting `main`, and land.
