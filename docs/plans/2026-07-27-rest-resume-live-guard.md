# REST Resume Live-Session Guard (D7) Implementation Plan

> **For agentic workers:** This plan is executed task-by-task by the
> workflow's execute stage: a fresh implementer per task, with a spec +
> quality review after each task. Steps use checkbox (`- [ ]`) syntax
> for tracking.

**Goal:** Enforce the D7 live-session guard (already present on the WS `terminal.create` path) on the REST spawn pipeline (`POST /api/tabs`, `POST /api/panes/:id/split`, `POST /api/panes/:id/respawn`), refusing with HTTP 409 `RESTORE_UNAVAILABLE` any resume onto a `(mode, sessionId)` that a currently-Running terminal already owns — via a shared helper, not a copy — and serializing the sessionRef-rung spawn with the same D8 per-sessionRef registry lease the WS path holds. Fixes kata **ks38**.

**Architecture:** Extract the D7 liveness join (identity-registry arm + registry-row arm) into a shared predicate `TerminalRegistry::live_session_owner(...)` in `freshell-terminal` (the lowest common ancestor crate — both `freshell-ws` and `freshell-freshagent` depend on it). The identity arm crosses the crate boundary through a new object-safe trait `SessionIdentityLookup` (implemented by `freshell-ws`'s `TerminalIdentityRegistry`, injected into `FreshAgentState` by `freshell-server` — the same seam pattern as the existing `PaneIdentitySink`). The WS guard is refactored to call the shared predicate; the REST spawn pipeline gains the guard at its single choke point in `spawn_terminal_pane`, covering all three REST callers at once (validation note: the formerly-cited fourth caller, the in-process tabs-sync restore driver, was deleted by commit `2ed6b948` — `POST /api/tabs-sync/restore` no longer exists, and `create_terminal_or_content_tab_deferred` is dead code; do not reference either). On the same sessionRef rung, the choke point additionally claims the registry's D8 per-sessionRef lease (`TerminalRegistry::claim_session_ref`) before spawning and completes it into a binding on success, so REST resumes are serialized against concurrent WS/REST claims rather than merely checked (see Design Decision 6 and Task 5).

**Tech Stack:** Rust (cargo workspace at repo root, `crates/*`), axum, tokio; tests are inline `#[cfg(test)]` modules using `tower::util::ServiceExt::oneshot` and the registry's headless-terminal seam.

## Global Constraints

- Work happens in the worktree `/home/dan/code/freshell/.worktrees/rest-resume-live-guard`, branch `fix/rest-resume-live-guard` (from `origin/main`); PR targets `main`. PR creation and self-merge are **explicitly pre-approved by the user for this change**.
- Red-Green-Refactor TDD: write the failing test first; never skip tests, never skip the refactor.
- This fix is entirely in the Rust crates. `cargo test` is a local-only gate (CI has NO cargo test job); CI enforces `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings` (rustc 1.96.0 pinned) — clippy lints test code too, so no unused imports in `#[cfg(test)]` modules.
- No TypeScript/Node files change in this plan, so the coordinated Vitest suite is unaffected; do not hold the coordinator gate for Rust runs.
- Never restart the self-hosted Freshell server; never use broad kill patterns (`pkill -f node` etc.). Registry tests must `registry.kill(<id>)` any terminal they create.
- Git identity: `dan@danshapiro.com` for `gh`. Every commit message ends with the Amplifier co-author footer:

  ```
  🤖 Generated with [Amplifier](https://github.com/microsoft/amplifier)

  Co-Authored-By: Amplifier <240397093+microsoft-amplifier@users.noreply.github.com>
  ```
- Error message text must mirror the WS guard verbatim: `Session {sid} is still running on the server.` Wire code string: `RESTORE_UNAVAILABLE`. REST status: `409 Conflict`.
- After the PR merges, close the kata from the repo root: `kata close ks38 --done --message "<substantive summary>" --commit <merged sha>`. Do NOT close it if anything is unverified.

## Design Decisions (locked)

1. **Shared helper location:** `crates/freshell-terminal/src/registry.rs`. Dependency direction is `freshell-ws → freshell-freshagent → … → freshell-terminal`; `freshell-terminal` is the only crate both guard consumers can reach, and it already owns the row-scan half (`live_terminal_for_session_ref`, registry.rs:2101).
2. **Full-width join, not row-scan-only:** the WS guard checks BOTH the identity registry and the registry rows. A row-scan-only REST guard would miss locator-adopted terminals (Running row with `resume_session_id: None`, session held only in the identity registry) — exactly the asymmetry commit `d9b71f50` fixed in `live_session_keys`. The identity arm is injected via trait; when unwired (`None`), the guard degrades to the row arm only.
3. **No respawn self-exemption:** `respawn_pane` deliberately never kills the pane's old terminal ("detach, don't kill", pane_ops.rs:688-693). If the old terminal is still Running and owns session S, respawning with `sessionRef` S would create a second live JSONL writer for S — the exact corruption the doctrine forbids. The guard therefore refuses uniformly (matching WS semantics, which have no self-exemption); a respawn-resume is only allowed once the owner has exited. A test pins this.
4. **Guard scope mirrors WS exactly:** fires only on the `sessionRef` rung — i.e. when `derive_resume_identity` produced an `accepted_session_ref` (provider already validated == mode) whose non-empty `session_id` equals the derived `resume_session_id`. The legacy bare-`resumeSessionId` rung keeps its existing behavior, same as WS.
5. **Known parity note (not a gap):** like the WS guard, this covers terminal-PTY duplication only. Fresh-agent in-process sessions (freshclaude/freshcodex/freshopencode) are tracked elsewhere and are out of scope for D7 on both paths — this is parity with the WS guard, which the kata asks us to mirror.
6. **D8 lease at the REST choke point (added by load-bearing validation):** the WS create path does not merely check-then-spawn — on its production-normal rung it holds the registry's per-sessionRef lease before spawning (`claim_session_ref`, registry.rs:1761-1767; RAII release via `fail_session_ref_claim`, ws/terminal.rs:987-1027), and the shipped client sends `capabilities.paneReconcileV1: true` unconditionally (src/lib/ws-client.ts:360), so the leased rung IS production WS behavior. A REST guard without the lease would re-open defect D8 (duplicate-JSONL-writer race) for concurrent REST×REST and REST×WS resumes. Task 5 therefore claims the lease on the same sessionRef rung before spawning and completes it into a binding on success (`complete_session_ref_claim`). Conservative v1 REST semantics: `Held`, `BoundElsewhere`, and `ExpiredNeedsKill` all answer the same 409 `RESTORE_UNAVAILABLE` envelope (no kill-and-adopt logic on REST; the 20s lease TTL is the crash backstop; a crashed holder can therefore stall a sid for ≤20s — accepted, upgradeable later to the WS kill-and-confirm port at ws/terminal.rs:1037-1053).

## File Structure

| File | Change |
|---|---|
| `crates/freshell-terminal/src/registry.rs` | Add `SessionIdentityLookup` trait + `TerminalRegistry::live_session_owner()` + unit tests |
| `crates/freshell-ws/src/identity.rs` | `impl SessionIdentityLookup for TerminalIdentityRegistry` + unit test |
| `crates/freshell-ws/src/terminal.rs` | Refactor D7 guard block (lines ~1357-1410) to call the shared predicate |
| `crates/freshell-freshagent/src/lib.rs` | Add `fail_json_code` helper; add `session_identity` field + `with_session_identity` builder to `FreshAgentState` |
| `crates/freshell-freshagent/src/terminal_tabs.rs` | Insert the guard + D8 sessionRef lease in `spawn_terminal_pane` (after line 637); add router tests |
| `crates/freshell-server/src/main.rs` | Wire the identity registry into `FreshAgentState` via `with_session_identity` |
| `crates/freshell-freshagent/Cargo.toml` | Only if `tracing` is not already a dependency: add `tracing.workspace = true` |

---

### Task 1: Shared D7 predicate in `freshell-terminal`

**Files:**
- Modify: `crates/freshell-terminal/src/registry.rs` (trait + method + tests in the existing `#[cfg(test)]` module, ~line 3840+)

**Interfaces:**
- Consumes: existing `TerminalRegistry::{probe, directory, register_headless, finish_pty_exit}`, `freshell_protocol::TerminalRunStatus`, `DirectoryEntry { terminal_id, mode, resume_session_id, status, .. }` (registry.rs:300-316).
- Produces (later tasks rely on these exact names):
  - `pub trait SessionIdentityLookup: Send + Sync + std::fmt::Debug { fn terminal_for_session(&self, provider: &str, session_id: &str) -> Option<String>; }` in module `freshell_terminal::registry` (path from other crates: `freshell_terminal::registry::SessionIdentityLookup`).
  - `pub fn live_session_owner(&self, identity: Option<&dyn SessionIdentityLookup>, mode: &str, session_id: &str) -> Option<String>` on `TerminalRegistry` — returns the terminal_id of a currently-Running owner of `(mode, session_id)`, else `None`.

- [ ] **Step 1: Write the failing tests**

Add to the existing `#[cfg(test)]` test module in `crates/freshell-terminal/src/registry.rs` (the one that already uses `register_headless`, around line 3840). Follow the module's existing import style; the tests need `HeadlessTerminal`, `TerminalRegistry`, and the new trait.

```rust
#[derive(Debug)]
struct StubIdentity {
    provider: &'static str,
    session_id: &'static str,
    terminal_id: &'static str,
}

impl SessionIdentityLookup for StubIdentity {
    fn terminal_for_session(&self, provider: &str, session_id: &str) -> Option<String> {
        (provider == self.provider && session_id == self.session_id)
            .then(|| self.terminal_id.to_string())
    }
}

#[test]
fn live_session_owner_finds_running_row_by_resume_session_id() {
    let registry = TerminalRegistry::new();
    registry.register_headless(HeadlessTerminal {
        terminal_id: "t-row-owner".into(),
        stream_id: "s-row-owner".into(),
        mode: "claude".into(),
        resume_session_id: Some("sess-live".into()),
        create_request_id: None,
        created_at: None,
    });

    assert_eq!(
        registry.live_session_owner(None, "claude", "sess-live"),
        Some("t-row-owner".to_string()),
        "row arm: Running row with matching mode+resume_session_id is a live owner"
    );
    // Wrong mode / unknown session: no owner.
    assert_eq!(registry.live_session_owner(None, "codex", "sess-live"), None);
    assert_eq!(registry.live_session_owner(None, "claude", "sess-other"), None);

    registry.kill("t-row-owner");
}

#[test]
fn live_session_owner_ignores_exited_rows() {
    let registry = TerminalRegistry::new();
    registry.register_headless(HeadlessTerminal {
        terminal_id: "t-exited".into(),
        stream_id: "s-exited".into(),
        mode: "claude".into(),
        resume_session_id: Some("sess-done".into()),
        create_request_id: None,
        created_at: None,
    });
    assert!(registry.finish_pty_exit("t-exited", 0));

    assert_eq!(
        registry.live_session_owner(None, "claude", "sess-done"),
        None,
        "an Exited owner must not block resume"
    );
}

#[test]
fn live_session_owner_finds_identity_bound_running_terminal() {
    // Locator-adopted shape (d9b71f50's case): Running row with NO
    // resume_session_id; the session binding exists only in the identity store.
    let registry = TerminalRegistry::new();
    registry.register_headless(HeadlessTerminal {
        terminal_id: "t-adopted".into(),
        stream_id: "s-adopted".into(),
        mode: "codex".into(),
        resume_session_id: None,
        create_request_id: None,
        created_at: None,
    });
    let identity = StubIdentity {
        provider: "codex",
        session_id: "sess-adopted",
        terminal_id: "t-adopted",
    };

    assert_eq!(
        registry.live_session_owner(Some(&identity), "codex", "sess-adopted"),
        Some("t-adopted".to_string()),
        "identity arm: identity-bound session of a Running terminal is live"
    );

    registry.kill("t-adopted");
}

#[test]
fn live_session_owner_identity_binding_to_dead_terminal_is_not_live() {
    let registry = TerminalRegistry::new();
    // No registry row at all for "t-gone" -- identity binding alone must not count.
    let identity = StubIdentity {
        provider: "codex",
        session_id: "sess-gone",
        terminal_id: "t-gone",
    };
    assert_eq!(
        registry.live_session_owner(Some(&identity), "codex", "sess-gone"),
        None,
        "identity arm requires the owner terminal to probe Running"
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:
```bash
cd /home/dan/code/freshell/.worktrees/rest-resume-live-guard
cargo test -p freshell-terminal --lib live_session_owner
```
Expected: **compile FAIL** — `SessionIdentityLookup` and `live_session_owner` do not exist yet (that is the red state for a new API).

- [ ] **Step 3: Write the implementation**

In `crates/freshell-terminal/src/registry.rs`, add the trait near the other cross-boundary seams (e.g. just above the `impl TerminalRegistry` block that contains `live_terminal_for_session_ref`, ~line 2100), and the method inside that same `impl TerminalRegistry` block:

```rust
/// Read-only lookup into a session-identity store (in production: the WS-side
/// `TerminalIdentityRegistry` in `freshell-ws`). Injected across the crate
/// boundary so the D7 live-session guard can join BOTH stores from crates that
/// cannot depend on `freshell-ws` (`freshell-freshagent` -- would be circular).
/// Implementations must NOT return retired/dead bindings.
pub trait SessionIdentityLookup: Send + Sync + std::fmt::Debug {
    /// The terminal_id currently bound to `(provider, session_id)`, if any.
    fn terminal_for_session(&self, provider: &str, session_id: &str) -> Option<String>;
}
```

```rust
    /// D7 live-session guard predicate, shared by the WS `terminal.create`
    /// path (`freshell-ws/src/terminal.rs`) and the REST spawn pipeline
    /// (`freshell-freshagent/src/terminal_tabs.rs`): returns the terminal_id
    /// of a currently-RUNNING terminal that already owns `(mode, session_id)`,
    /// if any. Two arms, exactly the WS guard's join (see commit d9b71f50):
    /// 1. identity arm: the injected identity store's owner, probed Running;
    /// 2. row arm: any directory row with this mode + resume_session_id, Running.
    /// `identity: None` (e.g. the seam is unwired) narrows to the row arm.
    pub fn live_session_owner(
        &self,
        identity: Option<&dyn SessionIdentityLookup>,
        mode: &str,
        session_id: &str,
    ) -> Option<String> {
        if let Some(owner_tid) = identity
            .and_then(|ident| ident.terminal_for_session(mode, session_id))
            .filter(|tid| {
                self.probe(tid)
                    .is_some_and(|r| r.status == freshell_protocol::TerminalRunStatus::Running)
            })
        {
            return Some(owner_tid);
        }
        self.directory().into_iter().find_map(|entry| {
            (entry.mode == mode
                && entry.resume_session_id.as_deref() == Some(session_id)
                && entry.status == freshell_protocol::TerminalRunStatus::Running)
                .then_some(entry.terminal_id)
        })
    }
```

If the file already has `TerminalRunStatus` imported (it does — `DirectoryEntry.status` uses it), use the imported name instead of the full path to match local style.

- [ ] **Step 4: Run tests to verify they pass**

Run:
```bash
cargo test -p freshell-terminal --lib live_session_owner
```
Expected: `test result: ok. 4 passed`.

Then the whole crate to catch regressions:
```bash
cargo test -p freshell-terminal --lib
```
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add crates/freshell-terminal/src/registry.rs
git commit -m "feat(terminal): add shared D7 live-session-owner predicate to TerminalRegistry

Extracts the D7 liveness join (identity arm behind a new
SessionIdentityLookup trait + registry-row arm) into
TerminalRegistry::live_session_owner so the WS create guard and the
REST spawn pipeline can share one predicate instead of copies.
Part of ks38.

🤖 Generated with [Amplifier](https://github.com/microsoft/amplifier)

Co-Authored-By: Amplifier <240397093+microsoft-amplifier@users.noreply.github.com>"
```

---

### Task 2: WS guard delegates to the shared predicate

**Files:**
- Modify: `crates/freshell-ws/src/identity.rs` (trait impl + unit test)
- Modify: `crates/freshell-ws/src/terminal.rs:1357-1410` (guard body refactor)

**Interfaces:**
- Consumes (from Task 1): `freshell_terminal::registry::SessionIdentityLookup`, `TerminalRegistry::live_session_owner(identity, mode, session_id) -> Option<String>`.
- Produces: `impl freshell_terminal::registry::SessionIdentityLookup for TerminalIdentityRegistry` (Task 3's production wiring in `freshell-server` relies on this impl existing).

- [ ] **Step 1: Write the failing test (trait impl through the real identity registry)**

In `crates/freshell-ws/src/identity.rs`, add to its `#[cfg(test)]` module (create `mod tests` at the bottom of the file if none exists):

```rust
#[test]
fn identity_registry_feeds_live_session_owner_join() {
    let registry = freshell_terminal::TerminalRegistry::new();
    registry.register_headless(freshell_terminal::registry::HeadlessTerminal {
        terminal_id: "t-live".into(),
        stream_id: "s-live".into(),
        mode: "codex".into(),
        resume_session_id: None, // fresh pane: row carries no resume id
        create_request_id: None,
        created_at: None,
    });
    let identity = TerminalIdentityRegistry::new();
    identity.upsert("t-live", Some("codex"), Some("sess-live-1"), None, 0);

    assert_eq!(
        registry.live_session_owner(Some(&identity), "codex", "sess-live-1"),
        Some("t-live".to_string()),
        "identity-registry-bound session of a Running terminal must be live"
    );

    // Retired bindings must not count (mirrors d9b71f50's negative pin).
    assert!(identity.retire("t-live"));
    assert_eq!(
        registry.live_session_owner(Some(&identity), "codex", "sess-live-1"),
        None,
        "a retired identity binding must not block resume"
    );

    registry.kill("t-live");
}
```

Note: the retired assertion relies on `find_by_session` excluding retired entries — the same reliance the existing WS guard already has. If this assertion fails, STOP and read `identity.rs`'s `find_by_session`/`retire` before proceeding: a failure would mean the pre-existing WS guard has a retire bug, which is out of scope to fix silently — surface it.

- [ ] **Step 2: Run to verify it fails**

```bash
cargo test -p freshell-ws --lib identity_registry_feeds_live_session_owner_join
```
Expected: **compile FAIL** — `TerminalIdentityRegistry` does not implement `SessionIdentityLookup` yet. (The `live_session_owner` call requires `Some(&identity)` to coerce to `Option<&dyn SessionIdentityLookup>`.)

- [ ] **Step 3: Implement the trait**

In `crates/freshell-ws/src/identity.rs`, below the `TerminalIdentityRegistry` impl block:

```rust
/// D7 guard seam: expose this registry to `TerminalRegistry::live_session_owner`
/// (and, via `freshell-server` wiring, to the REST spawn guard in
/// `freshell-freshagent`, which cannot depend on this crate directly).
/// Exactly reproduces the WS guard's identity arm: `find_by_session` -> owner
/// terminal_id (liveness is probed by the shared predicate, not here).
impl freshell_terminal::registry::SessionIdentityLookup for TerminalIdentityRegistry {
    fn terminal_for_session(&self, provider: &str, session_id: &str) -> Option<String> {
        self.find_by_session(provider, session_id)
            .map(|owner| owner.terminal_id)
    }
}
```

`TerminalIdentityRegistry` currently derives only `Clone, Default` (identity.rs:51) — add `Debug` to that derive list (the trait requires it; `TerminalIdentity` already derives `Debug`, so the derive is mechanical). Verified against source during plan validation.

- [ ] **Step 4: Run to verify it passes**

```bash
cargo test -p freshell-ws --lib identity_registry_feeds_live_session_owner_join
```
Expected: PASS.

- [ ] **Step 5: Refactor the WS guard to call the shared predicate (behavior-preserving)**

In `crates/freshell-ws/src/terminal.rs`, inside the D7 guard block (~lines 1357-1410 in `handle_create`), the current body is:

```rust
    let identity_owner_live =
        state
            .identity
            .find_by_session(&mode, live_sid)
            .is_some_and(|owner| {
                state
                    .registry
                    .probe(&owner.terminal_id)
                    .is_some_and(|r| r.status == freshell_protocol::TerminalRunStatus::Running)
            });
    let registry_row_live = identity_owner_live
        || state.registry.directory().into_iter().any(|entry| {
            entry.mode == mode
                && entry.resume_session_id.as_deref() == Some(live_sid)
                && entry.status == freshell_protocol::TerminalRunStatus::Running
        });
    if registry_row_live {
```

Replace those two `let` bindings and the `if` condition with:

```rust
    if state
        .registry
        .live_session_owner(Some(&state.identity), &mode, live_sid)
        .is_some()
    {
```

Keep everything else in the block byte-identical: the outer `if let Some(live_sid) = create.session_ref...` filter, the `tracing::warn!` line, and the `send_create_error(..., ErrorCode::RestoreUnavailable, format!("Session {live_sid} is still running on the server."), ...)` return. If `state.identity` is stored behind an `Arc`, use `Some(&*state.identity)` for the unsized coercion; otherwise `Some(&state.identity)` as shown.

- [ ] **Step 6: Run the WS guard's existing integration tests (the refactor safety net)**

```bash
cargo test -p freshell-ws --test live_session_ref_guard
cargo test -p freshell-ws --test claude_restore_unavailable
cargo test -p freshell-ws --test pane_ledger_restore
cargo test -p freshell-ws
```
Expected: all pass (notably `live_session_ref_create_is_refused_loudly`). These pin the exact wire behavior (`code == "RESTORE_UNAVAILABLE"`, message names the session, no duplicate spawn), so a semantic change in the refactor fails loudly here.

- [ ] **Step 7: Commit**

```bash
git add crates/freshell-ws/src/identity.rs crates/freshell-ws/src/terminal.rs
git commit -m "refactor(ws): route the D7 create guard through the shared live_session_owner predicate

TerminalIdentityRegistry now implements the freshell-terminal
SessionIdentityLookup seam, and the terminal.create D7 block delegates
its two-arm liveness join to TerminalRegistry::live_session_owner.
Behavior-preserving: live_session_ref_guard and the restore-unavailable
suites pin the wire contract. Part of ks38.

🤖 Generated with [Amplifier](https://github.com/microsoft/amplifier)

Co-Authored-By: Amplifier <240397093+microsoft-amplifier@users.noreply.github.com>"
```

---

### Task 3: REST guard — 409 RESTORE_UNAVAILABLE on `POST /api/tabs` + server wiring

**Files:**
- Modify: `crates/freshell-freshagent/src/lib.rs` (new `fail_json_code` helper next to `fail_json` at :1252; new `session_identity` field on `FreshAgentState` at :100-185; init in `FreshAgentState::new` near :240; builder near `with_terminal_registry` at :386)
- Modify: `crates/freshell-freshagent/src/terminal_tabs.rs` (guard inserted between lines 637 and 639 of `spawn_terminal_pane`; tests in the existing `#[cfg(test)] mod tests` at :1474)
- Modify: `crates/freshell-server/src/main.rs` (wiring)
- Modify (only if needed): `crates/freshell-freshagent/Cargo.toml` — if `tracing` is not already in `[dependencies]`, add `tracing.workspace = true`.

**Interfaces:**
- Consumes (Tasks 1-2): `freshell_terminal::registry::SessionIdentityLookup`, `TerminalRegistry::live_session_owner`, the `SessionIdentityLookup` impl on `TerminalIdentityRegistry`; existing `derive_resume_identity` output bindings `resume_session_id: Option<String>` / `accepted_session_ref: Option<SessionLocator>`; existing test helpers in `terminal_tabs.rs`'s test module: `state_with_registry()`, `app(state)`, `post(router, uri, body, auth)`, `recording_cli_spec(name, argv_file)`, `unique_argv_file(label)`.
- Produces:
  - `pub(crate) fn fail_json_code(status: StatusCode, code: &str, message: String) -> Response` in `lib.rs` (same visibility as `fail_json`).
  - `FreshAgentState.session_identity: Option<Arc<dyn freshell_terminal::registry::SessionIdentityLookup>>` and `pub fn with_session_identity(self, identity: Arc<dyn freshell_terminal::registry::SessionIdentityLookup>) -> Self`.
  - Wire contract Task 4 relies on: any REST spawn whose accepted `sessionRef` targets a live `(mode, sessionId)` returns `409` with body `{"status":"error","code":"RESTORE_UNAVAILABLE","message":"Session <sid> is still running on the server."}`.

- [ ] **Step 1: Write the failing router tests**

Add to the `#[cfg(test)] mod tests` in `crates/freshell-freshagent/src/terminal_tabs.rs` (harness helpers listed above already live there). Use the module's existing imports (`StatusCode`, `json!`, `Arc`, etc.).

```rust
    #[derive(Debug)]
    struct StubSessionIdentity {
        provider: &'static str,
        session_id: &'static str,
        terminal_id: &'static str,
    }

    impl freshell_terminal::registry::SessionIdentityLookup for StubSessionIdentity {
        fn terminal_for_session(&self, provider: &str, session_id: &str) -> Option<String> {
            (provider == self.provider && session_id == self.session_id)
                .then(|| self.terminal_id.to_string())
        }
    }

    const LIVE_SESSION: &str = "22222222-3333-4444-8555-666666666666";

    /// Forge what a REST-spawned live resume leaves behind: a Running registry
    /// row carrying (mode, resume_session_id). Headless: no real PTY.
    fn forge_live_owner(registry: &freshell_terminal::TerminalRegistry, terminal_id: &str) {
        registry.register_headless(freshell_terminal::registry::HeadlessTerminal {
            terminal_id: terminal_id.to_string(),
            stream_id: format!("s-{terminal_id}"),
            mode: "claude".to_string(),
            resume_session_id: Some(LIVE_SESSION.to_string()),
            create_request_id: None,
            created_at: None,
        });
    }

    #[tokio::test]
    async fn rest_create_resume_onto_live_session_is_refused_409_restore_unavailable() {
        let argv_file = unique_argv_file("d7-rest-live-refusal");
        let state = state_with_registry()
            .with_cli_commands(Arc::new(vec![recording_cli_spec("claude", &argv_file)]));
        let registry = state.terminal_registry.clone().unwrap();
        forge_live_owner(&registry, "t-live-owner");
        let rows_before = registry.identity_probe_rows().len();

        let (status, body) = post(
            app(state),
            "/api/tabs",
            json!({
                "mode": "claude",
                "cwd": std::env::temp_dir().to_string_lossy(),
                "sessionRef": { "provider": "claude", "sessionId": LIVE_SESSION },
            }),
            true,
        )
        .await;

        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert_eq!(body["status"], json!("error"), "{body}");
        assert_eq!(
            body["code"],
            json!("RESTORE_UNAVAILABLE"),
            "exact wire code: {body}"
        );
        let msg = body["message"].as_str().expect("message");
        assert!(
            msg.contains(LIVE_SESSION),
            "message must name the live session: {msg}"
        );
        // No duplicate spawn: only the forged owner exists.
        assert_eq!(registry.identity_probe_rows().len(), rows_before, "no new terminal");

        registry.kill("t-live-owner");
    }

    #[tokio::test]
    async fn rest_create_resume_onto_exited_session_still_works() {
        let argv_file = unique_argv_file("d7-rest-exited-ok");
        let state = state_with_registry()
            .with_cli_commands(Arc::new(vec![recording_cli_spec("claude", &argv_file)]));
        let registry = state.terminal_registry.clone().unwrap();
        forge_live_owner(&registry, "t-old-owner");
        assert!(registry.finish_pty_exit("t-old-owner", 0));

        let (status, body) = post(
            app(state),
            "/api/tabs",
            json!({
                "mode": "claude",
                "cwd": std::env::temp_dir().to_string_lossy(),
                "sessionRef": { "provider": "claude", "sessionId": LIVE_SESSION },
            }),
            true,
        )
        .await;

        assert_eq!(status, StatusCode::OK, "{body}");
        let new_tid = body["data"]["terminalId"].as_str().expect("terminalId").to_string();
        assert!(registry.is_running(&new_tid), "resume onto an exited session spawns");

        registry.kill(&new_tid);
    }

    #[tokio::test]
    async fn rest_create_resume_refused_when_identity_registry_owns_live_session() {
        // Locator-adopted shape (d9b71f50): Running row with NO resume id; the
        // binding lives only in the injected identity store.
        let argv_file = unique_argv_file("d7-rest-identity-refusal");
        let state = state_with_registry()
            .with_cli_commands(Arc::new(vec![recording_cli_spec("claude", &argv_file)]))
            .with_session_identity(Arc::new(StubSessionIdentity {
                provider: "claude",
                session_id: LIVE_SESSION,
                terminal_id: "t-adopted",
            }));
        let registry = state.terminal_registry.clone().unwrap();
        registry.register_headless(freshell_terminal::registry::HeadlessTerminal {
            terminal_id: "t-adopted".to_string(),
            stream_id: "s-t-adopted".to_string(),
            mode: "claude".to_string(),
            resume_session_id: None,
            create_request_id: None,
            created_at: None,
        });

        let (status, body) = post(
            app(state),
            "/api/tabs",
            json!({
                "mode": "claude",
                "cwd": std::env::temp_dir().to_string_lossy(),
                "sessionRef": { "provider": "claude", "sessionId": LIVE_SESSION },
            }),
            true,
        )
        .await;

        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert_eq!(body["code"], json!("RESTORE_UNAVAILABLE"), "{body}");

        registry.kill("t-adopted");
    }
```

If the test module's `post` helper has a different exact signature than `(router, uri, body, auth)`, match the local one — do not add a second helper.

- [ ] **Step 2: Run to verify they fail**

```bash
cargo test -p freshell-freshagent --lib rest_create_resume
```
Expected: **compile FAIL** first (`with_session_identity` does not exist). After Step 3's state/helper additions but before Step 4's guard, re-running yields assertion failures (`200 OK` where `409` is expected — the actual red for the guard behavior). Both red states must be observed.

- [ ] **Step 3: Add the seam and the error helper (compile scaffolding only — no behavior)**

In `crates/freshell-freshagent/src/lib.rs`:

(a) Next to `fail_json` (:1252):

```rust
/// `fail_json` + a machine-readable code, matching how the WS side keys
/// errors (`error["code"] == "RESTORE_UNAVAILABLE"`). Envelope is additive:
/// `{status:"error", code, message}`.
pub(crate) fn fail_json_code(status: StatusCode, code: &str, message: String) -> Response {
    (
        status,
        Json(json!({ "status": "error", "code": code, "message": message })),
    )
        .into_response()
}
```
(Match `fail_json`'s actual visibility — if it is private-with-`use super::` imports in sibling modules, mirror exactly what `fail_json` does so `terminal_tabs.rs` can call it the same way.)

(b) On `FreshAgentState` (field block :100-185, near `terminal_registry` at :118):

```rust
    /// Read-only session-identity lookup (in production: the WS-side
    /// TerminalIdentityRegistry behind the freshell-terminal
    /// SessionIdentityLookup seam, wired by freshell-server). Powers the
    /// identity arm of the REST D7 live-session guard. `None` (unwired)
    /// narrows the guard to the registry-row arm.
    pub(crate) session_identity:
        Option<Arc<dyn freshell_terminal::registry::SessionIdentityLookup>>,
```

(c) In `FreshAgentState::new` (the struct literal near :240): add `session_identity: None,`.

(d) Next to `with_terminal_registry` (:386):

```rust
    pub fn with_session_identity(
        mut self,
        identity: Arc<dyn freshell_terminal::registry::SessionIdentityLookup>,
    ) -> Self {
        self.session_identity = Some(identity);
        self
    }
```

If `FreshAgentState` derives `Debug`, the `std::fmt::Debug` supertrait on `SessionIdentityLookup` keeps the derive valid — no extra work.

Run `cargo test -p freshell-freshagent --lib rest_create_resume` again: compiles now; the live-refusal and identity-refusal tests FAIL on `assert_eq!(status, StatusCode::CONFLICT)` (got 200). That is the true red.

- [ ] **Step 4: Implement the guard at the choke point**

In `crates/freshell-freshagent/src/terminal_tabs.rs`, in `spawn_terminal_pane`, insert immediately after line 637 (`let (mut resume_session_id, accepted_session_ref) = derive_resume_identity(body, &mode)?;`) and before the `terminal_id`/`stream_id` minting at :639:

```rust
    // D7 live-session guard, REST rung -- mirrors the WS terminal.create guard
    // (freshell-ws/src/terminal.rs D7 block) via the shared
    // TerminalRegistry::live_session_owner predicate: a resume derived from a
    // wire `sessionRef` whose (provider, sessionId) is already owned by a
    // RUNNING terminal is refused. Never spawn a second `<cli> --resume <sid>`
    // while the original live PTY owns <sid> (one-JSONL-writer doctrine).
    // Placement: before any side effect (no PTY, no MCP write, no port alloc,
    // no codex plan), so refusal needs zero rollback. This is the single choke
    // point for POST /api/tabs, /api/panes/:id/split, and /api/panes/:id/respawn
    // (every spawn_terminal_pane caller). Scoped to the sessionRef rung exactly
    // like WS (`accepted_session_ref` already implies provider == mode); the
    // legacy bare-resumeSessionId rung keeps its existing behavior. No
    // self-exemption for respawn: the old terminal is deliberately never
    // killed ("detach, don't kill"), so resuming its live session in a second
    // PTY would be exactly the two-writers corruption this guard forbids.
    if let Some(live_sid) = accepted_session_ref
        .as_ref()
        .map(|r| r.session_id.as_str())
        .filter(|sid| !sid.is_empty() && resume_session_id.as_deref() == Some(*sid))
    {
        if registry
            .live_session_owner(state.session_identity.as_deref(), &mode, live_sid)
            .is_some()
        {
            tracing::warn!(
                target: "freshell_freshagent::terminal_tabs",
                mode = %mode,
                session_id = %live_sid,
                pane_id = %pane_id,
                "spawn_refused: a Running terminal already owns this session (D7 live-guard, REST rung)"
            );
            return Err(fail_json_code(
                StatusCode::CONFLICT,
                "RESTORE_UNAVAILABLE",
                format!("Session {live_sid} is still running on the server."),
            ));
        }
    }
```

Import `fail_json_code` alongside the module's existing `fail_json` import. `state.session_identity.as_deref()` turns `Option<Arc<dyn _>>` into `Option<&dyn _>`. If `tracing` is missing from `crates/freshell-freshagent/Cargo.toml` `[dependencies]`, add `tracing.workspace = true` (check first — the crate very likely already logs).

- [ ] **Step 5: Run to verify the new tests pass, then the whole crate**

```bash
cargo test -p freshell-freshagent --lib rest_create_resume
```
Expected: `3 passed`.

```bash
cargo test -p freshell-freshagent --lib
```
Expected: all pass (284 pre-existing + 3 new). The pre-existing resume tests (`create_claude_tab_with_canonical_resume_id_synthesizes_session_ref`, `create_codex_tab_accepts_session_ref_and_derives_resume_args`, etc.) are the over-blocking pins: they resume with NO live owner and must still return 200.

- [ ] **Step 6: Wire the identity registry in `freshell-server`**

In `crates/freshell-server/src/main.rs`:
1. Locate where the WS identity registry is constructed (the value assigned to the `identity` field of the `WsState` literal — a `freshell_ws::identity::TerminalIdentityRegistry`).
2. Locate where the freshagent state is built (the builder chain containing `.with_terminal_registry(...)` — the `FreshAgentState` that flows into both the REST router and `WsState`).
3. If the identity registry is currently constructed *after* the freshagent state, hoist its `let` binding above the freshagent builder chain (it is a cheap-clone handle; `WsState` keeps using the same binding).
4. Chain onto the freshagent builder, immediately after `.with_terminal_registry(...)`:

```rust
        .with_session_identity(std::sync::Arc::new(identity.clone()))
```

(`Arc<TerminalIdentityRegistry>` coerces to `Arc<dyn SessionIdentityLookup>` via the Task 2 impl. Use the actual local binding name for the identity registry.)

Verify:
```bash
cargo check -p freshell-server
cargo test -p freshell-server --lib
```
Expected: clean check; all server tests pass (including `recovery_inventory_tests::live_session_keys_*`).

- [ ] **Step 7: Commit**

```bash
git add crates/freshell-freshagent/src/lib.rs crates/freshell-freshagent/src/terminal_tabs.rs crates/freshell-server/src/main.rs
# plus crates/freshell-freshagent/Cargo.toml if it changed
git commit -m "fix(freshagent): enforce the D7 live-session guard on the REST spawn pipeline (ks38)

POST /api/tabs (and every spawn_terminal_pane caller) now refuses a
sessionRef resume onto a (mode, sessionId) owned by a Running terminal
with 409 {code: RESTORE_UNAVAILABLE}, via the shared
TerminalRegistry::live_session_owner predicate -- both arms: registry
rows plus the identity registry injected from freshell-server through
the new SessionIdentityLookup seam. Same message text as the WS guard;
refusal happens before any side effect, so no rollback is needed.

🤖 Generated with [Amplifier](https://github.com/microsoft/amplifier)

Co-Authored-By: Amplifier <240397093+microsoft-amplifier@users.noreply.github.com>"
```

---

### Task 4: Route coverage — respawn and split pins

**Files:**
- Modify: `crates/freshell-freshagent/src/terminal_tabs.rs` (tests only, same `#[cfg(test)] mod tests` — it has the full harness including `create_shell_tab` plus `recording_cli_spec`, which the `pane_ops.rs` test module lacks; the router under test is the same `crate::router(state)`)

**Interfaces:**
- Consumes: Task 3's wire contract (409 + `RESTORE_UNAVAILABLE` from any `spawn_terminal_pane` caller), and two helpers Task 3 already committed into this same test module — `const LIVE_SESSION: &str = "22222222-3333-4444-8555-666666666666";` and `fn forge_live_owner(registry: &freshell_terminal::TerminalRegistry, terminal_id: &str)` (registers a headless Running claude row with `resume_session_id: Some(LIVE_SESSION)`). `StubSessionIdentity` is NOT needed here.
- Produces: regression pins proving the guard covers `POST /api/panes/:id/respawn` and `POST /api/panes/:id/split`, that respawn has NO self-exemption, and that respawn-resume works once the owner exits.

- [ ] **Step 1: Write the route tests**

```rust
    #[tokio::test]
    async fn rest_respawn_resume_onto_live_session_is_refused_409() {
        let argv_file = unique_argv_file("d7-respawn-live-refusal");
        let state = state_with_registry()
            .with_cli_commands(Arc::new(vec![recording_cli_spec("claude", &argv_file)]));
        let registry = state.terminal_registry.clone().unwrap();
        let router = app(state);
        let (_tab_id, pane_id, shell_tid) = create_shell_tab(router.clone()).await;
        forge_live_owner(&registry, "t-live-owner-respawn");

        let (status, body) = post(
            router,
            &format!("/api/panes/{pane_id}/respawn"),
            json!({
                "mode": "claude",
                "cwd": std::env::temp_dir().to_string_lossy(),
                "sessionRef": { "provider": "claude", "sessionId": LIVE_SESSION },
            }),
            true,
        )
        .await;

        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert_eq!(body["code"], json!("RESTORE_UNAVAILABLE"), "{body}");

        registry.kill("t-live-owner-respawn");
        registry.kill(&shell_tid);
    }

    #[tokio::test]
    async fn rest_respawn_resume_after_owner_exits_succeeds() {
        let argv_file = unique_argv_file("d7-respawn-after-exit");
        let state = state_with_registry()
            .with_cli_commands(Arc::new(vec![recording_cli_spec("claude", &argv_file)]));
        let registry = state.terminal_registry.clone().unwrap();
        let router = app(state);
        let (_tab_id, pane_id, shell_tid) = create_shell_tab(router.clone()).await;
        forge_live_owner(&registry, "t-exited-owner-respawn");
        assert!(registry.finish_pty_exit("t-exited-owner-respawn", 0));

        let (status, body) = post(
            router,
            &format!("/api/panes/{pane_id}/respawn"),
            json!({
                "mode": "claude",
                "cwd": std::env::temp_dir().to_string_lossy(),
                "sessionRef": { "provider": "claude", "sessionId": LIVE_SESSION },
            }),
            true,
        )
        .await;

        assert_eq!(status, StatusCode::OK, "{body}");
        let new_tid = body["data"]["terminalId"].as_str().expect("terminalId").to_string();
        assert!(registry.is_running(&new_tid));

        registry.kill(&new_tid);
        registry.kill(&shell_tid);
    }

    /// No self-exemption: the pane's OWN still-running terminal counts as the
    /// live owner. Respawning pane P (which detaches -- never kills -- its old
    /// terminal) with the same sessionRef would make two live writers for S.
    #[tokio::test]
    async fn rest_respawn_same_pane_own_live_session_is_refused_409() {
        let argv_file = unique_argv_file("d7-respawn-self-collision");
        let state = state_with_registry()
            .with_cli_commands(Arc::new(vec![recording_cli_spec("claude", &argv_file)]));
        let registry = state.terminal_registry.clone().unwrap();
        let router = app(state);

        // First create resumes S with no live owner -> 200; leaves a Running
        // claude terminal whose row is stamped resume_session_id = S.
        let (status, body) = post(
            router.clone(),
            "/api/tabs",
            json!({
                "mode": "claude",
                "cwd": std::env::temp_dir().to_string_lossy(),
                "sessionRef": { "provider": "claude", "sessionId": LIVE_SESSION },
            }),
            true,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let pane_id = body["data"]["paneId"].as_str().expect("paneId").to_string();
        let first_tid = body["data"]["terminalId"].as_str().expect("terminalId").to_string();
        assert!(registry.is_running(&first_tid));

        let (status, body) = post(
            router,
            &format!("/api/panes/{pane_id}/respawn"),
            json!({
                "mode": "claude",
                "cwd": std::env::temp_dir().to_string_lossy(),
                "sessionRef": { "provider": "claude", "sessionId": LIVE_SESSION },
            }),
            true,
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert_eq!(body["code"], json!("RESTORE_UNAVAILABLE"), "{body}");
        assert!(registry.is_running(&first_tid), "old terminal untouched by refusal");

        registry.kill(&first_tid);
    }

    #[tokio::test]
    async fn rest_split_resume_onto_live_session_is_refused_409() {
        let argv_file = unique_argv_file("d7-split-live-refusal");
        let state = state_with_registry()
            .with_cli_commands(Arc::new(vec![recording_cli_spec("claude", &argv_file)]));
        let registry = state.terminal_registry.clone().unwrap();
        let router = app(state);
        let (_tab_id, pane_id, shell_tid) = create_shell_tab(router.clone()).await;
        forge_live_owner(&registry, "t-live-owner-split");

        let (status, body) = post(
            router,
            &format!("/api/panes/{pane_id}/split"),
            json!({
                "direction": "vertical",
                "mode": "claude",
                "cwd": std::env::temp_dir().to_string_lossy(),
                "sessionRef": { "provider": "claude", "sessionId": LIVE_SESSION },
            }),
            true,
        )
        .await;

        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert_eq!(body["code"], json!("RESTORE_UNAVAILABLE"), "{body}");

        registry.kill("t-live-owner-split");
        registry.kill(&shell_tid);
    }
```

If `create_shell_tab` does not exist in `terminal_tabs.rs`'s test module (it is documented as duplicated there from `pane_ops.rs:1007`; verify), copy the exact helper from `pane_ops.rs:1007-1021` into this module rather than reworking the tests (copy the WHOLE fn — validation found the function runs to :1021).

- [ ] **Step 2: Run the tests**

```bash
cargo test -p freshell-freshagent --lib rest_respawn
cargo test -p freshell-freshagent --lib rest_split_resume_onto_live_session_is_refused_409
```
Expected: **all PASS immediately** — that is the point: they prove the Task 3 choke point already covers the respawn and split routes (a deliberate superset of ks38 — the kata's text names the create path; respawn/split coverage is this plan's hardening, not a kata quote). If ANY of these fails, the choke-point assumption is wrong (a route bypasses `spawn_terminal_pane`); STOP and fix the guard placement in Task 3's code, then re-run — do not weaken the tests.

- [ ] **Step 3: Run the whole crate**

```bash
cargo test -p freshell-freshagent --lib
```
Expected: all pass.

- [ ] **Step 4: Commit**

```bash
git add crates/freshell-freshagent/src/terminal_tabs.rs
git commit -m "test(freshagent): pin D7 REST guard coverage on respawn and split routes

Respawn/split with a sessionRef onto a live (mode, sessionId) are
refused 409 RESTORE_UNAVAILABLE through the shared spawn choke point;
respawn-resume succeeds once the owner exits; and respawn has NO
self-exemption -- the pane's own detached-but-running predecessor
counts as the live owner (two-writers doctrine). Part of ks38.

🤖 Generated with [Amplifier](https://github.com/microsoft/amplifier)

Co-Authored-By: Amplifier <240397093+microsoft-amplifier@users.noreply.github.com>"
```

---

### Task 5: D8 session-ref lease at the REST choke point

Closes the check-then-spawn race the D7 guard alone cannot (Design Decision 6): two concurrent REST resumes on the same `(mode, sessionId)` — or a REST resume racing a WS create — could both pass the D7 check and spawn two writers. The WS path already serializes this with the registry's per-sessionRef lease; REST adopts the same primitive at the same choke point.

**Files:**
- Modify: `crates/freshell-freshagent/src/terminal_tabs.rs` (lease claim + RAII release in `spawn_terminal_pane`, immediately after Task 3's D7 guard, same sessionRef-rung scope; tests in the same `#[cfg(test)]` module)

**Interfaces:**
- Consumes (all `pub` on `freshell-terminal` — verified during plan validation; mirror the WS call site at `crates/freshell-ws/src/terminal.rs:987-1215` for exact signatures):
  - `SessionLocator { provider, session_id }` and `SessionRefClaim::{Acquired, Held, ExpiredNeedsKill, BoundElsewhere}` (registry.rs:456-473; check the exact import path — `freshell_terminal::registry::...` or a crate-root re-export — before writing).
  - `TerminalRegistry::claim_session_ref(&SessionLocator, holder_create_request_id: &str, holder_conn: u64, now_ms: u64) -> SessionRefClaim` (registry.rs:1761-1767).
  - `TerminalRegistry::{complete_session_ref_claim (:1913, atomic lease→binding, returns bool), fail_session_ref_claim (:1956), set_session_ref_lease_pid (:1890), force_release_after_confirmed_kill (:1997), pid_of (:2018), new_connection_id (:622), bound_terminal_for_session_ref (:2007, pub read of the sessionRef→terminalId bindings map — its doc-comment marks it a test probe; already used cross-crate by `freshell-ws/tests/session_ref_singleflight.rs`)}` and `freshell_terminal::registry::pid_alive` (:437).
- Produces: REST resume spawns on the sessionRef rung are serialized by the registry lease; success records the sessionRef→terminalId binding, so later WS claims answer `BoundElsewhere` (adopt) instead of double-spawning.

- [ ] **Step 1: Write the failing tests**

Add to the `terminal_tabs.rs` test module (reuse `LIVE_SESSION`, `recording_cli_spec`, `unique_argv_file`, `post`, `app`, `state_with_registry` from Tasks 3-4). Compute `now_ms` from `std::time::{SystemTime, UNIX_EPOCH}` (or reuse the crate's existing now-ms helper if one exists — check how the WS claim site gets it).

```rust
    #[tokio::test]
    async fn rest_create_resume_while_lease_held_is_refused_409() {
        let argv_file = unique_argv_file("d8-lease-held-refusal");
        let state = state_with_registry()
            .with_cli_commands(Arc::new(vec![recording_cli_spec("claude", &argv_file)]));
        let registry = state.terminal_registry.clone().unwrap();
        let router = app(state);

        // A foreign holder (e.g. an in-flight WS create) holds the lease.
        let locator = SessionLocator {
            provider: "claude".into(),
            session_id: LIVE_SESSION.into(),
        };
        assert!(matches!(
            registry.claim_session_ref(&locator, "foreign-holder", registry.new_connection_id(), test_now_ms()),
            SessionRefClaim::Acquired
        ));

        let (status, body) = post(
            router,
            "/api/tabs",
            json!({
                "mode": "claude",
                "cwd": std::env::temp_dir().to_string_lossy(),
                "sessionRef": { "provider": "claude", "sessionId": LIVE_SESSION },
            }),
            true,
        )
        .await;

        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert_eq!(body["code"], json!("RESTORE_UNAVAILABLE"), "{body}");

        registry.fail_session_ref_claim(&locator, "foreign-holder");
    }

    #[tokio::test]
    async fn rest_create_resume_completes_claim_into_binding() {
        let argv_file = unique_argv_file("d8-lease-completion");
        let state = state_with_registry()
            .with_cli_commands(Arc::new(vec![recording_cli_spec("claude", &argv_file)]));
        let registry = state.terminal_registry.clone().unwrap();
        let router = app(state);

        let locator = SessionLocator {
            provider: "claude".into(),
            session_id: LIVE_SESSION.into(),
        };
        // Precondition: nothing is bound before the spawn.
        assert_eq!(registry.bound_terminal_for_session_ref(&locator), None);

        let (status, body) = post(
            router,
            "/api/tabs",
            json!({
                "mode": "claude",
                "cwd": std::env::temp_dir().to_string_lossy(),
                "sessionRef": { "provider": "claude", "sessionId": LIVE_SESSION },
            }),
            true,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let tid = body["data"]["terminalId"].as_str().expect("terminalId").to_string();

        // The REST spawn must have completed its claim into a sessionRef->terminalId
        // binding. Observe the bindings map DIRECTLY via the pub test probe
        // `bound_terminal_for_session_ref` (registry.rs:2007-2013; only
        // complete_session_ref_claim writes that map). Do NOT probe this with a
        // late claim_session_ref call: its row-join arm (registry.rs:1771-1773)
        // answers BoundElsewhere from the Running row's resume_session_id stamp
        // alone, so that probe passes even when no binding was ever recorded --
        // it cannot distinguish completion from the D7 row-join.
        assert_eq!(
            registry.bound_terminal_for_session_ref(&locator),
            Some(tid.clone()),
            "REST resume spawn must complete its lease into a sessionRef binding"
        );

        registry.kill(&tid);
    }
```

(Adjust names/paths to the real API surface if the compiler disagrees — the semantics above are the contract; the WS call site is the reference implementation.)

- [ ] **Step 2: Run to verify they fail**

```bash
cargo test -p freshell-freshagent --lib rest_create_resume_while_lease_held_is_refused_409
cargo test -p freshell-freshagent --lib rest_create_resume_completes_claim_into_binding
```
Expected red: the lease-held test gets **200** (no lease logic yet — spawn proceeds); the completion test fails on the binding probe (`bound_terminal_for_session_ref` returns **`None`** instead of `Some(tid)`, because nothing recorded a binding — the spawn's Running row stamps `resume_session_id`, which feeds the row-join arm of `claim_session_ref`, but only `complete_session_ref_claim` writes the bindings map the probe reads).

- [ ] **Step 3: Implement the lease at the choke point**

In `spawn_terminal_pane`, immediately after Task 3's D7 guard block (same `accepted_session_ref`/`live_sid` rung — legacy bare-`resumeSessionId` stays lease-free, matching WS's legacy rung):

1. Add a small RAII guard type near `spawn_terminal_pane` (direct port of `SessionRefLeaseGuard`, ws/terminal.rs:993-1017 — ~20 lines): holds a `TerminalRegistry` clone (cheap handle), the `SessionLocator`, the holder id, and an `armed: bool`; `Drop` calls `fail_session_ref_claim` when still armed; `disarm()` for the winner path.
2. Claim, using the already-minted `create_request_id` (terminal_tabs.rs:617-622) as holder id and a fresh `registry.new_connection_id()` as `holder_conn` (collision-free with WS conn cleanup; REST leases rely on RAII drop + the 20s TTL instead of conn-death cleanup):

```rust
    let mut session_ref_lease = None;
    if let Some(live_sid) = /* same rung condition as the D7 guard */ {
        let locator = SessionLocator { provider: mode.clone(), session_id: live_sid.to_string() };
        match registry.claim_session_ref(&locator, &create_request_id, registry.new_connection_id(), now_ms()) {
            SessionRefClaim::Acquired => {
                session_ref_lease = Some(RestSessionRefLease::new(registry.clone(), locator, create_request_id.clone()));
            }
            // Conservative v1 (Design Decision 6): every non-Acquired arm answers the
            // same 409 envelope. Held = a claim is in flight; BoundElsewhere = a live
            // winner exists (D7's own answer); ExpiredNeedsKill = crashed holder — no
            // kill logic on REST, the 20s TTL is the backstop.
            SessionRefClaim::Held { .. }
            | SessionRefClaim::BoundElsewhere { .. }
            | SessionRefClaim::ExpiredNeedsKill { .. } => {
                tracing::warn!(/* mode, session_id, pane_id */ "spawn_refused: sessionRef lease unavailable (D8, REST rung)");
                return Err(fail_json_code(
                    StatusCode::CONFLICT,
                    "RESTORE_UNAVAILABLE",
                    format!("Session {live_sid} is still running on the server."),
                ));
            }
        }
    }
```

3. After `registry.create(...)` succeeds (terminal_id known, ~:884-888): if `session_ref_lease` is `Some`, set the lease pid (`registry.pid_of(&terminal_id)` → `set_session_ref_lease_pid`), then `complete_session_ref_claim(&locator, &holder_id, &terminal_id)`. On `true`: `disarm()` the guard (the binding now owns the record). On `false` (lease revoked mid-spawn): kill our own child (`registry.kill(&terminal_id)`, confirm via `pid_alive` polling — WS does 20×25ms — then `force_release_after_confirmed_kill`) and return the 409 envelope; never leave the orphan running.
4. Every error return between claim and completion needs no special handling — the Drop guard releases the lease (including the existing `registry.create` rollback arm at :889-918 and axum cancelling the request future).

- [ ] **Step 4: Run to verify green, then the crate**

```bash
cargo test -p freshell-freshagent --lib rest_create_resume_while_lease_held
cargo test -p freshell-freshagent --lib rest_create_resume_completes_claim_into_binding
cargo test -p freshell-freshagent --lib
```
Expected: both new tests pass; the whole crate stays green (Tasks 3-4's tests included — the D7 guard fires before the lease, so live-owner refusals are unchanged; single-create resume tests acquire and complete the lease invisibly).

Also re-run the WS suites (the lease is shared state — prove no cross-path regression):
```bash
cargo test -p freshell-ws
```

- [ ] **Step 5: Commit**

```bash
git add crates/freshell-freshagent/src/terminal_tabs.rs
git commit -m "fix(freshagent): serialize REST sessionRef resumes with the D8 session-ref lease

The REST choke point now claims the registry's per-sessionRef lease
(claim_session_ref) after the D7 guard and completes it into a binding
on success, closing the REST-x-REST / REST-x-WS duplicate-writer race
the check-then-spawn guard alone leaves open. Conservative arms:
Held/BoundElsewhere/ExpiredNeedsKill all answer 409 RESTORE_UNAVAILABLE;
RAII release on every failure path. Part of ks38.

🤖 Generated with [Amplifier](https://github.com/microsoft/amplifier)

Co-Authored-By: Amplifier <240397093+microsoft-amplifier@users.noreply.github.com>"
```

---

### Task 6: Full-workspace verification (the CI gate, locally)

**Files:** none expected (fix-ups only if a check fails).

**Interfaces:** consumes everything above; produces the verified state Task 7 lands.

- [ ] **Step 1: Run the full Rust suite**

```bash
cd /home/dan/code/freshell/.worktrees/rest-resume-live-guard
cargo test --workspace
```
Expected: all crates pass (Rust tests are not in CI — this local run is the only test gate, say so in the PR body). Timeout note: cold build can take minutes; use a generous timeout (e.g. 900s).

- [ ] **Step 2: Run the two checks CI actually enforces**

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```
Expected: both clean. If fmt fails, run `cargo fmt --all` and re-check. Clippy lints the new `#[cfg(test)]` code too.

- [ ] **Step 3: Commit any fix-ups (only if Steps 1-2 required changes)**

```bash
git add -A
git commit -m "chore: fmt/clippy fix-ups for the D7 REST guard

🤖 Generated with [Amplifier](https://github.com/microsoft/amplifier)

Co-Authored-By: Amplifier <240397093+microsoft-amplifier@users.noreply.github.com>"
```
If nothing changed, skip this step (do not create an empty commit).

---

### Task 7: Land — PR, merge, fast-forward main, close kata ks38

The user has **explicitly pre-approved** PR creation and self-merge for this change. Do NOT run this task unless Tasks 1-6 are complete and green.

**Files:** none (git/gh/kata operations only).

- [ ] **Step 1: Push the branch**

```bash
cd /home/dan/code/freshell/.worktrees/rest-resume-live-guard
git push -u origin fix/rest-resume-live-guard
```

- [ ] **Step 2: Open the PR (pre-approved)**

```bash
gh pr create --base main --title "fix(freshagent): enforce the D7 live-session guard on the REST spawn pipeline (ks38)" --body "$(cat <<'EOF'
Fixes kata ks38 (P1): REST POST /api/tabs (and /api/panes/:id/split, /api/panes/:id/respawn) could spawn a second `<cli> --resume <sid>` while a Running terminal already owned that session -- the two-JSONL-writers corruption the WS terminal.create D7 guard already refuses.

- Shared predicate `TerminalRegistry::live_session_owner` in freshell-terminal (identity arm via new `SessionIdentityLookup` seam + registry-row arm); WS guard refactored to call it (no copy).
- REST spawn choke point (`spawn_terminal_pane`) refuses with `409 {code: RESTORE_UNAVAILABLE, message: "Session <sid> is still running on the server."}` before any side effect.
- Identity registry injected into `FreshAgentState` from freshell-server (freshagent cannot depend on freshell-ws), so the REST guard runs the same two-store join as WS (d9b71f50 parity).
- No respawn self-exemption: the pane's own detached-but-running predecessor counts as the live owner.
- D8 parity: the REST choke point also claims the same per-sessionRef lease the WS create path holds (`claim_session_ref`) before spawning and completes it into a binding on success, closing the REST-x-REST / REST-x-WS duplicate-writer race -- conservative arms (Held/BoundElsewhere/ExpiredNeedsKill) all answer 409.
- Tests: registry predicate units, identity-arm unit through the real TerminalIdentityRegistry, and router tests pinning 409-on-live / 200-after-exit across create, respawn, and split. Rust tests are not in CI; `cargo test --workspace`, `cargo fmt --all --check`, and `cargo clippy --workspace --all-targets -- -D warnings` all pass locally.

🤖 Generated with [Amplifier](https://github.com/microsoft/amplifier)
EOF
)"
```

- [ ] **Step 3: Wait for required checks, then merge (self-merge is the repo norm)**

```bash
gh pr checks --watch
gh pr merge --squash --delete-branch
```
Expected: checks green (`rust-clippy`, `typecheck-client`), merge succeeds. If a required check fails, fix in the worktree, push, and re-watch — do not merge red.

- [ ] **Step 4: Fast-forward local main**

```bash
cd /home/dan/code/freshell
git checkout main
git pull --ff-only origin main
git log -1 --format=%H
```
Expected: fast-forward only (if it cannot fast-forward, stop and resolve explicitly — no local merge commit). Record the merged sha printed by the last command.

- [ ] **Step 5: Close the kata with evidence**

Only with everything verified and merged. From the repo root, with `<merged-sha>` from Step 4:

```bash
cd /home/dan/code/freshell
kata close ks38 --done --message "REST spawn pipeline (POST /api/tabs, /api/panes/:id/split, /api/panes/:id/respawn) now enforces the D7 live-session guard via the shared TerminalRegistry::live_session_owner predicate (identity arm injected through the new SessionIdentityLookup seam + registry-row arm), refusing resume onto a live (mode, sessionId) with 409 RESTORE_UNAVAILABLE before any side effect; sessionRef-rung spawns additionally claim the D8 per-sessionRef lease (claim_session_ref) and complete it into a binding, closing the REST-x-REST / REST-x-WS duplicate-writer race; WS guard refactored onto the same predicate; tests pin 409-on-live, 200-after-exit, the no-self-exemption respawn case, and the lease-held/claim-completion contract." --commit <merged-sha>
```
