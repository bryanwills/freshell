# Fix freshclaude/kilroy Attach Swallow (Lost-Session Frame) Implementation Plan

> **For agentic workers:** This plan is executed task-by-task by the
> workflow's execute stage: a fresh implementer per task, with a spec +
> quality review after each task. Steps use checkbox (`- [ ]`) syntax
> for tracking.

**Goal:** Route claude/kilroy `freshAgent.attach` in the Rust WS server to a handler that emits the `freshAgent.error{code:"INVALID_SESSION_ID"}` lost-session frame for untracked sessions, so the frozen client's existing recovery machinery un-wedges panes stuck BUSY after a server restart.

**Architecture:** Two thin changes, each proven by TDD. (1) Add `FreshClaudeState::handle_attach` in `crates/freshell-freshagent/src/claude.rs` — checks the in-process session map; untracked → broadcast the same lost-session frame shape codex/opencode emit (with the session type taken from the message, since provider `claude` covers both `freshclaude` and `kilroy`); tracked → no frame (wire-shape parity with codex tracked-and-alive). (2) Add the `AgentProvider::Claude` arm to the `FreshAgentAttach` dispatch in `crates/freshell-ws/src/terminal.rs`, replacing the `_ => {}` swallow for claude.

**Tech Stack:** Rust (tokio, serde_json, axum), cargo workspace crates `freshell-freshagent` + `freshell-ws`, `tokio-tungstenite` WS integration tests.

## Global Constraints

- Work happens in the existing worktree `/home/dan/code/freshell/.worktrees/claude-attach-lost-session` on branch `fix/claude-attach-lost-session` (already created by the workflow's workspace stage; branched from `origin/main`). All paths below are relative to that worktree root.
- The error code string is exactly `INVALID_SESSION_ID` — the frozen TS client folds `event.code === 'INVALID_SESSION_ID'` inside a `type: "freshAgent.event"` envelope into `markSessionLost` (`src/lib/fresh-agent-ws.ts:325-327`). Any other code or envelope shape does NOT engage recovery.
- The envelope's `sessionType` must come from the attach message (`freshclaude` OR `kilroy`) — unlike codex/opencode, it cannot be a hardcoded constant.
- **Scope (this slice ONLY, per campaign plan `/home/dan/code/freshell/docs/plans/2026-07-24-restart-resilience-architecture-analysis.md` §2.8 item 1 / P0.2):** stop the swallow + emit the lost frame. Do NOT build the real attach/resume arm, do NOT record `cliSessionId`, do NOT build the snapshot adapter, do NOT extend the sidecar protocol — those are explicit follow-on slices (§2.8 items 2-4). Recovery in this slice is client-driven (pane goes `.lost` → `triggerRecovery` re-creates with `resumeSessionId`).
- Do not touch the `AgentProvider::Amplifier` fallthrough behavior (it stays swallowed via `_ => {}`). There are no gemini/kimi arms in `AgentProvider` (they are terminal modes, not fresh-agent providers) — nothing to preserve there.
- NEVER restart the user's self-hosted freshell server. NEVER use broad kill patterns (`pkill -f node`, etc.). Integration tests bind ephemeral ports (`127.0.0.1:0`) only — never 3001/3002.
- PR creation is NOT user-approved: after verification is green, commit + push the branch, then STOP before `gh pr create`.
- TDD is mandatory: run each new test RED before implementing, then GREEN. Never skip the RED run.
- README.md is the only end-user markdown doc; this plan under `docs/plans/` is a working/agent doc.
- The campaign plan doc `2026-07-24-restart-resilience-architecture-analysis.md` is UNTRACKED on main — do NOT commit it or copy it into the worktree.
- Note on the campaign plan's "kill-server-while-busy e2e" proof gate: that gate applies **before building the follow-on slices** (§2.8 items 2-4), not inside this PR. This slice's own spec mandates Rust-level proof (unit + WS integration against the real dispatch), which is what this plan delivers. The server-restart case is exactly the "untracked session" case these tests pin (a restarted process tracks nothing).
- **Validated findings & accepted residual risks** (load-bearing validation stage; full ledger in the workflow logs — evidence-verified, do not re-litigate during implementation, do NOT expand scope to address them):
  - Safety of `tracked -> silence` is proven: the client's attach `sessionId` is always the server-issued placeholder id echoed in `freshAgent.created` (never swapped to the durable `cliSessionId`), and claude.rs inserts into the map *before* broadcasting `created` — so no live session can be declared lost on a transient WS reconnect.
  - Real client attach frames (up to 7 fields, nested `sessionRef{provider,sessionId}`) deserialize fine — `Option` fields + no `deny_unknown_fields`; the 4-field test frames are valid.
  - What "recovery" means in this slice: the lost frame un-wedges the pane; `triggerRecovery` re-creates with `resumeSessionId` only if the pane state holds a resumable claude id, otherwise it resets the pane to idle with `restoreError`. Both outcomes un-wedge; full-fidelity resume is the follow-on slices (§2.8.2-3).
  - Accepted (pre-existing, shared with codex/opencode's identical lost frames; out of scope here): (a) the frozen client's `markSessionLost` throws instead of no-oping in a client/tab that lacks the session record (e.g. a freshly reloaded page), so reload-window recovery can no-op — a one-line client-side guard is the recommended follow-on and would fix codex too; (b) no cross-client dedupe — two tabs showing the same pane can race duplicate resume-creates; (c) hidden panes don't send attach, so lost-detection fires when a pane becomes visible.

---

## File Structure

| File | Change | Responsibility |
|---|---|---|
| `crates/freshell-freshagent/src/claude.rs` | Modify | Add `handle_attach` method + `lost_session_frame` helper + unit tests (in the existing `#[cfg(test)] mod tests`) |
| `crates/freshell-ws/src/terminal.rs` | Modify (lines ~534-553) | Add `AgentProvider::Claude` arm to the `FreshAgentAttach` dispatch; update its doc comment |
| `crates/freshell-ws/tests/freshagent_claude_attach.rs` | Create | WS integration test proving the real dispatch routes claude/kilroy attach and the lost frame reaches a real WS client |

Interfaces between tasks:
- Task 1 produces `pub async fn handle_attach(&self, msg: FreshAgentAttach)` on `FreshClaudeState` (exported from `freshell_freshagent` via the existing `pub use claude::FreshClaudeState;`).
- Task 2 consumes exactly that method from `terminal.rs` via `state.fresh_claude.clone()` (field already exists on `WsState`, `crates/freshell-ws/src/lib.rs:99`).

---

### Task 1: `FreshClaudeState::handle_attach` — emit lost-session frame for untracked sessions

**Files:**
- Modify: `crates/freshell-freshagent/src/claude.rs` (imports ~line 57; new code after `handle_send`/near `send_error` ~line 380; helper near `session_type_str` ~line 474; tests in the `#[cfg(test)] mod tests` starting ~line 694)

**Interfaces:**
- Consumes: existing `FreshClaudeState` fields (`sessions: Arc<TokioMutex<HashMap<String, ClaudeSession>>>`, `broadcast_tx`), existing private helpers `fn broadcast(&self, msg: &ServerMessage)` (claude.rs:~148) and `fn session_type_str(session_type: SessionType) -> &'static str` (claude.rs:~474), protocol types `FreshAgentAttach` (crates/freshell-protocol/src/client_messages.rs:490-502), `ServerMessage::FreshAgentEvent` / `FreshAgentEvent`.
- Produces: `pub async fn handle_attach(&self, msg: FreshAgentAttach)` — takes an owned `FreshAgentAttach`, returns `()`, all output on the broadcast bus. Task 2 calls it as `fresh_claude.handle_attach(attach).await`.

- [ ] **Step 1: Add `FreshAgentAttach` to the protocol import**

In `crates/freshell-freshagent/src/claude.rs`, the import block at lines ~57-61 currently reads:

```rust
use freshell_protocol::{
    ErrorCode, ErrorMsg, FreshAgentCreate, FreshAgentCreateFailed, FreshAgentCreated,
    FreshAgentEvent, FreshAgentInterrupt, FreshAgentKill, FreshAgentKilled, FreshAgentSend,
    FreshAgentSendAccepted, ServerMessage, SessionType,
};
```

Change the second line to add `FreshAgentAttach`:

```rust
use freshell_protocol::{
    ErrorCode, ErrorMsg, FreshAgentAttach, FreshAgentCreate, FreshAgentCreateFailed,
    FreshAgentCreated, FreshAgentEvent, FreshAgentInterrupt, FreshAgentKill, FreshAgentKilled,
    FreshAgentSend, FreshAgentSendAccepted, ServerMessage, SessionType,
};
```

- [ ] **Step 2: Write the failing unit tests**

In the `#[cfg(test)] mod tests` module of `claude.rs` (starts ~line 694 with `use super::*;`), add these helpers and three tests. Place them after the existing `fn state()` helper (~line 698). Note: the existing `state()` drops its receiver, so we add a bus-keeping variant (mirrors `state_with_bus()` in `codex.rs:3320`):

```rust
    fn state_with_bus() -> (FreshClaudeState, tokio::sync::broadcast::Receiver<String>) {
        let (tx, rx) = tokio::sync::broadcast::channel::<String>(64);
        (FreshClaudeState::new(Arc::new(tx)), rx)
    }

    fn attach_msg(session_id: &str) -> FreshAgentAttach {
        FreshAgentAttach {
            provider: freshell_protocol::AgentProvider::Claude,
            session_id: session_id.to_string(),
            session_type: SessionType::Freshclaude,
            cwd: None,
            resume_session_id: None,
            session_ref: None,
        }
    }

    /// Insert a fake tracked session directly into the map, bypassing the sidecar spawn
    /// (the claude analog of codex.rs's `spawn_sleeper` + `insert_fake_session`). The
    /// `sleep 30` child stands in for the Node sidecar; `kill_on_drop` reaps it at test end.
    async fn insert_fake_claude_session(st: &FreshClaudeState, session_id: &str) {
        let mut child = tokio::process::Command::new("sleep")
            .arg("30")
            .stdin(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .expect("spawn sleeper");
        let stdin = child.stdin.take().expect("piped stdin");
        let consumer = tokio::spawn(async {});
        st.sessions.lock().await.insert(
            session_id.to_string(),
            ClaudeSession {
                stdin,
                child,
                ownership_id: format!("test-{session_id}"),
                consumer,
            },
        );
    }

    /// P0.2 slice 1 (restart-resilience §2.8): an attach for a session this process does
    /// not track (the always-true case after a server restart) must emit the
    /// `freshAgent.error{code:'INVALID_SESSION_ID'}` lost-session shape -- NOT be
    /// swallowed -- so the client marks the pane `.lost` and `triggerRecovery`
    /// re-creates with `resumeSessionId` (`fresh-agent-ws.ts:325-327`).
    #[tokio::test]
    async fn handle_attach_untracked_session_emits_lost_session_frame() {
        let (st, mut rx) = state_with_bus();

        st.handle_attach(attach_msg("does-not-exist")).await;

        let raw = rx.try_recv().expect("a lost-session frame was broadcast");
        let frame: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(frame["type"], "freshAgent.event");
        assert_eq!(frame["sessionId"], "does-not-exist");
        assert_eq!(frame["provider"], "claude");
        assert_eq!(frame["sessionType"], "freshclaude");
        assert_eq!(frame["event"]["type"], "freshAgent.error");
        assert_eq!(frame["event"]["code"], "INVALID_SESSION_ID");
    }

    /// Kilroy panes send `provider: "claude"` with `sessionType: "kilroy"` -- the envelope
    /// must echo the message's session type or the client builds the wrong locator and the
    /// pane never goes `.lost`.
    #[tokio::test]
    async fn handle_attach_untracked_kilroy_session_keeps_kilroy_session_type() {
        let (st, mut rx) = state_with_bus();
        let mut msg = attach_msg("kilroy-gone");
        msg.session_type = SessionType::Kilroy;

        st.handle_attach(msg).await;

        let frame: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
        assert_eq!(frame["sessionType"], "kilroy");
        assert_eq!(frame["provider"], "claude");
        assert_eq!(frame["event"]["code"], "INVALID_SESSION_ID");
    }

    /// Wire-shape parity with codex's tracked-and-alive row (codex.rs decision table /
    /// `handle_attach_known_alive_session_emits_no_frame_regardless_of_turn_state`):
    /// attaching to a session this process DOES track must broadcast nothing -- above all
    /// it must never declare a live session lost (which would make the client kill and
    /// re-create a healthy pane).
    #[tokio::test]
    async fn handle_attach_tracked_session_broadcasts_nothing() {
        let (st, mut rx) = state_with_bus();
        insert_fake_claude_session(&st, "still-alive").await;

        st.handle_attach(attach_msg("still-alive")).await;

        assert!(
            rx.try_recv().is_err(),
            "tracked attach must not broadcast any frame (wire-shape parity)"
        );
    }
```

- [ ] **Step 3: Run the tests to verify they fail (RED)**

```bash
cd /home/dan/code/freshell/.worktrees/claude-attach-lost-session
cargo test -p freshell-freshagent claude::tests::handle_attach
```

Expected: **compilation failure** — `error[E0599]: no method named 'handle_attach' found for struct 'FreshClaudeState'` (or for reference `&FreshClaudeState`). This is the structural RED: the handler does not exist yet. (First build in this worktree is cold — allow several minutes.)

- [ ] **Step 4: Implement `handle_attach` + `lost_session_frame`**

In `claude.rs`, add the method inside `impl FreshClaudeState`, after `handle_send` ends (~line 379, just before `fn send_error` at ~line 380):

```rust
    // ── freshAgent.attach (restart-resilience P0.2, slice 1: stop the swallow) ──────

    /// Handle a `freshAgent.attach` for claude/kilroy. Decision table (this slice):
    ///
    /// | State | Action |
    /// |---|---|
    /// | tracked | no-op -- NO frame (wire-shape parity with codex tracked-and-alive) |
    /// | NOT tracked | `lost_session_frame` (`INVALID_SESSION_ID`) -> the client marks the pane `.lost` and `triggerRecovery` re-creates with `resumeSessionId` |
    ///
    /// Unlike codex (`ensure_session_resumable`) and opencode (`resume_durable_session`)
    /// there is deliberately NO in-place resume here yet: claude has no server-side resume
    /// path in the Rust port, and the restart-resilience plan (§2.8) slices that as
    /// follow-on work (record cliSessionId, real attach arm, snapshot adapter). In this
    /// slice recovery is CLIENT-driven -- the lost frame un-wedges a pane stuck BUSY
    /// after a server restart instead of the prior silent swallow.
    pub async fn handle_attach(&self, msg: FreshAgentAttach) {
        let tracked = self.sessions.lock().await.contains_key(&msg.session_id);
        if tracked {
            return;
        }
        self.broadcast(&lost_session_frame(&msg.session_id, msg.session_type));
    }
```

And add the frame helper as a free function next to `session_type_str` (~line 474, after `sdk_line_to_frame`):

```rust
/// The `freshAgent.error{code:'INVALID_SESSION_ID'}` shape (`sdk-events.ts:37`) the client
/// folds into `markSessionLost` (`fresh-agent-ws.ts:326-328`) instead of hanging on a stale
/// `freshAgent.attach` for a session this server has never heard of. Third copy after
/// `codex.rs`/`opencode_ws.rs` (both document the duplication) -- but unlike those two this
/// one cannot hardcode the session type: provider `claude` covers BOTH `freshclaude` and
/// `kilroy`, so the envelope's sessionType comes from the attach message.
fn lost_session_frame(session_id: &str, session_type: SessionType) -> ServerMessage {
    ServerMessage::FreshAgentEvent(FreshAgentEvent {
        event: json!({
            "type": "freshAgent.error",
            "sessionId": session_id,
            "code": "INVALID_SESSION_ID",
            "message": format!("claude session {session_id} not found"),
        }),
        provider: PROVIDER.to_string(),
        session_id: session_id.to_string(),
        session_type: session_type_str(session_type).to_string(),
    })
}
```

- [ ] **Step 5: Run the tests to verify they pass (GREEN)**

```bash
cargo test -p freshell-freshagent claude::tests::handle_attach
```

Expected: `test result: ok. 3 passed` (the three `handle_attach_*` tests).

- [ ] **Step 6: Run the whole claude test module + crate to check for regressions**

```bash
cargo test -p freshell-freshagent claude::tests
cargo test -p freshell-freshagent
```

Expected: all tests pass (`node` must be on PATH — existing claude/codex tests spawn fake Node sidecars).

- [ ] **Step 7: Format and commit**

```bash
cargo fmt -p freshell-freshagent
git add crates/freshell-freshagent/src/claude.rs
git commit -m "feat(freshagent): claude/kilroy attach emits INVALID_SESSION_ID lost-session frame for untracked sessions"
```

---

### Task 2: Route claude attach in `terminal.rs` + WS integration proof

**Files:**
- Modify: `crates/freshell-ws/src/terminal.rs:534-553` (the `ClientMessage::FreshAgentAttach` arm)
- Create: `crates/freshell-ws/tests/freshagent_claude_attach.rs`

**Interfaces:**
- Consumes: `FreshClaudeState::handle_attach(&self, msg: FreshAgentAttach)` from Task 1 (via `state.fresh_claude`, field on `WsState` at `crates/freshell-ws/src/lib.rs:99`); the WS integration harness conventions from `crates/freshell-ws/tests/freshagent_claude_kill_interrupt.rs`.
- Produces: the closed dispatch gap — a raw `{"type":"freshAgent.attach","provider":"claude",...}` frame over a real WS connection yields the lost-session frame. Nothing later depends on new names from this task.

- [ ] **Step 1: Write the failing integration test**

Create `crates/freshell-ws/tests/freshagent_claude_attach.rs`. The WS integration harness is duplicated per test file by repo convention (verified: there is no shared `tests/common/` directory; each `tests/*.rs` file carries its own harness copy, e.g. `freshagent_claude_kill_interrupt.rs`). Copy these blocks **verbatim** from `crates/freshell-ws/tests/freshagent_claude_kill_interrupt.rs` into the new file (they are self-contained and do not reference the fake sidecar):

- `test_settings_value` — lines 145-163
- `spawn_server` — lines 165-220 (constructs the full 26-field `WsState` literal; copy exactly, do not reorder fields)
- the `TestWs` type alias — lines 222-223
- `connect_and_complete_handshake` — lines 225-256
- `send_json` — lines 258-262
- `await_frame` — lines 265-288

Do NOT copy `FakeClaudeSidecarEnv`, `CLAUDE_ENV_LOCK`, `uuid_like_suffix`, or `create_frame` — an untracked attach never spawns a sidecar and mutates no env vars, so this file needs none of them.

Top of the new file (imports pruned to what the copied harness + tests use — drop `std::io::Write` and `Mutex`, which only the sidecar fixture needed):

```rust
//! WS-level proof for restart-resilience P0.2 slice 1: the real dispatch
//! (`terminal.rs`'s `ClientMessage::FreshAgentAttach` arm) must route a claude/kilroy
//! `freshAgent.attach` to `FreshClaudeState::handle_attach` instead of swallowing it
//! via `_ => {}`. Unit-level coverage exists in `claude.rs::tests`, but -- exactly like
//! the kill/interrupt dispatch gap before it (`freshagent_claude_kill_interrupt.rs`) --
//! it is unreachable from the wire until the dispatch arm exists. Harness duplicated
//! from that file per the repo's per-test-file convention.

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message as WsMessage;

use freshell_ws::WsState;

const AUTH_TOKEN: &str = "s3cr3t-token-abcdef";
```

(If the compiler warns that `Arc` — or anything else — is unused after pasting the harness, drop exactly the warned imports; if the harness fails to compile for a missing import, add exactly what `rustc` names.)

Then add the two tests:

```rust
/// A claude `freshAgent.attach` for a session id this server process does not track
/// (the always-true case right after a server restart) must produce the
/// `freshAgent.error{code:'INVALID_SESSION_ID'}` lost-session frame on the wire --
/// the frame the frozen client folds into `markSessionLost` -> `triggerRecovery`.
/// Before the fix the dispatch swallowed the message and NO frame ever arrived
/// (this test then fails with `await_frame` panicking on its timeout budget).
#[tokio::test]
async fn claude_attach_for_untracked_session_emits_lost_session_frame_over_ws() {
    let url = spawn_server().await;
    let mut ws = connect_and_complete_handshake(&url).await;

    send_json(
        &mut ws,
        &serde_json::json!({
            "type": "freshAgent.attach",
            "provider": "claude",
            "sessionId": "restarted-away",
            "sessionType": "freshclaude",
        }),
    )
    .await;

    let frame = await_frame(&mut ws, Duration::from_secs(10), |v| {
        v["type"] == "freshAgent.event" && v["sessionId"] == "restarted-away"
    })
    .await;

    assert_eq!(frame["provider"], "claude");
    assert_eq!(frame["sessionType"], "freshclaude");
    assert_eq!(frame["event"]["type"], "freshAgent.error");
    assert_eq!(frame["event"]["code"], "INVALID_SESSION_ID");
}

/// Kilroy panes ride the same claude provider arm with `sessionType: "kilroy"`; the
/// envelope must echo it (through the real serde parse of `ClientMessage`, which the
/// unit tests bypass) or the client builds the wrong session locator.
#[tokio::test]
async fn kilroy_attach_for_untracked_session_emits_lost_session_frame_over_ws() {
    let url = spawn_server().await;
    let mut ws = connect_and_complete_handshake(&url).await;

    send_json(
        &mut ws,
        &serde_json::json!({
            "type": "freshAgent.attach",
            "provider": "claude",
            "sessionId": "kilroy-was-here",
            "sessionType": "kilroy",
        }),
    )
    .await;

    let frame = await_frame(&mut ws, Duration::from_secs(10), |v| {
        v["type"] == "freshAgent.event" && v["sessionId"] == "kilroy-was-here"
    })
    .await;

    assert_eq!(frame["provider"], "claude");
    assert_eq!(frame["sessionType"], "kilroy");
    assert_eq!(frame["event"]["code"], "INVALID_SESSION_ID");
}
```

- [ ] **Step 2: Run the integration tests to verify they fail (RED)**

```bash
cd /home/dan/code/freshell/.worktrees/claude-attach-lost-session
cargo test -p freshell-ws --test freshagent_claude_attach
```

Expected: **both tests FAIL** — the dispatch still swallows claude attach, so no `freshAgent.event` frame ever arrives and `await_frame` panics when its 10s budget expires (a timeout/panic message from the copied `await_frame` helper). If instead they fail to compile, fix imports per the compiler and re-run until the failure is the timeout.

- [ ] **Step 3: Add the `AgentProvider::Claude` arm in `terminal.rs`**

In `crates/freshell-ws/src/terminal.rs`, the attach arm currently reads (lines ~534-553):

```rust
        // `freshAgent.attach` (PR-4, reload-rehydrate): route codex/opencode to their
        // handlers (re-emit a status snapshot, transparently recover a crashed codex
        // sidecar, or emit the INVALID_SESSION_ID lost-session shape for an unknown
        // session). Claude keeps the prior swallow behavior (out of scope here, matching
        // the existing interrupt/kill dispatch's conservative default). Detached task,
        // same pattern as the other `freshAgent.*` arms.
        ClientMessage::FreshAgentAttach(attach) => {
            match attach.provider {
                freshell_protocol::AgentProvider::Codex => {
                    let fresh_codex = state.fresh_codex.clone();
                    tokio::spawn(async move { fresh_codex.handle_attach(attach).await });
                }
                freshell_protocol::AgentProvider::Opencode => {
                    let fresh_opencode = state.fresh_opencode.clone();
                    tokio::spawn(async move { fresh_opencode.handle_attach(attach).await });
                }
                _ => {}
            }
            true
        }
```

Replace it with (comment updated — claude no longer swallows; `_ => {}` remains only for `Amplifier`, which has no fresh-agent runtime):

```rust
        // `freshAgent.attach` (PR-4, reload-rehydrate): route codex/opencode to their
        // handlers (re-emit a status snapshot, transparently recover a crashed codex
        // sidecar, or emit the INVALID_SESSION_ID lost-session shape for an unknown
        // session). Claude/kilroy (restart-resilience P0.2 slice 1) route to
        // `FreshClaudeState::handle_attach`, which emits the same lost-session shape for
        // untracked sessions so the client's `.lost` -> `triggerRecovery` machinery
        // engages instead of a pane wedging BUSY after a server restart. Detached task,
        // same pattern as the other `freshAgent.*` arms. `_` keeps swallowing only
        // `Amplifier` (no fresh-agent runtime, same as the `FreshAgentSend` arm).
        ClientMessage::FreshAgentAttach(attach) => {
            match attach.provider {
                freshell_protocol::AgentProvider::Codex => {
                    let fresh_codex = state.fresh_codex.clone();
                    tokio::spawn(async move { fresh_codex.handle_attach(attach).await });
                }
                freshell_protocol::AgentProvider::Claude => {
                    let fresh_claude = state.fresh_claude.clone();
                    tokio::spawn(async move { fresh_claude.handle_attach(attach).await });
                }
                freshell_protocol::AgentProvider::Opencode => {
                    let fresh_opencode = state.fresh_opencode.clone();
                    tokio::spawn(async move { fresh_opencode.handle_attach(attach).await });
                }
                _ => {}
            }
            true
        }
```

- [ ] **Step 4: Run the integration tests to verify they pass (GREEN)**

```bash
cargo test -p freshell-ws --test freshagent_claude_attach
```

Expected: `test result: ok. 2 passed`.

- [ ] **Step 5: Run the full freshell-ws test suite for regressions**

```bash
cargo test -p freshell-ws
```

Expected: all tests pass (includes all `tests/*.rs` integration files; ephemeral ports only).

- [ ] **Step 6: Format and commit**

```bash
cargo fmt -p freshell-ws
git add crates/freshell-ws/src/terminal.rs crates/freshell-ws/tests/freshagent_claude_attach.rs
git commit -m "fix(ws): route claude/kilroy freshAgent.attach instead of swallowing it"
```

---

### Task 3: Full verification, coordinated suite, push — then STOP (no PR)

**Files:**
- No source changes. Verification + push only.

**Interfaces:**
- Consumes: the committed work from Tasks 1-2 on branch `fix/claude-attach-lost-session`.
- Produces: a pushed branch and a report; explicitly NOT a PR.

- [ ] **Step 1: Run the cargo suites for both touched crates**

```bash
cd /home/dan/code/freshell/.worktrees/claude-attach-lost-session
cargo test -p freshell-freshagent && cargo test -p freshell-ws
```

Expected: both end with `test result: ok.` (zero failures).

- [ ] **Step 2: Run the coordinated repo test suite (TS)**

The repo's shared test coordinator gates broad runs; other agents work concurrently in sibling worktrees.

```bash
npm run test:status
```

If it reports another run in progress (another agent holds the gate), WAIT and re-run `npm run test:status` every ~60s until clear — do not force or kill anything. Then:

```bash
FRESHELL_TEST_SUMMARY=1 npm test
```

Expected: the summary reports the suite green (this change touches no TS, so any failure here is pre-existing — if the same failure reproduces on unmodified `origin/main`, note it in the report rather than "fixing" unrelated code in this PR).

- [ ] **Step 3: Confirm a clean, focused branch**

```bash
git status --short
git log --oneline origin/main..HEAD
```

Expected: no uncommitted changes; commits are the plan commit plus the two feature commits from Tasks 1-2 (only `claude.rs`, `terminal.rs`, the new test file, and this plan doc).

- [ ] **Step 4: Push the branch and STOP**

```bash
git push -u origin fix/claude-attach-lost-session
```

Then **STOP — do not run `gh pr create`** (PR creation is not user-approved). Report:
- Branch: `fix/claude-attach-lost-session`
- What was proven (failing-then-passing tests):
  - `claude::tests::handle_attach_untracked_session_emits_lost_session_frame`
  - `claude::tests::handle_attach_untracked_kilroy_session_keeps_kilroy_session_type`
  - `claude::tests::handle_attach_tracked_session_broadcasts_nothing`
  - `freshagent_claude_attach::claude_attach_for_untracked_session_emits_lost_session_frame_over_ws`
  - `freshagent_claude_attach::kilroy_attach_for_untracked_session_emits_lost_session_frame_over_ws`
- The lost frame is the exact shape the frozen client folds into `markSessionLost` (`fresh-agent-ws.ts:325-327`), which un-wedges a BUSY-at-restart pane via `triggerRecovery` re-creating with `resumeSessionId`.
