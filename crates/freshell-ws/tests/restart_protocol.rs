use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use freshell_protocol::{
    AgentRestart, AgentRestartFailureCode, AgentRuntimeKind, FreshAgentCreated, FreshAgentEvent,
    InventoryTerminal, RuntimeDescriptor, ServerMessage, SessionLocator, TerminalAttachReady,
    TerminalCreated, TerminalExit, TerminalInventory, TerminalOutput, TerminalRunStatus,
};
use freshell_ws::restart::{RestartCoordinator, RestartFailure, RestartRuntime, RuntimeLocator};

struct FakeRuntime {
    events: Mutex<Vec<&'static str>>,
    running: AtomicBool,
    shutdowns: AtomicUsize,
    resumable: bool,
    replacement_id: &'static str,
}

impl FakeRuntime {
    fn resumable(replacement_id: &'static str) -> Self {
        Self {
            events: Mutex::new(Vec::new()),
            running: AtomicBool::new(true),
            shutdowns: AtomicUsize::new(0),
            resumable: true,
            replacement_id,
        }
    }

    fn unresumable() -> Self {
        Self {
            events: Mutex::new(Vec::new()),
            running: AtomicBool::new(true),
            shutdowns: AtomicUsize::new(0),
            resumable: false,
            replacement_id: "unused",
        }
    }
}

impl RestartRuntime for FakeRuntime {
    type ResumePlan = ();

    async fn preflight(&self, _request: &AgentRestart) -> Result<Self::ResumePlan, RestartFailure> {
        self.events.lock().unwrap().push("preflight");
        if self.resumable {
            Ok(())
        } else {
            Err(RestartFailure::new(
                AgentRestartFailureCode::Unresumable,
                "durable session is unavailable",
                false,
            ))
        }
    }

    async fn shutdown_for_restart(
        &self,
        _request: &AgentRestart,
        _plan: &Self::ResumePlan,
    ) -> Result<(), RestartFailure> {
        assert_eq!(
            self.events.lock().unwrap().as_slice(),
            ["preflight"],
            "shutdown must happen only after successful preflight"
        );
        self.events.lock().unwrap().push("shutdown");
        self.shutdowns.fetch_add(1, Ordering::SeqCst);
        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }

    async fn create_replacement(
        &self,
        _request: &AgentRestart,
        _plan: Self::ResumePlan,
    ) -> Result<String, RestartFailure> {
        assert!(!self.running.load(Ordering::SeqCst));
        self.events.lock().unwrap().push("replace");
        Ok(self.replacement_id.to_string())
    }
}

struct OrderedRuntime {
    phase: Arc<AtomicUsize>,
}

impl RestartRuntime for OrderedRuntime {
    type ResumePlan = ();

    async fn preflight(&self, _request: &AgentRestart) -> Result<(), RestartFailure> {
        assert_eq!(self.phase.swap(1, Ordering::SeqCst), 0);
        Ok(())
    }

    async fn shutdown_for_restart(
        &self,
        _request: &AgentRestart,
        _plan: &(),
    ) -> Result<(), RestartFailure> {
        assert_eq!(
            self.phase.swap(3, Ordering::SeqCst),
            2,
            "started must be emitted after preflight and before shutdown"
        );
        Ok(())
    }

    async fn create_replacement(
        &self,
        _request: &AgentRestart,
        _plan: (),
    ) -> Result<String, RestartFailure> {
        assert_eq!(self.phase.swap(4, Ordering::SeqCst), 3);
        Ok("term-2".to_string())
    }
}

fn locator() -> RuntimeLocator {
    RuntimeLocator::new(AgentRuntimeKind::Terminal, "claude", "durable-1")
}

fn restart(request_id: &str, live_id: &str, generation: u64) -> AgentRestart {
    AgentRestart {
        request_id: request_id.to_string(),
        provider: "claude".to_string(),
        session_id: "durable-1".to_string(),
        kind: AgentRuntimeKind::Terminal,
        live_id: live_id.to_string(),
        expected_generation: generation,
    }
}

#[tokio::test]
async fn restart_preflights_before_stopping_the_selected_runtime() {
    let coordinator = RestartCoordinator::new();
    coordinator.register_initial(locator(), "term-1");
    let runtime = FakeRuntime::resumable("term-2");

    let outcome = coordinator
        .execute(restart("r1", "term-1", 1), &runtime)
        .await;

    assert!(!outcome.replayed);
    assert!(matches!(
        outcome.messages.as_slice(),
        [
            ServerMessage::AgentRestartStarted(_),
            ServerMessage::AgentRestartReplaced(_)
        ]
    ));
    assert_eq!(
        runtime.events.lock().unwrap().as_slice(),
        ["preflight", "shutdown", "replace"]
    );
}

#[tokio::test]
async fn started_is_emitted_after_preflight_and_before_shutdown() {
    let coordinator = RestartCoordinator::new();
    coordinator.register_initial(locator(), "term-1");
    let phase = Arc::new(AtomicUsize::new(0));
    let runtime = OrderedRuntime {
        phase: Arc::clone(&phase),
    };
    coordinator
        .execute_with_events(
            restart("r1", "term-1", 1),
            &runtime,
            |message| match message {
                ServerMessage::AgentRestartStarted(_) => {
                    assert_eq!(phase.swap(2, Ordering::SeqCst), 1);
                }
                ServerMessage::AgentRestartReplaced(_) => {
                    assert_eq!(phase.swap(5, Ordering::SeqCst), 4);
                }
                other => panic!("unexpected event: {other:?}"),
            },
        )
        .await;
    assert_eq!(phase.load(Ordering::SeqCst), 5);
}

#[tokio::test]
async fn unresumable_restart_fails_without_stopping_the_live_runtime() {
    let coordinator = RestartCoordinator::new();
    coordinator.register_initial(locator(), "term-1");
    let runtime = FakeRuntime::unresumable();

    let outcome = coordinator
        .execute(restart("r1", "term-1", 1), &runtime)
        .await;

    assert!(matches!(
        outcome.messages.as_slice(),
        [ServerMessage::AgentRestartFailed(message)]
            if message.code == AgentRestartFailureCode::Unresumable
    ));
    assert!(runtime.running.load(Ordering::SeqCst));
    assert_eq!(runtime.shutdowns.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn runtime_generation_is_stable_across_attach_and_reconnect_and_changes_on_replacement() {
    let coordinator = RestartCoordinator::new();
    let created = coordinator.register_initial(locator(), "term-1");
    let attached = coordinator
        .runtime_for_live(AgentRuntimeKind::Terminal, "term-1")
        .unwrap();
    let reconnected = coordinator.register_initial(locator(), "term-1");

    assert_eq!(created, attached);
    assert_eq!(created, reconnected);

    let outcome = coordinator
        .execute(
            restart("r1", "term-1", created.generation),
            &FakeRuntime::resumable("term-2"),
        )
        .await;
    let ServerMessage::AgentRestartReplaced(replaced) = &outcome.messages[1] else {
        panic!("expected replacement");
    };
    assert_eq!(replaced.old_runtime.as_runtime_descriptor(), created);
    assert_eq!(replaced.runtime.runtime_id, "term-2");
    assert_eq!(replaced.runtime.generation, created.generation + 1);
    assert_eq!(
        coordinator
            .runtime_for_live(AgentRuntimeKind::Terminal, "term-2")
            .unwrap(),
        replaced.runtime
    );
}

#[tokio::test]
async fn resend_after_requester_disconnect_replays_the_stored_terminal_result() {
    let coordinator = RestartCoordinator::new();
    coordinator.register_initial(locator(), "term-1");
    let runtime = FakeRuntime::resumable("term-2");
    let request = restart("r1", "term-1", 1);

    let first = coordinator.execute(request.clone(), &runtime).await;
    assert!(matches!(
        first.messages.last(),
        Some(ServerMessage::AgentRestartReplaced(_))
    ));

    let replay = coordinator.execute(request, &runtime).await;
    assert!(replay.replayed);
    assert!(matches!(
        replay.messages.as_slice(),
        [ServerMessage::AgentRestartReplaced(_)]
    ));
    assert_eq!(
        runtime.shutdowns.load(Ordering::SeqCst),
        1,
        "replay must not run teardown twice"
    );
}

#[tokio::test]
async fn request_id_reuse_with_a_different_fingerprint_is_rejected() {
    let coordinator = RestartCoordinator::new();
    coordinator.register_initial(locator(), "term-1");
    let runtime = FakeRuntime::resumable("term-2");
    coordinator
        .execute(restart("r1", "term-1", 1), &runtime)
        .await;

    let conflict = coordinator
        .execute(restart("r1", "term-other", 1), &runtime)
        .await;
    assert!(conflict.replayed);
    assert!(matches!(
        conflict.messages.as_slice(),
        [ServerMessage::AgentRestartFailed(message)]
            if message.code == AgentRestartFailureCode::RequestIdConflict
    ));
    assert_eq!(runtime.shutdowns.load(Ordering::SeqCst), 1);
}

#[test]
fn terminal_lifecycle_surfaces_share_one_server_owned_descriptor() {
    let coordinator = RestartCoordinator::new();
    let session_ref = SessionLocator {
        provider: "claude".to_string(),
        session_id: "durable-1".to_string(),
    };
    let mut created = ServerMessage::TerminalCreated(TerminalCreated {
        created_at: 1,
        request_id: "create-1".to_string(),
        terminal_id: "term-1".to_string(),
        clear_codex_durability: None,
        cwd: None,
        restore_error: None,
        session_ref: Some(session_ref.clone()),
        runtime: None,
    });
    coordinator.observe_server_message(&mut created);
    let ServerMessage::TerminalCreated(created) = created else {
        unreachable!()
    };
    let expected = created.runtime.unwrap();

    let mut attach = ServerMessage::TerminalAttachReady(TerminalAttachReady {
        head_seq: 0,
        replay_from_seq: 1,
        replay_to_seq: 0,
        stream_id: "stream-1".to_string(),
        terminal_id: "term-1".to_string(),
        attach_request_id: Some("attach-1".to_string()),
        effective_since_seq: Some(0),
        geometry_authority: None,
        geometry_epoch: None,
        replay_reset_reason: None,
        requested_since_seq: Some(0),
        session_ref: Some(session_ref.clone()),
        runtime: None,
    });
    coordinator.observe_server_message(&mut attach);
    let ServerMessage::TerminalAttachReady(attach) = attach else {
        unreachable!()
    };
    assert_eq!(attach.runtime, Some(expected.clone()));

    let mut inventory = ServerMessage::TerminalInventory(TerminalInventory {
        boot_id: "boot-1".to_string(),
        terminals: vec![InventoryTerminal {
            created_at: 1,
            last_activity_at: 2,
            mode: "claude".to_string(),
            status: TerminalRunStatus::Running,
            terminal_id: "term-1".to_string(),
            title: "Claude".to_string(),
            codex_durability: None,
            cwd: None,
            description: None,
            runtime_status: None,
            session_ref: Some(session_ref),
            runtime: None,
        }],
        terminal_meta: vec![],
    });
    coordinator.observe_server_message(&mut inventory);
    let ServerMessage::TerminalInventory(inventory) = inventory else {
        unreachable!()
    };
    assert_eq!(inventory.terminals[0].runtime, Some(expected.clone()));

    let mut output = ServerMessage::TerminalOutput(TerminalOutput {
        data: "hi".to_string(),
        seq_end: 1,
        seq_start: 1,
        stream_id: "stream-1".to_string(),
        terminal_id: "term-1".to_string(),
        attach_request_id: None,
        source: None,
        runtime: None,
    });
    coordinator.observe_server_message(&mut output);
    let ServerMessage::TerminalOutput(output) = output else {
        unreachable!()
    };
    assert_eq!(output.runtime, Some(expected.clone()));

    let mut exit = ServerMessage::TerminalExit(TerminalExit {
        exit_code: 0,
        terminal_id: "term-1".to_string(),
        runtime: None,
    });
    coordinator.observe_server_message(&mut exit);
    let ServerMessage::TerminalExit(exit) = exit else {
        unreachable!()
    };
    assert_eq!(exit.runtime, Some(expected));
}

#[test]
fn ordinary_terminal_lifecycle_still_receives_a_runtime_fence() {
    let coordinator = RestartCoordinator::new();
    let mut created = ServerMessage::TerminalCreated(TerminalCreated {
        created_at: 1,
        request_id: "create-shell".to_string(),
        terminal_id: "shell-1".to_string(),
        clear_codex_durability: None,
        cwd: None,
        restore_error: None,
        session_ref: None,
        runtime: None,
    });

    coordinator.observe_server_message(&mut created);

    let ServerMessage::TerminalCreated(created) = created else {
        unreachable!()
    };
    assert_eq!(
        created.runtime,
        Some(RuntimeDescriptor {
            runtime_id: "shell-1".to_string(),
            generation: 1,
        })
    );
}

#[tokio::test]
async fn retired_runtime_frames_keep_the_old_generation_fence() {
    let coordinator = RestartCoordinator::new();
    let old = coordinator.register_initial(locator(), "term-1");
    coordinator
        .execute(
            restart("r1", "term-1", old.generation),
            &FakeRuntime::resumable("term-2"),
        )
        .await;

    assert_eq!(
        coordinator
            .runtime_for_live(AgentRuntimeKind::Terminal, "term-1")
            .unwrap(),
        old
    );
    assert_eq!(
        coordinator
            .runtime_for_live(AgentRuntimeKind::Terminal, "term-2")
            .unwrap(),
        RuntimeDescriptor {
            runtime_id: "term-2".to_string(),
            generation: old.generation + 1,
        }
    );
}

#[test]
fn fresh_agent_create_and_stream_frames_share_one_descriptor() {
    let coordinator = RestartCoordinator::new();
    let mut created = ServerMessage::FreshAgentCreated(FreshAgentCreated {
        provider: "claude".to_string(),
        request_id: "create-1".to_string(),
        runtime_provider: "claude".to_string(),
        session_id: "live-1".to_string(),
        session_type: "freshclaude".to_string(),
        session_ref: Some(SessionLocator {
            provider: "claude".to_string(),
            session_id: "durable-1".to_string(),
        }),
        runtime: None,
    });
    coordinator.observe_server_message(&mut created);
    let ServerMessage::FreshAgentCreated(created) = created else {
        unreachable!()
    };
    let expected = created.runtime.unwrap();

    let mut event = ServerMessage::FreshAgentEvent(FreshAgentEvent {
        event: serde_json::json!({"type": "freshAgent.stream"}),
        provider: "claude".to_string(),
        session_id: "live-1".to_string(),
        session_type: "freshclaude".to_string(),
        runtime: None,
    });
    coordinator.observe_server_message(&mut event);
    let ServerMessage::FreshAgentEvent(event) = event else {
        unreachable!()
    };
    assert_eq!(event.runtime, Some(expected));
}
