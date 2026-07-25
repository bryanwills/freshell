# Dead Restore-Code Deletion Implementation Plan

> **For agentic workers:** This plan is executed task-by-task by the
> workflow's execute stage: a fresh implementer per task, with a spec +
> quality review after each task. Steps use checkbox (`- [ ]`) syntax
> for tracking.

**Goal:** Delete the restore-related machinery in the Rust freshell port that has zero non-test callers (campaign plan item P0.5, scoped subset) — it reads as the restore path and is not, and must be removed before new restore work reads the wrong code as the real path.

**Architecture:** Three surgical deletions across two crates (`freshell-activity`, `freshell-codex`), each proven dead first (LSP `findReferences` / exhaustive grep), then removed together with its tests and `pub use` exports, then proven harmless (`cargo check` + crate tests), with a full coordinated verification at the end. No new code is written; the "TDD" discipline here is the deletion form: prove dead → delete → prove still green.

**Tech Stack:** Rust (cargo workspace at the worktree root, 12 member crates), rust-analyzer LSP, ripgrep, npm-coordinated repo test suite.

## Global Constraints

- **Worktree:** all work happens in `/home/dan/code/freshell/.worktrees/dead-restore-code-deletion` on branch `chore/dead-restore-code-deletion` (already created off `origin/main`). All paths below are relative to this worktree root; run all commands from it.
- **DO NOT TOUCH (other agents own these / scheduled work):**
  - `crates/freshell-ws/**` (an agent is working in `terminal.rs` concurrently) and `crates/freshell-protocol/**` — leave both crates entirely alone.
  - The create-message wire fields `liveTerminal` / `codexDurability` / `recoveryIntent` and the `terminal.codex.candidate.persisted` protocol message — being WIRED by a parallel task, not deleted.
  - The reconcile/existence subsystem (`reconcile.rs`, `existence.rs`, ~1,400 lines) — DORMANT, scheduled for activation at campaign item P1.7. Not dead. Do not delete.
  - gemini/kimi code — removed by a separate effort.
  - `ClaudeActivityTracker::bind_session` (`crates/freshell-activity/src/claude.rs:114-124`) and `AmplifierActivityTracker::bind_session` — out of scope (Claude's has a test; Amplifier's is live).
  - All TypeScript (`server/`, `src/`, `shared/`, `test/`) — the TS symbols with similar names are the legacy reference implementation, not callers of the Rust code.
- **HALT RULE (per spec):** before deleting each symbol, re-verify it has zero non-test callers *at execution time* (a parallel task may have landed a caller since this plan was written). If a symbol has a real production caller: STOP deleting that symbol, keep it (and anything it needs), and record it in the Task 4 inventory. Do not force it.
- **Feature flags:** `launch_lifecycle` (a consumer of `launch_plan`) is behind `#[cfg(feature = "real-transport")]`. A default-feature check is insufficient — always use `--all-features` on `freshell-codex` checks/tests.
- **Process safety:** NEVER restart the user's self-hosted freshell server (historically pid 1262455 on port 3001 — the pid may have changed or the server may be down; the rule holds regardless), never bind :3001, never use broad kill patterns (`pkill node`, `pkill -f vite`, etc.).
- **Coordinated suite:** the full repo suite runs only via the coordinator gate: check `npm run test:status` first; if another agent holds the gate, WAIT (poll; never kill the holder). Run as `FRESHELL_TEST_SUMMARY="..." npm test`.
- **Docs:** do NOT commit `/home/dan/code/freshell/docs/plans/2026-07-24-restart-resilience-architecture-analysis.md` (untracked on main; read-only reference). Create no new markdown files beyond this plan.
- **PR policy:** PR creation is NOT user-approved. After verification is green: commit, push the branch, then STOP before `gh pr create` and report branch name + deletion inventory.
- **Line numbers** below were verified on 2026-07-24 at the branch point. This worktree is exclusive to this task (verified: clean tree, single checkout of the branch, only the plan commit beyond base) so they should hold, but always locate regions by the quoted symbol names / marker comments, not by line number alone, and re-check boundaries before deleting.

### Validated baseline facts (load-bearing validation, 2026-07-24 — evidence in `.worktrees/.the-usual-logs/dead-restore-code-deletion/load-bearing-ledger.md`)

- **origin/main == branch point a53f185a** at validation time — no commit after the branch point exists, so no caller can have landed yet. A mandatory pre-push re-check (Task 4 Step 5) guards the window between now and push.
- **The parallel codexDurability wiring task does not consume the restore-decision symbols** (campaign doc §2.3.1 routes `terminal.codex.candidate.persisted` → identity registry, around the decision table; all 11 symbols grepped across docs, worktree plans, worktree crates, and open/merged PRs — zero consumers). Task 3's execution-time HALT re-check remains mandatory anyway.
- **`cargo check --workspace --all-features --all-targets` runs green on this host** (12.19s warm) — the Task 4 gate is viable as written. It emits **4 pre-existing warnings in `freshell-server`** (dead_code: `DEFAULT_CLI_DETECTION_SPECS`, `has_cli`, `get`/`get_all`) — these are baseline, NOT caused by this deletion; only a *new* warning is attributable. `cargo fmt --check` and both clippy `-D warnings` gates are fully green at base.
- **`cargo doc -p freshell-codex --all-features --no-deps` exits 0 at base but emits 4 pre-existing warnings** (3× `rustdoc::private_intra_doc_links` in remote_proxy/status/json_scan areas + 1× redundant explicit link target in transport.rs). Expectation everywhere below = exit 0, no `broken_intra_doc_links`, no NEW warnings; the 4 baseline warnings persist and are fine.
- **`cargo test -p freshell-codex --all-features` is hermetic and host-safe** (inspected: loopback-ephemeral binds only, the sole spawn is a committed node fixture killed as its own child, the /proc reaper matches only a per-run UUID env needle, no real user-data access) and is **green at base: 190/190 tests** (lib 149; integration: app_server_drive 4, completion_gating 3, interrupt_rpc 1, launch_lifecycle 16, remote_proxy_relay 17). The launch_lifecycle spawn test requires `node` on PATH and the `ws` npm module resolvable (currently satisfied via the parent checkout's node_modules); if that breaks, the failure is environmental, not deletion-caused.
- **The coordinated npm suite recorded a green full-suite run at exactly a53f185a on 2026-07-24 on this host** (coordinator store: `full-suite success exit=0`) — that record is the baseline for Task 4 Step 4. `npm test` runs vitest only (no cargo, no Rust binary, no :3001 contact; coordinator mutex is a unix socket). The host's WSL UDP-port-exhaustion failure mode is still live (remediation unimplemented; Tcpip event 4266 fires daily) — see Task 4 Step 4 triage guidance.

---

## File Structure

No files are created (besides this plan). Files modified:

| File | Change | Responsibility |
|---|---|---|
| `crates/freshell-activity/src/codex.rs` | delete `CodexActivityTracker::bind_session` (12 lines); 1-sentence doc addendum | codex PTY-lane activity tracker |
| `crates/freshell-codex/src/durability.rs` | delete `default_durability_store_dir`, `home_dir`, `DurabilityCandidate`, `CandidateImmutableError` + impls + 1 test + stale doc paragraph (~112 lines) | codex thread-id / sidecar-ownership helpers (live parts stay) |
| `crates/freshell-codex/src/lib.rs` | remove 3 names from the `pub use durability::{...}` block | crate API surface |
| `crates/freshell-codex/src/launch_plan.rs` | delete the restore-decision machinery: 9 items + 2 consts + 16 tests + stale header lines (~440 lines) | codex launch planning (live launch-plan half stays) |

Symbols confirmed LIVE that sit adjacent to deletions and MUST stay (with their production callers) are listed inside each task.

---

### Task 1: Delete `CodexActivityTracker::bind_session`

**Files:**
- Modify: `crates/freshell-activity/src/codex.rs` (method at lines 152–163; deviation-1 doc block at lines 19–28)

**Interfaces:**
- Consumes: nothing from other tasks (fully independent).
- Produces: `freshell-activity` crate API without `CodexActivityTracker::bind_session`. The only consumer of the type, `freshell-ws/src/activity.rs`, never calls it — its call surface (`list`, `list_latest_completions`, `track_terminal`, `note_input`, `note_output`, `note_exit`, `expire`, `next_deadline`) is unchanged.

- [ ] **Step 1: Baseline — prove the crates are green before any deletion**

Run:
```bash
cd /home/dan/code/freshell/.worktrees/dead-restore-code-deletion
cargo test -p freshell-activity
cargo test -p freshell-codex --all-features
```
Expected: both PASS (exit 0). Record the test counts printed (e.g. `test result: ok. N passed`) — later tasks assert against them. If the baseline is red, STOP: report the failure instead of deleting anything (the base must be green first).

Known-good reference (validated 2026-07-24 at base): `freshell-codex --all-features` = **190 tests total** (lib 149; integration: app_server_drive 4, completion_gating 3, interrupt_rpc 1, launch_lifecycle 16, remote_proxy_relay 17). If your counts differ, investigate before proceeding. Note: the launch_lifecycle spawn test needs `node` on PATH and the `ws` npm module resolvable (currently via the parent checkout's node_modules); a failure there with a module-resolution/spawn error is environmental — fix the environment, don't touch code.

- [ ] **Step 2: Prove `bind_session` is dead (the "failing test" of a deletion)**

Preferred — LSP (rust-analyzer): run `findReferences` on `bind_session` at its declaration (`crates/freshell-activity/src/codex.rs`, the line reading `pub fn bind_session(&mut self, terminal_id: &str, session_id: &str) -> Vec<CodexEffect> {`).
Expected: **exactly 1 result — the declaration itself, zero call sites.** (Sanity control: `findReferences` on the neighboring `note_exit` must return ≥2 results, proving the index is live.)

Grep fallback / cross-check:
```bash
rg -n 'bind_session' crates/
```
Expected hits — ALL of them, and nothing else touching the codex tracker:
- `crates/freshell-activity/src/codex.rs:152` — the declaration (our target)
- `crates/freshell-activity/src/claude.rs:114` and `:428` — `ClaudeActivityTracker` (different type, out of scope)
- `crates/freshell-activity/src/amplifier/tracker.rs:143` and `:235` — `AmplifierActivityTracker` (live, out of scope)
- `crates/freshell-ws/src/activity.rs:460` (comment) and `:467` — amplifier tracker call (out of scope crate, do not touch)

If any OTHER hit calls the **codex** tracker's `bind_session`: HALT this deletion, keep the method, record in the Task 4 inventory.

- [ ] **Step 3: Delete the method**

In `crates/freshell-activity/src/codex.rs`, delete exactly this block (lines 152–164: the method plus its trailing blank line, preserving one blank line between `track_terminal` and `note_exit`):

```rust
    pub fn bind_session(&mut self, terminal_id: &str, session_id: &str) -> Vec<CodexEffect> {
        let Some(state) = self.states.get_mut(terminal_id) else {
            return Vec::new();
        };
        if state.session_id.as_deref() == Some(session_id) {
            return Vec::new();
        }
        let previous = state.to_record();
        state.session_id = Some(session_id.to_string());
        let next = state.to_record();
        changed(Some(&previous), next)
    }
```

Delete NOTHING else: every item the method touches (`states`, `TerminalActivity::session_id`, `to_record`, `changed`, `has_public_change`, `CodexEffect`) has live users (`session_id` is written by `track_terminal` and read by `to_record` and `record_completion_if_idle`). No imports become unused. No tests exercise `bind_session` (all 9 tests in the file's `mod tests` cover other paths).

- [ ] **Step 4: Update the deviation-1 module comment so no text implies a binder exists**

In the same file, the `//! DOCUMENTED DEVIATIONS` block (lines 19–28) ends deviation 1 with:

```rust
//!    record at all — the exact TERM-15 bug this crate fixes. The PTY-lane
//!    state machine itself is ported faithfully.
```

Replace those two lines with:

```rust
//!    record at all — the exact TERM-15 bug this crate fixes. The PTY-lane
//!    state machine itself is ported faithfully. (A vestigial `bind_session`
//!    binder for that lane was deleted as dead code; session identity arrives
//!    via `track_terminal`'s `session_id` argument, and a future port of the
//!    lane would introduce its own binder.)
```

- [ ] **Step 5: Prove nothing depended on it**

Run:
```bash
cargo check -p freshell-activity
cargo clippy -p freshell-activity -- -D warnings
cargo test -p freshell-activity
cargo check -p freshell-ws --all-features
```
Expected: all four PASS. `cargo test -p freshell-activity` reports the same test count as Step 1 (zero tests deleted). The `freshell-ws` check proves the sole consumer of the tracker still compiles (checking a dependent crate is read-only on its sources — it does not violate the do-not-touch rule).

- [ ] **Step 6: Commit**

```bash
git add crates/freshell-activity/src/codex.rs
git commit -m "chore: delete dead CodexActivityTracker::bind_session (unported JSONL-reconcile binder)"
```

---

### Task 2: Delete the dead durability-store exports

**Files:**
- Modify: `crates/freshell-codex/src/durability.rs` (272 lines → ~160)
- Modify: `crates/freshell-codex/src/lib.rs:62-66` (the `pub use durability::{...}` block)

**Interfaces:**
- Consumes: nothing from Task 1.
- Produces: `freshell-codex` crate API without `default_durability_store_dir`, `DurabilityCandidate`, `CandidateImmutableError`. The retained durability API that later tasks and live code rely on: `CODEX_SIDECAR_OWNERSHIP_ENV`, `ownership_needle()`, `mint_ownership_id()`, `is_codex_thread_id()`, `extract_session_id_from_filename()`, `default_server_instance_id()` — all still re-exported from `lib.rs`.

**LIVE symbols in this file — DO NOT DELETE** (spec caution: verify each symbol individually):

| Symbol | Why it stays |
|---|---|
| `CODEX_SIDECAR_OWNERSHIP_ENV` (L20–24) | prod: `launch_plan.rs:474` (`codex_sidecar_spawn_spec`), `freshell-freshagent/src/codex.rs:1422` |
| `ownership_needle` (L33–36) | prod: `transport.rs:92` (`reap_owned_codex_sidecars`, linux + `real-transport`) |
| `mint_ownership_id` (L38–42) | prod: `launch_lifecycle.rs:599`, `freshell-freshagent/src/codex.rs:1402` |
| `is_codex_thread_id` (L26–31), `extract_session_id_from_filename` (L65–77), `default_server_instance_id` (L44–49) | pub API + unit tests; zero prod callers **today** but scoped KEEP per spec (rollout/instance-id helpers for the app-server lane). Record the zero-prod-caller fact in the Task 4 inventory. |
| `is_hex` / `matches_uuid_at` / `find_uuid` (L151–185) | reachable from `is_codex_thread_id` / `extract_session_id_from_filename` |

- [ ] **Step 1: Prove the store/candidate symbols are dead**

```bash
rg -n 'default_durability_store_dir|durability_store' crates/ src/ server/ shared/
rg -n 'DurabilityCandidate|CandidateImmutableError' crates/ src/ server/ shared/ test/
```
Expected hits, exhaustively:
- `default_durability_store_dir`: only `crates/freshell-codex/src/durability.rs:53` (definition) and `crates/freshell-codex/src/lib.rs:63` (re-export). (`server/coding-cli/codex-app-server/durability-store.ts` is the independent TS implementation — not a caller.)
- `DurabilityCandidate`: only `durability.rs:13` (doc link), `:83` (struct), `:109` (impl), `:251` (its own test), and `lib.rs:65` (re-export).
- `CandidateImmutableError`: only `durability.rs:91/:97/:107/:119/:124/:127/:136` (definition + impls + intra-file use by `DurabilityCandidate::set`) and `lib.rs:64` (re-export).

Any hit outside these sets that is production Rust code → HALT for that symbol (keep it + its cascade), record in Task 4 inventory.

Also confirm the private `home_dir` cascade: `rg -n 'home_dir' crates/freshell-codex/` → expected only `durability.rs:57` (call) and `:61` (definition). (`freshell-server/src/files.rs` has an unrelated private `home_dir` — different module, untouched.)

- [ ] **Step 2: Delete, bottom-up so line numbers stay valid**

In `crates/freshell-codex/src/durability.rs`, apply in this order:

1. Delete lines **248–271** — the blank line + the test `durability_candidate_is_immutable_once_set` (it reads `err.field` and dies with the struct). The other 4 tests (`codex_thread_id_shape_is_a_bare_uuid`, `rollout_filename_yields_embedded_thread_uuid`, `ownership_id_and_needle_shapes`, `server_instance_id_defaults_to_srv_pid_without_env`) all cover retained symbols — keep them.
2. Delete lines **79–148** — `DurabilityCandidate` doc/derive/struct, `CandidateImmutableError` doc/derive/struct, `impl Display for CandidateImmutableError`, `impl std::error::Error for CandidateImmutableError`, `impl DurabilityCandidate` (`candidate_thread_id`, `rollout_path`, `set`), plus the trailing blank line before the `── UUID matching ──` banner comment (which stays).
3. Delete lines **51–64** — the `default_durability_store_dir` doc + fn and the private `home_dir` fn (its only caller), plus the trailing blank line before the `extract_session_id_from_filename` doc comment (which stays). Leaving `home_dir` would produce a `dead_code` warning — the crate has no `#![allow(dead_code)]`.
4. Edit line **16**: `use std::path::{Path, PathBuf};` → `use std::path::Path;` (`PathBuf` was used only in the deleted fns; `Path` survives in `extract_session_id_from_filename`). `use uuid::Uuid;` stays (used by `mint_ownership_id`).
5. Delete lines **11–14** — the module-doc separator + this now-stale paragraph (it intra-doc-links the deleted struct; leaving it breaks `cargo doc`):

```rust
//!
//! The **immutable-candidate** rule from the durability store
//! (`durability-store.ts`, `coding-cli.md §4c`) is modeled by [`DurabilityCandidate`]: once a
//! `{ candidateThreadId, rolloutPath }` is set it cannot change.
```

- [ ] **Step 3: Update `crates/freshell-codex/src/lib.rs`**

Replace lines 62–66, currently:

```rust
pub use durability::{
    default_durability_store_dir, default_server_instance_id, extract_session_id_from_filename,
    is_codex_thread_id, mint_ownership_id, ownership_needle, CandidateImmutableError,
    DurabilityCandidate, CODEX_SIDECAR_OWNERSHIP_ENV,
};
```

with:

```rust
pub use durability::{
    default_server_instance_id, extract_session_id_from_filename, is_codex_thread_id,
    mint_ownership_id, ownership_needle, CODEX_SIDECAR_OWNERSHIP_ENV,
};
```

(`lib.rs:41` `pub mod durability;` stays; the doc-table row at `lib.rs:18` mentions the module generically, no symbol names — no edit.)

- [ ] **Step 4: Prove nothing depended on it**

```bash
cargo check -p freshell-codex --all-features
cargo test -p freshell-codex --all-features
cargo clippy -p freshell-codex --all-features -- -D warnings
cargo check -p freshell-freshagent -p freshell-server -p freshell-ws
cargo fmt --check
```
Expected: all PASS. `durability::tests` reports **4 passed** (was 5). No NEW `dead_code` warnings in `freshell-codex` (clippy `-D warnings` enforces this). Total `freshell-codex` count = Task 1 baseline minus 1 (= 189 if baseline was 190). Baseline caveat: `cargo check -p freshell-freshagent -p freshell-server -p freshell-ws` emits **4 pre-existing `freshell-server` dead_code warnings** (`DEFAULT_CLI_DETECTION_SPECS`, `has_cli`, `get`/`get_all`) — these exist at base and are NOT caused by this deletion; only a warning that is not in that set is attributable.

- [ ] **Step 5: Commit**

```bash
git add crates/freshell-codex/src/durability.rs crates/freshell-codex/src/lib.rs
git commit -m "chore: delete dead durability-store exports (default_durability_store_dir, DurabilityCandidate)"
```

---

### Task 3: Delete the restore-decision machinery in `launch_plan.rs`

**Files:**
- Modify: `crates/freshell-codex/src/launch_plan.rs` (1229 lines → ~789)

**Interfaces:**
- Consumes: Task 2's `lib.rs` state (no interaction — `launch_plan` has NO `pub use` re-exports in `lib.rs`; consumers use full paths `freshell_codex::launch_plan::<Symbol>`, so `lib.rs` needs **zero** changes in this task; `lib.rs:44` `pub mod launch_plan;` stays).
- Produces: `launch_plan` module containing only the live launch-planning half. Later tasks and live code rely on these staying exactly as-is: `FRESHELL_CODEX_MANAGED_LAUNCH_ENV`, `codex_managed_launch_enabled()`, `CODEX_INITIAL_LAUNCH_ATTEMPTS`, `CODEX_INITIAL_LAUNCH_RETRY_DELAY_MS`, `CodexLaunchPlanInput`, `CodexLaunchPlan`, `plan_codex_launch()`, `CodexLaunchConfigError`, `CodexLaunchRetryDecision`, `plan_codex_launch_retry()`, `codex_sidecar_spawn_spec()`, `CodexSidecarSpawnSpec`.

**The file is NOT whole-file dead.** Its header claim ("Nothing outside this crate's tests calls into this module yet") is stale — S4 landed; production callers exist in `freshell-ws/src/terminal.rs`, `freshell-freshagent/src/terminal_tabs.rs`, and in-crate `launch_lifecycle.rs`. Only the restore-decision half is dead. **LIVE items that must stay** (do not delete even though some have no external textual refs — they are transitively required): `CODEX_MANAGED_REMOTE_CONFIG_ARGS`, `CODEX_REMOTE_INVALID_URL_MESSAGE`, `CODEX_REMOTE_NON_LOOPBACK_MESSAGE`, `CodexSandboxMode` + impl, `normalize_codex_sandbox_setting`, `CodexSessionBindingReason` + impl, `get_codex_session_binding_reason` (dead but NOT restore machinery — record, don't delete), `codex_remote_args` + `CodexRemoteArgsError` (test-only-reachable but NOT restore machinery — record, don't delete), both `use` statements (lines 26–27), and everything in §"Produces" above.

- [ ] **Step 1: Prove each restore-decision symbol is dead**

```bash
for s in plan_codex_create_restore_decision is_exact_live_codex_candidate \
         CodexCreateRestorePlan CodexRestoreDecisionInput CodexRestoreRejectKind \
         CodexRestoreRejectCode CodexDurabilityEvidence CodexCandidateIdentity \
         SessionRefInput INVALID_RAW_CODEX_RESUME_MESSAGE MISSING_CODEX_SESSION_REF_MESSAGE; do
  echo "=== $s ==="; rg -n "\b$s\b" crates/
done
```
Expected: for every symbol, hits ONLY inside `crates/freshell-codex/src/launch_plan.rs`, with exactly one documented exception:
- `INVALID_RAW_CODEX_RESUME_MESSAGE` also hits `crates/freshell-freshagent/src/terminal_tabs.rs:63` — that is an **independent private `const` declaration of the same name** (plus comments at `:60,94,141` and a use of the *local* const at `:128`), NOT an import of our symbol. Deleting ours does not affect it. Do NOT edit `terminal_tabs.rs`.

**Special caution — `CodexCandidateIdentity`:** the parallel `codexDurability` wiring task may have landed a Rust caller since this plan was written. If ANY non-test hit outside `launch_plan.rs` appears for it (or any other symbol above): HALT for that symbol — keep it (and, if it's a type another kept item needs, keep that dependency), delete the rest, and record the kept symbol + its caller in the Task 4 inventory.

Preferred additional check: LSP `findReferences` on `plan_codex_create_restore_decision` (fn declaration, ~line 257) and on `CodexCandidateIdentity` (~line 184) — expected: declaration + in-file `#[cfg(test)]` references only.

- [ ] **Step 2: Delete the dead test asserting the dead constants (bottom of file first)**

Delete lines **1217–1228**: the blank line at 1217 plus the test `restore_messages_match_restore_decision_ts` (its `#[test]` attribute at 1218 through its closing `}` at 1228). It asserts both `INVALID_RAW_CODEX_RESUME_MESSAGE` and `MISSING_CODEX_SESSION_REF_MESSAGE`.

The preceding blank line at 1217 MUST be included in the deletion: this test is the last item in `mod tests`, and leaving a trailing blank line immediately before the module's closing `}` (line 1229) fails default rustfmt (`cargo fmt --check`, gated in Step 7 and Task 4 Step 3). After deletion, the previous test's closing `}` (old line 1216) is directly followed by the `mod tests` closing `}` — no blank line between them.

- [ ] **Step 3: Delete the restore-decision test block**

Delete lines **606–889** (through the closing `}` of the last test, plus the section's leading comment), i.e. the block starting at the section comment:

```rust
    // ── planCodexCreateRestoreDecision — vectors ported from … restore-decision.test.ts ──
```

and containing exactly 2 helpers + 15 tests: `candidate_evidence()`, `durable_evidence()`, `rejects_restore_requests_that_only_provide_a_raw_legacy_resume_id`, `rejects_non_restore_creates_that_provide_a_raw_legacy_resume_id`, `requires_a_canonical_session_ref_for_codex_restore`, `routes_canonical_session_ref_restores_directly`, `ignores_durable_codex_durability_without_a_canonical_session_ref`, `ignores_candidate_codex_durability_without_a_canonical_session_ref`, `uses_explicit_session_ref_before_any_durability_evidence`, `fresh_creates_when_restore_is_not_requested_even_if_durability_is_present`, `non_codex_session_ref_never_counts_as_restore_identity`, `codex_session_ref_wins_over_a_raw_legacy_resume_id`, `empty_legacy_resume_id_is_not_raw_and_plans_fresh`, `plain_create_with_no_identity_plans_fresh`, `reject_code_wire_strings_match_legacy`, `matches_exact_live_candidates_only_by_rollout_path_and_candidate_thread_id`, `a_terminal_without_a_live_candidate_never_matches`.

The next section (the `plan_codex_launch` golden tests, ~line 891) must remain intact; keep single-blank-line spacing.

**Do NOT delete** the `get_codex_session_binding_reason` tests (lines 517–557) — that symbol stays (recorded as a separate dead-code finding, out of this deletion's scope).

- [ ] **Step 4: Delete the restore-decision implementation block**

Delete lines **171–305** plus the trailing blank line 306: from the section divider

```rust
// ─── restore decision (restore-decision.ts) ───…
```

through the end of `is_exact_live_codex_candidate`, leaving the next divider (`// ─── launch plan ───…`, line 307) as the follower of the `get_codex_session_binding_reason` fn. The block contains exactly these 9 items (each with its doc comment / derives): `SessionRefInput<'a>`, `CodexCandidateIdentity<'a>`, `CodexDurabilityEvidence<'a>`, `CodexRestoreDecisionInput<'a>`, `CodexRestoreRejectKind`, `CodexRestoreRejectCode` + `impl CodexRestoreRejectCode`, `CodexCreateRestorePlan`, `plan_codex_create_restore_decision`, `is_exact_live_codex_candidate`.

- [ ] **Step 5: Delete the two dead message constants**

Delete lines **31–37** (both consts with their doc lines, keeping one blank line between the `// ─── constants ───` divider at 29 and `CODEX_MANAGED_REMOTE_CONFIG_ARGS` at 39):

```rust
/// `restore-decision.ts:8-10` — reject message for a raw legacy resume id.
pub const INVALID_RAW_CODEX_RESUME_MESSAGE: &str = …;

/// `restore-decision.ts:30` — reject message for a missing session ref.
pub const MISSING_CODEX_SESSION_REF_MESSAGE: &str = …;
```

(Exact doc text may differ slightly — identify by the two `pub const` names; delete each const with its attached doc comment.)

- [ ] **Step 6: Fix the module header so no text points at removed code**

In the header doc comment (lines 1–24):

1. Delete the two legacy-source table rows (lines 9–10) — they intra-doc-link deleted items and would break `cargo doc`:
```rust
//! | [`plan_codex_create_restore_decision`] (full decision table) | `server/coding-cli/codex-app-server/restore-decision.ts:32-65` |
//! | [`is_exact_live_codex_candidate`] | `restore-decision.ts:87-94` |
```
2. Replace the stale S4/S5 sentence (lines 16–18), currently:
```rust
//! This module is the DECISION half only: S4 (wiring into the two terminal-create paths)
//! and S5 (durability binding, DEV-0008) consume these plans later. Nothing outside this
//! crate's tests calls into this module yet — that is by design (spec §5, slice ordering).
```
with:
```rust
//! This module is the DECISION half only. Its plans are consumed by
//! `launch_lifecycle` (in-crate) and the two terminal-create paths
//! (`freshell-ws/src/terminal.rs`, `freshell-freshagent/src/terminal_tabs.rs`).
//! The restore-decision table that once lived here was deleted as dead code
//! (zero non-test callers); the future restore path will be built against the
//! server-side pane-identity ledger, not this module.
```
3. Delete the TS-truthiness bullet (line 24) that describes only the deleted decision table:
```rust
//! - `hasRawLegacyResume` requires `length > 0` — an empty legacy resume id is NOT raw.
```
(The other two truthiness bullets, lines 21–23, describe `plan_codex_launch` / sandbox normalization — they stay.)

- [ ] **Step 7: Prove nothing depended on it**

```bash
cargo check -p freshell-codex --all-features
cargo test -p freshell-codex --all-features
cargo doc -p freshell-codex --all-features --no-deps
cargo clippy -p freshell-codex --all-features -- -D warnings
cargo check -p freshell-freshagent -p freshell-server -p freshell-ws
cargo fmt --check
```
Expected: all PASS. `launch_plan::tests` reports **32 passed** (was 48: −15 restore-decision tests, −1 `restore_messages_match_restore_decision_ts`). Crate total = Task 1 baseline − 17 (= 173 if baseline was 190; lib 132). `cargo doc` exits 0 with no `broken_intra_doc_links` warnings and no NEW warnings — but note it emits **4 pre-existing warnings at base** (3× `rustdoc::private_intra_doc_links` in the remote_proxy/status/json_scan areas + 1× redundant explicit link target in transport.rs); those persist and are fine. No NEW `dead_code` warnings (the 4 baseline `freshell-server` ones persist in the multi-crate check).

- [ ] **Step 8: Commit**

```bash
git add crates/freshell-codex/src/launch_plan.rs
git commit -m "chore: delete dead codex restore-decision machinery from launch_plan.rs"
```

---

### Task 4: Full verification, push, and deletion inventory (STOP before PR)

**Files:**
- No source modifications (verification + push + report only). If a check fails, fix forward within the constraints above and re-run.

**Interfaces:**
- Consumes: the three commits from Tasks 1–3.
- Produces: pushed branch `chore/dead-restore-code-deletion` + a textual inventory report (in the task's completion report — NOT a new markdown file).

- [ ] **Step 1: Residual-reference sweep — no Rust text points at removed code**

```bash
rg -n 'plan_codex_create_restore_decision|is_exact_live_codex_candidate|CodexCreateRestorePlan|CodexRestoreDecisionInput|CodexRestoreRejectKind|CodexRestoreRejectCode|CodexDurabilityEvidence|CodexCandidateIdentity|SessionRefInput|DurabilityCandidate|CandidateImmutableError|default_durability_store_dir' crates/
rg -n 'bind_session' crates/freshell-activity/src/codex.rs
```
Expected: **zero hits** from the first command (any hit that was HALT-kept per the halt rule is fine — name it in the inventory); from the second command, **exactly one hit** — the deviation-1 doc-comment addendum added in Task 1 Step 4 (the intentional tombstone note), and no code hits. `INVALID_RAW_CODEX_RESUME_MESSAGE` hits in `crates/freshell-freshagent/src/terminal_tabs.rs` are the independent local const — expected and correct. TS files under `server/`, `shared/`, `docs/` still mention same-named TS symbols — expected, out of scope, do not edit.

- [ ] **Step 2: Full workspace compile with all targets and features**

```bash
cargo check --workspace --all-features --all-targets
```
Expected: PASS (this is what catches feature-gated consumers like `launch_lifecycle` behind `real-transport`, and integration tests under `crates/*/tests/`). Verified runnable green at base (12.19s warm). The 4 pre-existing `freshell-server` dead_code warnings (see Validated baseline facts) will appear — they are baseline; only a NEW warning indicates a problem.

- [ ] **Step 3: Crate test suites**

```bash
cargo test -p freshell-codex --all-features
cargo test -p freshell-activity
cargo fmt --check
```
Expected: PASS. `freshell-codex` total = Task 1 baseline − 17 (16 launch_plan tests + 1 durability test; = 173 with lib 132 if baseline was the validated 190/149); `freshell-activity` total = baseline (unchanged).

- [ ] **Step 4: Coordinated full repo suite (the coordinator gate)**

```bash
npm run test:status
```
If another agent holds the gate: WAIT and re-poll (e.g. every 60s). NEVER kill the holder, never bypass the gate. When clear:

```bash
FRESHELL_TEST_SUMMARY="dead-restore-code-deletion: verify Rust dead-code removal did not affect the suite" npm test
```
Expected: PASS (exit 0). (If `node_modules` is missing in this worktree, run `npm ci` first.) This deletion is Rust-only and cargo is not part of `npm test` (verified: the coordinated suite is vitest-only — no cargo, no Rust binary, no :3001 contact), so this run proves the TS/server suite is unaffected; the cargo runs above are the Rust proof.

Additional guardrails (from load-bearing validation):
- Do NOT set `FRESHELL_RUN_REAL_PROVIDER_CONTRACTS=1` in the gate shell — it would un-skip real-provider contract tests.
- A green baseline EXISTS: the coordinator recorded `full-suite success exit=0` at exactly the base commit a53f185a on 2026-07-24 on this host. If this run is RED, do not immediately attribute it to the deletion: (1) check for the host's known-live WSL UDP-port-exhaustion signature (DNS/timeout-flavored errors; Tcpip event 4266 fires daily on this machine), (2) retry once — both vitest configs shuffle test order, so a pass on retry indicates flake/environment, not the deletion. Only a reproducible failure that names affected code is attributable; diagnose against the green baseline.

- [ ] **Step 5: Pre-push freshness re-check, then push the branch — and STOP (no PR)**

The dead-verdicts were re-confirmed against origin/main == a53f185a on 2026-07-24, but main may have moved since (parallel tasks are merging). Immediately before pushing, run the read-only re-check:

```bash
git ls-remote origin refs/heads/main
```

- If the SHA is still `a53f185a...`: proceed to push.
- If main MOVED: inspect the new commits without fetching — `gh api "repos/{owner}/{repo}/compare/a53f185a...<new-sha>"` (derive owner/repo from `git remote get-url origin`) and search the returned file patches for every deleted symbol (`plan_codex_create_restore_decision`, `is_exact_live_codex_candidate`, `CodexCreateRestorePlan`, `CodexRestoreDecisionInput`, `CodexRestoreRejectKind`, `CodexRestoreRejectCode`, `CodexDurabilityEvidence`, `CodexCandidateIdentity`, `SessionRefInput`, `INVALID_RAW_CODEX_RESUME_MESSAGE`, `MISSING_CODEX_SESSION_REF_MESSAGE`, `DurabilityCandidate`, `CandidateImmutableError`, `default_durability_store_dir`, `bind_session`). If a new production Rust caller of a deleted symbol landed: restore that symbol (revert the relevant hunk from the deletion commit), re-run this task's verification steps, and record the restore + its caller in the Step 6 inventory. If no hit (or hits are TS/tests/the independent `terminal_tabs.rs` const): proceed.

```bash
git push -u origin chore/dead-restore-code-deletion
```
Do NOT run `gh pr create` — PR creation is not user-approved.

- [ ] **Step 6: Emit the deletion inventory report**

Produce this report as the task's completion output (fill in real numbers from `git diff --stat origin/main..HEAD` and the halt-rule outcomes):

```
Branch: chore/dead-restore-code-deletion (pushed; NO PR created — awaiting user approval)

DELETED (symbol — file — approx lines removed):
- CodexActivityTracker::bind_session — freshell-activity/src/codex.rs — 13
- default_durability_store_dir + home_dir — freshell-codex/src/durability.rs — ~14
- DurabilityCandidate + CandidateImmutableError (+3 impls) + 1 test + doc para — durability.rs — ~98
- lib.rs pub use trim (3 names) — freshell-codex/src/lib.rs — 2
- Restore-decision machinery (9 items: SessionRefInput, CodexCandidateIdentity,
  CodexDurabilityEvidence, CodexRestoreDecisionInput, CodexRestoreRejectKind,
  CodexRestoreRejectCode+impl, CodexCreateRestorePlan,
  plan_codex_create_restore_decision, is_exact_live_codex_candidate)
  + 2 consts + 16 tests + header fixes — freshell-codex/src/launch_plan.rs — ~440
Total: ~<N> lines removed. git diff --stat: <paste>

KEPT because live (verified callers):
- CODEX_SIDECAR_OWNERSHIP_ENV, ownership_needle, mint_ownership_id (durability.rs)
- The launch-plan half of launch_plan.rs (callers in freshell-ws/terminal.rs,
  freshell-freshagent/terminal_tabs.rs, launch_lifecycle.rs)
- <any symbol kept via the HALT rule, with its discovered caller — or "none">

KEPT per scope, but found to have ZERO production callers (follow-up candidates):
- is_codex_thread_id, extract_session_id_from_filename, default_server_instance_id (durability.rs — pub API + unit tests only)
- get_codex_session_binding_reason (launch_plan.rs — logic duplicated inline in plan_codex_launch:356-359)
- codex_remote_args + CodexRemoteArgsError + CODEX_REMOTE_*_MESSAGE (launch_plan.rs — integration-test-only; production uses the independent copy in freshell-platform/src/cli_launch.rs)
- ClaudeActivityTracker::bind_session (claude.rs:114-124 — same unported-binder species, has one test; out of scope)

VERIFICATION: cargo check --workspace --all-features --all-targets PASS;
cargo test -p freshell-codex --all-features PASS (<n> tests);
cargo test -p freshell-activity PASS (<n> tests); cargo doc PASS; clippy -D warnings PASS;
fmt --check PASS; coordinated npm test PASS.
```

---

## Load-Bearing Validation Addendum (2026-07-24, stage 2)

Eight load-bearing assumptions were surfaced and validated (ledger + full evidence: `.worktrees/.the-usual-logs/dead-restore-code-deletion/load-bearing-ledger.md` and `reports/V1..V5.md`). Six verified (origin/main unchanged since branch point; wiring task does not consume the restore-decision symbols; worktree exclusive; workspace all-features gate runnable; `--all-features` tests hermetic and green 190/190; npm suite is vitest-only with no Rust/:3001 contact). Two falsified, plan fixed accordingly: (1) doc/warning gates are NOT warning-free at base — baseline warning sets recorded above and expectations scoped to "no NEW warnings"; (2) the npm suite has a same-day green baseline at a53f185a but the host's WSL UDP-exhaustion flake is still live — Task 4 Step 4 gained triage guidance. One accepted residual (time-of-check race on origin/main) is mitigated by the mandatory pre-push re-check added to Task 4 Step 5. Self-review re-run over all edited tasks: expectations remain exact (commands + expected outputs), no placeholders introduced, no scope change, symbol lists consistent with the deletion sets.

## Self-Review Notes (performed at plan-writing time)

- **Spec coverage:** spec item 1 (launch_plan decision table) → Task 3; item 2 (bind_session) → Task 1; item 3 (durability-store exports, surgical) → Task 2; item 4 (update docs/comments pointing at removed lanes) → Task 1 Step 4, Task 2 Step 2 edit 5 (module-doc paragraph), Task 3 Step 6 (header rows/sentence/bullet), plus Task 4 Step 1 residual sweep proving no Rust text names deleted symbols. Per-deletion `cargo check` + crate tests → each task's verify step; full coordinated suite at the end → Task 4 Step 4. Halt-and-record rule → Global Constraints + per-task Step 1s + inventory. PR policy (push, stop, report inventory) → Task 4 Steps 5–6. The remaining `S5` deferral comments in `launch_lifecycle.rs`/`remote_proxy.rs` reference the *future* durability store (campaign §4.2 ledger), not the deleted code, and name no deleted symbols — verified; they stay.
- **No silent deferrals:** this task's observable production outcome is negative (code absent, everything still green); it is proven by real compile/test/doc/clippy runs and the real coordinated suite — no stubs or mocks anywhere. The "follow-up candidates" list defers nothing the spec required: each listed item is explicitly OUTSIDE the spec's deletion scope (spec: "delete ONLY the store/candidate parts", "scoped subset") and is recorded in the inventory exactly as the spec's halt/report rule mandates.
- **Placeholder scan:** every deletion step names the exact symbols, boundary markers, and line ranges; every verify step has exact commands and expected results (including exact expected test-count deltas: −1 durability, −16 launch_plan, −0 activity). No TBDs.
- **Type consistency:** symbol names cross-checked against the exploration reports (LSP + exhaustive grep, 2026-07-24): `CodexActivityTracker` (spec said `CodexTracker` — corrected), `lib.rs` durability block is lines 62–66 (spec said 63–65 — the body lines; opener/closer included), `launch_plan` has no `lib.rs` re-exports (so Task 3 correctly touches only the one file).
