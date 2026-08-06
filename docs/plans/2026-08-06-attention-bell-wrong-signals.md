# Attention-Bell Wrong-Signal Fixes Implementation Plan

> **For agentic workers:** This plan is executed task-by-task by the
> workflow's execute stage: a fresh implementer per task, with a spec +
> quality review after each task. Steps use checkbox (`- [ ]`) syntax
> for tracking.

**Goal:** Land the eleven attention-bell wrong-signal fixes (GitHub issues
#603–#613 on danshapiro/freshell) so the per-pane status light always tells
the truth: blue = agent working, green = agent done/needs you, never a false
"done", never a silent "needs you", never a false death bell.

**Architecture:** Every fix follows one owner-approved pattern — *verify
against a truth source before changing any light* ("ask the agent"). The
correct in-repo template is the codex deadman self-heal
(`crates/freshell-activity/src/codex.rs:44-56`): on uncertainty, emit
`TrackerEffect::ForceRead`, STAY busy, and let a truth-source read decide.
Truth sources: opencode `GET /session/status` (via the per-pane SSE lane),
the claude session JSONL under `~/.claude/projects` (spike-verified on
claude-code 2.1.223: turn-start = `user` record with `promptSource`;
turn-end = `system`/`turn_duration` record), the amplifier `events.jsonl`
tail, and — for death bells — freshell's own PTY input stream (quit-intent
detection). When a verify probe itself FAILS, the owner ruling applies:
clear busy AND fire the attention/death engagement signal (never hold
silently, never invent an "unknown" state).

**Tech Stack:** Rust (workspace under `crates/`, toolchain 1.96.0), tokio,
serde_json, tracing; the pure tracker crates (`freshell-activity`) stay
IO-free and synchronous; IO lives in `freshell-ws` (hub + lanes) behind
injected trait seams so tests run on fakes. No client (TypeScript) changes:
the client's "no busy record + bound session ⇒ persistent green" rule
(`src/lib/pane-activity.ts:225-261`) is exactly why every fix is
server-side.

## Global Constraints

These apply to EVERY task below. Each task's requirements implicitly
include this section.

- **Worktree:** all work happens inside
  `/home/dan/code/freshell/.worktrees/attention-bell-wrong-signals`
  (branched from `origin/main` @ `bbf3bad96`). Every command in this plan
  runs from that directory unless stated otherwise.
- **Owner policy (BINDING, from Dan — verbatim from the handoff):**
  1. "Deterministic behavior only. No 'unknown' status states in wire
     protocol or UI (REJECTED direction)."
  2. "Approved pattern: 'ask the agent' — verify against a truth source
     before changing any light (opencode GET /session/status, codex rollout
     force-read, claude session JSONL)."
  3. "No heuristic/probabilistic fallbacks."
  4. "Verify-probe failure = crash/needs-attention: clear busy AND fire the
     attention/death engagement signal (applies to #603, #604, #605, #606,
     #609, #610)."
  5. "#607: no notification promise for unmanaged/PTY-only codex panes;
     scope guarantee to managed panes, document the limitation, no PTY
     text-parsing heuristics."
  6. "#612: external kills SHOULD ring the death bell (no work for that
     sub-case); user-typed quits through freshell's own input stream
     (/quit, /exit, Ctrl+D, Ctrl+C) must NOT ring it."
  7. "#611: bounded submit-grace gate acceptable ONLY if the claude-JSONL
     truth-source spike fails; prefer the verify-backed approach." (The
     spike SUCCEEDED — see Task 9 — so the verify-backed approach is used.)
- **Strict red-green-refactor TDD** (AGENTS.md): write the failing test
  first, run it to see it fail, implement minimally, run it to see it pass,
  commit. Never weaken an assertion to make a test pass without a stated
  reason in the test comment.
- **NEVER restart or stop the live self-hosted Rust server** (the
  production freshell-server on this machine). Deploying is explicitly OUT
  OF SCOPE for this run — code lands via PR only (Task 16).
- **Commits:** author is `Dan Shapiro <3732858+danshapiro@users.noreply.github.com>`
  (repo git config already set — do NOT override with `--author` or
  `git config` changes). Focused, atomic commits; one per plan task step
  that says "Commit".
- **Test commands:** Rust tests are per-crate local gates (CI runs only
  `cargo fmt`/`clippy` + `cargo test -p freshell-protocol`): use
  `cargo test -p freshell-activity`, `cargo test -p freshell-ws`,
  `cargo test -p freshell-server`. The Node/vitest suite runs via
  `npm run test:vitest` (coordinated; used in Task 16 — no TS source is
  modified by this plan).
- **`AttentionBoundary` is the engagement signal.** Owner ruling 4's
  "attention/death engagement signal" is implemented as the existing
  `TrackerEffect::AttentionBoundary { terminal_id, at }`: the `*_frames`
  mappers call `idle.note_turn_boundary(...)`, which arms the truly-idle
  gate and produces a bare `terminal.idle` frame (~2s later) — wire-identical
  to a death bell (`{"type":"terminal.idle", ..., "reason":"grace"}`). No
  wire-protocol change; the frozen client already rings/shades on it.
- **Crash-semantics convention** used throughout this plan: on verify-probe
  failure with a busy record present, the tracker emits
  `[Changed{remove:[tid]}, AttentionBoundary{tid, at}]` (in that order —
  removal first so `note_changed_to_gate` clears gate state, then the
  boundary re-arms grace-only; this ordering is the existing D7 contract in
  `opencode_frames`, `activity.rs:1719-1722`) plus an `error!`-level
  tracing log. With no busy record and no pending pauses, a probe failure
  emits nothing (there is no light to correct).
- **Rust file paths** are relative to the worktree root. Line numbers cited
  are against `main` @ `bbf3bad96` and drift as tasks land — anchor by the
  quoted code, not the number.

## Scope note (one plan, one subsystem)

All eleven issues live in one subsystem — the activity/attention-bell
pipeline (`crates/freshell-activity` trackers + `crates/freshell-ws` hub
and lanes) — plus one small build-tooling fix (#613) in
`crates/freshell-server`. They share machinery (the #603 verify cycle is
reused by #604, #608, #610; the claude JSONL truth source serves #606 and
#611), so they land as one plan in the dependency order the handoff
prescribes. Each task still produces working, independently testable
software.

## File Structure

Files created:
- `crates/freshell-ws/src/claude_truth.rs` — claude session-JSONL truth
  source: trait `ClaudeTruth` + production `FsClaudeTruth` + probe types.
  One responsibility: answer "is this claude session's turn in flight,
  ended, or unknowable?" and "did a turn start after byte offset N?" from
  disk. (Task 9)
- `scripts/build-stamp-check.sh` — scripted integration check for the
  build.rs commit stamp (throwaway git repo; proves the packed-ref →
  loose-ref transition restamps). (Task 15)

Files modified (responsibility of each change):
- `crates/freshell-activity/src/opencode.rs` — deadman becomes
  verify-then-decide (`expire` emits `ForceRead`, stays busy); new
  `note_verify_failed` (crash semantics); lane busy-root evidence binds
  identity directly (Quiet → KnownBusy, no Candidate detour); Ambiguous
  re-promotes from a single-busy-root snapshot; permission pauses survive
  busy snapshots. (Tasks 1, 6, 7, 8)
- `crates/freshell-ws/src/opencode_lane.rs` — verify-request channel
  (hub → lane), `verify()` re-fetch of `/session/status` stamped with the
  CURRENT cycle/stream, `SnapshotFailed` on probe failure, snapshot
  parse-hardening (unknown status ⇒ Busy, shape break ⇒ failure), v2 +
  question event vocabulary, drift contradiction detector, `GET
  /permission` resync on connect. (Tasks 2, 3, 4, 5, 8)
- `crates/freshell-ws/src/activity.rs` — `opencode_frames`/`claude_frames`
  return force-reads; hub services opencode verifies via the lane channel
  and claude verifies via `ClaudeTruth`; `SnapshotFailed` intake arm;
  amplifier lane retry becomes capped-repeat with crash-semantics
  escalation; quit-intent markers consulted by the Exit arm; claude submit
  offsets stashed at input time. (Tasks 2, 5, 8, 10, 11, 12, 13)
- `crates/freshell-activity/src/claude.rs` — deadman becomes
  verify-then-decide; `note_verified_busy` / `note_verified_ended` /
  `note_verify_failed`; provisional submit-grace with probe-backed
  confirmation; `set_busy_deadman_ms` test hook. (Tasks 10, 11)
- `crates/freshell-activity/src/amplifier/tracker.rs` — confirmed-busy
  signal loss keeps busy + `ForceRead`; `note_verify_failed` (crash
  semantics). (Task 12)
- `crates/freshell-activity/src/signal.rs` — quit-intent input
  classification (`QuitIntentState`, `classify_input`). (Task 13)
- `crates/freshell-activity/src/idle.rs` — Accepted Residuals doc-comment
  registry updated per task as residuals are closed/re-scoped (Tasks 2, 5,
  6, 8, 13, 14).
- `crates/freshell-server/build.rs` — unconditional loose-ref watch.
  (Task 15)
- `crates/freshell-server/src/main.rs` + `crates/freshell-server/src/diag.rs`
  — self-identifying boot line (timestamp + pid + commit + dirty);
  `opencodeDriftEvents` on `/api/server-info`. (Tasks 5, 15)

Key existing types the tasks build on (defined in
`crates/freshell-activity/src/lib.rs:41-57`, shared by all trackers):

```rust
pub enum TrackerEffect<R> {
    Changed { upsert: Vec<R>, remove: Vec<String> },
    TurnComplete { terminal_id: String, session_id: Option<String>, at: i64, completion_seq: i64 },
    ForceRead { terminal_id: String, at: i64 },
    AttentionBoundary { terminal_id: String, at: i64 },
}
```

---

## Task Right-Sizing Map (issue → tasks)

| Issue | Tasks | Truth source / mechanism |
|---|---|---|
| #603 opencode busy deadman false-green | 1, 2 | GET /session/status via lane verify channel |
| #604 opencode event drift | 3, 4, 5 | snapshot poll is the safety net + shape-tolerant vocabulary + loud drift detector |
| #609 opencode first-turn / never-bound | 6 | per-pane endpoint busy-root evidence binds directly |
| #610 opencode ambiguous quiet drain | 7 | verify snapshot collapses to one root → re-promote |
| #608 opencode permission resync | 8 | GET /permission on connect + pause survives busy snapshots |
| #606 claude output deadman | 9, 10 | session JSONL tail (turn_duration / interrupt marker) |
| #611 claude stray Enter | 9, 11 | session JSONL offset probe (turn-start user record) |
| #605 amplifier signal loss / give-up | 12 | events.jsonl force-read + capped-repeat retry + crash semantics |
| #612 death bell on human quit | 13 | freshell-owned PTY input stream (quit-intent marker) |
| #607 codex unmanaged approval | 14 | documentation of owner ruling (no code behavior change) |
| #613 build stamp / boot forensics | 15 | unconditional ref watch + self-identifying boot line |
| landing | 16 | full suites, push, PR, merge |

---

### Task 1: opencode tracker — deadman verifies instead of dropping (#603, tracker half)

**Files:**
- Modify: `crates/freshell-activity/src/opencode.rs` (fn `expire` at ~:617-628; new fn `note_verify_failed`; test module ~:1062+)

**Interfaces:**
- Consumes: existing `TrackerEffect::{ForceRead, AttentionBoundary}` variants (`lib.rs:41-57`), existing `clear_record(state, force)` / `set_busy_record` helpers, existing `set_busy_deadman_ms` test hook (`opencode.rs:186`).
- Produces (Task 2 relies on these exact signatures):
  - `pub fn expire(&mut self, at: i64) -> Vec<OpencodeEffect>` — now emits `TrackerEffect::ForceRead { terminal_id, at }` for each busy-record terminal silent past the window, KEEPS the record, and re-arms by setting `last_observed_at = at`.
  - `pub fn note_verify_failed(&mut self, terminal_id: &str, at: i64) -> Vec<OpencodeEffect>` — crash semantics.

- [ ] **Step 1: Write the failing tests**

In the `#[cfg(test)] mod tests` of `crates/freshell-activity/src/opencode.rs`,
REPLACE the existing test `deadman_expiry_removes_silently` (at ~:1304-1319,
which pins the old wrong behavior — this inversion is deliberate: the silent
drop IS bug #603) with:

```rust
    #[test]
    fn deadman_expiry_requests_verify_and_stays_busy() {
        // #603: the deadman is verify-then-decide, mirroring the codex
        // self-heal (codex.rs:44-56). No silent record drop, ever.
        let mut tracker = OpencodeActivityTracker::new();
        tracker.set_busy_deadman_ms(1000);
        tracker.track_terminal("t1", Some("ses-r"), 0);
        tracker.note_status("t1", "ses-r", OpencodeStatus::Busy, 1, 1, 0);
        assert_eq!(tracker.next_deadline(), Some(1000));
        assert!(
            tracker.expire(1000).is_empty(),
            "not yet silent PAST the window"
        );
        // Past the window: a verify request, record RETAINED, deadline
        // re-armed (a wedged verify cannot hot-loop — anchor-disarm lesson).
        assert_eq!(
            tracker.expire(2000),
            vec![TrackerEffect::ForceRead {
                terminal_id: "t1".to_string(),
                at: 2000,
            }]
        );
        assert_eq!(tracker.list(), vec![rec(Some("ses-r"), 0)]);
        assert_eq!(tracker.next_deadline(), Some(3000));
        assert!(tracker.list_latest_completions().is_empty());
    }

    #[test]
    fn verify_snapshot_busy_keeps_the_record_and_empty_clears_with_completion() {
        // The verify answer flows through the EXISTING note_snapshot path.
        let mut tracker = OpencodeActivityTracker::new();
        tracker.set_busy_deadman_ms(1000);
        tracker.track_terminal("t1", Some("ses-r"), 0);
        tracker.note_status("t1", "ses-r", OpencodeStatus::Busy, 1, 1, 0);
        tracker.expire(2000); // verify requested, still busy
        // Verify answer: still busy — record retained, deadman re-armed.
        assert!(tracker
            .note_snapshot(
                "t1",
                &[("ses-r".to_string(), OpencodeStatus::Busy)],
                1,
                1,
                2100
            )
            .is_empty()); // same-session busy refresh is not a public change
        assert_eq!(tracker.list(), vec![rec(Some("ses-r"), 2100)]);
        assert_eq!(tracker.next_deadline(), Some(3100));
        // Next window: verify again; answer: idle — clear WITH completion.
        assert_eq!(
            tracker.expire(3200),
            vec![TrackerEffect::ForceRead {
                terminal_id: "t1".to_string(),
                at: 3200,
            }]
        );
        assert_eq!(
            tracker.note_snapshot("t1", &[], 1, 1, 3300),
            vec![remove(), turn_complete("ses-r", 3300, 1)]
        );
    }

    #[test]
    fn verify_failed_clears_busy_and_rings_attention() {
        // Owner ruling: verify-probe failure = crash/needs-attention —
        // clear busy AND fire the engagement signal. Never silent.
        let mut tracker = OpencodeActivityTracker::new();
        tracker.track_terminal("t1", Some("ses-r"), 0);
        tracker.note_status("t1", "ses-r", OpencodeStatus::Busy, 1, 1, 100);
        tracker.note_permission_asked("t1", "ses-r", "perm-1", 150);
        assert_eq!(
            tracker.note_verify_failed("t1", 200),
            // Record was already removed by the pause; the forced remove
            // still emits (mid-pause crash must cancel the client's state)
            // followed by the attention boundary.
            vec![remove(), boundary(200)]
        );
        assert!(!tracker.has_pending_permissions("t1"));
        assert!(!tracker.blocks_death_bell("t1"));
        assert_eq!(tracker.next_deadline(), None);
        // No record, no pause: probe failure is a no-op.
        assert!(tracker.note_verify_failed("t1", 300).is_empty());
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p freshell-activity deadman_expiry_requests_verify verify_snapshot_busy verify_failed_clears`
(also confirm the old `deadman_expiry_removes_silently` no longer exists:
`grep -n "deadman_expiry_removes_silently" crates/freshell-activity/src/opencode.rs` returns nothing)
Expected: compile FAILS with `no method named note_verify_failed`, and after
stubbing, `deadman_expiry_requests_verify_and_stays_busy` FAILS (current
`expire` returns `vec![remove()]`).

- [ ] **Step 3: Implement**

Replace `expire` (`opencode.rs:617-628`) with:

```rust
    /// Busy-deadman sweep — verify-then-decide (#603). Silence past the
    /// window no longer drops the record: it emits a verify request
    /// (`ForceRead`) and STAYS busy; the hub answers by re-fetching
    /// `/session/status` through the lane, and the snapshot reducer
    /// decides (busy → refreshed, empty → cleared WITH completion gating,
    /// probe failure → [`Self::note_verify_failed`]). `last_observed_at`
    /// re-arms here so a wedged verify cannot hot-loop (the codex
    /// anchor-disarm lesson, codex.rs:49-53).
    pub fn expire(&mut self, at: i64) -> Vec<OpencodeEffect> {
        let mut effects = Vec::new();
        for state in self.states.values_mut() {
            if state.record.is_some() && at - state.last_observed_at > self.busy_deadman_ms {
                state.last_observed_at = at;
                effects.push(TrackerEffect::ForceRead {
                    terminal_id: state.terminal_id.clone(),
                    at,
                });
            }
        }
        effects
    }
```

Add `note_verify_failed` immediately after `note_exit` (~:615):

```rust
    /// The deadman verify probe itself failed (serve unreachable, snapshot
    /// endpoint broken). Owner ruling (2026-08-05): treat as
    /// crash/needs-attention — clear busy AND fire the attention/death
    /// engagement signal. Deterministic; never a silent clear, never an
    /// "unknown" state. Ownership resets to Quiet (keeping the confirmed
    /// identity when there is one) so a later reconnect re-establishes
    /// cleanly; the pending-permission set is retired with the episode.
    pub fn note_verify_failed(&mut self, terminal_id: &str, at: i64) -> Vec<OpencodeEffect> {
        let Some(state) = self.states.get_mut(terminal_id) else {
            return Vec::new();
        };
        let had_record = state.record.is_some();
        let had_pause = !state.pending_permissions.is_empty();
        if !had_record && !had_pause {
            return Vec::new(); // nothing to correct
        }
        tracing::error!(
            component = "opencode-activity-tracker",
            event = "opencode_verify_failed",
            terminal_id = %state.terminal_id,
            "opencode verify probe failed; clearing busy and ringing attention (owner ruling: probe failure = crash)"
        );
        state.pending_permissions.clear();
        let known = match &state.ownership {
            Ownership::Quiet { known_session_id } => known_session_id.clone(),
            Ownership::KnownBusy { session_id, .. } => Some(session_id.clone()),
            Ownership::Candidate { previous_known, .. }
            | Ownership::AwaitingAssociation { previous_known, .. } => previous_known.clone(),
            Ownership::Ambiguous { known_session_id, .. } => known_session_id.clone(),
        };
        state.ownership = Ownership::Quiet {
            known_session_id: known,
        };
        // Force the remove even when the record is already absent (a
        // mid-pause crash must cancel the armed pause window on the gate),
        // then arm the attention boundary — D7 order: remove FIRST.
        let mut effects = clear_record(state, true);
        effects.push(TrackerEffect::AttentionBoundary {
            terminal_id: state.terminal_id.clone(),
            at,
        });
        effects
    }
```

Also update the stale doc comment on the `expire` test hook / D8(e)
references in this file: in the module-level docs and `idle.rs` the change
lands in Task 2 (single doc commit there covers the registry).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p freshell-activity opencode`
Expected: ALL opencode tracker tests PASS (the three new ones plus the
existing suite — nothing else pinned `expire`'s old removal except the
deleted test).

- [ ] **Step 5: Commit**

```bash
git add crates/freshell-activity/src/opencode.rs
git commit -m "fix(opencode): busy deadman verifies via ForceRead and stays busy; probe failure rings attention (#603)"
```

---

### Task 2: opencode lane + hub — service the verify cycle (#603, lane/hub half)

**Files:**
- Modify: `crates/freshell-ws/src/opencode_lane.rs` (Lane struct ~:130-143, `spawn_opencode_lane` ~:102-117, `run` ~:145-246, tests ~:716+)
- Modify: `crates/freshell-ws/src/activity.rs` (`OpencodeLaneEvent` ~:187-222, intake ~:820-891, `opencode_frames` ~:1709-1760, `expire_due` ~:1478-1550, `opencode_lanes` map ~:271-289, Exit arm ~:1174-1180, `register_opencode_lane_for_tests` ~:508-514)
- Modify: `crates/freshell-activity/src/idle.rs` (Accepted Residuals entry 12, ~:87-90)

**Interfaces:**
- Consumes: Task 1's `expire` (emits `ForceRead`) and `note_verify_failed(terminal_id, at)`; existing `Lane::note(cycle, stream, event)` seam; existing fakes `FakeLaneHttp`/`FakeLaneStream` + `wait_for_ingress` + `register_opencode_lane_for_tests` harness.
- Produces:
  - `OpencodeLaneEvent::SnapshotFailed { error: String }` — new variant, lane → hub, meaning "the verify/connect snapshot probe failed in a way that must not read as idle".
  - `pub(crate) fn spawn_opencode_lane(deps, hub, terminal_id, base_url, generation) -> (tokio::task::JoinHandle<()>, tokio::sync::mpsc::UnboundedSender<()>)` — second element is the verify-request sender.
  - `HubInner.opencode_lanes: HashMap<String, (u64, tokio::task::JoinHandle<()>, tokio::sync::mpsc::UnboundedSender<()>)>`.
  - `fn opencode_frames(idle: &mut IdleGate, effects: Vec<TrackerEffect<OpencodeActivityRecord>>) -> (Vec<ServerMessage>, Vec<String>)` — now returns force-read terminal ids like `codex_frames`.
  - Test hook `#[cfg(test)] pub(crate) fn set_opencode_busy_deadman_for_tests(&self, ms: i64)` on `ActivityHub`.

- [ ] **Step 1: Write the failing lane test**

In `crates/freshell-ws/src/opencode_lane.rs` tests (reuse the harness at
~:736-882 exactly as-is), add:

```rust
    /// #603: a verify request makes the lane re-fetch /session/status and
    /// note the result with the CURRENT cycle/stream stamps (so the
    /// tracker's sameSessionStream guards accept it); a failing probe
    /// notes SnapshotFailed instead of anything idle-shaped.
    #[tokio::test(flavor = "multi_thread")]
    async fn verify_request_refetches_snapshot_with_current_stamps() {
        let log: CallLog = Arc::new(Mutex::new(Vec::new()));
        let snapshot_calls = Arc::new(AtomicUsize::new(0));
        let snapshot_calls_in_responder = snapshot_calls.clone();
        let http = FakeLaneHttp {
            log: log.clone(),
            respond: Box::new(move |url| {
                if url.ends_with("/global/health") {
                    return Ok((200, json!({"healthy": true, "version": "1.18.14"})));
                }
                if url.ends_with("/session/status") {
                    let n = snapshot_calls_in_responder.fetch_add(1, Ordering::SeqCst);
                    if n <= 1 {
                        // connect snapshot + first verify: busy
                        return Ok((200, json!({"ses-1": {"type": "busy"}})));
                    }
                    // second verify: probe failure
                    return Err("connection refused".to_string());
                }
                if url.ends_with("/session/ses-1") {
                    return Ok((200, json!({"id": "ses-1"})));
                }
                Ok((404, json!({})))
            }),
        };
        let stream = FakeLaneStream {
            log: log.clone(),
            scripts: Mutex::new(VecDeque::from([StreamScript {
                buffered: vec![],
                live: vec![],
                finish: false, // park: the cycle stays open
            }])),
        };
        let (hub, _rx) = hub();
        hub.register_opencode_lane_for_tests("t1", 7);
        let deps = Arc::new(OpencodeLaneDeps {
            http: Arc::new(http),
            events: Arc::new(stream),
        });
        let (lane, verify_tx) = spawn_opencode_lane(
            deps,
            hub.clone(),
            "t1".to_string(),
            "http://127.0.0.1:1".to_string(),
            7,
        );
        // Connect snapshot arrives first.
        let log1 = wait_for_ingress(&hub, 2, 2000).await; // SessionCreated + Snapshot
        let (gen0, cycle0, stream0, _) = log1[log1.len() - 1].clone();
        // Verify request → a SECOND /session/status GET, same stamps.
        verify_tx.send(()).expect("verify channel open");
        let log2 = wait_for_ingress(&hub, 3, 2000).await;
        let (gen1, cycle1, stream1, event1) = log2[log2.len() - 1].clone();
        assert_eq!((gen1, cycle1, stream1), (gen0, cycle0, stream0));
        assert_eq!(
            event1,
            OpencodeLaneEvent::Snapshot {
                statuses: vec![("ses-1".to_string(), OpencodeStatus::Busy)]
            }
        );
        // Second verify: the probe fails → SnapshotFailed, never idle.
        verify_tx.send(()).expect("verify channel open");
        let log3 = wait_for_ingress(&hub, 4, 2000).await;
        match &log3[log3.len() - 1].3 {
            OpencodeLaneEvent::SnapshotFailed { error } => {
                assert!(error.contains("connection refused"), "got: {error}");
            }
            other => panic!("expected SnapshotFailed, got {other:?}"),
        }
        assert_eq!(snapshot_calls.load(Ordering::SeqCst), 3);
        lane.abort();
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p freshell-ws verify_request_refetches_snapshot`
Expected: compile FAILS (`spawn_opencode_lane` returns one value; no
`SnapshotFailed` variant).

- [ ] **Step 3: Implement the lane side**

In `crates/freshell-ws/src/activity.rs`, add the variant to
`OpencodeLaneEvent` (after `PermissionReplied`, ~:221):

```rust
    /// The verify/connect `/session/status` probe failed in a way that
    /// must NOT read as idle (#603/#604). The hub applies crash semantics
    /// via [`crate::…OpencodeActivityTracker::note_verify_failed`].
    SnapshotFailed { error: String },
```

In `crates/freshell-ws/src/opencode_lane.rs`:

1. `spawn_opencode_lane` creates the channel, passes the receiver INTO
   `run` as a parameter (NOT a `Lane` field — a field would put a `&mut
   self` borrow from `verify_rx.recv()` in the same `select!` as the
   `&self` borrows of `self.note`/`self.verify`), and returns the sender:

```rust
pub(crate) fn spawn_opencode_lane(
    deps: Arc<OpencodeLaneDeps>,
    hub: ActivityHub,
    terminal_id: String,
    base_url: String,
    generation: u64,
) -> (
    tokio::task::JoinHandle<()>,
    tokio::sync::mpsc::UnboundedSender<()>,
) {
    let (verify_tx, verify_rx) = tokio::sync::mpsc::unbounded_channel();
    let lane = Lane {
        deps,
        hub,
        terminal_id,
        base_url,
        generation,
    };
    (tokio::spawn(lane.run(verify_rx)), verify_tx)
}
```

   (`async fn run(self)` becomes
   `async fn run(self, mut verify_rx: tokio::sync::mpsc::UnboundedReceiver<()>)`;
   the `Lane` struct is unchanged.)

2. Add the verify servicing method on `Lane` (next to `fetch_snapshot`):

```rust
    /// #603: service one hub verify request — re-fetch /session/status and
    /// note the answer with the CURRENT cycle/stream stamps so the
    /// tracker's stream guards accept it. A probe failure is noted as
    /// SnapshotFailed (crash semantics downstream) — NEVER as an empty
    /// (idle-shaped) snapshot.
    async fn verify(&self, cycle: u64, stream: u64, known_sessions: &mut HashSet<String>) {
        match self.fetch_snapshot().await {
            Ok(statuses) => {
                for (session_id, _) in &statuses {
                    self.resolve_root(cycle, stream, session_id, known_sessions)
                        .await;
                }
                self.note(cycle, stream, OpencodeLaneEvent::Snapshot { statuses });
            }
            Err(error) => {
                self.note(cycle, stream, OpencodeLaneEvent::SnapshotFailed { error });
            }
        }
    }
```

3. In `run`, replace the step-4 pump loop
   (`while let Some(parsed) = events_rx.recv().await { … }`) with a select
   that also drains verify requests (the body handling `parsed` is
   UNCHANGED — move it verbatim into the first arm):

```rust
                loop {
                    tokio::select! {
                        maybe_parsed = events_rx.recv() => {
                            let Some(parsed) = maybe_parsed else { break };
                            let Some(event) = translate_serve_event(&parsed) else {
                                continue;
                            };
                            // … existing known_sessions / resolve_root match,
                            //   verbatim from the current loop body …
                            self.note(cycle, stream, event);
                        }
                        Some(()) = verify_rx.recv() => {
                            self.verify(cycle, stream, &mut known_sessions).await;
                        }
                    }
                }
```

4. Also drain verify requests while the lane is between cycles (serve
   down / backing off), so a dead serve still produces the crash signal:
   replace the unconditional backoff sleep at the end of `run`
   (`tokio::time::sleep(Duration::from_millis(backoff)).await;`) with:

```rust
            // Between cycles: a verify request arriving while disconnected
            // still probes once — on a dead serve that yields
            // SnapshotFailed → crash semantics, exactly the owner ruling.
            let sleep = tokio::time::sleep(Duration::from_millis(backoff));
            tokio::pin!(sleep);
            loop {
                tokio::select! {
                    _ = &mut sleep => break,
                    Some(()) = verify_rx.recv() => {
                        self.verify(cycle, stream, &mut known_sessions).await;
                    }
                }
            }
```

- [ ] **Step 4: Implement the hub side**

In `crates/freshell-ws/src/activity.rs`:

1. `opencode_lanes` map value becomes a 3-tuple; update:
   - the field doc/type (~:276),
   - the `OpencodeAttach` arm (~:914-941): `let (handle, verify_tx) = spawn_opencode_lane(…)`, insert `(generation, handle, verify_tx)`, abort via `.1`,
   - the Exit arm (~:1174-1180) destructure `(_, lane_task, _)`,
   - the generation guard (~:841): `.map(|(g, _, _)| *g)`,
   - `register_opencode_lane_for_tests`: create a dummy channel — `let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();` and store it; ALSO add a variant used by the new lane test if needed (the lane test above registers before spawning, so the map entry gets REPLACED at attach — the lane test does not go through `OpencodeAttach`, it spawns directly, so `register_opencode_lane_for_tests("t1", 7)` with a dummy sender is sufficient for the ingress guard).
2. `opencode_frames` returns `(Vec<ServerMessage>, Vec<String>)`: change
   `TrackerEffect::ForceRead { .. } => {}` (~:1757) to
   `TrackerEffect::ForceRead { terminal_id, .. } => force_reads.push(terminal_id),`
   mirroring `codex_frames` (~:1697); update ALL call sites (the compiler
   finds them; known: lane intake ~:888, Created arm, Exit arm ~:1198,
   `expire_due` ~:1494) — outside `expire_due` the force-read list is
   dropped with a comment `// ForceRead only arises from expire()`.
3. In `expire_due`, capture opencode force reads and service them AFTER
   the lock is released (alongside the codex/amplifier drains):

```rust
            let opencode = inner.opencode.expire(now);
            let (mut f, opencode_verifies) = opencode_frames(&mut inner.idle, opencode);
            frames.append(&mut f);
```

   and after `self.emit(frames);`:

```rust
        for terminal_id in opencode_verifies {
            self.request_opencode_verify(&terminal_id);
        }
```

   with:

```rust
    /// #603: ask the terminal's lane to re-fetch /session/status. A pane
    /// with no lane has no truth source — owner ruling: probe failure =
    /// crash semantics, applied immediately.
    fn request_opencode_verify(&self, terminal_id: &str) {
        let send_failed = {
            let inner = self.inner.lock().expect("activity hub lock");
            match inner.opencode_lanes.get(terminal_id) {
                Some((_, _, verify_tx)) => verify_tx.send(()).is_err(),
                None => true,
            }
        };
        if send_failed {
            let frames = {
                let mut inner = self.inner.lock().expect("activity hub lock");
                let at = now_ms();
                let effects = inner.opencode.note_verify_failed(terminal_id, at);
                let (frames, _) = opencode_frames(&mut inner.idle, effects);
                frames
            };
            self.emit(frames);
        }
    }
```

4. Add the intake arm for the new variant (in the `OpencodeLaneEvent`
   match, ~:845-887):

```rust
                        OpencodeLaneEvent::SnapshotFailed { error } => {
                            tracing::error!(
                                terminal_id = %terminal_id,
                                %error,
                                "opencode snapshot probe failed; applying crash semantics (clear busy + attention)"
                            );
                            inner.opencode.note_verify_failed(&terminal_id, at)
                        }
```

5. Add the test hook on `ActivityHub`:

```rust
    #[cfg(test)]
    pub(crate) fn set_opencode_busy_deadman_for_tests(&self, ms: i64) {
        self.inner
            .lock()
            .expect("activity hub lock")
            .opencode
            .set_busy_deadman_ms(ms);
    }
```

- [ ] **Step 5: Write the failing hub-level end-to-end test**

In `crates/freshell-ws/src/activity.rs` tests (harness: `hub()` +
`observer_send` + `next_frame_matching`, style of the opencode death tests
at ~:2129+ / lane deps installation as in `opencode_lane.rs` tests):

```rust
    /// #603 end-to-end: deadman fires ⇒ exactly one verification GET ⇒ the
    /// busy record STAYS on the wire (no removal frame, no idle).
    #[tokio::test(flavor = "multi_thread")]
    async fn opencode_deadman_verify_keeps_busy_on_the_wire() { … }
```

Build it as: install fake lane deps whose `/session/status` always returns
`{"ses-1": {"type":"busy"}}`; send the `Created{mode:"opencode"}` registry
event + `OpencodeAttach`; drive a busy `Status` lane event via the real
lane (or `note_opencode_lane_event` with the attach generation);
`hub.set_opencode_busy_deadman_for_tests(500)`; sleep ~1.2s; assert (a) the
shared `CallLog` contains ≥2 `GET …/session/status` entries, and (b) NO
`opencode.activity.updated` frame with `remove:["t1"]` and NO
`terminal.idle` was broadcast in the interval (poll the broadcast receiver
with `next_frame_matching(…, 200ms, …)` returning `None`). Follow with a
sibling test `opencode_deadman_verify_failure_rings`: same setup but
`/session/status` starts failing after the connect snapshot — assert the
removal frame AND a `terminal.idle` (reason `grace`) arrive.

- [ ] **Step 6: Run the tests**

Run: `cargo test -p freshell-ws opencode_deadman_verify && cargo test -p freshell-ws verify_request_refetches_snapshot && cargo test -p freshell-ws && cargo test -p freshell-activity`
Expected: PASS (full crate suites — the tuple/signature changes ripple
through existing tests; fix compile errors mechanically without changing
any assertion).

- [ ] **Step 7: Update the residual registry**

In `crates/freshell-activity/src/idle.rs`, rewrite Accepted Residuals
entry 12 (~:87-90, the D8(e) mirror) to:

```rust
//! 12. (CLOSED by #603, 2026-08-06) The opencode busy deadman no longer
//!     drops the record on event silence: it verifies via GET
//!     /session/status through the lane and stays busy; a failed probe
//!     clears busy AND rings the attention boundary (owner ruling:
//!     verify-probe failure = crash/needs-attention).
```

Run: `cargo test -p freshell-activity` (doc-only change; suite green).

- [ ] **Step 8: Commit**

```bash
git add crates/freshell-ws/src/opencode_lane.rs crates/freshell-ws/src/activity.rs crates/freshell-activity/src/idle.rs
git commit -m "fix(opencode): hub+lane service deadman verify via /session/status; probe failure = crash semantics (#603)"
```

---

### Task 3: opencode snapshot parse-failure must not read as idle (#604, part 1)

**Files:**
- Modify: `crates/freshell-ws/src/opencode_lane.rs` (`fetch_snapshot` ~:280-304; `run` step 3 ~:182-192; tests)

**Interfaces:**
- Consumes: Task 2's `OpencodeLaneEvent::SnapshotFailed` and hub intake arm.
- Produces: hardened `fetch_snapshot` semantics relied on by every later snapshot consumer:
  - entry with a RECOGNIZED `status.type` (`busy`/`retry`/`idle`) → as today;
  - entry with an UNRECOGNIZED-but-present `status.type` string → `OpencodeStatus::Busy` (conservative-toward-busy, matching the stream translation's `_ => Busy`) + one `warn!` naming the unknown vocabulary;
  - entry whose value is not an object or has no string `type` → `Err(…)` (shape break — the endpoint contract itself drifted);
  - non-200 / non-object body / transport error → `Err(…)` (unchanged).
- The `run`-loop connect-snapshot failure path (step 3) now NOTES `SnapshotFailed` before backing off, instead of only `tracing::debug!`.

- [ ] **Step 1: Write the failing tests**

In `opencode_lane.rs` tests:

```rust
    /// #604: /session/status parse trouble must never read as "all idle".
    /// Unknown status VOCABULARY degrades toward busy; a SHAPE break is a
    /// probe failure (crash semantics downstream) — pinned both ways.
    #[tokio::test(flavor = "multi_thread")]
    async fn snapshot_unknown_vocabulary_is_busy_and_shape_break_is_failure() {
        // Build a Lane directly (same pattern as the other lane tests) with
        // a responder returning, on successive /session/status calls:
        //   1) {"ses-1": {"type": "hyperbusy"}}   → Busy + warn
        //   2) {"ses-1": 42}                       → Err (shape break)
        // Drive via two verify requests after connect; assert ingress:
        //   Snapshot{[("ses-1", Busy)]}, then SnapshotFailed{..}.
        …
    }
```

Write it concretely by copying `verify_request_refetches_snapshot_with_current_stamps`
(Task 2) and swapping the responder bodies and assertions:
first verify → `OpencodeLaneEvent::Snapshot { statuses: vec![("ses-1".into(), OpencodeStatus::Busy)] }`;
second verify → `SnapshotFailed { error }` with `error.contains("not an object")`
(exact message from Step 3 below). Also add a connect-time test:

```rust
    /// A failing CONNECT-cycle snapshot notes SnapshotFailed (loud, crash
    /// semantics) instead of silently backing off.
    #[tokio::test(flavor = "multi_thread")]
    async fn connect_snapshot_failure_is_noted() { … }
```

(responder: health OK, `/session/status` → `Err("boom")`, one parked
stream script; assert the FIRST ingress entry is `SnapshotFailed`).

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p freshell-ws snapshot_unknown_vocabulary connect_snapshot_failure`
Expected: FAIL — today an unknown `status.type` is skipped (test sees an
EMPTY snapshot, not Busy) and a connect snapshot failure notes nothing.

- [ ] **Step 3: Implement**

Replace the entry-parsing loop in `fetch_snapshot` (~:294-302) with:

```rust
        let mut statuses = Vec::new();
        for (session_id, entry) in map {
            match entry.get("type").and_then(|t| t.as_str()) {
                Some("busy") => statuses.push((session_id.clone(), OpencodeStatus::Busy)),
                Some("retry") => statuses.push((session_id.clone(), OpencodeStatus::Retry)),
                Some("idle") => statuses.push((session_id.clone(), OpencodeStatus::Idle)),
                Some(other) => {
                    // #604: unknown status VOCABULARY degrades toward busy
                    // (same conservative direction as the stream
                    // translation's `_ => Busy`) — a drifted vocabulary must
                    // never render a working agent as idle-green.
                    tracing::warn!(
                        terminal_id = %self.terminal_id,
                        session_id = %session_id,
                        status = %other,
                        "opencode /session/status: unknown status vocabulary; treating as busy"
                    );
                    statuses.push((session_id.clone(), OpencodeStatus::Busy));
                }
                None => {
                    // Shape break: the endpoint contract itself drifted.
                    return Err(format!(
                        "GET /session/status: entry for {session_id} is not an object with a string `type`"
                    ));
                }
            }
        }
        Ok(statuses)
```

In `run` step 3 (~:182-192), change the `Err` arm to note the failure
before backing off:

```rust
                    let statuses = match self.fetch_snapshot().await {
                        Ok(statuses) => statuses,
                        Err(error) => {
                            tracing::debug!(
                                terminal_id = %self.terminal_id,
                                %error,
                                "opencode lane snapshot failed; backing off"
                            );
                            // #604: a failing snapshot probe must not read
                            // as idle — surface it (crash semantics in the
                            // hub) rather than silently holding state.
                            self.note(
                                cycle,
                                stream,
                                OpencodeLaneEvent::SnapshotFailed { error },
                            );
                            break 'cycle false;
                        }
                    };
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p freshell-ws && cargo test -p freshell-activity`
Expected: PASS (note: `lane_gates_on_health_then_snapshots_then_streams`
and `reconnect_bumps_stream_and_resnapshots` must remain green — their
responders return well-formed snapshots).

- [ ] **Step 5: Commit**

```bash
git add crates/freshell-ws/src/opencode_lane.rs
git commit -m "fix(opencode): snapshot parse trouble degrades toward busy or fails loud, never idle (#604)"
```

---

### Task 4: opencode event vocabulary — v2 + question families (#604, part 2)

**Files:**
- Modify: `crates/freshell-ws/src/opencode_lane.rs` (`translate_serve_event` ~:394-463; table-driven test ~:886-1008)
- Modify: `crates/freshell-activity/src/idle.rs` (Accepted Residuals entry 9, ~:68-74)

**Interfaces:**
- Consumes: spike-verified opencode 1.18.14 schemas (from the live OpenAPI doc): BOTH v1 and v2 event families coexist in the `Event` union; `/event` applies NO type downgrade (whichever type the bus publishes arrives raw). Payload facts: `permission.v2.asked` keeps `id`/`sessionID` (renames only `permission→action`, `patterns→resources`, `always→save`, `tool→source` — none of which freshell reads); `permission.v2.replied` is field-identical to v1 (`sessionID`, `requestID`, `reply`); `question.asked`/`question.v2.asked` carry `id` (pattern `^que`), `sessionID`, `questions`; `question.replied`/`.v2.replied` carry `requestID`; `question.rejected`/`.v2.rejected` carry `requestID`. Question ids (`^que`) and permission ids (`^per`) cannot collide, so questions reuse the permission pause machinery unchanged.
- Produces: `translate_serve_event` rows mapping all of the following onto the EXISTING `OpencodeLaneEvent` variants (reducer untouched):
  - `permission.v2.asked` → `PermissionAsked`
  - `permission.v2.replied` → `PermissionReplied`
  - `question.asked`, `question.v2.asked` → `PermissionAsked`
  - `question.replied`, `question.v2.replied`, `question.rejected`, `question.v2.rejected` → `PermissionReplied`

- [ ] **Step 1: Add failing rows to the table-driven test**

In `translate_covers_the_attention_vocabulary` (~:890), append to the
`frames` array (indexes 14-21):

```rust
            // #604: v2 + question families — schema-verified against the
            // installed opencode 1.18.14 OpenAPI (spike 2026-08-06). The
            // /event stream applies NO type downgrade, so both families
            // can arrive raw.
            json!({"type":"permission.v2.asked","properties":{"sessionID":"ses-1","id":"per-2","action":"bash","resources":["*"]}}),
            json!({"type":"permission.v2.replied","properties":{"sessionID":"ses-1","requestID":"per-2","reply":"once"}}),
            json!({"type":"question.asked","properties":{"sessionID":"ses-1","id":"que-1","questions":[{"question":"Proceed?","header":"Confirm","options":[]}]}}),
            json!({"type":"question.replied","properties":{"sessionID":"ses-1","requestID":"que-1","answers":[["yes"]]}}),
            json!({"type":"question.v2.asked","properties":{"sessionID":"ses-1","id":"que-2","questions":[{"question":"Proceed?","header":"Confirm","options":[]}]}}),
            json!({"type":"question.v2.rejected","properties":{"sessionID":"ses-1","requestID":"que-2"}}),
            json!({"type":"question.rejected","properties":{"sessionID":"ses-1","requestID":"que-1"}}),
            json!({"type":"question.v2.replied","properties":{"sessionID":"ses-1","requestID":"que-2","answers":[[]]}}),
```

and the matching positional assertions:

```rust
        assert_eq!(
            translated[14],
            Some(OpencodeLaneEvent::PermissionAsked {
                session_id: "ses-1".to_string(),
                permission_id: "per-2".to_string(),
            }),
            "permission.v2.asked keeps id/sessionID — one reducer, two families"
        );
        assert_eq!(
            translated[15],
            Some(OpencodeLaneEvent::PermissionReplied {
                permission_id: "per-2".to_string(),
            })
        );
        assert_eq!(
            translated[16],
            Some(OpencodeLaneEvent::PermissionAsked {
                session_id: "ses-1".to_string(),
                permission_id: "que-1".to_string(),
            }),
            "question.asked is a blocker identically to permission.asked (opencode's own TUI treats it so)"
        );
        assert_eq!(
            translated[17],
            Some(OpencodeLaneEvent::PermissionReplied {
                permission_id: "que-1".to_string(),
            })
        );
        assert_eq!(
            translated[18],
            Some(OpencodeLaneEvent::PermissionAsked {
                session_id: "ses-1".to_string(),
                permission_id: "que-2".to_string(),
            })
        );
        assert_eq!(
            translated[19],
            Some(OpencodeLaneEvent::PermissionReplied {
                permission_id: "que-2".to_string(),
            }),
            "a rejected question ends the pause exactly like a reply"
        );
        assert_eq!(
            translated[20],
            Some(OpencodeLaneEvent::PermissionReplied {
                permission_id: "que-1".to_string(),
            })
        );
        assert_eq!(
            translated[21],
            Some(OpencodeLaneEvent::PermissionReplied {
                permission_id: "que-2".to_string(),
            })
        );
```

(Also bump the `assert_eq!(events.len(), frames.len(), …)` expectation —
it derives from `frames.len()`, so no literal changes.)

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p freshell-ws translate_covers_the_attention_vocabulary`
Expected: FAIL — indexes 14-21 currently translate to `None`.

- [ ] **Step 3: Implement**

In `translate_serve_event`, extend the two permission arms and add the
question arms (replace the `"permission.asked"` and `"permission.replied"`
match arms):

```rust
        // #604: v1 + v2 + question families all feed the SAME two lane
        // events (one reducer, many spellings). Verified against opencode
        // 1.18.14's OpenAPI: v2 renames payload fields freshell doesn't
        // read (permission→action, patterns→resources, always→save,
        // tool→source) and keeps id/sessionID; question ids (^que) can't
        // collide with permission ids (^per), so questions reuse the
        // permission pause machinery unchanged. question.rejected ends
        // the pause exactly like a reply.
        "permission.asked" | "permission.v2.asked" | "question.asked" | "question.v2.asked" => {
            Some(OpencodeLaneEvent::PermissionAsked {
                session_id: props.get("sessionID")?.as_str()?.to_string(),
                permission_id: props.get("id")?.as_str()?.to_string(),
            })
        }
        "permission.replied"
        | "permission.v2.replied"
        | "question.replied"
        | "question.v2.replied"
        | "question.rejected"
        | "question.v2.rejected" => Some(OpencodeLaneEvent::PermissionReplied {
            permission_id: props.get("requestID")?.as_str()?.to_string(),
        }),
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p freshell-ws`
Expected: PASS.

- [ ] **Step 5: Update the residual registry**

In `crates/freshell-activity/src/idle.rs`, rewrite Accepted Residuals
entry 9 (~:68-74, the D8(d) mirror):

```rust
//!  9. (RE-SCOPED by #604, 2026-08-06) permission.v2.* and question.*
//!     families (v1 and v2) are now translated onto the same pause
//!     machinery, schema-verified against opencode 1.18.14. Residual:
//!     a FUTURE never-before-seen event family cannot be handled in
//!     advance — mitigated by the snapshot poll (turn lights stay
//!     correct regardless of stream vocabulary, #603) and the loud
//!     drift detector (#604); bells for a brand-new family stay deaf
//!     until vocabulary is updated (adjudicated: acceptable).
```

- [ ] **Step 6: Commit**

```bash
git add crates/freshell-ws/src/opencode_lane.rs crates/freshell-activity/src/idle.rs
git commit -m "fix(opencode): translate permission.v2.* and question.* families onto the pause machinery (#604)"
```

---

### Task 5: opencode drift contradiction detector (#604, part 3)

**Files:**
- Modify: `crates/freshell-ws/src/opencode_lane.rs` (run loop locals + `verify()`; new pure fn + unit test)
- Modify: `crates/freshell-ws/src/lib.rs` or `opencode_lane.rs` (public `OPENCODE_DRIFT_EVENTS` counter)
- Modify: `crates/freshell-server/src/diag.rs` (`server_info_body` ~:92-106 + its unit test)

**Interfaces:**
- Consumes: Task 2's verify path (`Lane::verify`), `Lane::run` locals.
- Produces:
  - `pub fn drift_contradiction(busy_in_snapshot: bool, recognized_since_verify: u64) -> bool` in `opencode_lane.rs` — pure rule: `busy_in_snapshot && recognized_since_verify == 0`.
  - `pub static OPENCODE_DRIFT_EVENTS: std::sync::atomic::AtomicU64` (exported from `freshell-ws`, e.g. `pub static OPENCODE_DRIFT_EVENTS: AtomicU64 = AtomicU64::new(0);` in `opencode_lane.rs`, re-exported from the crate root).
  - `/api/server-info` gains `"opencodeDriftEvents": <u64>` (additive field).

The deterministic contradiction: the deadman verify fired (⇒ ≥120s of
event silence), the snapshot says a session is BUSY, and the stream
translated ZERO recognized events since the previous verify on this
stream — machine-checkable evidence the stream vocabulary has drifted
while the snapshot (the load-bearing truth source after #603) keeps the
lights correct. Surface: `error!`-level log, once per stream, plus a
monotonic counter on `/api/server-info`.

- [ ] **Step 1: Write the failing tests**

In `opencode_lane.rs` tests:

```rust
    #[test]
    fn drift_contradiction_rule() {
        assert!(drift_contradiction(true, 0));
        assert!(!drift_contradiction(true, 3));
        assert!(!drift_contradiction(false, 0));
    }
```

In `crates/freshell-server/src/diag.rs`, find the existing unit test of
`server_info_body` (the fn was split out to be unit-testable — grep
`server_info_body` in the file's test module) and add to it:

```rust
        assert!(
            body.get("opencodeDriftEvents").and_then(|v| v.as_u64()).is_some(),
            "server-info surfaces the opencode drift counter (#604)"
        );
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p freshell-ws drift_contradiction_rule && cargo test -p freshell-server server_info`
Expected: compile FAIL (`drift_contradiction` and the field don't exist).

- [ ] **Step 3: Implement**

In `opencode_lane.rs` (top level):

```rust
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

/// #604: count of detected stream-vocabulary drift contradictions since
/// boot (snapshot busy + zero recognized stream events across a verify
/// window). Read by GET /api/server-info.
pub static OPENCODE_DRIFT_EVENTS: AtomicU64 = AtomicU64::new(0);

/// #604 drift rule: the deadman verify found the session busy while the
/// open stream produced zero recognized events since the last verify.
pub fn drift_contradiction(busy_in_snapshot: bool, recognized_since_verify: u64) -> bool {
    busy_in_snapshot && recognized_since_verify == 0
}
```

Wire the counter through the run loop:
- `run` gains locals `let mut recognized_since_verify: u64 = 0;` and
  `let mut drift_logged_this_stream = false;` (reset both where
  `stream += 1` happens).
- In the pump arm, after `let Some(event) = translate_serve_event(&parsed) else { continue; };`
  add `recognized_since_verify += 1;`.
- `verify()` gains two `&mut` params (`recognized_since_verify`,
  `drift_logged_this_stream`) — or simplest: inline the drift check at the
  verify call sites in `run`, where the locals are in scope:

```rust
                        Some(()) = self.verify_rx.recv() => {
                            let busy = self
                                .verify(cycle, stream, &mut known_sessions)
                                .await;
                            if drift_contradiction(busy, recognized_since_verify)
                                && !drift_logged_this_stream
                            {
                                drift_logged_this_stream = true;
                                OPENCODE_DRIFT_EVENTS.fetch_add(1, AtomicOrdering::SeqCst);
                                tracing::error!(
                                    terminal_id = %self.terminal_id,
                                    "opencode stream vocabulary drift suspected: session busy in /session/status but ZERO recognized stream events across a verify window; turn lights remain snapshot-driven (#604)"
                                );
                            }
                            recognized_since_verify = 0;
                        }
```

  Change `verify()` to return `bool` ("any busy/retry entry in the
  snapshot"; `false` on failure):

```rust
    async fn verify(
        &self,
        cycle: u64,
        stream: u64,
        known_sessions: &mut HashSet<String>,
    ) -> bool {
        match self.fetch_snapshot().await {
            Ok(statuses) => {
                let busy = statuses
                    .iter()
                    .any(|(_, s)| *s != OpencodeStatus::Idle);
                for (session_id, _) in &statuses {
                    self.resolve_root(cycle, stream, session_id, known_sessions)
                        .await;
                }
                self.note(cycle, stream, OpencodeLaneEvent::Snapshot { statuses });
                busy
            }
            Err(error) => {
                self.note(cycle, stream, OpencodeLaneEvent::SnapshotFailed { error });
                false
            }
        }
    }
```

  (The between-cycles verify arm from Task 2 ignores the return value —
  there is no open stream to contradict.)

In `crates/freshell-ws/src/lib.rs` ensure `opencode_lane` is reachable for
the server crate (it already is — `freshell_ws::opencode_lane::ReqwestLaneHttp`
is referenced from `main.rs:517`; the static is `pub` in that module).

In `crates/freshell-server/src/diag.rs`, add to `server_info_body`'s
`json!` (after `"buildDirty"`):

```rust
        // #604: drift contradictions detected by the opencode lane since
        // boot (additive diagnostics field, never replacing an existing
        // one — same rule as commit/buildDirty).
        "opencodeDriftEvents": freshell_ws::opencode_lane::OPENCODE_DRIFT_EVENTS
            .load(std::sync::atomic::Ordering::SeqCst),
```

(If `freshell-server` does not already depend on `freshell-ws` in
`crates/freshell-server/Cargo.toml`, it does — the hub is constructed in
`main.rs`; no manifest change needed.)

- [ ] **Step 4: Run the tests**

Run: `cargo test -p freshell-ws && cargo test -p freshell-server`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/freshell-ws/src/opencode_lane.rs crates/freshell-server/src/diag.rs
git commit -m "feat(opencode): loud drift contradiction detector + server-info counter (#604)"
```

---

### Task 6: opencode deterministic identity — lane busy roots bind directly (#609)

**Files:**
- Modify: `crates/freshell-activity/src/opencode.rs` (`reduce_busy_edge` Quiet arm ~:748-767; `reduce_snapshot` Quiet + single-foreign-root arm ~:1024-1034; affected tests)
- Modify: `crates/freshell-ws/src/activity.rs` (new hub death test)
- Modify: `crates/freshell-activity/src/idle.rs` (Accepted Residuals entry 6, ~:52-59)

**Rationale (from the issue's approved direction):** the Rust lane talks to
the pane's OWN per-pane opencode server; lanes exist only for
freshell-managed panes, events are generation/cycle/stream-guarded, and
session ids are root-resolved before they reach the tracker. On a per-pane
endpoint, *the busy root session observed on that endpoint IS the pane's
session* — identity is confirmed by construction, so a Quiet pane's busy
root promotes DIRECTLY to `KnownBusy` (no `Candidate` detour, no wait for
SQLite-locator/plugin luck). D4's rule is preserved (`blocks_death_bell`
still blocks Candidate/Ambiguous/AwaitingAssociation) — those states simply
stop arising on the lane path, which by construction fixes both first-turn
death silence and the indefinite-candidate tail. Shared-endpoint
externally-attached panes have no lane, produce no busy edges, and keep
today's behavior (adjudicated: keep-D4-silence). The `Candidate`/
`AwaitingAssociation` variants and `bind_session` REMAIN — the SQLite
locator and TUI rebind plugin producers still call `bind_session`
(`activity.rs:820`), and defense-in-depth keeps the gate rules intact.

**Interfaces:**
- Consumes: existing `Ownership` machine, `set_busy_record`, `bind_session`.
- Produces: new reducer semantics relied on by Tasks 7/8:
  - `reduce_busy_edge`, `Ownership::Quiet` arm: ANY busy root → `KnownBusy { session_id, cycle, stream, turn_aborted: false }` (whether or not it matches `known_session_id`).
  - `reduce_snapshot`, `Quiet{known: Some(k)}` + single foreign busy root arm: → `KnownBusy` (was `Candidate{previous_known: Some(k)}`).

- [ ] **Step 1: Write the failing tests**

In `opencode.rs` tests, add:

```rust
    #[test]
    fn first_turn_busy_root_binds_directly_and_is_death_eligible() {
        // #609: on the pane's own per-pane endpoint the busy root IS the
        // pane's session — no Candidate detour, first-turn deaths ring.
        let mut tracker = OpencodeActivityTracker::new();
        tracker.track_terminal("t1", None, 0);
        assert_eq!(
            tracker.note_status("t1", "ses-x", OpencodeStatus::Busy, 1, 1, 100),
            vec![upsert(rec(Some("ses-x"), 100))]
        );
        assert!(
            !tracker.blocks_death_bell("t1"),
            "first-turn ownership is confirmed by construction (#609)"
        );
        // The first turn's idle edge completes IMMEDIATELY — no deferral.
        assert_eq!(
            tracker.note_session_idle("t1", "ses-x", 1, 1, 200),
            vec![remove(), turn_complete("ses-x", 200, 1)]
        );
    }

    #[test]
    fn superseded_session_rebinds_directly() {
        // A NEW root going busy on the pane's endpoint (e.g. /new in the
        // TUI) is the pane's new session — rebind, don't candidate.
        let mut tracker = OpencodeActivityTracker::new();
        tracker.track_terminal("t1", Some("ses-old"), 0);
        assert_eq!(
            tracker.note_status("t1", "ses-new", OpencodeStatus::Busy, 1, 1, 100),
            vec![upsert(rec(Some("ses-new"), 100))]
        );
        assert!(!tracker.blocks_death_bell("t1"));
        assert_eq!(
            tracker.note_session_idle("t1", "ses-new", 1, 1, 200),
            vec![remove(), turn_complete("ses-new", 200, 1)]
        );
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p freshell-activity first_turn_busy_root superseded_session`
Expected: FAIL — today both go `Candidate` (`blocks_death_bell` true,
completion deferred).

- [ ] **Step 3: Implement**

Replace the `Ownership::Quiet` arm of `reduce_busy_edge` (~:749-767) with:

```rust
        Ownership::Quiet { .. } => {
            // #609: busy edges reach this tracker ONLY from the pane's own
            // per-pane lane (generation/cycle/stream-guarded, root-
            // resolved), so the busy root IS the pane's session — identity
            // confirms by construction. Direct KnownBusy: first-turn asks
            // ring (KnownBusy arming), first-turn deaths are eligible, and
            // the indefinite-candidate tail cannot form. Candidate/
            // AwaitingAssociation remain for the locator/plugin bind
            // producers and defense in depth.
            state.ownership = Ownership::KnownBusy {
                session_id: session_id.to_string(),
                cycle,
                stream,
                turn_aborted: false,
            };
            set_busy_record(state, Some(session_id.to_string()), at)
        }
```

In `reduce_snapshot`, change the `Quiet{known: Some(k)}` + single foreign
busy root arm (~:1024-1034) the same way: ownership becomes
`Ownership::KnownBusy { session_id: root.clone(), cycle, stream, turn_aborted: false }`
(instead of `Candidate{previous_known: Some(k), …}`), record via
`set_busy_record(state, Some(root), at)`. Keep every other arm unchanged.

**Existing tests to update deliberately (each inversion is the #609 fix,
not a regression — say so in each test's comment):**
- `candidate_completion_defers_to_bind_session` (~:1206) — DELETE;
  superseded by `first_turn_busy_root_binds_directly_and_is_death_eligible`.
- `candidate_pause_arms_and_deferred_completion_is_swallowed_at_bind`
  (~:1448) — REWRITE as `first_turn_pause_arms_and_completion_is_swallowed`:
  same event sequence, but now the busy edge yields `KnownBusy`, the ask
  arms (`[remove(), boundary(150)]` unchanged), the idle edge mid-pause
  yields `Vec::new()` (KnownBusy mid-pause arm), no `bind_session` needed,
  and `has_pending_permissions` clears on the idle edge (KnownBusy arm
  clears at :665). Assert the follow-up turn completes with seq 1.
- `snapshot_single_foreign_busy_from_quiet_known_enters_candidate`
  (~:1262) — RENAME to `snapshot_single_foreign_busy_from_quiet_known_rebinds`
  and invert: after the snapshot, `blocks_death_bell` is false and the
  subsequent idle edge mints a completion.
- `death_predicates` (~:1564) — the middle section that builds Candidate
  via `track_terminal(None)` + busy: now expects `blocks_death_bell ==
  false` after the busy edge; the pause-claim-survives-into-
  AwaitingAssociation sub-case is no longer constructible via note_status —
  replace that sub-case with the Ambiguous construction (two busy roots)
  which still blocks. Keep the Quiet/KnownBusy and Ambiguous sections.
- `rejected_bind_clears_stale_pause_claim` (~:1631) — this constructs
  AwaitingAssociation via a candidate idle; no longer constructible from
  busy edges. DELETE (its invariant — bind_session's AwaitingAssociation
  mismatch arm retires stale claims — keeps compile coverage via
  `bind_session` itself; the arm stays for locator/plugin producers).
- Ambiguous tests (`ambiguous_is_conservative_no_completions`,
  `ambiguous_drain_via_*`) — still valid: two busy roots still demote to
  Ambiguous (now with `known_session_id: Some(first)`); assertions on
  effects are unchanged. Verify they still pass; adjust only if an
  assertion inspected `previous_known`.

- [ ] **Step 4: Run the tracker suite**

Run: `cargo test -p freshell-activity`
Expected: PASS after the deliberate test updates above.

- [ ] **Step 5: Hub-level death test**

In `activity.rs` tests (exit-arm harness, style of
`spontaneous_exit_while_busy_rings_terminal_idle_once` at ~:2128):

```rust
    /// #609: an opencode pane's FIRST turn is death-eligible — a busy
    /// root on the pane's own lane confirms identity by construction.
    #[tokio::test(flavor = "multi_thread")]
    async fn opencode_first_turn_spontaneous_exit_rings() { … }
```

Concretely: `Created{mode:"opencode", resume_session_id: None}`;
`register_opencode_lane_for_tests("t1", 1)`; inject
`note_opencode_lane_event("t1", 1, 1, 1, Status{session_id:"ses-x", status:Busy})`;
then `observer_send(Exit{terminal_id:"t1", at, spontaneous:true})`; assert a
`terminal.idle` frame with `reason == "grace"` arrives (use
`next_frame_matching(rx, "terminal.idle", …)`). Before this task the same
sequence stays silent (Candidate blocks) — run the test once before Step 3
lands if you want to see the red state; in execution order it is written
after, so instead pin the INVERSE guard: add a sibling
`opencode_ambiguous_exit_stays_silent` (two busy roots, then spontaneous
exit ⇒ NO terminal.idle within 300ms) so D4's remaining gate is pinned.

Run: `cargo test -p freshell-ws opencode_first_turn opencode_ambiguous_exit`
Expected: PASS.

- [ ] **Step 6: Update the residual registry**

`crates/freshell-activity/src/idle.rs` entry 6 (~:52-59):

```rust
//!  6. (CLOSED for lane-backed panes by #609, 2026-08-06) A busy root on
//!     the pane's own per-pane lane binds identity directly (KnownBusy) —
//!     first-turn deaths ring and the indefinite-candidate tail cannot
//!     form. D4 unchanged: Candidate/Ambiguous/AwaitingAssociation still
//!     never death-ring. Residual (adjudicated): externally-attached
//!     panes on a SHARED opencode endpoint have no lane and keep
//!     conservative silence.
```

- [ ] **Step 7: Commit**

```bash
git add crates/freshell-activity/src/opencode.rs crates/freshell-ws/src/activity.rs crates/freshell-activity/src/idle.rs
git commit -m "fix(opencode): per-pane lane busy roots bind identity directly; first turns get bells and death rings (#609)"
```

---

### Task 7: opencode Ambiguous resolves via the verify snapshot (#610)

**Files:**
- Modify: `crates/freshell-activity/src/opencode.rs` (`reduce_snapshot` Ambiguous arms ~:864-1060; tests)

**Interfaces:**
- Consumes: Task 1's verify cycle (`expire` fires `ForceRead` for ANY busy record — the Ambiguous session-less record included, so resolution retries actively every deadman window); the lane's snapshot root-resolution (snapshot ids are resolved to roots via synthetic `SessionCreated` BEFORE the snapshot is noted — `run` step 3 and `verify()` both do this).
- Produces: `reduce_snapshot` Ambiguous semantics relied on by the tests:
  - Ambiguous + snapshot collapsing to EXACTLY ONE busy root → re-promote to `KnownBusy{root, cycle, stream, turn_aborted:false}` + `set_busy_record(Some(root))`; `pending_permissions` retained (a still-outstanding ask keeps its claim; a stale one drains via replied/idle rules).
  - Ambiguous + snapshot with 2+ busy roots → stay Ambiguous (blocked := the snapshot's busy roots) + `warn!` (genuinely-plural — adjudicated residual, structurally near-impossible on per-pane endpoints after #609).
  - Ambiguous + empty snapshot → unchanged (drain to Quiet, clear pause claim — existing behavior, already pinned).

- [ ] **Step 1: Write the failing test**

```rust
    #[test]
    fn ambiguous_repromotes_on_single_root_snapshot_and_then_bells() {
        // #610: resolve the ambiguity deterministically instead of
        // waiting it out — the verify snapshot's root collapse picks the
        // one true root; the next idle edge mints the completion that the
        // old quiet drain silently skipped.
        let mut tracker = OpencodeActivityTracker::new();
        tracker.track_terminal("t1", None, 0);
        tracker.note_status("t1", "ses-a", OpencodeStatus::Busy, 1, 1, 100);
        // ses-c was mis-seen as a root during an SSE gap (D8(c)) → Ambiguous.
        tracker.note_status("t1", "ses-c", OpencodeStatus::Busy, 1, 1, 110);
        assert!(tracker.blocks_death_bell("t1"));
        // The lane's root resolution catches up: ses-c is a CHILD of ses-a.
        tracker.note_session_created("t1", "ses-c", Some("ses-a"), 120);
        // Verify snapshot: only ses-c busy — collapses to root ses-a.
        assert_eq!(
            tracker.note_snapshot(
                "t1",
                &[("ses-c".to_string(), OpencodeStatus::Busy)],
                1,
                1,
                130
            ),
            vec![upsert(rec(Some("ses-a"), 130))],
            "re-promotion restores the session on the record"
        );
        assert!(!tracker.blocks_death_bell("t1"));
        // The turn's idle edge now MINTS the completion (the whole point).
        assert_eq!(
            tracker.note_session_idle("t1", "ses-a", 1, 1, 200),
            vec![remove(), turn_complete("ses-a", 200, 1)]
        );
    }

    #[test]
    fn ambiguous_with_two_true_roots_stays_conservative() {
        let mut tracker = OpencodeActivityTracker::new();
        tracker.track_terminal("t1", None, 0);
        tracker.note_status("t1", "ses-a", OpencodeStatus::Busy, 1, 1, 100);
        tracker.note_status("t1", "ses-b", OpencodeStatus::Busy, 1, 1, 110);
        // Two independent busy ROOTS in the snapshot: no deterministic
        // single owner — stay Ambiguous (adjudicated residual), honest
        // blue, quiet drain.
        assert!(tracker
            .note_snapshot(
                "t1",
                &[
                    ("ses-a".to_string(), OpencodeStatus::Busy),
                    ("ses-b".to_string(), OpencodeStatus::Busy)
                ],
                1,
                1,
                130
            )
            .is_empty());
        assert!(tracker.blocks_death_bell("t1"));
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p freshell-activity ambiguous_repromotes ambiguous_with_two_true_roots`
Expected: `ambiguous_repromotes…` FAILS (today the snapshot updates the
blocked set / keeps Ambiguous; no re-promotion upsert). The two-root test
may already pass — keep it as the pin.

- [ ] **Step 3: Implement**

In `reduce_snapshot`'s `Ownership::Ambiguous` handling (the non-empty-roots
arm; the empty-roots arm at ~:876-883 stays as-is), make the branch:

```rust
            Ownership::Ambiguous {
                known_session_id,
                blocked,
            } => {
                if busy_roots.is_empty() {
                    // existing drain-to-Quiet arm — UNCHANGED
                    …
                } else if busy_roots.len() == 1 {
                    // #610: the snapshot's root collapse resolved the
                    // ambiguity — one busy root on the pane's own endpoint
                    // is the pane's session (same determinism as #609).
                    // Re-promote; the pause claim (if any) stays with the
                    // episode and drains via the normal D3 rules.
                    let root = busy_roots[0].clone();
                    state.ownership = Ownership::KnownBusy {
                        session_id: root.clone(),
                        cycle,
                        stream,
                        turn_aborted: false,
                    };
                    set_busy_record(state, Some(root), at)
                } else {
                    // Genuinely plural busy roots: no deterministic single
                    // owner (adjudicated residual — structurally
                    // near-impossible on per-pane endpoints after #609).
                    tracing::warn!(
                        component = "opencode-activity-tracker",
                        terminal_id = %state.terminal_id,
                        roots = busy_roots.len(),
                        "opencode pane observes multiple busy ROOT sessions; staying conservatively silent (D8(a))"
                    );
                    state.ownership = Ownership::Ambiguous {
                        known_session_id,
                        blocked: unique_sorted(busy_roots),
                    };
                    let _ = blocked;
                    set_busy_record(state, None, at)
                }
            }
```

(Adapt to the actual match structure of `reduce_snapshot` — the existing
arm already destructures `known_session_id`/`blocked`; keep the empty-roots
sub-arm byte-identical.)

- [ ] **Step 4: Run the suite**

Run: `cargo test -p freshell-activity`
Expected: PASS. Check `ambiguous_drain_via_snapshot_clears_stale_pause_claim`
(~:1700) specifically — it drains via an EMPTY snapshot, untouched by this
change.

- [ ] **Step 5: Commit**

```bash
git add crates/freshell-activity/src/opencode.rs
git commit -m "fix(opencode): ambiguous ownership re-promotes from a single-busy-root verify snapshot; completions ring again (#610)"
```

---

### Task 8: opencode permission pause survives snapshots + GET /permission resync (#608)

**Files:**
- Modify: `crates/freshell-activity/src/opencode.rs` (`reduce_snapshot` KnownBusy single-matching-root arm ~:945-954; tests)
- Modify: `crates/freshell-ws/src/opencode_lane.rs` (connect sequence step 3.5; new `fetch_permissions`; tests)

**Interfaces:**
- Consumes: spike-verified `GET /permission` on opencode 1.18.x (verified live on 1.18.14, opId `permission.list`: "all pending permission requests across all sessions", legacy shape `{ id: "^per", sessionID: "^ses", permission, patterns, metadata, always, tool? }`, `[]`/HTTP 200 when none; version floor: 1.18.x — the endpoint is part of the same 1.18 surface the lane already requires; on any older opencode without it the fetch fails and reconnect behaves as today, name this floor in the PR). Existing `note_permission_asked` idempotence ("Only a NEWLY inserted permission id arms", `opencode.rs:508-510`).
- Produces:
  - Tracker: while `pending_permissions` is non-empty, a busy snapshot for the owned root REFRESHES stamps only — it does NOT restore the busy record and does NOT clear the pause (mid-pause is record-absent by design, D3).
  - Lane: `async fn fetch_permissions(&self) -> Result<Vec<(String, String)>, String>` (session_id, permission_id pairs) + connect-sequence ordering: permissions are replayed into the tracker BEFORE the snapshot is noted.

- [ ] **Step 1: Write the failing tracker test**

```rust
    #[test]
    fn busy_snapshot_does_not_clear_an_outstanding_pause() {
        // #608: a blocked-on-permission session still reports BUSY in
        // /session/status — the reconnect snapshot must not resurrect the
        // busy record or forget the pause (that is exactly how the pending
        // bell got lost, residual D8(b)).
        let mut tracker = OpencodeActivityTracker::new();
        tracker.track_terminal("t1", Some("ses-r"), 0);
        tracker.note_status("t1", "ses-r", OpencodeStatus::Busy, 1, 1, 100);
        assert_eq!(
            tracker.note_permission_asked("t1", "ses-r", "perm-1", 150),
            vec![remove(), boundary(150)]
        );
        // Reconnect snapshot (new cycle): session busy — pause SURVIVES.
        assert!(tracker
            .note_snapshot(
                "t1",
                &[("ses-r".to_string(), OpencodeStatus::Busy)],
                2,
                1,
                200
            )
            .is_empty());
        assert!(tracker.has_pending_permissions("t1"));
        assert!(tracker.list().is_empty(), "mid-pause: record stays absent");
        // The reply still resumes busy normally.
        assert_eq!(
            tracker.note_permission_replied("t1", "perm-1", 300),
            vec![upsert(rec(Some("ses-r"), 300))]
        );
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p freshell-activity busy_snapshot_does_not_clear_an_outstanding_pause`
Expected: FAIL — today the snapshot's KnownBusy single-matching-root arm
clears `pending_permissions` (`:947`) and calls `set_busy_record`.

- [ ] **Step 3: Implement the tracker half**

In `reduce_snapshot`'s KnownBusy + single-matching-root arm (~:945-954),
guard the busy-resume on the pause being empty:

```rust
                // Busy refresh for the owned root. #608: while a pause is
                // outstanding the record stays absent and the claim stays —
                // only permission.replied (or a STREAM busy edge = genuine
                // resume) ends a pause; a snapshot is an observation, not a
                // resume. Stamps still refresh so stream guards keep
                // accepting this turn's edges.
                if state.pending_permissions.is_empty() {
                    state.ownership = Ownership::KnownBusy {
                        session_id: own.clone(),
                        cycle,
                        stream,
                        turn_aborted: false,
                    };
                    set_busy_record(state, Some(own), at)
                } else {
                    state.ownership = Ownership::KnownBusy {
                        session_id: own.clone(),
                        cycle,
                        stream,
                        turn_aborted: false,
                    };
                    Vec::new()
                }
```

Check `busy_snapshot_refresh_rearms_the_abort_gate` (~:1609) still passes
(its scenario has no pending permissions — unaffected).

- [ ] **Step 4: Write the failing lane test (replay-before-snapshot ordering)**

In `opencode_lane.rs` tests (copy the connect-flow harness from
`lane_gates_on_health_then_snapshots_then_streams`):

```rust
    /// #608: on (re)connect the lane asks GET /permission and replays
    /// outstanding asks into the tracker BEFORE the snapshot is noted, so
    /// an ask that happened during the SSE gap still arms the pause.
    #[tokio::test(flavor = "multi_thread")]
    async fn connect_replays_outstanding_permissions_before_snapshot() {
        // Responder: health OK; /permission →
        //   [{"id":"perm-9","sessionID":"ses-1","permission":"bash",
        //     "patterns":[],"metadata":{},"always":[]}]
        //   ; /session/status → {"ses-1":{"type":"busy"}};
        //   /session/ses-1 → {"id":"ses-1"}. One parked stream script.
        // Assert ingress ordering: the PermissionAsked{ses-1, perm-9}
        // entry's index is LOWER than the Snapshot entry's index, and both
        // carry the same (generation, cycle, stream).
        …
    }
```

Write it fully using the Task 2 test as the template; the ordering
assertion is
`assert!(idx_of_permission_asked < idx_of_snapshot, "replay must precede the snapshot")`,
computed with `.iter().position(...)` over `wait_for_ingress(&hub, 3, 2000)`.

Run: `cargo test -p freshell-ws connect_replays_outstanding_permissions`
Expected: FAIL (no `/permission` GET happens; no PermissionAsked ingress).

- [ ] **Step 5: Implement the lane half**

Add next to `fetch_snapshot`:

```rust
    /// #608: GET {base}/permission — all pending permission asks across
    /// sessions (legacy shape; endpoint verified live on opencode 1.18.14,
    /// opId permission.list; version floor 1.18.x). Returns
    /// (session_id, permission_id) pairs. Failure is NON-FATAL for the
    /// cycle: the stream + snapshot still carry the lights; the pause
    /// resync just doesn't happen this cycle (retried next reconnect).
    async fn fetch_permissions(&self) -> Result<Vec<(String, String)>, String> {
        let url = format!("{}/permission", self.base_url);
        let (status, body) = self.deps.http.get_json(&url).await?;
        if status != 200 {
            return Err(format!("GET /permission returned {status}"));
        }
        let list = body
            .as_array()
            .ok_or_else(|| "GET /permission: body is not an array".to_string())?;
        let mut asks = Vec::new();
        for entry in list {
            let (Some(session_id), Some(permission_id)) = (
                entry.get("sessionID").and_then(|v| v.as_str()),
                entry.get("id").and_then(|v| v.as_str()),
            ) else {
                return Err("GET /permission: entry missing id/sessionID".to_string());
            };
            asks.push((session_id.to_string(), permission_id.to_string()));
        }
        Ok(asks)
    }
```

In `run` step 3, AFTER a successful `fetch_snapshot` result is bound to
`statuses` but BEFORE `self.note(cycle, stream, OpencodeLaneEvent::Snapshot …)`,
insert:

```rust
                    // #608: replay outstanding permission asks BEFORE the
                    // snapshot is noted — ordering is load-bearing (the
                    // snapshot's busy row must not race an unarmed pause).
                    // Duplicate replays are safe: only a NEWLY inserted
                    // permission id arms (opencode.rs:508-510).
                    match self.fetch_permissions().await {
                        Ok(asks) => {
                            for (session_id, permission_id) in asks {
                                self.resolve_root(cycle, stream, &session_id, &mut known_sessions)
                                    .await;
                                self.note(
                                    cycle,
                                    stream,
                                    OpencodeLaneEvent::PermissionAsked {
                                        session_id,
                                        permission_id,
                                    },
                                );
                            }
                        }
                        Err(error) => {
                            tracing::warn!(
                                terminal_id = %self.terminal_id,
                                %error,
                                "opencode permission resync failed; pauses armed during the gap may be lost until the next reconnect (#608)"
                            );
                        }
                    }
```

Note the existing test `lane_gates_on_health_then_snapshots_then_streams`
asserts the ordered GET call list — its responder's `Ok((404, json!({})))`
catch-all now serves `/permission` a 404 → the resync warns and continues,
but the call-order slice assertions (`calls[..6]` style) will include the
new `GET …/permission` entry: UPDATE that test's expected call list to
include `GET {base}/permission` between the snapshot GET and the stream
CONNECT (or wherever it lands per your insertion point — keep snapshot GET
first, permission GET second, both before the Snapshot note). This is an
expected-order update, not an assertion weakening.

- [ ] **Step 6: Run the suites**

Run: `cargo test -p freshell-ws && cargo test -p freshell-activity`
Expected: PASS.

- [ ] **Step 7: Update the residual registry**

`crates/freshell-activity/src/idle.rs` entry 8 (~:61-63, the D8(b) mirror):

```rust
//!  8. (CLOSED by #608, 2026-08-06) SSE reconnect resyncs outstanding
//!     permission asks via GET /permission (replayed BEFORE the snapshot)
//!     and busy snapshots no longer clear an outstanding pause — the
//!     pending-attention bell survives connection blips.
```

- [ ] **Step 8: Commit**

```bash
git add crates/freshell-activity/src/opencode.rs crates/freshell-ws/src/opencode_lane.rs crates/freshell-activity/src/idle.rs
git commit -m "fix(opencode): permission pauses survive reconnect — GET /permission resync + snapshot no longer clears pauses (#608)"
```

---

### Task 9: claude session-JSONL truth source (#606/#611 shared seam)

**Files:**
- Create: `crates/freshell-ws/src/claude_truth.rs`
- Modify: `crates/freshell-ws/src/lib.rs` (add `pub mod claude_truth;`)
- Test: in-file `#[cfg(test)] mod tests` with tempdir fixtures

**Spike verdict this design bakes in (claude-code 2.1.223, verified on
this machine 2026-08-06):**
- Transcript: `<root>/projects/<cwd-slug>/<session-uuid>.jsonl`; the slug
  is LOSSY — location is a filename scan for `<session_id>.jsonl`, never a
  path computation. Roots in priority order: `$CLAUDE_CONFIG_DIR`,
  `$CLAUDE_HOME`, `$HOME/.claude` (same ladder as
  `crates/freshell-freshagent/src/claude_snapshot.rs`).
- Turn START (deterministic): a `"type":"user"` record WITH a
  `promptSource` field (`typed`/`queued`/`system`/`sdk`) — appended at
  submit time (~100-250ms flush lag). Tool-result user records have NO
  `promptSource`.
- Turn END (deterministic for terminal CLI panes, `entrypoint:"cli"` —
  which is what freshell claude PTY panes run): a
  `"type":"system","subtype":"turn_duration"` record, always the last
  record of a completed turn. `stop_hook_summary` under-fires — do not
  use. `away_summary` fires minutes later — match `subtype` EXACTLY.
- Interrupt (ESC) writes NO turn_duration: the marker is a `user` record
  whose `message.content` is the literal string
  `"[Request interrupted by user]"` — it terminates the turn.
- Timestamps are NOT monotonic (attachment records replay older stamps) —
  append order is truth; never sort or compare timestamps.
- Files reach 31MB — tail-seek, never slurp. Records are one JSON per
  line; individual lines can be hundreds of KB.
- Subagent sidechains live in separate files; main-file records have
  `isSidechain:false` — skip any record with `isSidechain:true`
  defensively.

**Interfaces:**
- Produces (Tasks 10/11 depend on these EXACT signatures):

```rust
pub enum TurnProbe {
    /// A turn-start record with no terminating record after it (or
    /// mid-turn transcript records at the tail) — the agent is working.
    InFlight,
    /// The last started turn has a terminating record (turn_duration or
    /// the interrupt marker) after it.
    Ended,
    /// No transcript found / unreadable / empty — no truth source.
    Unavailable,
}

pub enum SubmitProbe {
    /// A turn-start user record was appended at/after the given offset.
    Confirmed,
    /// The appended region parsed but contains no turn-start record.
    NoTurnStarted,
    /// Transcript missing/unreadable — cannot verify.
    Unavailable,
}

pub trait ClaudeTruth: Send + Sync {
    fn probe_turn_state(&self, session_id: &str) -> TurnProbe;
    /// Byte length of the transcript right now (None if not found) —
    /// captured at submit time so probe_submit reads only appended bytes.
    fn transcript_len(&self, session_id: &str) -> Option<u64>;
    fn probe_submit(&self, session_id: &str, from_offset: u64) -> SubmitProbe;
}

pub struct FsClaudeTruth { roots: Vec<std::path::PathBuf> }
impl FsClaudeTruth {
    pub fn from_env() -> Self;               // CLAUDE_CONFIG_DIR > CLAUDE_HOME > $HOME/.claude
    pub fn with_roots(roots: Vec<std::path::PathBuf>) -> Self;  // tests
}
```

- [ ] **Step 1: Write the failing tests**

In `claude_truth.rs`'s test module, build fixtures with `std::fs` under
`tempfile::tempdir()` (the workspace already uses `tempfile` in dev-deps
elsewhere; add `tempfile` to `crates/freshell-ws/Cargo.toml`
`[dev-dependencies]` if absent). Helper:

```rust
    fn write_transcript(root: &std::path::Path, session: &str, lines: &[&str]) {
        let dir = root.join("projects").join("-home-user-proj");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{session}.jsonl")), lines.join("\n")).unwrap();
    }

    const TURN_START: &str = r#"{"type":"user","promptSource":"typed","origin":{"kind":"human"},"promptId":"p1","isSidechain":false,"message":{"role":"user","content":"hi"},"uuid":"u1","timestamp":"2026-08-06T08:00:00.000Z","sessionId":"S","entrypoint":"cli"}"#;
    const TOOL_RESULT: &str = r#"{"type":"user","promptId":"p1","isSidechain":false,"toolUseResult":"x","message":{"role":"user","content":[{"type":"tool_result","content":"ok"}]},"uuid":"u2","timestamp":"2026-08-06T08:00:05.000Z"}"#;
    const ASSISTANT: &str = r#"{"type":"assistant","isSidechain":false,"message":{"role":"assistant","content":[{"type":"text","text":"…"}],"stop_reason":"tool_use"},"uuid":"u3","timestamp":"2026-08-06T08:00:06.000Z"}"#;
    const TURN_END: &str = r#"{"type":"system","subtype":"turn_duration","durationMs":1234,"messageCount":3,"isSidechain":false,"uuid":"u4","timestamp":"2026-08-06T08:00:07.000Z"}"#;
    const INTERRUPT: &str = r#"{"type":"user","isSidechain":false,"message":{"role":"user","content":"[Request interrupted by user]"},"uuid":"u5","timestamp":"2026-08-06T08:00:08.000Z"}"#;
    const AWAY: &str = r#"{"type":"system","subtype":"away_summary","isSidechain":false,"uuid":"u6","timestamp":"2026-08-06T08:03:00.000Z"}"#;

    #[test]
    fn probe_turn_state_classifies_the_tail() {
        let dir = tempfile::tempdir().unwrap();
        let truth = FsClaudeTruth::with_roots(vec![dir.path().to_path_buf()]);
        assert!(matches!(truth.probe_turn_state("S"), TurnProbe::Unavailable));

        write_transcript(dir.path(), "S", &[TURN_START, TOOL_RESULT, ASSISTANT]);
        assert!(matches!(truth.probe_turn_state("S"), TurnProbe::InFlight));

        write_transcript(dir.path(), "S", &[TURN_START, ASSISTANT, TURN_END]);
        assert!(matches!(truth.probe_turn_state("S"), TurnProbe::Ended));

        // Interrupt terminates a turn (NO turn_duration is written).
        write_transcript(dir.path(), "S", &[TURN_START, ASSISTANT, INTERRUPT]);
        assert!(matches!(truth.probe_turn_state("S"), TurnProbe::Ended));

        // away_summary is NOT an end marker; the started turn is open.
        write_transcript(dir.path(), "S", &[TURN_END, TURN_START, ASSISTANT, AWAY]);
        assert!(matches!(truth.probe_turn_state("S"), TurnProbe::InFlight));

        // Redundant end (compaction/resume boundary): tolerated.
        write_transcript(dir.path(), "S", &[TURN_END]);
        assert!(matches!(truth.probe_turn_state("S"), TurnProbe::Ended));
    }

    #[test]
    fn probe_submit_reads_only_appended_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let truth = FsClaudeTruth::with_roots(vec![dir.path().to_path_buf()]);
        write_transcript(dir.path(), "S", &[TURN_START, TURN_END]);
        let offset = truth.transcript_len("S").unwrap();
        // Nothing appended yet: no turn started past the offset.
        assert!(matches!(
            truth.probe_submit("S", offset),
            SubmitProbe::NoTurnStarted
        ));
        // Append a new turn-start: confirmed.
        {
            use std::io::Write;
            let dir2 = dir.path().join("projects").join("-home-user-proj");
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(dir2.join("S.jsonl"))
                .unwrap();
            writeln!(f).unwrap();
            writeln!(f, "{TURN_START}").unwrap();
        }
        assert!(matches!(truth.probe_submit("S", offset), SubmitProbe::Confirmed));
        // Missing transcript: unavailable.
        assert!(matches!(
            truth.probe_submit("MISSING", 0),
            SubmitProbe::Unavailable
        ));
    }

    #[test]
    fn tool_result_user_records_are_not_turn_starts() {
        let dir = tempfile::tempdir().unwrap();
        let truth = FsClaudeTruth::with_roots(vec![dir.path().to_path_buf()]);
        write_transcript(dir.path(), "S", &[TURN_START, TURN_END]);
        let offset = truth.transcript_len("S").unwrap();
        {
            use std::io::Write;
            let dir2 = dir.path().join("projects").join("-home-user-proj");
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(dir2.join("S.jsonl"))
                .unwrap();
            writeln!(f).unwrap();
            writeln!(f, "{TOOL_RESULT}").unwrap();
        }
        assert!(matches!(
            truth.probe_submit("S", offset),
            SubmitProbe::NoTurnStarted
        ));
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p freshell-ws claude_truth`
Expected: compile FAIL (module doesn't exist).

- [ ] **Step 3: Implement**

`crates/freshell-ws/src/claude_truth.rs` (complete):

```rust
//! #606/#611: claude session-JSONL truth source ("ask the agent" via its
//! transcript ledger). Spike-verified against claude-code 2.1.223
//! (2026-08-06): turn-start = `user` record WITH `promptSource`;
//! turn-end = `system`/`turn_duration` OR the interrupt marker
//! (`"[Request interrupted by user]"` user record — ESC writes no
//! turn_duration). Append order is truth (timestamps are NOT monotonic);
//! files reach tens of MB, so probes tail-seek and never slurp. Applies
//! to terminal CLI panes (`entrypoint:"cli"`) — exactly the population
//! the PTY claude tracker covers.

use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

/// Read at most this many bytes from the tail for a turn-state probe.
/// Individual records can be hundreds of KB; the boundary records
/// (turn-start / turn_duration / interrupt) are small and frequent, so a
/// 256 KiB window virtually always contains the decisive record. A window
/// with transcript records but NO boundary record classifies as InFlight
/// (mid-turn streaming) — conservative toward busy, never toward a false
/// green.
const TAIL_PROBE_BYTES: u64 = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnProbe {
    InFlight,
    Ended,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitProbe {
    Confirmed,
    NoTurnStarted,
    Unavailable,
}

pub trait ClaudeTruth: Send + Sync {
    fn probe_turn_state(&self, session_id: &str) -> TurnProbe;
    fn transcript_len(&self, session_id: &str) -> Option<u64>;
    fn probe_submit(&self, session_id: &str, from_offset: u64) -> SubmitProbe;
}

pub struct FsClaudeTruth {
    roots: Vec<PathBuf>,
}

impl FsClaudeTruth {
    /// Candidate roots, priority order — the same ladder as
    /// `claude_snapshot.rs`: CLAUDE_CONFIG_DIR > CLAUDE_HOME > ~/.claude.
    pub fn from_env() -> Self {
        let mut roots = Vec::new();
        if let Ok(dir) = std::env::var("CLAUDE_CONFIG_DIR") {
            roots.push(PathBuf::from(dir));
        }
        if let Ok(dir) = std::env::var("CLAUDE_HOME") {
            roots.push(PathBuf::from(dir));
        }
        if let Some(home) = std::env::var_os("HOME") {
            roots.push(PathBuf::from(home).join(".claude"));
        }
        Self { roots }
    }

    pub fn with_roots(roots: Vec<PathBuf>) -> Self {
        Self { roots }
    }

    /// The cwd→project-dir slug is LOSSY, so location is a filename scan:
    /// first `<root>/projects/*/<session_id>.jsonl` wins (priority order).
    fn locate(&self, session_id: &str) -> Option<PathBuf> {
        let file_name = format!("{session_id}.jsonl");
        for root in &self.roots {
            let projects = root.join("projects");
            let Ok(entries) = std::fs::read_dir(&projects) else {
                continue;
            };
            for entry in entries.flatten() {
                let candidate = entry.path().join(&file_name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
        None
    }

    /// Read [from, EOF) as lossy UTF-8, split lines, drop the first line
    /// when `from` landed mid-record (from > 0 and the byte before is not
    /// a newline — we cannot know, so we drop the first PARTIAL line by
    /// checking it fails to parse as JSON; whole-line records always
    /// parse).
    fn read_records_from(path: &PathBuf, from: u64) -> Option<Vec<serde_json::Value>> {
        let mut file = std::fs::File::open(path).ok()?;
        let len = file.metadata().ok()?.len();
        let start = from.min(len);
        file.seek(SeekFrom::Start(start)).ok()?;
        let mut buf = Vec::with_capacity((len - start) as usize);
        file.read_to_end(&mut buf).ok()?;
        let text = String::from_utf8_lossy(&buf);
        let mut records = Vec::new();
        for (i, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            match serde_json::from_str::<serde_json::Value>(line) {
                Ok(v) => records.push(v),
                // The first line may be a partial record when we seeked
                // into the middle of one — skip it; any later parse
                // failure is also skipped (a torn concurrent append).
                Err(_) if i == 0 => continue,
                Err(_) => continue,
            }
        }
        Some(records)
    }
}

fn is_sidechain(record: &serde_json::Value) -> bool {
    record
        .get("isSidechain")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Turn start: a `user` record WITH `promptSource` (typed/queued/system/
/// sdk). Tool-result user records carry no promptSource.
fn is_turn_start(record: &serde_json::Value) -> bool {
    !is_sidechain(record)
        && record.get("type").and_then(|v| v.as_str()) == Some("user")
        && record.get("promptSource").is_some()
}

/// Turn end: `system`/`turn_duration` (exact subtype match — never any
/// other trailing system record) OR the interrupt marker user record.
fn is_turn_end(record: &serde_json::Value) -> bool {
    if is_sidechain(record) {
        return false;
    }
    let ty = record.get("type").and_then(|v| v.as_str());
    if ty == Some("system") {
        return record.get("subtype").and_then(|v| v.as_str()) == Some("turn_duration");
    }
    if ty == Some("user") {
        return record
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            == Some("[Request interrupted by user]");
    }
    false
}

/// Any transcript-shaped record (used to distinguish "mid-turn streaming
/// tail with no boundary in the window" from "nothing here").
fn is_transcript_record(record: &serde_json::Value) -> bool {
    matches!(
        record.get("type").and_then(|v| v.as_str()),
        Some("user") | Some("assistant") | Some("system") | Some("attachment")
    )
}

impl ClaudeTruth for FsClaudeTruth {
    fn probe_turn_state(&self, session_id: &str) -> TurnProbe {
        let Some(path) = self.locate(session_id) else {
            return TurnProbe::Unavailable;
        };
        let Ok(meta) = std::fs::metadata(&path) else {
            return TurnProbe::Unavailable;
        };
        let from = meta.len().saturating_sub(TAIL_PROBE_BYTES);
        let Some(records) = Self::read_records_from(&path, from) else {
            return TurnProbe::Unavailable;
        };
        // Append order is truth: track the LAST boundary seen.
        let mut last: Option<bool /* true = start, false = end */> = None;
        let mut any_transcript = false;
        for record in &records {
            any_transcript |= is_transcript_record(record);
            if is_turn_end(record) {
                last = Some(false);
            } else if is_turn_start(record) {
                last = Some(true);
            }
        }
        match last {
            Some(true) => TurnProbe::InFlight,
            Some(false) => TurnProbe::Ended,
            // Boundary outside the window but records streaming: a huge
            // mid-turn tail — conservative toward busy.
            None if any_transcript => TurnProbe::InFlight,
            None => TurnProbe::Unavailable,
        }
    }

    fn transcript_len(&self, session_id: &str) -> Option<u64> {
        let path = self.locate(session_id)?;
        std::fs::metadata(&path).ok().map(|m| m.len())
    }

    fn probe_submit(&self, session_id: &str, from_offset: u64) -> SubmitProbe {
        let Some(path) = self.locate(session_id) else {
            return SubmitProbe::Unavailable;
        };
        let Some(records) = Self::read_records_from(&path, from_offset) else {
            return SubmitProbe::Unavailable;
        };
        if records.iter().any(is_turn_start) {
            SubmitProbe::Confirmed
        } else {
            SubmitProbe::NoTurnStarted
        }
    }
}
```

Add `pub mod claude_truth;` to `crates/freshell-ws/src/lib.rs` (next to
the other module declarations) and `tempfile` under `[dev-dependencies]`
in `crates/freshell-ws/Cargo.toml` if not already present (use the
workspace's existing tempfile version — check with
`grep -rn "tempfile" Cargo.toml crates/*/Cargo.toml`).

- [ ] **Step 4: Run the tests**

Run: `cargo test -p freshell-ws claude_truth`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/freshell-ws/src/claude_truth.rs crates/freshell-ws/src/lib.rs crates/freshell-ws/Cargo.toml
git commit -m "feat(claude): session-JSONL truth source (turn-state + submit probes) behind ClaudeTruth seam (#606 #611)"
```

---

### Task 10: claude deadman verifies against the JSONL (#606)

**Files:**
- Modify: `crates/freshell-activity/src/claude.rs` (`expire` ~:207-235; new fns; tests ~:261-438)
- Modify: `crates/freshell-ws/src/activity.rs` (`claude_frames` ~:1602-1650; `expire_due` ~:1478-1550; new claude-truth wiring; tests)
- Modify: `crates/freshell-server/src/main.rs` (boot wiring: install `FsClaudeTruth`)

**Interfaces:**
- Consumes: Task 9's `ClaudeTruth` trait + `TurnProbe`.
- Produces (Task 11 also uses these):
  - `ClaudeActivityTracker::set_busy_deadman_ms(&mut self, ms: i64)` test hook (mirrors `codex.rs:227` / opencode `:186`; adds field `busy_deadman_ms: i64` defaulted to `CLAUDE_BUSY_DEADMAN_MS` — replace both uses of the bare constant in `expire`/`next_deadline`).
  - `pub fn expire(&mut self, at: i64) -> Vec<ClaudeEffect>` — busy past the window emits `ForceRead{terminal_id, at}`, KEEPS Busy, re-arms via `last_observed_at = at`. No demotion, no `in_flight` reset.
  - `pub fn note_verified_busy(&mut self, terminal_id: &str, at: i64) -> Vec<ClaudeEffect>` — refresh `last_observed_at`; no public change (returns `Vec::new()`).
  - `pub fn note_verified_ended(&mut self, terminal_id: &str, at: i64) -> Vec<ClaudeEffect>` — the verified turn end the old deadman swallowed: phase→Idle, `in_flight = 0`, ONE ledger completion → `[Changed{upsert:[Idle rec]}, TurnComplete{…, completion_seq}]` (bell rings).
  - `pub fn note_verify_failed(&mut self, terminal_id: &str, at: i64) -> Vec<ClaudeEffect>` — crash semantics: phase→Idle, `in_flight = 0`, `[Changed{upsert:[Idle rec]}, AttentionBoundary{…}]` + `error!` log. (Claude records use an Idle-phase upsert, not a remove — the claude wire contract has an Idle phase.)
  - `pub fn session_id_of(&self, terminal_id: &str) -> Option<String>` — hub accessor for probe routing.
  - `fn claude_frames(…) -> (Vec<ServerMessage>, Vec<String>)` — returns force-reads (like `codex_frames`); `AttentionBoundary` now arms the gate (`idle.note_turn_boundary`) instead of being ignored.
  - `ActivityHub::set_claude_truth(&self, truth: Arc<dyn ClaudeTruth>)` (+ `HubInner.claude_truth: Option<Arc<dyn ClaudeTruth>>`), installed at boot in `main.rs` next to `set_opencode_lane_deps` (~:515): `activity_hub.set_claude_truth(std::sync::Arc::new(freshell_ws::claude_truth::FsClaudeTruth::from_env()));`
  - Test hooks: `#[cfg(test)] pub(crate) fn set_claude_busy_deadman_for_tests(&self, ms: i64)`.

**Servicing rule (hub, `expire_due`)** — collect claude force-read terminal
ids; after the lock is released, for each id:
1. read `session_id_of(tid)` and `claude_truth` under a short lock;
2. probe OUTSIDE the lock (file IO);
3. re-lock and apply: `InFlight → note_verified_busy`; `Ended →
   note_verified_ended`; `Unavailable` OR no bound session OR no truth
   installed → `note_verify_failed` (owner ruling: no reachable truth
   source = crash semantics — this includes fresh claude panes that never
   got a session id, which previously deadman-cleared silently); emit the
   resulting frames.

- [ ] **Step 1: Write the failing tracker tests**

REPLACE `deadman_clears_stuck_busy_without_completion` (claude.rs ~:361-375
— the pin of the old wrong behavior; deliberate inversion, it IS bug #606)
with:

```rust
    #[test]
    fn deadman_requests_verify_and_stays_busy() {
        let mut tracker = ClaudeActivityTracker::new();
        tracker.set_busy_deadman_ms(1000);
        tracker.track_terminal("t1", None, 0);
        tracker.note_input("t1", "\r", 10);
        assert!(tracker.expire(1010).is_empty(), "not past the window yet");
        let effects = tracker.expire(1011 + 1);
        assert_eq!(
            effects,
            vec![TrackerEffect::ForceRead {
                terminal_id: "t1".to_string(),
                at: 1012,
            }]
        );
        assert_eq!(tracker.list()[0].phase, ClaudePhase::Busy);
        assert!(tracker.next_deadline().is_some(), "re-armed, no hot loop");
        assert!(completions(&effects).is_empty());
    }

    #[test]
    fn verified_ended_clears_with_a_completion_bell() {
        // The old deadman swallowed the bell; the verified end mints it.
        let mut tracker = ClaudeActivityTracker::new();
        tracker.track_terminal("t1", None, 0);
        tracker.note_input("t1", "\r", 10);
        let effects = tracker.note_verified_ended("t1", 500);
        assert_eq!(
            busy_upserts(&effects),
            vec![("t1".into(), ClaudePhase::Idle)]
        );
        assert_eq!(completions(&effects), vec![1]);
        // A stray later BEL is a false positive: ignored (in_flight == 0).
        assert!(completions(&tracker.note_output("t1", "\u{07}", 600)).is_empty());
    }

    #[test]
    fn verified_busy_refreshes_and_verify_failed_rings_attention() {
        let mut tracker = ClaudeActivityTracker::new();
        tracker.set_busy_deadman_ms(1000);
        tracker.track_terminal("t1", None, 0);
        tracker.note_input("t1", "\r", 10);
        tracker.expire(2000); // verify requested
        assert!(tracker.note_verified_busy("t1", 2100).is_empty());
        assert_eq!(tracker.next_deadline(), Some(2100 + 1000 + 1));
        // Probe failure: crash semantics — idle + attention boundary.
        let effects = tracker.note_verify_failed("t1", 3000);
        assert_eq!(
            busy_upserts(&effects),
            vec![("t1".into(), ClaudePhase::Idle)]
        );
        assert!(matches!(
            effects.last(),
            Some(TrackerEffect::AttentionBoundary { at: 3000, .. })
        ));
        assert!(completions(&effects).is_empty());
    }
```

Also UPDATE `output_feeds_the_deadman` (~:377-390): the final expectation
changes from an Idle upsert to a `ForceRead` effect (silence measured from
last output still pins the re-feed):

```rust
    #[test]
    fn output_feeds_the_deadman() {
        let mut tracker = ClaudeActivityTracker::new();
        tracker.track_terminal("t1", None, 0);
        tracker.note_input("t1", "\r", 10);
        tracker.note_output("t1", "streamed output", 100_000);
        // Silence measured from the LAST output, not the submit.
        assert!(tracker.expire(10 + CLAUDE_BUSY_DEADMAN_MS + 1).is_empty());
        let effects = tracker.expire(100_001 + CLAUDE_BUSY_DEADMAN_MS);
        assert!(
            matches!(effects.as_slice(), [TrackerEffect::ForceRead { .. }]),
            "past the window: verify, don't demote (#606); got {effects:?}"
        );
    }
```

And `next_deadline_exists_only_while_busy` (~:410-422) stays green
unmodified (the deadline math is unchanged while busy; after a BEL it is
None).

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p freshell-activity claude`
Expected: compile FAIL (`set_busy_deadman_ms`, `note_verified_*`,
`note_verify_failed` missing), then assertion failures on `expire`.

- [ ] **Step 3: Implement the tracker**

In `claude.rs`:

1. Struct + hook:

```rust
#[derive(Debug)]
pub struct ClaudeActivityTracker {
    states: HashMap<String, TerminalActivity>,
    ledger: TurnCompletionLedger,
    /// Busy-deadman window; [`CLAUDE_BUSY_DEADMAN_MS`] in production.
    /// Test-scale hook, same shape as the codex/opencode trackers'.
    busy_deadman_ms: i64,
}

impl Default for ClaudeActivityTracker {
    fn default() -> Self {
        Self {
            states: HashMap::new(),
            ledger: TurnCompletionLedger::default(),
            busy_deadman_ms: CLAUDE_BUSY_DEADMAN_MS,
        }
    }
}
```

(keep `pub fn new() -> Self { Self::default() }`; the `#[derive(Debug,
Default)]` on the old struct is replaced by this manual Default) and:

```rust
    pub fn set_busy_deadman_ms(&mut self, ms: i64) {
        self.busy_deadman_ms = ms;
    }

    pub fn session_id_of(&self, terminal_id: &str) -> Option<String> {
        self.states
            .get(terminal_id)
            .and_then(|s| s.session_id.clone())
    }
```

2. `expire` (replace the demotion body, keep the strict `>` window):

```rust
    /// Busy-deadman — verify-then-decide (#606). Emits a verify request
    /// (`ForceRead`) and STAYS busy; the hub answers from the session
    /// JSONL truth source: verified-busy → refreshed, verified-ended →
    /// [`Self::note_verified_ended`] (idle WITH the completion the old
    /// deadman swallowed), probe failure → [`Self::note_verify_failed`]
    /// (crash semantics). Re-arms via `last_observed_at` so a wedged
    /// probe cannot hot-loop.
    pub fn expire(&mut self, at: i64) -> Vec<ClaudeEffect> {
        let mut effects = Vec::new();
        for state in self.states.values_mut() {
            if state.phase != ClaudePhase::Busy {
                continue;
            }
            let idle_age_ms = at - state.last_observed_at;
            if idle_age_ms <= self.busy_deadman_ms {
                continue;
            }
            state.last_observed_at = at;
            tracing::warn!(
                component = "claude-activity-tracker",
                event = "claude_activity_deadman_verify",
                terminal_id = %state.terminal_id,
                age_ms = idle_age_ms,
                "Claude terminal silent past deadman; requesting JSONL verify (staying busy)."
            );
            effects.push(TrackerEffect::ForceRead {
                terminal_id: state.terminal_id.clone(),
                at,
            });
        }
        effects
    }
```

   `next_deadline` swaps the constant for `self.busy_deadman_ms`.

3. The three probe-result methods:

```rust
    /// Truth source says the turn is still in flight: refresh liveness.
    pub fn note_verified_busy(&mut self, terminal_id: &str, at: i64) -> Vec<ClaudeEffect> {
        if let Some(state) = self.states.get_mut(terminal_id) {
            if state.phase == ClaudePhase::Busy {
                state.last_observed_at = at;
            }
        }
        Vec::new()
    }

    /// Truth source says the turn ENDED (turn_duration / interrupt): clear
    /// busy WITH the completion the old silent deadman swallowed.
    pub fn note_verified_ended(&mut self, terminal_id: &str, at: i64) -> Vec<ClaudeEffect> {
        let Some(state) = self.states.get_mut(terminal_id) else {
            return Vec::new();
        };
        if state.phase != ClaudePhase::Busy {
            return Vec::new();
        }
        let previous = state.to_record();
        state.phase = ClaudePhase::Idle;
        state.in_flight = 0;
        state.updated_at = at;
        state.last_observed_at = at;
        let next = state.to_record();
        let mut effects = commit_change(Some(&previous), next);
        let seq = self.ledger.record_turn_completion(terminal_id, at);
        effects.push(TrackerEffect::TurnComplete {
            terminal_id: terminal_id.to_string(),
            session_id: self
                .states
                .get(terminal_id)
                .and_then(|s| s.session_id.clone()),
            at,
            completion_seq: seq,
        });
        effects
    }

    /// The verify probe failed (no JSONL / unreadable / no bound session /
    /// no truth source installed). Owner ruling: crash semantics — clear
    /// busy AND fire the attention/death engagement signal.
    pub fn note_verify_failed(&mut self, terminal_id: &str, at: i64) -> Vec<ClaudeEffect> {
        let Some(state) = self.states.get_mut(terminal_id) else {
            return Vec::new();
        };
        if state.phase != ClaudePhase::Busy {
            return Vec::new();
        }
        tracing::error!(
            component = "claude-activity-tracker",
            event = "claude_verify_failed",
            terminal_id = %state.terminal_id,
            "claude verify probe failed; clearing busy and ringing attention (owner ruling: probe failure = crash)"
        );
        let previous = state.to_record();
        state.phase = ClaudePhase::Idle;
        state.in_flight = 0;
        state.updated_at = at;
        state.last_observed_at = at;
        let next = state.to_record();
        let mut effects = commit_change(Some(&previous), next);
        effects.push(TrackerEffect::AttentionBoundary {
            terminal_id: terminal_id.to_string(),
            at,
        });
        effects
    }
```

Run: `cargo test -p freshell-activity claude` — Expected: PASS.

- [ ] **Step 4: Implement the hub servicing**

In `activity.rs`:

1. `claude_frames` returns `(Vec<ServerMessage>, Vec<String>)`:
   `TrackerEffect::ForceRead { terminal_id, .. } => force_reads.push(terminal_id),`
   and `TrackerEffect::AttentionBoundary { terminal_id, at } => { idle.note_turn_boundary(&terminal_id, at); }`
   (copy the codex arm's comment). Update ALL call sites (known: Created
   ~:966, Input ~:1074, Output ~:1106, Exit ~:1183-1184, expire_due
   ~:1485-1486); outside `expire_due` drop the list with
   `// ForceRead only arises from expire()`.
2. `HubInner` gains `claude_truth: Option<Arc<dyn crate::claude_truth::ClaudeTruth>>`
   (Default None) + setter:

```rust
    pub fn set_claude_truth(&self, truth: std::sync::Arc<dyn crate::claude_truth::ClaudeTruth>) {
        self.inner.lock().expect("activity hub lock").claude_truth = Some(truth);
    }
```

3. In `expire_due`, capture `(frames, codex_force_reads, force_reads, claude_verifies, reattaches)`
   — the claude arm becomes:

```rust
            let claude = inner.claude.expire(now);
            let (mut f, claude_verifies) = claude_frames(&mut inner.idle, claude);
            frames.append(&mut f);
```

   and after the existing drains:

```rust
        for terminal_id in claude_verifies {
            self.service_claude_verify(&terminal_id);
        }
```

   with:

```rust
    /// #606: answer a claude deadman verify from the session-JSONL truth
    /// source. Probe runs OUTSIDE the hub lock (file IO). No bound
    /// session / no truth source / unreadable transcript = probe failure
    /// = crash semantics (owner ruling).
    fn service_claude_verify(&self, terminal_id: &str) {
        use crate::claude_truth::TurnProbe;
        let (session, truth) = {
            let inner = self.inner.lock().expect("activity hub lock");
            (
                inner.claude.session_id_of(terminal_id),
                inner.claude_truth.clone(),
            )
        };
        let probe = match (&session, &truth) {
            (Some(session_id), Some(truth)) => truth.probe_turn_state(session_id),
            _ => TurnProbe::Unavailable,
        };
        let frames = {
            let mut inner = self.inner.lock().expect("activity hub lock");
            let at = now_ms();
            let effects = match probe {
                TurnProbe::InFlight => inner.claude.note_verified_busy(terminal_id, at),
                TurnProbe::Ended => inner.claude.note_verified_ended(terminal_id, at),
                TurnProbe::Unavailable => inner.claude.note_verify_failed(terminal_id, at),
            };
            let (frames, _) = claude_frames(&mut inner.idle, effects);
            frames
        };
        self.emit(frames);
    }
```

4. Test hooks on `ActivityHub`:

```rust
    #[cfg(test)]
    pub(crate) fn set_claude_busy_deadman_for_tests(&self, ms: i64) {
        self.inner
            .lock()
            .expect("activity hub lock")
            .claude
            .set_busy_deadman_ms(ms);
    }
```

5. Boot wiring in `crates/freshell-server/src/main.rs`, next to
   `set_opencode_lane_deps` (~:515):

```rust
    activity_hub.set_claude_truth(std::sync::Arc::new(
        freshell_ws::claude_truth::FsClaudeTruth::from_env(),
    ));
```

- [ ] **Step 5: Hub-level test**

In `activity.rs` tests, add a `FakeClaudeTruth` (a struct holding
`Mutex<TurnProbe>` implementing `ClaudeTruth`; `transcript_len → None` —
IMPORTANT: `None` keeps these deadman tests on the legacy confirmed-busy
input flavor once Task 11 lands, so they stay valid; `probe_submit →
SubmitProbe::Unavailable`) and:

```rust
    /// #606 end-to-end: deadman → JSONL probe; InFlight keeps busy on the
    /// wire; Ended emits idle + terminal.turn.complete; Unavailable rings
    /// the attention boundary.
    #[tokio::test(flavor = "multi_thread")]
    async fn claude_deadman_probes_the_jsonl_truth_source() { … }
```

Build: `Created{mode:"claude", resume_session_id: Some("S")}` via
`observer_send`; `hub.set_claude_truth(Arc::new(fake))`;
`hub.set_claude_busy_deadman_for_tests(300)`; submit input
(`Input{data:"\r"}`); with the fake returning `InFlight`, sleep ~700ms and
assert NO `claude.activity.updated` frame carrying an idle upsert arrived;
flip the fake to `Ended`, sleep ≥400ms, assert a
`terminal.turn.complete` frame (provider `claude`) arrives. Add the
sibling `claude_deadman_unavailable_truth_rings_attention` asserting a
`terminal.idle` frame (reason `grace`) arrives after the deadman when the
fake returns `Unavailable`.

Run: `cargo test -p freshell-ws claude_deadman`
Expected: PASS.

- [ ] **Step 6: Run full crate suites**

Run: `cargo test -p freshell-activity && cargo test -p freshell-ws && cargo build -p freshell-server`
Expected: PASS / builds.

- [ ] **Step 7: Commit**

```bash
git add crates/freshell-activity/src/claude.rs crates/freshell-ws/src/activity.rs crates/freshell-server/src/main.rs
git commit -m "fix(claude): output deadman verifies against session JSONL; verified end rings, probe failure rings attention (#606)"
```

---

### Task 11: claude submit-grace — provisional busy confirmed by the JSONL (#611)

**Files:**
- Modify: `crates/freshell-activity/src/claude.rs` (state fields, `note_input`, `note_output` provisional-BEL rule, `expire` grace arm, `next_deadline`; new probe-result methods; tests)
- Modify: `crates/freshell-ws/src/activity.rs` (Input arm stash; `service_claude_verify` confirm flavor; tests)

**Design (the spike SUCCEEDED, so this is the owner-preferred verify-backed
option; the codex-style fixed pending gate is NOT used):**
- `CLAUDE_SUBMIT_GRACE_MS: i64 = 2_000` (matches amplifier's).
- A submit on an Idle pane whose truth source is usable
  (`confirmable == true`) marks PROVISIONAL busy: `busy_confirmed = false`,
  `in_flight` NOT incremented (this kills the phantom-BEL-consumption skew),
  grace deadline armed. Confirmation comes from the JSONL: a turn-start
  user record appended past the byte offset captured at submit time.
- Grace expiry: first lapse → ONE `ForceRead` (confirm probe) + extend
  grace once (amplifier's exact contract); second lapse → SILENT revert to
  Idle (no completion, no bell — a no-op Enter is not attention-worthy;
  ruling 4's crash semantics applies to the issues it names, and #611 is
  not among them — the amplifier precedent governs).
- Probe results: `Confirmed → busy_confirmed = true, in_flight = 1` (turn
  is real; BEL machinery proceeds as today); `NoTurnStarted` → immediate
  silent revert; `Unavailable` → keep provisional busy and STOP probing
  (clear the grace deadline; the #606 deadman verify is the backstop and
  will apply crash semantics at the 120s mark if the truth source is still
  unreachable).
- A Stop-BEL arriving DURING provisional busy confirms and completes in
  one step (a real fast turn must not lose its bell): mint one completion,
  go Idle.
- `confirmable == false` (no truth source installed, session unbound, or
  the submit-time `transcript_len` failed): keep TODAY'S behavior exactly
  (`in_flight += 1`, confirmed busy) — real turns keep working via BEL;
  phantom Enters on such panes fall to the #606 deadman.
- Repeated Enter while provisional: re-arm the grace deadline (no
  `in_flight`); repeated Enter while CONFIRMED busy: `in_flight += 1`
  (queued turn, today's behavior).

**Interfaces:**
- Consumes: Task 9's `probe_submit`/`transcript_len`; Task 10's force-read
  servicing plumbing.
- Produces:
  - `TerminalActivity` gains `busy_confirmed: bool`, `submit_grace_deadline: Option<i64>`, `submit_grace_retried: bool` (initialize in `track_terminal`: `busy_confirmed: false`, `submit_grace_deadline: None`, `submit_grace_retried: false`).
  - `pub fn note_input(&mut self, terminal_id: &str, data: &str, at: i64, confirmable: bool) -> Vec<ClaudeEffect>` — SIGNATURE CHANGE (callers updated: hub Input arm passes the computed flag; existing tracker tests pass `false` to keep legacy pins, new tests pass `true`).
  - `pub fn note_submit_confirmed(&mut self, terminal_id: &str, at: i64) -> Vec<ClaudeEffect>` — sets `busy_confirmed = true; in_flight = 1; submit_grace_deadline = None` when provisional; no public change.
  - `pub fn note_submit_unconfirmed(&mut self, terminal_id: &str, at: i64) -> Vec<ClaudeEffect>` — silent revert when provisional: phase→Idle, no completion.
  - `pub fn note_submit_probe_unavailable(&mut self, terminal_id: &str)` — clears the grace deadline, keeps provisional busy (deadman backstop takes over).
  - `pub fn is_awaiting_submit_confirm(&self, terminal_id: &str) -> bool` — hub routing: distinguishes the confirm-probe flavor from the deadman-verify flavor of `ForceRead`.
  - Hub: `HubInner.claude_submit_offsets: HashMap<String, u64>` — stashed at submit time in the Input arm; consumed by the confirm probe.

- [ ] **Step 1: Write the failing tracker tests**

```rust
    #[test]
    fn confirmable_enter_is_provisional_and_silently_reverts() {
        // #611: a bare Enter must not claim "working" for 120s. Amplifier
        // contract: one confirm probe, one extension, then silent revert.
        let mut tracker = ClaudeActivityTracker::new();
        tracker.track_terminal("t1", Some("S"), 0);
        let effects = tracker.note_input("t1", "\r", 10, true);
        assert_eq!(
            busy_upserts(&effects),
            vec![("t1".into(), ClaudePhase::Busy)]
        );
        assert_eq!(tracker.next_deadline(), Some(10 + 2000));
        // First grace lapse: ONE confirm probe, still busy, extended.
        let effects = tracker.expire(2010);
        assert!(matches!(
            effects.as_slice(),
            [TrackerEffect::ForceRead { .. }]
        ));
        assert_eq!(tracker.list()[0].phase, ClaudePhase::Busy);
        // Second lapse: SILENT revert — idle, no completion, no boundary.
        let effects = tracker.expire(4020);
        assert_eq!(
            busy_upserts(&effects),
            vec![("t1".into(), ClaudePhase::Idle)]
        );
        assert!(completions(&effects).is_empty());
        assert!(!effects
            .iter()
            .any(|e| matches!(e, TrackerEffect::AttentionBoundary { .. })));
        // The phantom left NO in_flight skew: a real turn now completes
        // with its own single BEL.
        tracker.note_input("t1", "\r", 5000, true);
        tracker.note_submit_confirmed("t1", 5100);
        let effects = tracker.note_output("t1", "\u{07}", 6000);
        assert_eq!(completions(&effects), vec![1]);
        assert_eq!(tracker.list()[0].phase, ClaudePhase::Idle);
    }

    #[test]
    fn confirmed_submit_behaves_like_todays_turn() {
        let mut tracker = ClaudeActivityTracker::new();
        tracker.track_terminal("t1", Some("S"), 0);
        tracker.note_input("t1", "\r", 10, true);
        assert!(tracker.note_submit_confirmed("t1", 200).is_empty());
        // Grace is disarmed: nothing at the old deadline.
        assert!(tracker.expire(2010).is_empty());
        let effects = tracker.note_output("t1", "\u{07}", 3000);
        assert_eq!(completions(&effects), vec![1]);
    }

    #[test]
    fn probe_says_no_turn_reverts_immediately_and_unavailable_keeps_busy() {
        let mut tracker = ClaudeActivityTracker::new();
        tracker.track_terminal("t1", Some("S"), 0);
        tracker.note_input("t1", "\r", 10, true);
        let effects = tracker.note_submit_unconfirmed("t1", 100);
        assert_eq!(
            busy_upserts(&effects),
            vec![("t1".into(), ClaudePhase::Idle)]
        );
        assert!(completions(&effects).is_empty());
        // Unavailable: keep provisional busy, stop the grace probing —
        // the #606 deadman verify is the deterministic backstop.
        tracker.note_input("t1", "\r", 200, true);
        tracker.note_submit_probe_unavailable("t1");
        assert_eq!(tracker.list()[0].phase, ClaudePhase::Busy);
        assert_eq!(
            tracker.next_deadline(),
            Some(200 + CLAUDE_BUSY_DEADMAN_MS + 1),
            "grace disarmed; deadman remains"
        );
    }

    #[test]
    fn bel_during_provisional_confirms_and_completes() {
        // A real fast turn (<2s) must not lose its bell.
        let mut tracker = ClaudeActivityTracker::new();
        tracker.track_terminal("t1", Some("S"), 0);
        tracker.note_input("t1", "\r", 10, true);
        let effects = tracker.note_output("t1", "\u{07}", 1500);
        assert_eq!(completions(&effects), vec![1]);
        assert_eq!(tracker.list()[0].phase, ClaudePhase::Idle);
    }

    #[test]
    fn unconfirmable_enter_keeps_legacy_semantics() {
        // No truth source: today's contract exactly (in_flight, BEL).
        let mut tracker = ClaudeActivityTracker::new();
        tracker.track_terminal("t1", None, 0);
        tracker.note_input("t1", "\r", 10, false);
        assert!(tracker.expire(2011).is_empty(), "no grace machinery");
        let effects = tracker.note_output("t1", "\u{07}", 3000);
        assert_eq!(completions(&effects), vec![1]);
    }
```

Also mechanically update every existing test's `note_input` call to pass
`false` (legacy flavor) — e.g. `tracker.note_input("t1", "\r", 10, false)`
— so the existing pins (`submit_marks_busy_and_stop_bel_completes_exactly_once`,
`stacked_submits_need_matching_bels`, etc.) keep their exact semantics.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p freshell-activity claude`
Expected: compile FAIL (signature/methods), then behavior failures.

- [ ] **Step 3: Implement the tracker**

1. `TerminalActivity` new fields + init in `track_terminal`.
2. `note_input`:

```rust
    pub fn note_input(
        &mut self,
        terminal_id: &str,
        data: &str,
        at: i64,
        confirmable: bool,
    ) -> Vec<ClaudeEffect> {
        let Some(state) = self.states.get_mut(terminal_id) else {
            return Vec::new();
        };
        if !is_submit_input(data) {
            return Vec::new();
        }
        state.last_observed_at = at;
        if !confirmable {
            // Legacy flavor (no truth source): today's contract — the
            // #606 deadman verify is the backstop for phantoms.
            let previous = state.to_record();
            state.in_flight += 1;
            state.busy_confirmed = true;
            if state.phase != ClaudePhase::Busy {
                state.phase = ClaudePhase::Busy;
                state.updated_at = at;
            }
            let next = state.to_record();
            return commit_change(Some(&previous), next);
        }
        if state.phase == ClaudePhase::Busy {
            if state.busy_confirmed {
                // Queued turn while a confirmed turn runs — today's rule.
                state.in_flight += 1;
            } else {
                // Repeated Enter while provisional: re-arm the grace.
                state.submit_grace_deadline = Some(at + CLAUDE_SUBMIT_GRACE_MS);
                state.submit_grace_retried = false;
            }
            return Vec::new();
        }
        // #611: provisional busy — no in_flight until the JSONL confirms
        // a turn actually started (kills the phantom-BEL skew).
        let previous = state.to_record();
        state.phase = ClaudePhase::Busy;
        state.updated_at = at;
        state.busy_confirmed = false;
        state.submit_grace_deadline = Some(at + CLAUDE_SUBMIT_GRACE_MS);
        state.submit_grace_retried = false;
        let next = state.to_record();
        commit_change(Some(&previous), next)
    }
```

   with `pub const CLAUDE_SUBMIT_GRACE_MS: i64 = 2_000;` next to the
   deadman constant.
3. `note_output`: before the existing `clear_count` loop, add the
   provisional-BEL rule:

```rust
        // #611: a Stop-BEL during a PROVISIONAL turn is the strongest
        // confirmation there is — confirm and complete in one step so a
        // real fast turn (<grace) never loses its bell.
        if clear_count > 0 && state.phase == ClaudePhase::Busy && !state.busy_confirmed {
            state.busy_confirmed = true;
            state.submit_grace_deadline = None;
            state.in_flight = 1;
        }
```

   (placed right after `let clear_count = …; if clear_count == 0 { … }`,
   before `let previous = state.to_record();` — the existing loop then
   consumes the single in_flight normally.)
4. `expire`: add the grace arm BEFORE the Task 10 deadman check. The
   deadman keeps covering ALL busy states — INCLUDING provisional busy
   (deliberate divergence from amplifier's `busy_confirmed` deadman gate):
   after `note_submit_probe_unavailable` clears the grace, the #606
   deadman verify at 120s is the deterministic backstop that
   crash-signals an unverifiable provisional pane. The `continue` at the
   end of the grace arm prevents a double fire in one tick:

```rust
            // #611 submit-grace: first lapse probes once and extends; the
            // second silently reverts (no completion, no bell).
            if let Some(deadline) = state.submit_grace_deadline {
                if at >= deadline && state.phase == ClaudePhase::Busy && !state.busy_confirmed {
                    if !state.submit_grace_retried {
                        state.submit_grace_retried = true;
                        state.submit_grace_deadline = Some(at + CLAUDE_SUBMIT_GRACE_MS);
                        effects.push(TrackerEffect::ForceRead {
                            terminal_id: state.terminal_id.clone(),
                            at,
                        });
                    } else {
                        state.submit_grace_deadline = None;
                        let previous = state.to_record();
                        state.phase = ClaudePhase::Idle;
                        state.updated_at = at;
                        state.last_observed_at = at;
                        let next = state.to_record();
                        effects.extend(commit_change(Some(&previous), next));
                    }
                    continue;
                }
                if at >= deadline {
                    state.submit_grace_deadline = None;
                }
            }
```

5. `next_deadline`: fold the grace deadline in: for each state, grace =
   `submit_grace_deadline` when Busy+unconfirmed; deadman =
   `last_observed_at + busy_deadman_ms + 1` when Busy (ANY confirmation
   state — see item 4's backstop rationale); min of the two, min across
   states.
6. Probe-result methods:

```rust
    pub fn note_submit_confirmed(&mut self, terminal_id: &str, at: i64) -> Vec<ClaudeEffect> {
        if let Some(state) = self.states.get_mut(terminal_id) {
            if state.phase == ClaudePhase::Busy && !state.busy_confirmed {
                state.busy_confirmed = true;
                state.in_flight = 1;
                state.submit_grace_deadline = None;
                state.last_observed_at = at;
            }
        }
        Vec::new()
    }

    pub fn note_submit_unconfirmed(&mut self, terminal_id: &str, at: i64) -> Vec<ClaudeEffect> {
        let Some(state) = self.states.get_mut(terminal_id) else {
            return Vec::new();
        };
        if state.phase != ClaudePhase::Busy || state.busy_confirmed {
            return Vec::new();
        }
        state.submit_grace_deadline = None;
        let previous = state.to_record();
        state.phase = ClaudePhase::Idle;
        state.updated_at = at;
        state.last_observed_at = at;
        let next = state.to_record();
        commit_change(Some(&previous), next)
    }

    pub fn note_submit_probe_unavailable(&mut self, terminal_id: &str) {
        if let Some(state) = self.states.get_mut(terminal_id) {
            state.submit_grace_deadline = None; // deadman backstop takes over
        }
    }

    pub fn is_awaiting_submit_confirm(&self, terminal_id: &str) -> bool {
        self.states
            .get(terminal_id)
            .map(|s| s.phase == ClaudePhase::Busy && !s.busy_confirmed)
            .unwrap_or(false)
    }
```

Note on Task 10 interplay: `note_verified_ended`/`note_verify_failed` also
set `busy_confirmed = false` and `submit_grace_deadline = None` when they
clear (add those two lines to each) so state never wedges.

- [ ] **Step 4: Implement the hub half**

In `activity.rs`:

1. `HubInner.claude_submit_offsets: HashMap<String, u64>` (Default empty;
   remove the entry in the Exit teardown block alongside `lanes.remove`).
2. Input arm (claude case, ~:1071-1075): compute `confirmable` and stash
   the offset BEFORE calling the tracker:

```rust
                        "claude" => {
                            let confirmable = if freshell_activity::signal::is_submit_input(&data)
                            {
                                let session = inner.claude.session_id_of(&terminal_id);
                                match (&session, &inner.claude_truth) {
                                    (Some(session_id), Some(truth)) => {
                                        match truth.transcript_len(session_id) {
                                            Some(len) => {
                                                inner
                                                    .claude_submit_offsets
                                                    .insert(terminal_id.clone(), len);
                                                true
                                            }
                                            None => false,
                                        }
                                    }
                                    _ => false,
                                }
                            } else {
                                false
                            };
                            let effects =
                                inner.claude.note_input(&terminal_id, &data, at, confirmable);
                            let (frames, _) = claude_frames(&mut inner.idle, effects);
                            frames
                        }
```

   NOTE: `transcript_len` is one `read_dir` + `stat` under the hub lock —
   acceptable (sub-millisecond on a local FS, same class as the SQLite
   locator); if review flags it, move the stash to a pre-lock step in the
   Input arm.
3. `service_claude_verify` grows the confirm flavor — at the top, after
   taking `session`/`truth`, also read
   `inner.claude.is_awaiting_submit_confirm(terminal_id)` and the stashed
   offset; then:

```rust
        if awaiting_confirm {
            use crate::claude_truth::SubmitProbe;
            let probe = match (&session, &truth, offset) {
                (Some(session_id), Some(truth), Some(offset)) => {
                    truth.probe_submit(session_id, offset)
                }
                _ => SubmitProbe::Unavailable,
            };
            let frames = {
                let mut inner = self.inner.lock().expect("activity hub lock");
                let at = now_ms();
                let effects = match probe {
                    SubmitProbe::Confirmed => inner.claude.note_submit_confirmed(terminal_id, at),
                    SubmitProbe::NoTurnStarted => {
                        inner.claude.note_submit_unconfirmed(terminal_id, at)
                    }
                    SubmitProbe::Unavailable => {
                        inner.claude.note_submit_probe_unavailable(terminal_id);
                        Vec::new()
                    }
                };
                let (frames, _) = claude_frames(&mut inner.idle, effects);
                frames
            };
            self.emit(frames);
            return;
        }
        // …existing deadman-verify flavor from Task 10 unchanged…
```

- [ ] **Step 5: Hub-level test**

Extend `FakeClaudeTruth` (Task 10) with scripted `transcript_len` /
`probe_submit` results and add:

```rust
    /// #611 end-to-end: a bare Enter goes provisional; the confirm probe
    /// says NoTurnStarted; the pane silently reverts to idle well before
    /// the 120s deadman — and no bell rings.
    #[tokio::test(flavor = "multi_thread")]
    async fn claude_bare_enter_reverts_via_submit_probe() { … }
```

(`Created{mode:"claude", resume_session_id:Some("S")}`; fake:
`transcript_len → Some(100)`, `probe_submit → NoTurnStarted`; send
`Input{data:"\r"}`; within ~3s expect a `claude.activity.updated` upsert
with `phase == "idle"`; assert NO `terminal.turn.complete` and NO
`terminal.idle` frame arrived meanwhile.)

Run: `cargo test -p freshell-ws claude_bare_enter && cargo test -p freshell-activity && cargo test -p freshell-ws`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/freshell-activity/src/claude.rs crates/freshell-ws/src/activity.rs
git commit -m "fix(claude): bare Enter is provisional busy confirmed by the session JSONL; phantom submits revert silently (#611)"
```

---

### Task 12: amplifier — signal loss verifies; retry caps and repeats; exhaustion rings (#605)

**Files:**
- Modify: `crates/freshell-activity/src/amplifier/tracker.rs` (`note_events_signal_lost` ~:156-175; new `note_verify_failed`; tests)
- Modify: `crates/freshell-ws/src/activity.rs` (`lane_retry_delay_ms` ~:83-88; `note_lane_failure` ~:1390-1450; `amplifier_frames` AttentionBoundary arm ~:1809; tests ~:2687-2713, ~:2817+)

**Interfaces:**
- Consumes: existing `TrackerEffect::ForceRead` drain (`expire_due` → `drain_lane`), `LaneRetry` bookkeeping, `AMPLIFIER_LANE_RETRY_DELAYS_MS: [i64; 3] = [250, 1000, 3000]`.
- Produces:
  - Tracker: `note_events_signal_lost` — CONFIRMED busy keeps busy + emits `ForceRead` (grace machinery cleared); PROVISIONAL busy (unconfirmed) silently reverts (today's behavior — no confirmed turn existed); idle unchanged.
  - Tracker: `pub fn note_verify_failed(&mut self, terminal_id: &str, at: i64) -> Vec<AmplifierEffect>` — crash semantics: phase→Idle upsert + `AttentionBoundary` + `error!` log (no-op unless Busy).
  - Hub: `pub(crate) const AMPLIFIER_LANE_RETRY_CAP_MS: i64 = 3_000;` and `lane_retry_delay_ms(failures: u32) -> i64` (never `None` — past the schedule it returns the cap; retries continue FOREVER, the `amplifier_events_lane_dead` terminal state no longer exists).
  - Hub: crash-semantics escalation rule — when `failures` first EXCEEDS the bounded schedule (i.e. `failures == AMPLIFIER_LANE_RETRY_DELAYS_MS.len() as u32 + 1`, ~4.25s of consecutive failures), call `inner.amplifier.note_verify_failed(...)` (bell) while STILL scheduling the next capped retry; later failures keep retrying without re-ringing (the tracker method no-ops while Idle). A subsequent `Ok` read resets `failures` (existing `lane_retries.remove` on success, `activity.rs:1358`).
  - `amplifier_frames`: `AttentionBoundary` arms the gate (`idle.note_turn_boundary(&terminal_id, at)`) instead of the current no-op.

- [ ] **Step 1: Write the failing tracker tests**

REPLACE `signal_loss_reverts_busy_silently` (~:581) with (deliberate
inversion — the silent revert IS bug #605):

```rust
    #[test]
    fn signal_loss_on_confirmed_busy_verifies_and_stays_busy() {
        // #605: losing the tailer is not evidence the turn ended — hold
        // the light and force-read the events tail (disk truth decides).
        let mut tracker = AmplifierActivityTracker::new();
        tracker.track_terminal("t1", Some("sess-1"), 0);
        tracker.note_input("t1", "\r", 10);
        // Confirm busy via the reducer's TurnBegan (same helper the
        // existing tests use — see pty_enter_is_provisional_and_prompt_submit_confirms
        // at :499 for the apply_lifecycle call shape).
        confirm_busy(&mut tracker, "t1", 20);
        let effects = tracker.note_events_signal_lost("t1", 100);
        assert_eq!(force_reads(&effects), vec!["t1".to_string()]);
        assert_eq!(phases(&tracker), vec![("t1".into(), AmplifierPhase::Busy)]);
    }

    #[test]
    fn signal_loss_on_provisional_busy_reverts_silently() {
        // No confirmed turn existed — provisional reverts (unchanged).
        let mut tracker = AmplifierActivityTracker::new();
        tracker.track_terminal("t1", Some("sess-1"), 0);
        tracker.note_input("t1", "\r", 10);
        let effects = tracker.note_events_signal_lost("t1", 100);
        assert_eq!(phases(&tracker), vec![("t1".into(), AmplifierPhase::Idle)]);
        assert!(completions(&effects).is_empty());
        assert!(force_reads(&effects).is_empty());
    }

    #[test]
    fn verify_failed_clears_busy_and_rings_attention() {
        let mut tracker = AmplifierActivityTracker::new();
        tracker.track_terminal("t1", Some("sess-1"), 0);
        tracker.note_input("t1", "\r", 10);
        confirm_busy(&mut tracker, "t1", 20);
        let effects = tracker.note_verify_failed("t1", 5000);
        assert_eq!(phases(&tracker), vec![("t1".into(), AmplifierPhase::Idle)]);
        assert!(completions(&effects).is_empty());
        assert!(effects
            .iter()
            .any(|e| matches!(e, TrackerEffect::AttentionBoundary { at: 5000, .. })));
        // Idempotent: a second failure while idle is a no-op.
        assert!(tracker.note_verify_failed("t1", 6000).is_empty());
    }
```

Add the small helper next to the existing ones (`phases`/`completions`/
`force_reads` at :454-482), modeled on how
`prompt_complete_is_the_single_boundary_and_emits_one_completion` (:536)
confirms busy — check that test for the exact `apply_lifecycle` /
`ReducerEffect::TurnBegan` construction and extract:

```rust
    fn confirm_busy(tracker: &mut AmplifierActivityTracker, terminal_id: &str, at: i64) {
        // Exact ReducerEffect variant per reducer.rs — mirror the
        // TurnBegan application used by the existing confirmation tests.
        tracker.apply_lifecycle(terminal_id, &ReducerEffect::TurnBegan { at }, at);
    }
```

(Adjust the `TurnBegan` field list to the real `ReducerEffect` definition
in `crates/freshell-activity/src/amplifier/reducer.rs` — the existing
tests at :499/:536 show it.)

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p freshell-activity amplifier`
Expected: FAIL (signal loss currently reverts confirmed busy; no
`note_verify_failed`).

- [ ] **Step 3: Implement the tracker**

Replace `note_events_signal_lost` (~:156-175):

```rust
    /// The events signal for this terminal is gone (tailer degraded or
    /// detached). #605: a CONFIRMED busy turn holds its light and requests
    /// a force-read — only disk truth (the events tail) may end it. A
    /// PROVISIONAL busy (PTY Enter, never confirmed) reverts silently as
    /// before: no confirmed turn existed.
    pub fn note_events_signal_lost(&mut self, terminal_id: &str, at: i64) -> Vec<AmplifierEffect> {
        let Some(state) = self.states.get_mut(terminal_id) else {
            return Vec::new();
        };
        state.submit_grace_deadline = None;
        state.force_read_logged = false;
        state.next_force_read_at = None;
        if state.phase != AmplifierPhase::Busy {
            return Vec::new();
        }
        if state.busy_confirmed {
            return vec![TrackerEffect::ForceRead {
                terminal_id: terminal_id.to_string(),
                at,
            }];
        }
        state.busy_confirmed = false;
        let previous = state.to_record();
        state.phase = AmplifierPhase::Idle;
        state.updated_at = at;
        state.last_observed_at = at;
        let next = state.to_record();
        changed(Some(&previous), next)
    }
```

Add `note_verify_failed` (same file, after `note_exit`):

```rust
    /// The re-attach/verify path exhausted its bounded backoff — no
    /// readable truth source. Owner ruling: crash semantics — clear busy
    /// AND fire the attention/death engagement signal. No-op unless Busy
    /// (so repeated escalations never double-ring).
    pub fn note_verify_failed(&mut self, terminal_id: &str, at: i64) -> Vec<AmplifierEffect> {
        let Some(state) = self.states.get_mut(terminal_id) else {
            return Vec::new();
        };
        if state.phase != AmplifierPhase::Busy {
            return Vec::new();
        }
        tracing::error!(
            component = "amplifier-activity-tracker",
            event = "amplifier_verify_failed",
            terminal_id = %state.terminal_id,
            "amplifier events lane unreadable past bounded retries; clearing busy and ringing attention (owner ruling: probe failure = crash)"
        );
        state.busy_confirmed = false;
        state.submit_grace_deadline = None;
        state.next_force_read_at = None;
        let previous = state.to_record();
        state.phase = AmplifierPhase::Idle;
        state.updated_at = at;
        state.last_observed_at = at;
        let next = state.to_record();
        let mut effects = changed(Some(&previous), next);
        effects.push(TrackerEffect::AttentionBoundary {
            terminal_id: terminal_id.to_string(),
            at,
        });
        effects
    }
```

Run: `cargo test -p freshell-activity amplifier` — Expected: PASS.

- [ ] **Step 4: Write the failing hub tests**

REPLACE `lane_retry_schedule_is_bounded` (~:2687) with:

```rust
    #[test]
    fn lane_retry_schedule_caps_and_repeats() {
        // #605: no permanent give-up — past the bounded schedule the lane
        // retries forever at the cap. The crash-semantics bell fires ONCE
        // when the schedule is first exceeded (see note_lane_failure).
        assert_eq!(lane_retry_delay_ms(1), 250);
        assert_eq!(lane_retry_delay_ms(2), 1000);
        assert_eq!(lane_retry_delay_ms(3), 3000);
        assert_eq!(lane_retry_delay_ms(4), AMPLIFIER_LANE_RETRY_CAP_MS);
        assert_eq!(lane_retry_delay_ms(99), AMPLIFIER_LANE_RETRY_CAP_MS);
    }
```

REWRITE `exhausted_lane_retries_give_up_loudly` (~:2817) as
`exhausted_lane_retries_ring_and_keep_retrying`: same failure-injection
setup, but assert that after the 4th consecutive failure (a) an
`amplifier.activity.updated` upsert with `phase == "idle"` AND a
`terminal.idle` frame (reason `grace`) are broadcast, and (b) the
`lane_retries` entry STILL exists with `next_attempt_at == Some(now + 3000)`
(assert via `hub_next_deadline`-observable behavior or a `#[cfg(test)]`
accessor — the sibling test `lane_retry_deadline_feeds_hub_next_deadline`
at ~:2696 shows the pattern), and (c) no `amplifier_events_lane_dead` state
exists (the tracker still accepts a later `Created`/re-attach — assert a
subsequent successful attach+read drives a busy record again, following
`degraded_lane_reattaches_and_recovers` at ~:2716).

- [ ] **Step 5: Implement the hub half**

In `activity.rs`:

1. Constants + fn:

```rust
/// #605: past the bounded schedule the lane retries FOREVER at this cap —
/// a degraded events lane is never abandoned (the crash-semantics bell at
/// schedule exhaustion is the loud signal; recovery stays possible).
pub(crate) const AMPLIFIER_LANE_RETRY_CAP_MS: i64 = 3_000;

/// Backoff delay before the retry that follows the `failures`-th
/// consecutive failure (1-based). Never gives up: past the schedule the
/// cap repeats.
pub(crate) fn lane_retry_delay_ms(failures: u32) -> i64 {
    failures
        .checked_sub(1)
        .and_then(|i| AMPLIFIER_LANE_RETRY_DELAYS_MS.get(i as usize).copied())
        .unwrap_or(AMPLIFIER_LANE_RETRY_CAP_MS)
}
```

2. `note_lane_failure` (~:1390-1450): the give-up arm disappears; the body
   becomes:

```rust
        let failures = inner
            .lane_retries
            .get(terminal_id)
            .map(|retry| retry.failures)
            .unwrap_or(0)
            + 1;
        let delay_ms = lane_retry_delay_ms(failures);
        // #605 crash-semantics escalation: the FIRST failure past the
        // bounded schedule clears busy and rings the attention boundary
        // (owner ruling: verify failure = crash) — but retries continue
        // at the cap; a later Ok read resets `failures` and recovery is
        // full. `permanent` failures escalate immediately.
        let exhausted = permanent
            || failures == AMPLIFIER_LANE_RETRY_DELAYS_MS.len() as u32 + 1;
        if exhausted {
            tracing::error!(
                terminal_id = %terminal_id,
                failures,
                permanent,
                "amplifier_events_lane_verify_failed: events lane unreadable past bounded re-attach; ringing attention and continuing capped retries"
            );
            let effects = inner.amplifier.note_verify_failed(terminal_id, now_ms());
            let (mut f, _) = amplifier_frames(&mut inner.idle, effects);
            frames.append(&mut f);
        } else {
            tracing::warn!(
                terminal_id = %terminal_id,
                failures,
                delay_ms,
                "amplifier_events_lane_retry_scheduled"
            );
        }
        // Gap-loss bookkeeping (see step 5): capture the file length at
        // the FIRST failure of the episode; later failures keep it.
        let bytes_at_degrade = if failures == 1 {
            std::fs::metadata(events_path).ok().map(|m| m.len())
        } else {
            inner
                .lane_retries
                .get(terminal_id)
                .and_then(|retry| retry.bytes_at_degrade)
        };
        inner.lane_retries.insert(
            terminal_id.to_string(),
            LaneRetry {
                session_id: session_id.to_string(),
                events_path: events_path.to_path_buf(),
                failures,
                next_attempt_at: Some(now_ms() + delay_ms),
                bytes_at_degrade,
            },
        );
```

   (Note `note_verify_failed` no-ops when the tracker is already Idle, so
   `permanent` re-entries and post-escalation failures never double-ring.)
3. `amplifier_frames` (~:1809): replace the AttentionBoundary no-op with:

```rust
            TrackerEffect::AttentionBoundary { terminal_id, at } => {
                // #605: crash-semantics boundary — arms the gate WITHOUT a
                // turn.complete frame, same contract as codex approvals.
                idle.note_turn_boundary(&terminal_id, at);
            }
```

4. In `drain_lane`'s `Degraded` arm (~:1370-1372), the `ForceRead` that
   `note_events_signal_lost` now returns for a confirmed-busy pane must be
   intentionally DROPPED there (comment:
   `// ForceRead against a degraded tailer would loop; the bounded
   re-attach owns recovery`) — the effects still flow through
   `amplifier_frames` for their `Changed` frames, but the collected
   force-read list at that call site is discarded.
5. **Gap-loss rule (deterministic, stated):** records appended during a
   degrade gap are unverifiable (re-attach is `AttachAt::Eof`). Extend
   `LaneRetry` with `bytes_at_degrade: Option<u64>`, captured in
   `note_lane_failure` on the FIRST failure of an episode
   (`failures == 1`) via
   `std::fs::metadata(events_path).ok().map(|m| m.len())`. In
   `expire_due`'s re-attach loop, BEFORE `attach_lane`, re-stat the
   resolved events path: if the length differs from `bytes_at_degrade`
   (records landed while we were blind), the in-flight turn cannot be
   verified — apply crash semantics
   (`inner.amplifier.note_verify_failed(...)` + emit) and then attach at
   Eof for fresh tracking; if the length is unchanged, attach at Eof with
   the busy record intact (nothing was missed). This closes the
   "turn completed during the gap ⇒ busy-forever" hole deterministically:
   grew = ring-and-clear, unchanged = safe hold. Pin it with a hub test
   `degrade_gap_growth_rings_and_clears` (write extra bytes to the temp
   events file between the injected degrade and the retry window, assert
   the `terminal.idle` + idle upsert; and the inverse: no growth ⇒ busy
   record survives re-attach).

- [ ] **Step 6: Run the suites**

Run: `cargo test -p freshell-ws && cargo test -p freshell-activity`
Expected: PASS (the rewritten hub tests included; also confirm
`degraded_lane_reattaches_and_recovers` still passes — the recovery path
is unchanged).

- [ ] **Step 7: Update the residual/doc comments**

- `amplifier/tracker.rs` module doc line ~16 ("Signal loss … reverts busy
  to idle silently") → "Signal loss on a CONFIRMED turn holds busy and
  force-reads the events tail; only provisional busy reverts silently
  (#605). Exhausted re-attach escalates to crash semantics (clear +
  attention) while retries continue at a capped interval."
- `activity.rs` comment at ~:78 ("after the last entry the lane gives up
  LOUDLY") → "after the last entry the cap repeats forever; the first
  past-schedule failure rings crash semantics (#605)".

Run: `cargo test -p freshell-ws` (doc-only recheck).

- [ ] **Step 8: Commit**

```bash
git add crates/freshell-activity/src/amplifier/tracker.rs crates/freshell-ws/src/activity.rs
git commit -m "fix(amplifier): signal loss verifies via force-read; retries cap-and-repeat; exhaustion rings attention (#605)"
```

---

### Task 13: death bell — suppress human-initiated quits (#612)

**Files:**
- Modify: `crates/freshell-activity/src/signal.rs` (new quit-intent classifier + tests)
- Modify: `crates/freshell-ws/src/activity.rs` (Input-arm classification, Exit-arm consultation, teardown; tests)
- Modify: `crates/freshell-activity/src/idle.rs` (Accepted Residuals entries 1-3 and 11)

**Owner ruling recap:** external `kill -9`/SIGTERM keeps ringing (intended;
no work). User-typed quits through freshell's own input stream — `/quit`,
`/exit`, Ctrl+D (0x04), Ctrl+C (0x03) — must NOT ring. Freshell owns the
PTY input, so these are deterministic byte observations.

**Marker rules (exact, stated — not tuned probabilities):**
1. A quit-intent input observed at time T sets the terminal's marker to T
   (overwriting any previous marker).
2. A later SUBMIT-shaped input whose line is NOT a quit command CLEARS the
   marker (the user is still driving the pane).
3. The marker EXPIRES `QUIT_INTENT_TTL_MS = 15_000` ms after T (covers
   slow TUI shutdowns including opencode's 5s dispose cap, while a "much
   later" real crash still rings). Typing `/quit` as literal prompt TEXT
   and crashing within 15s is the one accepted false-suppress; it is named
   in the idle.rs residual note below.
4. A spontaneous exit with an unexpired marker suppresses the death bell
   (info-level log states why); everything else rings exactly as today.

**Line-buffer rules for detecting typed `/quit` / `/exit` (exact):** per
terminal, keep a small line buffer (cap 32 bytes). For each char of each
input chunk, in order: `\r`/`\n` → evaluate (trimmed buffer == "/quit" or
"/exit" ⇒ quit-intent), then clear the buffer; printable (` `..`~` or
non-ASCII) → append (if the cap is hit, mark the line unmatchable until
the next newline); 0x7f/0x08 (backspace) → pop one; 0x03 (Ctrl+C) / 0x04
(Ctrl+D) → quit-intent immediately + clear buffer; ESC 0x1b or any other
control byte → clear the buffer and mark unmatchable until the next
newline (arrow-key/TUI-menu escape sequences make the buffer meaningless —
that narrow slice, e.g. a TUI menu quit via arrows+Enter, remains
agent-evidence-dependent; it is named in the residual note).

**Interfaces:**
- Produces in `signal.rs`:

```rust
#[derive(Debug, Default)]
pub struct QuitIntentState {
    line: String,
    unmatchable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputClass {
    /// Ctrl+C / Ctrl+D, or a submitted line equal to /quit or /exit.
    QuitIntent,
    /// A submitted line that is NOT a quit command.
    NonQuitSubmit,
    /// Anything else (typing, escape sequences, partial chunks).
    Other,
}

pub fn classify_input(state: &mut QuitIntentState, data: &str) -> InputClass;
```

- Produces in `activity.rs`: `HubInner.quit_intent_lines: HashMap<String, QuitIntentState>`, `HubInner.quit_intents: HashMap<String, i64>`, `pub(crate) const QUIT_INTENT_TTL_MS: i64 = 15_000;`, and pure fn `pub(crate) fn quit_intent_active(marker_at: Option<i64>, exit_at: i64) -> bool` (`marker_at.is_some_and(|t| exit_at - t <= QUIT_INTENT_TTL_MS)`).

- [ ] **Step 1: Write the failing classifier tests**

In `signal.rs` tests (next to `is_submit_input_matches_the_reference_regex`
at ~:283):

```rust
    #[test]
    fn quit_intent_classification_rules() {
        let mut s = QuitIntentState::default();
        // Typed char-by-char: /quit + Enter.
        for c in ["/", "q", "u", "i", "t"] {
            assert_eq!(classify_input(&mut s, c), InputClass::Other);
        }
        assert_eq!(classify_input(&mut s, "\r"), InputClass::QuitIntent);

        // Pasted whole line.
        assert_eq!(classify_input(&mut s, "/exit\r"), InputClass::QuitIntent);

        // Control-key quits.
        assert_eq!(classify_input(&mut s, "\u{4}"), InputClass::QuitIntent);
        assert_eq!(classify_input(&mut s, "\u{3}"), InputClass::QuitIntent);

        // An ordinary prompt is a NonQuitSubmit at its Enter.
        assert_eq!(classify_input(&mut s, "fix the bug"), InputClass::Other);
        assert_eq!(classify_input(&mut s, "\r"), InputClass::NonQuitSubmit);

        // Backspace editing: "/quitX" + BS + Enter is a quit.
        assert_eq!(classify_input(&mut s, "/quitX"), InputClass::Other);
        assert_eq!(classify_input(&mut s, "\u{7f}"), InputClass::Other);
        assert_eq!(classify_input(&mut s, "\r"), InputClass::QuitIntent);

        // An escape sequence poisons the line until the next newline:
        // arrow-key navigation + Enter is NOT a detectable quit.
        assert_eq!(classify_input(&mut s, "/quit"), InputClass::Other);
        assert_eq!(classify_input(&mut s, "\u{1b}[A"), InputClass::Other);
        assert_eq!(classify_input(&mut s, "\r"), InputClass::NonQuitSubmit);

        // …and the poison clears after that newline.
        assert_eq!(classify_input(&mut s, "/quit\r"), InputClass::QuitIntent);
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p freshell-activity quit_intent_classification`
Expected: compile FAIL.

- [ ] **Step 3: Implement the classifier**

In `signal.rs`:

```rust
/// #612: deterministic quit-intent detection on freshell's own PTY input
/// stream. Rules (exact, stated): Ctrl+C/Ctrl+D are immediate quit
/// intents; a submitted line equal to "/quit" or "/exit" (after trimming)
/// is a quit intent; escape sequences poison the line buffer until the
/// next newline (TUI-menu quits are NOT detectable here — that residual
/// stays agent-evidence-dependent, idle.rs entry 11).
const QUIT_LINE_CAP: usize = 32;

pub fn classify_input(state: &mut QuitIntentState, data: &str) -> InputClass {
    let mut class = InputClass::Other;
    for c in data.chars() {
        match c {
            '\u{3}' | '\u{4}' => {
                state.line.clear();
                state.unmatchable = false;
                class = InputClass::QuitIntent;
            }
            '\r' | '\n' => {
                let line = state.line.trim();
                let is_quit = !state.unmatchable && (line == "/quit" || line == "/exit");
                // A quit anywhere in the chunk wins over a later submit.
                if is_quit {
                    class = InputClass::QuitIntent;
                } else if class != InputClass::QuitIntent && !line.is_empty() {
                    class = InputClass::NonQuitSubmit;
                }
                state.line.clear();
                state.unmatchable = false;
            }
            '\u{7f}' | '\u{8}' => {
                state.line.pop();
            }
            c if c >= ' ' => {
                if state.line.len() >= QUIT_LINE_CAP {
                    state.unmatchable = true;
                } else if !state.unmatchable {
                    state.line.push(c);
                }
            }
            _ => {
                // ESC / other control bytes: the buffer no longer
                // represents the visible line.
                state.line.clear();
                state.unmatchable = true;
            }
        }
    }
    class
}
```

(Note: a bare Enter on an empty buffer is `Other` — it neither sets nor
clears the marker; that keeps stray-Enter noise out of the marker rules.)

Run: `cargo test -p freshell-activity quit_intent_classification` — PASS.

- [ ] **Step 4: Write the failing hub tests**

In `activity.rs` tests (exit-arm harness — copy
`spontaneous_exit_while_busy_rings_terminal_idle_once` ~:2128 and
`freshell_initiated_kill_while_busy_stays_silent` ~:2181 for the shapes):

```rust
    #[test]
    fn quit_intent_activity_window() {
        assert!(quit_intent_active(Some(1_000), 1_000 + QUIT_INTENT_TTL_MS));
        assert!(!quit_intent_active(Some(1_000), 1_001 + QUIT_INTENT_TTL_MS));
        assert!(!quit_intent_active(None, 5_000));
    }

    /// #612: busy pane + observed quit input + spontaneous exit ⇒ NO
    /// death bell; busy pane + ordinary input + spontaneous exit ⇒ the
    /// bell still rings (both directions pinned).
    #[tokio::test(flavor = "multi_thread")]
    async fn quit_intent_suppresses_the_death_bell_and_ordinary_input_does_not() { … }
```

Build the async test on a `mode:"claude"` pane: `Created` →
`Input{data:"\r", …}` (busy) → `Input{data:"\u{4}", …}` (Ctrl+D) →
`Exit{spontaneous:true}` → assert NO `terminal.idle` frame within ~300ms.
Then a second terminal: `Created` → `Input{"\r"}` → `Input{"fix it\r"}`
(NonQuitSubmit clears nothing here — no marker set) →
`Exit{spontaneous:true}` → assert the `terminal.idle` frame ARRIVES. Also
pin the clearing rule with a third terminal: `Input{"\u{4}"}` (marker) →
`Input{"continue\r"}` (NonQuitSubmit clears) → `Exit{spontaneous:true}` →
bell RINGS.

- [ ] **Step 5: Implement the hub half**

In `activity.rs`:

1. `HubInner` fields (Default empty maps) + `QUIT_INTENT_TTL_MS` +
   `quit_intent_active` as specified in Interfaces.
2. Input arm (~:1052-1092), BEFORE the per-mode tracker dispatch (applies
   to ALL modes — codex/claude/amplifier/opencode all have quit-capable
   TUIs):

```rust
                    // #612: quit-intent bookkeeping on the input stream
                    // freshell owns. Rules: see signal::classify_input.
                    let quit_state = inner
                        .quit_intent_lines
                        .entry(terminal_id.clone())
                        .or_default();
                    match freshell_activity::signal::classify_input(quit_state, &data) {
                        freshell_activity::signal::InputClass::QuitIntent => {
                            inner.quit_intents.insert(terminal_id.clone(), at);
                        }
                        freshell_activity::signal::InputClass::NonQuitSubmit => {
                            inner.quit_intents.remove(&terminal_id);
                        }
                        freshell_activity::signal::InputClass::Other => {}
                    }
```

3. Exit arm (~:1129-1148): consult the marker in the ring predicate:

```rust
                    let quit_intent =
                        quit_intent_active(inner.quit_intents.get(&terminal_id).copied(), at);
                    let ring_death_bell = spontaneous
                        && !quit_intent
                        && ((inner.idle.is_engaged(&terminal_id) && opencode_death_eligible)
                            || inner.codex.has_pending_approvals(&terminal_id)
                            || (inner.opencode.has_pending_permissions(&terminal_id)
                                && opencode_death_eligible));
                    if spontaneous && quit_intent {
                        tracing::info!(
                            terminal_id = %terminal_id,
                            "death bell suppressed: human quit intent observed on the input stream (#612)"
                        );
                    }
```

4. Teardown block (inside `if let Some(mode) = inner.modes.remove(…)`):
   `inner.quit_intents.remove(&terminal_id); inner.quit_intent_lines.remove(&terminal_id);`

- [ ] **Step 6: Run the suites**

Run: `cargo test -p freshell-ws && cargo test -p freshell-activity`
Expected: PASS — including the untouched pins
`spontaneous_exit_while_busy_rings_terminal_idle_once` (real crash still
rings) and `freshell_initiated_kill_while_busy_stays_silent`.

- [ ] **Step 7: Update the residual registry**

`crates/freshell-activity/src/idle.rs` entries 1-3 (~:38-46) and 11
(~:79-85): annotate each with:

```rust
//!  (#612, 2026-08-06) User-typed quits through freshell's own input
//!  stream — /quit, /exit, Ctrl+D, Ctrl+C — now suppress the death bell
//!  via a 15s quit-intent marker (signal::classify_input; exact rules
//!  there). Remaining residuals, adjudicated: out-of-band kill -9 RINGS
//!  (intended — a working agent killed externally is worth announcing);
//!  TUI-menu quits driven by escape sequences produce no detectable byte
//!  sequence and stay agent-evidence-dependent; /quit typed as literal
//!  prompt text followed by a crash within 15s is the one accepted
//!  false-suppress.
```

(Fold into the existing entries' text rather than duplicating the list —
keep one registry entry per residual with the update inline.)

- [ ] **Step 8: Commit**

```bash
git add crates/freshell-activity/src/signal.rs crates/freshell-ws/src/activity.rs crates/freshell-activity/src/idle.rs
git commit -m "fix(activity): suppress death bell on observed human quit intent (/quit, /exit, Ctrl+C/D) (#612)"
```

---

### Task 14: codex unmanaged approvals — document the owner ruling (#607)

**Files:**
- Modify: `crates/freshell-activity/src/idle.rs` (Accepted Residuals entry 7, ~:60)
- Modify: `crates/freshell-activity/src/codex.rs` (module-doc note near the pending_approvals doc, ~:153-157)

No code behavior changes. Owner ruling (2026-08-05): unmanaged/PTY-only
codex panes make NO approval-notification promise; the guarantee is scoped
to managed (proxy-lane) panes; PTY text-parsing heuristics are rejected.
This task makes the scoped promise explicit where implementers will look.

- [ ] **Step 1: Update `idle.rs` entry 7**

Replace the entry-7 text (~:60) with:

```rust
//!  7. (ADJUDICATED by owner ruling 2026-08-05, #607) Unmanaged/PTY-only
//!     codex panes make NO approval-notification promise: when codex
//!     pauses for approval, such panes keep an honest busy light until
//!     the turn resumes or ends. The approval-pause guarantee (demote +
//!     attention boundary + death-bell engagement) is scoped to MANAGED
//!     proxy-lane panes (note_approval_requested/resolved). PTY
//!     text-parsing heuristics were considered and REJECTED (owner
//!     policy: no heuristics). Freshell-launched codex panes use the
//!     managed lane, so the unmanaged population is externally-attached
//!     panes only.
```

- [ ] **Step 2: Add the scoping note in `codex.rs`**

Above `pending_approvals` (~:153), extend the doc comment's first line to:

```rust
    /// Outstanding server→client approval request ids (managed proxy lane
    /// ONLY — the approval-notification promise is scoped to managed
    /// panes per owner ruling 2026-08-05, #607; unmanaged/PTY-only panes
    /// have no approval signal and stay honestly busy).
```

- [ ] **Step 3: Verify + commit**

Run: `cargo test -p freshell-activity` (doc-only; suite green).

```bash
git add crates/freshell-activity/src/idle.rs crates/freshell-activity/src/codex.rs
git commit -m "docs(codex): scope the approval-notification promise to managed panes per owner ruling (#607)"
```

---

### Task 15: build stamp closes the packed-ref gap; boot line self-identifies (#613)

**Files:**
- Modify: `crates/freshell-server/build.rs` (`rerun_paths` ~:104-111)
- Modify: `crates/freshell-server/src/diag.rs` (new `boot_line` + `iso8601_utc` helpers + unit tests)
- Modify: `crates/freshell-server/src/main.rs` (boot `eprintln!` ~:1376-1379)
- Create: `scripts/build-stamp-check.sh`

**Scope note (from the issue's approved direction):** gap (1) — the
`exists()`-gated loose-ref watch — is closed here. Gap (2) — `buildDirty`
staleness on purely-worktree-side edits — stays an accepted, in-code
documented residual per the issue ("leave as-is unless Dan wants it
closed"; adjudication was offered, no closure ruling issued). Gap (3) —
boot-line forensics — is closed here (timestamp + PID + commit + dirty).
Verify-at-read (fix direction item 3) is served by `/api/server-info`
already reporting `commit`/`buildDirty` (unchanged) — the boot line now
carries the same self-identification so log archaeology never guesses.

- [ ] **Step 1: Write the scripted integration check (red first)**

Create `scripts/build-stamp-check.sh` (executable):

```bash
#!/usr/bin/env bash
# #613 acceptance check for crates/freshell-server/build.rs's commit
# stamp: proves a same-branch ref update performed while the ref is
# PACKED (loose file absent at stamp time, written loose later by
# git update-ref — the fetch/ff-pull shape) still restamps
# FRESHELL_BUILD_COMMIT. Step (e) FAILS against the exists()-gated watch
# and PASSES after the unconditional ref watch.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "${WORK}"' EXIT

echo "--- setup: throwaway git repo + tiny crate embedding build.rs ---"
mkdir -p "${WORK}/proj/src"
cp "${REPO_ROOT}/crates/freshell-server/build.rs" "${WORK}/proj/build.rs"
cat > "${WORK}/proj/Cargo.toml" <<'EOF'
[package]
name = "stampcheck"
version = "0.0.0"
edition = "2021"

[workspace]
EOF
cat > "${WORK}/proj/src/main.rs" <<'EOF'
fn main() {
    println!("{}", option_env!("FRESHELL_BUILD_COMMIT").unwrap_or("unknown"));
}
EOF
cd "${WORK}/proj"
git init -q
git config user.email "check@example.com"
git config user.name "Stamp Check"
git add -A
git commit -qm "c1"
C1="$(git rev-parse HEAD)"

echo "--- (a)(b) build once, then pack refs (loose ref file goes away) ---"
cargo build -q
git pack-refs --all
test ! -f .git/refs/heads/*  2>/dev/null || true

echo "--- (c) build in the packed-ref state (stamps under packed refs) ---"
cargo build -q
OUT1="$(./target/debug/stampcheck)"
[ "${OUT1}" = "${C1}" ] || { echo "FAIL: baseline stamp ${OUT1} != ${C1}"; exit 1; }

echo "--- (d) advance the branch ref WITHOUT touching HEAD bytes or index ---"
git commit -q --allow-empty -m "c2"
C2="$(git rev-parse HEAD)"
# The empty commit wrote the ref LOOSE again and did not change the index
# or working tree — exactly the watched-file blind spot.

echo "--- (e) rebuild and assert the NEW sha is compiled in ---"
cargo build -q
OUT2="$(./target/debug/stampcheck)"
if [ "${OUT2}" = "${C2}" ]; then
  echo "PASS: stamp followed the ref update (${C2})"
else
  echo "FAIL: stale stamp ${OUT2}; expected ${C2}"
  exit 1
fi
```

Run: `chmod +x scripts/build-stamp-check.sh && ./scripts/build-stamp-check.sh`
Expected: **FAIL at step (e)** against the current build.rs (the loose-ref
path was not watched because it did not exist at stamp time).
(Note: this script needs network-free cargo + a few seconds; it is a
manual/scripted gate, not a `cargo test`.)

- [ ] **Step 2: Close the gap in build.rs**

In `rerun_paths()` (~:104-111), remove the `exists()` gate on the ref file
ONLY (keep the gates on `packed-refs` and `index` — those genuinely may
not exist):

```rust
    if let Some(ref_name) = run_git(&["symbolic-ref", "-q", "HEAD"]) {
        if let Some(ref_path) = run_git(&["rev-parse", "--git-path", &ref_name]) {
            // #613: watch the resolved loose-ref path UNCONDITIONALLY.
            // When the ref is packed the loose file is absent — cargo
            // treats a watched-but-missing path as changed and reruns
            // this script every build in that state (4 subprocess calls,
            // an acceptable deterministic cost) — which guarantees a
            // later fetch/ff-pull writing the ref LOOSE can never be
            // missed by the stamp.
            paths.push(PathBuf::from(ref_path));
        }
    }
```

Run: `./scripts/build-stamp-check.sh`
Expected: PASS at step (e).

- [ ] **Step 3: Write the failing boot-line unit test**

In `crates/freshell-server/src/diag.rs`'s test module:

```rust
    #[test]
    fn boot_line_is_self_identifying() {
        // #613 forensics: timestamp + pid + commit + dirty on ONE line —
        // an append-mode log can always attribute a boot line to a run.
        let line = boot_line(
            "127.0.0.1:3001",
            "abc123",
            "false",
            4242,
            "2026-08-06T12:34:56Z",
        );
        assert_eq!(
            line,
            "[2026-08-06T12:34:56Z] freshell-server listening on http://127.0.0.1:3001 (ws://127.0.0.1:3001/ws) [pid 4242] [commit abc123] [dirty false]"
        );
    }

    #[test]
    fn iso8601_utc_formats_epoch_seconds() {
        assert_eq!(iso8601_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso8601_utc(1_754_500_000), "2025-08-06T17:06:40Z");
    }
```

(The second literal was verified with `date -u -d @1754500000 +%FT%TZ` →
`2025-08-06T17:06:40Z`.)

Run: `cargo test -p freshell-server boot_line iso8601`
Expected: compile FAIL.

- [ ] **Step 4: Implement the formatter (std-only — no new deps)**

In `diag.rs`:

```rust
/// #613: one self-identifying boot line — timestamp + pid + commit +
/// dirty — so append-mode logs with multiple runs can always attribute a
/// line to a binary. Pure formatter; unit-tested.
pub(crate) fn boot_line(
    addr: &str,
    commit: &str,
    dirty: &str,
    pid: u32,
    timestamp_utc: &str,
) -> String {
    format!(
        "[{timestamp_utc}] freshell-server listening on http://{addr} (ws://{addr}/ws) [pid {pid}] [commit {commit}] [dirty {dirty}]"
    )
}

/// Minimal std-only ISO-8601 UTC (seconds precision) from unix seconds —
/// civil-from-days per Howard Hinnant's algorithm; avoids adding a chrono
/// dependency for one log line.
pub(crate) fn iso8601_utc(unix_seconds: i64) -> String {
    let days = unix_seconds.div_euclid(86_400);
    let secs = unix_seconds.rem_euclid(86_400);
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}
```

In `main.rs` (~:1376-1379), replace the `eprintln!` with:

```rust
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    eprintln!(
        "{}",
        diag::boot_line(
            &addr.to_string(),
            diag::build_commit(),
            diag::build_dirty_str(),
            std::process::id(),
            &diag::iso8601_utc(now_secs),
        )
    );
```

(`build_dirty_str`: `diag.rs` has `build_dirty()` — check its return type;
if it returns a bool/Value, add `pub(crate) fn build_dirty_str() -> &'static str
{ option_env!("FRESHELL_BUILD_DIRTY").unwrap_or("unknown") }` beside
`build_commit()` and use that, keeping the fail-closed "unknown" text.)

- [ ] **Step 5: Run the tests**

Run: `cargo test -p freshell-server && ./scripts/build-stamp-check.sh`
Expected: PASS + PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/freshell-server/build.rs crates/freshell-server/src/diag.rs crates/freshell-server/src/main.rs scripts/build-stamp-check.sh
git commit -m "fix(build): watch the loose-ref path unconditionally; self-identifying boot line + stamp check script (#613)"
```

---

### Task 16: full verification and landing (pre-approved)

**Files:** none created/modified (verification + git/GitHub operations only).

Pushing the branch, creating the PR to `main`, and self-merging once
required checks pass are ALL pre-approved by Dan for this run. Deploying
is out of scope: do NOT touch the live server.

- [ ] **Step 1: Full Rust verification**

```bash
cargo fmt --all --check
cargo clippy -p freshell-activity -p freshell-ws -p freshell-server --all-targets
cargo test -p freshell-activity
cargo test -p freshell-ws
cargo test -p freshell-server
cargo test -p freshell-protocol
./scripts/build-stamp-check.sh
```

Expected: all green. Fix any fmt/clippy fallout (formatting-only commits
are fine: `style: cargo fmt fallout`).

- [ ] **Step 2: Node suite (no TS was modified — regression gate only)**

```bash
npm run test:vitest
```

Expected: green (identical to a pre-branch run). If a failure appears,
bisect against `origin/main` before assuming it is ours — this plan
touches no `server/`, `shared/`, or `src/` TypeScript.

- [ ] **Step 3: Push and open the PR**

```bash
git push -u origin attention-bell-wrong-signals
gh pr create --base main --title "fix(activity): attention-bell wrong-signal fixes — verify-before-change across all four trackers (#603-#613)" --body "$(cat <<'EOF'
Lands the attention-bell wrong-signal audit fixes. Closes #603, #604,
#605, #606, #607, #608, #609, #610, #611, #612, #613.

Every fix follows the owner-approved pattern: verify against a truth
source before changing any light; verify-probe failure = crash semantics
(clear busy + attention boundary). No unknown states, no heuristics.

- #603/#604: opencode deadman verifies via GET /session/status through
  the lane (current cycle/stream stamps); snapshot parse trouble degrades
  toward busy or fails loud; permission.v2.*/question.* vocabulary;
  drift contradiction detector (+ server-info counter).
- #609/#610: per-pane lane busy roots bind identity directly (first-turn
  bells + death rings); Ambiguous re-promotes from a single-busy-root
  verify snapshot.
- #608: GET /permission resync on reconnect; busy snapshots no longer
  clear an outstanding pause.
- #606/#611: claude session-JSONL truth source (spike-verified on
  2.1.223): deadman verifies (verified end RINGS, probe failure rings
  attention); bare Enter is provisional busy confirmed by an
  offset-based JSONL probe (phantom submits revert silently; the
  phantom-BEL completion skew is gone).
- #605: amplifier signal loss verifies via force-read; lane retries
  cap-and-repeat forever; exhaustion rings attention (no more
  lane-dead-forever).
- #612: user-typed quits (/quit, /exit, Ctrl+C/D) observed on freshell's
  own input stream suppress the death bell (15s marker, exact rules in
  signal.rs); external kills still ring (intended).
- #607: approval-notification promise documented as scoped to managed
  codex panes (owner ruling).
- #613: unconditional loose-ref watch (scripted check proves the
  packed→loose transition restamps); self-identifying boot line
  (timestamp + pid + commit + dirty).

Residual registry (idle.rs) updated per fix. No wire-protocol changes;
no client changes; deploy intentionally NOT performed.
EOF
)"
```

- [ ] **Step 4: Merge when checks pass (pre-approved)**

```bash
gh pr checks --watch
gh pr merge --squash
```

Expected: checks green, PR merged into main. Do NOT deploy/restart
anything; the live server keeps running its current build.

---

## Self-Review (performed against the handoff + issue bodies)

**1. Spec coverage.** Each issue maps to tasks (see the Task Right-Sizing
Map). Owner rulings: ruling 1 (probe failure = crash) is implemented for
#603/#604 (Tasks 1-3: `note_verify_failed` + `SnapshotFailed`), #605
(Task 12 escalation), #606 (Task 10 `Unavailable → note_verify_failed`,
explicitly including never-bound sessions), #609/#610 (the same opencode
`note_verify_failed` path covers verify failures in every ownership state,
Task 1's test pins the mid-pause case). Ruling 2 (#607) → Task 14. Ruling
3 (#612) → Task 13, both directions pinned. Ruling 4 (#611) → the spike
succeeded, Task 11 is verify-backed; the bounded-gate fallback is
deliberately NOT built.

**1b. No silent deferrals.** Production outcomes are real, not stubbed:
the opencode verify uses the production lane HTTP seam (fakes exist only
in tests, as they already do today); `FsClaudeTruth` is production file IO
installed at boot (Task 10 Step 4.5) and unit-tested against real record
shapes; the amplifier path reuses the production tailer. Named residuals
are all OWNER-ADJUDICATED scope statements, not silent deferrals:
shared-endpoint opencode panes (#609, ruling in issue), genuinely-plural
busy roots (#610), unmanaged codex approvals (#607, ruling 2), external
kill -9 rings (#612, ruling 3), TUI-menu quits + the 15s literal-text
false-suppress (#612 — the issue itself requires "stated rules", which
Task 13 states and documents in idle.rs), buildDirty worktree staleness
(#613 — the issue's own fix direction says leave as-is absent a closure
ruling). #604's "loud surface" ships BOTH the error-level log and the
`/api/server-info` counter. No task leaves a TODO, stub, or "future work"
in production code.

**2. Placeholder scan.** Two `…` ellipses remain by design: (a) inside
test-skeleton blocks that are immediately followed by concrete build
instructions naming the exact events, fakes, and assertions to use (Tasks
2/6/10/11/13 hub tests — the harness helpers are quoted from the existing
suite and the assertion targets are named exactly); (b) "existing code
unchanged — move verbatim" markers (Task 2's pump body, Task 7's
empty-roots arm) where copying the current code into this document would
only invite drift. Every new function, test constant, and rule is given in
full. No TBD/TODO/"handle edge cases" anywhere.

**3. Type consistency.** Checked: `TrackerEffect` variant spellings match
`lib.rs:41-57`; `note_verify_failed(&mut self, terminal_id: &str, at: i64)`
is uniform across opencode/claude/amplifier; `claude_frames` and
`opencode_frames` both change to the `(Vec<ServerMessage>, Vec<String>)`
shape `codex_frames` already has; `ClaudeTruth` method names
(`probe_turn_state`/`transcript_len`/`probe_submit`) are identical in
Task 9 (definition) and Tasks 10/11 (consumers); `spawn_opencode_lane`'s
new return tuple is consumed with the same shape in the attach arm and the
Task 2 lane test; `classify_input`/`QuitIntentState`/`InputClass` match
between signal.rs (Task 13 Step 3) and the hub intake (Step 5).
`lane_retry_delay_ms` changes from `Option<i64>` to `i64` in exactly one
task (12) and both its callers (`note_lane_failure`, the schedule test)
are updated there.

**Known execution-order note:** Tasks 10 and 11 both touch
`claude.rs`/`activity.rs`; Task 11's Step 3.4 amends Task 10's
`note_verified_ended`/`note_verify_failed` (two added field resets) —
executed in order, this is a plain edit, and the plan states it explicitly
in Task 11's interface notes.
