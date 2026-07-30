mod common;

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use freshell_protocol::{
    AgentRestart, AgentRestartFailureCode, AgentRuntimeKind, ClientMessage, FreshAgentCreated,
    FreshAgentEvent, FreshAgentSessionMaterialized, InventoryTerminal, PaneReconcileResult,
    PaneVerdict, ReconcileVerdict, RuntimeDescriptor, ServerMessage, SessionLocator,
    TerminalAttachReady, TerminalCreated, TerminalExit, TerminalInventory, TerminalOutput,
    TerminalRunStatus, TerminalSessionAssociated,
};
use freshell_ws::restart::{
    ProductionFreshRuntime, RestartCoordinator, RestartFailure, RestartResumeContext,
    RestartRuntime, RuntimeLocator,
};
use futures_util::SinkExt;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::Layer;

#[derive(Debug, Clone, Default)]
struct CapturedTraceEvent {
    message: String,
    fields: BTreeMap<String, String>,
}

#[derive(Default)]
struct TraceFieldVisitor {
    message: String,
    fields: BTreeMap<String, String>,
}

impl Visit for TraceFieldVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let rendered = format!("{value:?}");
        if field.name() == "message" {
            self.message = rendered;
        } else {
            self.fields.insert(field.name().to_string(), rendered);
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            self.fields
                .insert(field.name().to_string(), value.to_string());
        }
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.fields
            .insert(field.name().to_string(), value.to_string());
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.fields
            .insert(field.name().to_string(), value.to_string());
    }
}

struct TraceCaptureLayer {
    events: Arc<Mutex<Vec<CapturedTraceEvent>>>,
}

impl<S: Subscriber> Layer<S> for TraceCaptureLayer {
    fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
        let mut visitor = TraceFieldVisitor::default();
        event.record(&mut visitor);
        self.events
            .lock()
            .expect("trace capture lock")
            .push(CapturedTraceEvent {
                message: visitor.message,
                fields: visitor.fields,
            });
    }
}

fn capture_traces() -> (
    Arc<Mutex<Vec<CapturedTraceEvent>>>,
    tracing::subscriber::DefaultGuard,
) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::registry().with(TraceCaptureLayer {
        events: Arc::clone(&events),
    });
    let guard = tracing::subscriber::set_default(subscriber);
    (events, guard)
}

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

#[async_trait::async_trait]
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

#[async_trait::async_trait]
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

struct PresentSessions;

impl freshell_ws::existence::SessionExistenceProbe for PresentSessions {
    fn exists(
        &self,
        _provider: &str,
        _session_id: &str,
    ) -> freshell_ws::existence::SessionExistence {
        freshell_ws::existence::SessionExistence::Present
    }

    fn ever_observed(&self, _provider: &str, _session_id: &str) -> bool {
        true
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
fn late_terminal_association_binds_the_existing_live_runtime_to_the_durable_locator() {
    let coordinator = RestartCoordinator::new();
    let descriptor = coordinator.register_live(AgentRuntimeKind::Terminal, "term-late");
    let mut associated = ServerMessage::TerminalSessionAssociated(TerminalSessionAssociated {
        terminal_id: "term-late".to_string(),
        session_ref: SessionLocator {
            provider: "opencode".to_string(),
            session_id: "durable-late".to_string(),
        },
        previous_session_id: None,
        runtime: None,
    });

    coordinator.observe_server_message(&mut associated);

    assert_eq!(
        coordinator.runtime_for_locator(&RuntimeLocator::new(
            AgentRuntimeKind::Terminal,
            "opencode",
            "durable-late",
        )),
        Some(descriptor.clone())
    );
    let ServerMessage::TerminalSessionAssociated(associated) = associated else {
        unreachable!()
    };
    assert_eq!(associated.runtime, Some(descriptor));
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

#[tokio::test]
async fn late_association_from_retired_terminal_cannot_reclaim_the_locator() {
    let coordinator = RestartCoordinator::new();
    let old = coordinator.register_initial(locator(), "term-1");
    coordinator
        .execute(
            restart("r1", "term-1", old.generation),
            &FakeRuntime::resumable("term-2"),
        )
        .await;

    let mut late = ServerMessage::TerminalSessionAssociated(TerminalSessionAssociated {
        terminal_id: "term-1".to_string(),
        session_ref: SessionLocator {
            provider: "claude".to_string(),
            session_id: "durable-1".to_string(),
        },
        previous_session_id: None,
        runtime: None,
    });
    coordinator.observe_server_message(&mut late);

    assert_eq!(
        coordinator.runtime_for_locator(&locator()),
        Some(RuntimeDescriptor {
            runtime_id: "term-2".to_string(),
            generation: old.generation + 1,
        }),
        "a delayed association from the retired predecessor must not become current"
    );
    let ServerMessage::TerminalSessionAssociated(late) = late else {
        unreachable!()
    };
    assert_eq!(
        late.runtime,
        Some(old),
        "the delayed frame still carries the predecessor fence"
    );
}

#[tokio::test]
async fn replacement_must_report_a_distinct_live_runtime() {
    let coordinator = RestartCoordinator::new();
    coordinator.register_initial(locator(), "term-1");

    let outcome = coordinator
        .execute(
            restart("same-runtime", "term-1", 1),
            &FakeRuntime::resumable("term-1"),
        )
        .await;

    assert!(matches!(
        outcome.messages.last(),
        Some(ServerMessage::AgentRestartFailed(message))
            if message.code == AgentRestartFailureCode::ReplacementFailed
                && message.retryable
    ));
    assert_eq!(coordinator.runtime_for_locator(&locator()), None);
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

#[test]
fn negotiated_restart_descriptor_matrix_is_enriched_and_verifiable() {
    let coordinator = RestartCoordinator::new();
    let session_ref = SessionLocator {
        provider: "claude".to_string(),
        session_id: "durable-matrix".to_string(),
    };
    let mut messages = vec![
        ServerMessage::FreshAgentCreated(FreshAgentCreated {
            provider: "claude".to_string(),
            request_id: "matrix-create".to_string(),
            runtime_provider: "claude".to_string(),
            session_id: "durable-matrix".to_string(),
            session_type: "freshclaude".to_string(),
            session_ref: Some(session_ref.clone()),
            runtime: None,
        }),
        ServerMessage::FreshAgentEvent(FreshAgentEvent {
            event: serde_json::json!({"type": "freshAgent.snapshot"}),
            provider: "claude".to_string(),
            session_id: "durable-matrix".to_string(),
            session_type: "freshclaude".to_string(),
            runtime: None,
        }),
        ServerMessage::FreshAgentSessionMaterialized(FreshAgentSessionMaterialized {
            previous_session_id: "placeholder".to_string(),
            provider: "claude".to_string(),
            session_id: "durable-matrix".to_string(),
            session_type: "freshclaude".to_string(),
            session_ref: Some(session_ref.clone()),
            runtime: None,
        }),
        ServerMessage::PaneReconcileResult(PaneReconcileResult {
            reconcile_id: "matrix-reconcile".to_string(),
            boot_id: "boot-test".to_string(),
            server_instance_id: "srv-test".to_string(),
            verdicts: vec![PaneVerdict {
                pane_key: "fresh-pane".to_string(),
                verdict: ReconcileVerdict::Attach,
                terminal_id: None,
                session_ref: Some(session_ref),
                corrected: None,
                reason: None,
                duplicate: None,
                runtime: None,
            }],
        }),
    ];

    for message in &mut messages {
        coordinator.observe_server_message(message);
        assert!(
            RestartCoordinator::restart_runtime_contract_satisfied(message),
            "agentRestartV1 must make the promised runtime descriptor non-optional: {message:?}"
        );
    }
}

#[test]
fn negotiated_runtime_surface_without_an_authoritative_descriptor_is_not_forwarded() {
    let coordinator = RestartCoordinator::new();
    let missing = serde_json::to_string(&ServerMessage::FreshAgentEvent(FreshAgentEvent {
        event: serde_json::json!({"type": "freshAgent.stream", "delta": "orphan"}),
        provider: "claude".to_string(),
        session_id: "unknown-runtime".to_string(),
        session_type: "freshclaude".to_string(),
        runtime: None,
    }))
    .unwrap();

    assert!(
        coordinator.observe_serialized(&missing).is_none(),
        "the upgraded server must not violate its agentRestartV1 descriptor guarantee"
    );
}

#[test]
fn fresh_agent_replacement_uses_a_distinct_live_identity_and_fences_old_frames() {
    let coordinator = RestartCoordinator::new();
    let session_ref = SessionLocator {
        provider: "claude".to_string(),
        session_id: "durable-1".to_string(),
    };
    let old = RuntimeDescriptor {
        runtime_id: "fresh-runtime-old".to_string(),
        generation: 1,
    };
    let replacement = RuntimeDescriptor {
        runtime_id: "fresh-runtime-new".to_string(),
        generation: 2,
    };
    for (request_id, runtime) in [
        ("create-old", old.clone()),
        ("create-new", replacement.clone()),
    ] {
        let mut created = ServerMessage::FreshAgentCreated(FreshAgentCreated {
            provider: "claude".to_string(),
            request_id: request_id.to_string(),
            runtime_provider: "claude".to_string(),
            session_id: "durable-1".to_string(),
            session_type: "freshclaude".to_string(),
            session_ref: Some(session_ref.clone()),
            runtime: Some(runtime),
        });
        coordinator.observe_server_message(&mut created);
    }

    let mut queued_old = ServerMessage::FreshAgentEvent(FreshAgentEvent {
        event: serde_json::json!({"type": "freshAgent.stream", "delta": "old"}),
        provider: "claude".to_string(),
        session_id: "durable-1".to_string(),
        session_type: "freshclaude".to_string(),
        runtime: Some(old.clone()),
    });
    coordinator.observe_server_message(&mut queued_old);

    let ServerMessage::FreshAgentEvent(queued_old) = queued_old else {
        unreachable!()
    };
    assert_eq!(queued_old.runtime, Some(old));
    assert_eq!(
        coordinator.runtime_for_locator(&RuntimeLocator::new(
            AgentRuntimeKind::FreshAgent,
            "claude",
            "durable-1",
        )),
        Some(replacement)
    );
}

#[tokio::test]
async fn terminal_result_replays_after_coordinator_reopens_from_disk() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("restart-state.json");
    let coordinator = RestartCoordinator::new_persistent(path.clone()).unwrap();
    coordinator.register_initial(locator(), "term-1");
    let request = restart("durable-r1", "term-1", 1);
    coordinator
        .execute(request.clone(), &FakeRuntime::resumable("term-2"))
        .await;
    drop(coordinator);

    let reopened = RestartCoordinator::new_persistent(path).unwrap();
    let runtime = FakeRuntime::resumable("must-not-run");
    let replay = reopened.execute(request, &runtime).await;

    assert!(replay.replayed);
    assert!(matches!(
        replay.messages.as_slice(),
        [ServerMessage::AgentRestartReplaced(_)]
    ));
    assert_eq!(runtime.shutdowns.load(Ordering::SeqCst), 0);
}

struct ReplacementFails;

#[async_trait::async_trait]
impl RestartRuntime for ReplacementFails {
    type ResumePlan = ();

    async fn preflight(&self, _request: &AgentRestart) -> Result<(), RestartFailure> {
        Ok(())
    }

    async fn shutdown_for_restart(
        &self,
        _request: &AgentRestart,
        _plan: &(),
    ) -> Result<(), RestartFailure> {
        Ok(())
    }

    async fn create_replacement(
        &self,
        _request: &AgentRestart,
        _plan: (),
    ) -> Result<String, RestartFailure> {
        Err(RestartFailure::new(
            AgentRestartFailureCode::ReplacementFailed,
            "replacement failed",
            true,
        ))
    }
}

struct NonRetryableReplacementFails;

#[async_trait::async_trait]
impl RestartRuntime for NonRetryableReplacementFails {
    type ResumePlan = ();

    async fn preflight(&self, _request: &AgentRestart) -> Result<(), RestartFailure> {
        Ok(())
    }

    async fn shutdown_for_restart(
        &self,
        _request: &AgentRestart,
        _plan: &(),
    ) -> Result<(), RestartFailure> {
        Ok(())
    }

    async fn create_replacement(
        &self,
        _request: &AgentRestart,
        _plan: (),
    ) -> Result<String, RestartFailure> {
        Err(RestartFailure::new(
            AgentRestartFailureCode::ReplacementFailed,
            "replacement cannot resume this durable session",
            false,
        ))
    }
}

#[tokio::test]
async fn non_retryable_replacement_failure_logs_its_terminal_restart_outcome() {
    let coordinator = RestartCoordinator::new();
    coordinator.register_initial(locator(), "term-1");
    let (events, _trace_guard) = capture_traces();

    let outcome = coordinator
        .execute(
            restart("terminal-failure-r1", "term-1", 1),
            &NonRetryableReplacementFails,
        )
        .await;

    assert!(matches!(
        outcome.messages.as_slice(),
        [ServerMessage::AgentRestartStarted(_), ServerMessage::AgentRestartFailed(message)]
            if message.code == AgentRestartFailureCode::ReplacementFailed && !message.retryable
    ));
    let event = events
        .lock()
        .expect("trace capture lock")
        .iter()
        .find(|event| event.message == "agent.restart.replacement.failed")
        .cloned()
        .expect("non-retryable replacement failure must be logged");
    assert_eq!(
        event.fields.get("request_id").map(String::as_str),
        Some("terminal-failure-r1")
    );
    assert_eq!(
        event.fields.get("provider").map(String::as_str),
        Some("claude")
    );
    assert_eq!(
        event.fields.get("session_id").map(String::as_str),
        Some("durable-1")
    );
    assert_eq!(
        event.fields.get("runtime_id").map(String::as_str),
        Some("term-1")
    );
    assert_eq!(
        event.fields.get("generation").map(String::as_str),
        Some("1")
    );
    assert_eq!(
        event.fields.get("code").map(String::as_str),
        Some("ReplacementFailed")
    );
    assert_eq!(
        event.fields.get("retryable").map(String::as_str),
        Some("false")
    );
    assert_eq!(
        event.fields.get("error").map(String::as_str),
        Some("replacement cannot resume this durable session")
    );
}

#[tokio::test]
async fn retryable_post_shutdown_failure_is_durable_without_claiming_old_runtime_current() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("restart-state.json");
    let coordinator = RestartCoordinator::new_persistent(path.clone()).unwrap();
    coordinator.register_initial(locator(), "term-1");
    coordinator
        .execute(restart("pending-r1", "term-1", 1), &ReplacementFails)
        .await;
    assert_eq!(coordinator.runtime_for_locator(&locator()), None);
    drop(coordinator);

    let reopened = RestartCoordinator::new_persistent(path).unwrap();
    assert_eq!(reopened.runtime_for_locator(&locator()), None);
    assert_eq!(reopened.pending_recoveries().len(), 1);
    assert_eq!(reopened.pending_recoveries()[0].request_id, "pending-r1");
}

struct RecoveringReplacement {
    attempts: AtomicUsize,
}

#[async_trait::async_trait]
impl RestartRuntime for RecoveringReplacement {
    type ResumePlan = ();

    async fn preflight(&self, _request: &AgentRestart) -> Result<(), RestartFailure> {
        Ok(())
    }

    async fn shutdown_for_restart(
        &self,
        _request: &AgentRestart,
        _plan: &(),
    ) -> Result<(), RestartFailure> {
        panic!("a pending post-shutdown recovery must not tear down the old runtime twice")
    }

    async fn create_replacement(
        &self,
        _request: &AgentRestart,
        _plan: (),
    ) -> Result<String, RestartFailure> {
        self.attempts.fetch_add(1, Ordering::SeqCst);
        Ok("term-recovered".to_string())
    }
}

#[tokio::test]
async fn identical_retry_resumes_a_persisted_post_shutdown_replacement() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("restart-state.json");
    let request = restart("pending-r1", "term-1", 1);
    let coordinator = RestartCoordinator::new_persistent(path.clone()).unwrap();
    coordinator.register_initial(locator(), "term-1");
    coordinator
        .execute(request.clone(), &ReplacementFails)
        .await;
    drop(coordinator);

    let reopened = RestartCoordinator::new_persistent(path).unwrap();
    let runtime = RecoveringReplacement {
        attempts: AtomicUsize::new(0),
    };
    let recovered = reopened.execute(request.clone(), &runtime).await;

    assert!(!recovered.replayed);
    assert!(matches!(
        recovered.messages.last(),
        Some(ServerMessage::AgentRestartReplaced(message))
            if message.runtime.runtime_id == "term-recovered"
    ));
    assert_eq!(runtime.attempts.load(Ordering::SeqCst), 1);
    assert!(reopened.pending_recoveries().is_empty());

    let replay = reopened.execute(request, &runtime).await;
    assert!(replay.replayed);
    assert_eq!(runtime.attempts.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn persistence_failure_before_shutdown_fails_closed() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("restart-state.json");
    let coordinator = RestartCoordinator::new_persistent(path.clone()).unwrap();
    coordinator.register_initial(locator(), "term-1");
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    let runtime = FakeRuntime::resumable("must-not-start");

    let outcome = coordinator
        .execute(restart("persistence-r1", "term-1", 1), &runtime)
        .await;

    assert_eq!(runtime.shutdowns.load(Ordering::SeqCst), 0);
    assert!(runtime.running.load(Ordering::SeqCst));
    assert!(matches!(
        outcome.messages.as_slice(),
        [ServerMessage::AgentRestartFailed(message)]
            if message.retryable
                && message.code == AgentRestartFailureCode::PreflightFailed
    ));
}

#[tokio::test]
async fn reopened_late_terminal_association_advances_persisted_generation() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("restart-state.json");
    let coordinator = RestartCoordinator::new_persistent(path.clone()).unwrap();
    coordinator.register_initial(locator(), "term-1");
    coordinator
        .execute(
            restart("generation-r1", "term-1", 1),
            &FakeRuntime::resumable("term-2"),
        )
        .await;
    drop(coordinator);

    let reopened = RestartCoordinator::new_persistent(path).unwrap();
    reopened.register_live(AgentRuntimeKind::Terminal, "term-late");
    let mut associated = ServerMessage::TerminalSessionAssociated(TerminalSessionAssociated {
        terminal_id: "term-late".to_string(),
        session_ref: SessionLocator {
            provider: "claude".to_string(),
            session_id: "durable-1".to_string(),
        },
        previous_session_id: None,
        runtime: None,
    });
    reopened.observe_server_message(&mut associated);

    let ServerMessage::TerminalSessionAssociated(associated) = associated else {
        unreachable!()
    };
    assert_eq!(
        associated.runtime,
        Some(RuntimeDescriptor {
            runtime_id: "term-late".to_string(),
            generation: 3,
        })
    );
}

#[tokio::test]
async fn terminal_results_are_not_evicted_before_a_requester_can_replay_them() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("bounded-restart-state.json");
    let coordinator = RestartCoordinator::new_persistent_with_limits(path.clone(), 2, 2).unwrap();
    for index in 0..3 {
        let locator = RuntimeLocator::new(
            AgentRuntimeKind::Terminal,
            "claude",
            format!("durable-{index}"),
        );
        let live = format!("term-{index}");
        coordinator.register_initial(locator, &live);
        coordinator
            .execute(
                AgentRestart {
                    request_id: format!("bounded-r{index}"),
                    provider: "claude".to_string(),
                    session_id: format!("durable-{index}"),
                    kind: AgentRuntimeKind::Terminal,
                    live_id: live,
                    expected_generation: 1,
                },
                &FakeRuntime::resumable("replacement"),
            )
            .await;
    }

    assert_eq!(
        coordinator.retained_result_count(),
        3,
        "a count-based cache limit is not a replay acknowledgement or expiry protocol"
    );
    assert!(coordinator.retained_lock_counts().0 <= 2);
    assert!(coordinator.retained_lock_counts().1 <= 2);
    drop(coordinator);

    let reopened = RestartCoordinator::new_persistent_with_limits(path, 2, 2).unwrap();
    assert_eq!(reopened.retained_result_count(), 3);
    let runtime = FakeRuntime::resumable("must-not-run");
    let replay = reopened
        .execute(
            AgentRestart {
                request_id: "bounded-r0".to_string(),
                provider: "claude".to_string(),
                session_id: "durable-0".to_string(),
                kind: AgentRuntimeKind::Terminal,
                live_id: "term-0".to_string(),
                expected_generation: 1,
            },
            &runtime,
        )
        .await;
    assert!(replay.replayed);
    assert_eq!(runtime.shutdowns.load(Ordering::SeqCst), 0);
}

struct RecoveryMustNotSpawn {
    attempts: AtomicUsize,
}

#[async_trait::async_trait]
impl RestartRuntime for RecoveryMustNotSpawn {
    type ResumePlan = ();

    async fn preflight(&self, _request: &AgentRestart) -> Result<(), RestartFailure> {
        panic!("an already registered replacement must be adopted before recovery preflight")
    }

    async fn shutdown_for_restart(
        &self,
        _request: &AgentRestart,
        _plan: &(),
    ) -> Result<(), RestartFailure> {
        panic!("the retired runtime must not be shut down twice")
    }

    async fn create_replacement(
        &self,
        _request: &AgentRestart,
        _plan: (),
    ) -> Result<String, RestartFailure> {
        self.attempts.fetch_add(1, Ordering::SeqCst);
        panic!("an already registered replacement must not be spawned twice")
    }

    async fn recover_replacement(
        &self,
        _request: &AgentRestart,
        _context: Option<&RestartResumeContext>,
    ) -> Result<String, RestartFailure> {
        self.attempts.fetch_add(1, Ordering::SeqCst);
        panic!("an already registered replacement must not be spawned twice")
    }
}

struct ReplacementBreaksDurableCommit {
    coordinator: RestartCoordinator,
    persistence_path: std::path::PathBuf,
    creates: AtomicUsize,
}

#[async_trait::async_trait]
impl RestartRuntime for ReplacementBreaksDurableCommit {
    type ResumePlan = ();

    async fn preflight(&self, _request: &AgentRestart) -> Result<(), RestartFailure> {
        Ok(())
    }

    async fn shutdown_for_restart(
        &self,
        _request: &AgentRestart,
        _plan: &(),
    ) -> Result<(), RestartFailure> {
        Ok(())
    }

    async fn create_replacement(
        &self,
        _request: &AgentRestart,
        _plan: (),
    ) -> Result<String, RestartFailure> {
        self.creates.fetch_add(1, Ordering::SeqCst);
        std::fs::remove_file(&self.persistence_path).unwrap();
        std::fs::create_dir(&self.persistence_path).unwrap();
        self.coordinator
            .register_initial(locator(), "term-ambiguous-success");
        Ok("term-ambiguous-success".to_string())
    }
}

#[tokio::test]
async fn pending_retry_adopts_registered_replacement_after_ambiguous_durable_commit() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("restart-state.json");
    let coordinator = RestartCoordinator::new_persistent(path.clone()).unwrap();
    coordinator.register_initial(locator(), "term-1");
    let request = restart("ambiguous-r1", "term-1", 1);
    let first_runtime = ReplacementBreaksDurableCommit {
        coordinator: coordinator.clone(),
        persistence_path: path.clone(),
        creates: AtomicUsize::new(0),
    };

    let first = coordinator.execute(request.clone(), &first_runtime).await;
    assert!(matches!(
        first.messages.last(),
        Some(ServerMessage::AgentRestartFailed(message)) if message.retryable
    ));
    assert_eq!(first_runtime.creates.load(Ordering::SeqCst), 1);
    assert_eq!(
        coordinator.runtime_for_locator(&locator()),
        Some(RuntimeDescriptor {
            runtime_id: "term-ambiguous-success".to_string(),
            generation: 2,
        })
    );

    std::fs::remove_dir(&path).unwrap();
    let retry_runtime = RecoveryMustNotSpawn {
        attempts: AtomicUsize::new(0),
    };
    let retry = coordinator.execute(request.clone(), &retry_runtime).await;

    assert!(matches!(
        retry.messages.last(),
        Some(ServerMessage::AgentRestartReplaced(message))
            if message.runtime.runtime_id == "term-ambiguous-success"
                && message.runtime.generation == 2
    ));
    assert_eq!(retry_runtime.attempts.load(Ordering::SeqCst), 0);
    assert!(coordinator.pending_recoveries().is_empty());

    let replay = coordinator.execute(request, &retry_runtime).await;
    assert!(replay.replayed);
    assert_eq!(retry_runtime.attempts.load(Ordering::SeqCst), 0);
}

#[test]
fn coordinator_bounds_runtime_identity_and_generation_retention() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("bounded-ownership-state.json");
    let coordinator = RestartCoordinator::new_persistent_with_limits(path.clone(), 2, 2).unwrap();

    for index in 0..8 {
        let locator = RuntimeLocator::new(
            AgentRuntimeKind::FreshAgent,
            "claude",
            format!("durable-{index}"),
        );
        let descriptor = coordinator.register_initial(locator, format!("fresh-{index}"));
        let mut created = ServerMessage::FreshAgentCreated(FreshAgentCreated {
            provider: "claude".to_string(),
            request_id: format!("create-{index}"),
            runtime_provider: "claude".to_string(),
            session_id: format!("alias-{index}"),
            session_type: "freshclaude".to_string(),
            session_ref: Some(SessionLocator {
                provider: "claude".to_string(),
                session_id: format!("durable-{index}"),
            }),
            runtime: Some(descriptor),
        });
        coordinator.observe_server_message(&mut created);
    }

    let counts = coordinator.retained_ownership_counts();
    assert!(counts.descriptors <= 2, "{counts:?}");
    assert!(counts.live_locators <= 2, "{counts:?}");
    assert!(counts.current_locators <= 2, "{counts:?}");
    assert!(counts.fresh_aliases <= 2, "{counts:?}");
    assert!(counts.generation_high_waters <= 2, "{counts:?}");
    drop(coordinator);

    let reopened = RestartCoordinator::new_persistent_with_limits(path, 2, 2).unwrap();
    assert!(reopened.retained_ownership_counts().generation_high_waters <= 2);
}

#[tokio::test]
async fn retired_descriptor_fences_late_frames_only_for_the_bounded_window() {
    let coordinator = RestartCoordinator::new_with_limits(2, 2);
    let old = coordinator.register_initial(locator(), "term-old");
    coordinator
        .execute(
            restart("bounded-fence", "term-old", old.generation),
            &FakeRuntime::resumable("term-new"),
        )
        .await;
    assert_eq!(
        coordinator.runtime_for_live(AgentRuntimeKind::Terminal, "term-old"),
        Some(old)
    );

    coordinator.register_live(AgentRuntimeKind::Terminal, "unrelated");

    assert_eq!(
        coordinator.runtime_for_live(AgentRuntimeKind::Terminal, "term-old"),
        None,
        "the oldest retired fence is eventually evicted instead of leaking forever"
    );
}

struct ContextRecoveryRuntime {
    expected_cwd: String,
}

#[async_trait::async_trait]
impl RestartRuntime for ContextRecoveryRuntime {
    type ResumePlan = ();

    async fn preflight(&self, _request: &AgentRestart) -> Result<(), RestartFailure> {
        Ok(())
    }

    async fn shutdown_for_restart(
        &self,
        _request: &AgentRestart,
        _plan: &(),
    ) -> Result<(), RestartFailure> {
        Ok(())
    }

    async fn create_replacement(
        &self,
        _request: &AgentRestart,
        _plan: (),
    ) -> Result<String, RestartFailure> {
        Err(RestartFailure::new(
            AgentRestartFailureCode::ReplacementFailed,
            "restart server before replacement",
            true,
        ))
    }

    fn persisted_resume_context(
        &self,
        _request: &AgentRestart,
        _plan: &(),
    ) -> Option<RestartResumeContext> {
        Some(RestartResumeContext {
            terminal_cwd: Some(self.expected_cwd.clone()),
        })
    }

    async fn recover_replacement(
        &self,
        _request: &AgentRestart,
        context: Option<&RestartResumeContext>,
    ) -> Result<String, RestartFailure> {
        assert_eq!(
            context.and_then(|context| context.terminal_cwd.as_deref()),
            Some(self.expected_cwd.as_str())
        );
        Ok("term-context-recovered".to_string())
    }
}

#[tokio::test]
async fn terminal_resume_context_survives_boot_recovery() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("restart-state.json");
    let request = restart("context-r1", "term-1", 1);
    let runtime = ContextRecoveryRuntime {
        expected_cwd: "/workspace/project".to_string(),
    };
    let coordinator = RestartCoordinator::new_persistent(path.clone()).unwrap();
    coordinator.register_initial(locator(), "term-1");
    coordinator.execute(request.clone(), &runtime).await;
    drop(coordinator);

    let reopened = RestartCoordinator::new_persistent(path).unwrap();
    let recovered = reopened.execute(request, &runtime).await;

    assert!(matches!(
        recovered.messages.last(),
        Some(ServerMessage::AgentRestartReplaced(message))
            if message.runtime.runtime_id == "term-context-recovered"
    ));
}

#[tokio::test]
async fn production_boot_recovery_recreates_terminal_with_persisted_cwd() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("restart-state.json");
    let cwd = temp.path().join("restored-working-directory");
    std::fs::create_dir(&cwd).unwrap();
    let locator = RuntimeLocator::new(AgentRuntimeKind::Terminal, "amplifier", "durable-context");
    let request = AgentRestart {
        request_id: "production-context-r1".to_string(),
        provider: "amplifier".to_string(),
        session_id: "durable-context".to_string(),
        kind: AgentRuntimeKind::Terminal,
        live_id: "term-context-old".to_string(),
        expected_generation: 1,
    };
    let failing = ContextRecoveryRuntime {
        expected_cwd: cwd.to_string_lossy().into_owned(),
    };
    let coordinator = RestartCoordinator::new_persistent(path.clone()).unwrap();
    coordinator.register_initial(locator, &request.live_id);
    coordinator.execute(request.clone(), &failing).await;
    drop(coordinator);

    let (_url, registry, mut state) =
        common::spawn_server_with_specs_and_state(vec![common::sleeper_cli_spec("amplifier")])
            .await;
    state.session_existence = Arc::new(PresentSessions);
    let reopened = RestartCoordinator::new_persistent(path).unwrap();
    let production = freshell_ws::restart::ProductionRestartRuntime::new(state);
    let recovered = reopened.execute(request, &production).await;
    let ServerMessage::AgentRestartReplaced(replaced) =
        recovered.messages.last().expect("terminal result")
    else {
        panic!("expected production replacement: {:?}", recovered.messages)
    };
    let probe = registry
        .probe(&replaced.runtime.runtime_id)
        .expect("replacement terminal is live");
    assert_eq!(probe.cwd.as_deref(), cwd.to_str());
    registry.kill_all();
}

struct ConcurrentRuntime {
    entered: Arc<tokio::sync::Barrier>,
    replacement: &'static str,
}

#[async_trait::async_trait]
impl RestartRuntime for ConcurrentRuntime {
    type ResumePlan = ();

    async fn preflight(&self, _request: &AgentRestart) -> Result<(), RestartFailure> {
        self.entered.wait().await;
        Ok(())
    }

    async fn shutdown_for_restart(
        &self,
        _request: &AgentRestart,
        _plan: &(),
    ) -> Result<(), RestartFailure> {
        Ok(())
    }

    async fn create_replacement(
        &self,
        _request: &AgentRestart,
        _plan: (),
    ) -> Result<String, RestartFailure> {
        Ok(self.replacement.to_string())
    }
}

#[tokio::test]
async fn unrelated_provider_runtimes_restart_concurrently() {
    let coordinator = RestartCoordinator::new();
    let second_locator = RuntimeLocator::new(AgentRuntimeKind::Terminal, "codex", "durable-2");
    coordinator.register_initial(locator(), "term-1");
    coordinator.register_initial(second_locator, "term-2");
    let entered = Arc::new(tokio::sync::Barrier::new(2));
    let first = ConcurrentRuntime {
        entered: Arc::clone(&entered),
        replacement: "term-1b",
    };
    let second = ConcurrentRuntime {
        entered,
        replacement: "term-2b",
    };
    let second_request = AgentRestart {
        request_id: "r2".to_string(),
        provider: "codex".to_string(),
        session_id: "durable-2".to_string(),
        kind: AgentRuntimeKind::Terminal,
        live_id: "term-2".to_string(),
        expected_generation: 1,
    };

    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        tokio::join!(
            coordinator.execute(restart("r1", "term-1", 1), &first),
            coordinator.execute(second_request, &second),
        );
    })
    .await
    .expect("unrelated runtime locks must not serialize each other");
}

#[tokio::test]
async fn connected_websocket_dispatches_restart_started_replaced_and_failed() {
    let (url, registry, state) = common::spawn_server_with_specs_and_state(vec![]).await;
    let _runtime_registration = state
        .restart
        .set_runtime(Arc::new(FakeRuntime::resumable("term-ws-2")));
    state.restart.register_initial(locator(), "term-ws-1");
    let (mut ws, _) = common::connect_and_capture_inventory_with_capabilities(
        &url,
        Some(serde_json::json!({ "agentRestartV1": true })),
    )
    .await;

    ws.send(WsMessage::Text(
        serde_json::to_string(&ClientMessage::AgentRestart(restart(
            "ws-r1",
            "term-ws-1",
            1,
        )))
        .unwrap(),
    ))
    .await
    .unwrap();
    let started = common::next_frame_of_type(&mut ws, "agent.restart.started").await;
    let replaced = common::next_frame_of_type(&mut ws, "agent.restart.replaced").await;
    assert_eq!(started["requestId"], "ws-r1");
    assert_eq!(replaced["runtimeId"], "term-ws-2");

    let failed_locator =
        RuntimeLocator::new(AgentRuntimeKind::Terminal, "claude", "durable-failed");
    state
        .restart
        .register_initial(failed_locator, "term-ws-failed");
    let _runtime_registration = state
        .restart
        .set_runtime(Arc::new(FakeRuntime::unresumable()));
    let failed_request = AgentRestart {
        request_id: "ws-r2".to_string(),
        provider: "claude".to_string(),
        session_id: "durable-failed".to_string(),
        kind: AgentRuntimeKind::Terminal,
        live_id: "term-ws-failed".to_string(),
        expected_generation: 1,
    };
    ws.send(WsMessage::Text(
        serde_json::to_string(&ClientMessage::AgentRestart(failed_request)).unwrap(),
    ))
    .await
    .unwrap();
    let failed = common::next_frame_of_type(&mut ws, "agent.restart.failed").await;
    assert_eq!(failed["requestId"], "ws-r2");
    assert_eq!(failed["code"], "UNRESUMABLE");

    registry.kill_all();
}

#[tokio::test]
async fn v7_connection_without_restart_negotiation_receives_a_correlated_failure() {
    let (url, registry, state) = common::spawn_server_with_specs_and_state(vec![]).await;
    let runtime = Arc::new(FakeRuntime::resumable("must-not-start"));
    let _runtime_registration = state.restart.set_runtime(runtime.clone());
    state
        .restart
        .register_initial(locator(), "term-unnegotiated");
    let (mut ws, _) = common::connect_and_capture_inventory(&url).await;

    ws.send(WsMessage::Text(
        serde_json::to_string(&ClientMessage::AgentRestart(restart(
            "unnegotiated-r1",
            "term-unnegotiated",
            1,
        )))
        .unwrap(),
    ))
    .await
    .unwrap();

    let failed = common::next_frame_of_type(&mut ws, "agent.restart.failed").await;
    assert_eq!(failed["requestId"], "unnegotiated-r1");
    assert_eq!(failed["runtimeId"], "term-unnegotiated");
    assert_eq!(failed["generation"], 1);
    assert_eq!(failed["code"], "CAPABILITY_NOT_NEGOTIATED");
    assert_eq!(failed["retryable"], false);
    assert_eq!(runtime.shutdowns.load(Ordering::SeqCst), 0);
    registry.kill_all();
}

#[tokio::test]
async fn production_terminal_adapter_uses_the_builtin_restore_path() {
    let (url, registry, mut state) =
        common::spawn_server_with_specs_and_state(vec![common::sleeper_cli_spec("amplifier")])
            .await;
    state.session_existence = Arc::new(PresentSessions);
    let _runtime_registration = state.restart.set_runtime(Arc::new(
        freshell_ws::restart::ProductionRestartRuntime::new(state.clone()),
    ));
    let (mut ws, _) = common::connect_and_capture_inventory_with_capabilities(
        &url,
        Some(serde_json::json!({ "agentRestartV1": true })),
    )
    .await;
    ws.send(WsMessage::Text(
        serde_json::json!({
            "type": "terminal.create",
            "requestId": "production-adapter-create",
            "mode": "amplifier",
            "shell": "system",
            "cwd": std::env::temp_dir().to_string_lossy(),
        })
        .to_string(),
    ))
    .await
    .unwrap();
    let created = common::next_frame_of_type(&mut ws, "terminal.created").await;
    let terminal_id = created["terminalId"].as_str().unwrap().to_string();
    let session_id = created["sessionRef"]["sessionId"]
        .as_str()
        .unwrap()
        .to_string();
    let generation = created["runtime"]["generation"].as_u64().unwrap();

    ws.send(WsMessage::Text(
        serde_json::to_string(&ClientMessage::AgentRestart(AgentRestart {
            request_id: "production-adapter-restart".to_string(),
            provider: "amplifier".to_string(),
            session_id,
            kind: AgentRuntimeKind::Terminal,
            live_id: terminal_id.clone(),
            expected_generation: generation,
        }))
        .unwrap(),
    ))
    .await
    .unwrap();
    common::next_frame_of_type(&mut ws, "agent.restart.started").await;
    let replaced = common::next_frame_of_type(&mut ws, "agent.restart.replaced").await;
    let replacement_id = replaced["runtimeId"].as_str().unwrap();

    assert_ne!(replacement_id, terminal_id);
    assert!(!registry.is_live(&terminal_id));
    assert!(registry.is_live(replacement_id));
    registry.kill_all();
}

struct FaithfulFreshRuntime {
    coordinator: RestartCoordinator,
    broadcast_tx: Arc<tokio::sync::broadcast::Sender<String>>,
    live: Mutex<HashMap<(String, String), String>>,
    events: Mutex<Vec<String>>,
}

fn provider_name(provider: freshell_protocol::AgentProvider) -> &'static str {
    match provider {
        freshell_protocol::AgentProvider::Claude => "claude",
        freshell_protocol::AgentProvider::Codex => "codex",
        freshell_protocol::AgentProvider::Opencode => "opencode",
        freshell_protocol::AgentProvider::Amplifier => "amplifier",
    }
}

#[async_trait::async_trait]
impl ProductionFreshRuntime for FaithfulFreshRuntime {
    async fn has_live_session(
        &self,
        provider: freshell_protocol::AgentProvider,
        session_id: &str,
    ) -> bool {
        let provider = provider_name(provider);
        self.events
            .lock()
            .unwrap()
            .push(format!("preflight:{provider}:{session_id}"));
        self.live
            .lock()
            .unwrap()
            .contains_key(&(provider.to_string(), session_id.to_string()))
    }

    async fn shutdown_for_restart(
        &self,
        provider: freshell_protocol::AgentProvider,
        session_id: &str,
        expected_runtime_id: &str,
    ) -> bool {
        let provider = provider_name(provider);
        self.events.lock().unwrap().push(format!(
            "shutdown:{provider}:{session_id}:{expected_runtime_id}"
        ));
        let mut live = self.live.lock().unwrap();
        let key = (provider.to_string(), session_id.to_string());
        if live.get(&key).map(String::as_str) != Some(expected_runtime_id) {
            return false;
        }
        live.remove(&key);
        true
    }

    async fn handle_create(
        &self,
        provider: freshell_protocol::AgentProvider,
        create: freshell_protocol::FreshAgentCreate,
    ) {
        let provider = provider_name(provider);
        let durable_id = create
            .resume_session_id
            .as_deref()
            .expect("production restart must use the resume path");
        self.events
            .lock()
            .unwrap()
            .push(format!("resume:{provider}:{durable_id}"));
        let runtime_id = format!("fresh-{provider}-replacement");
        let runtime = self.coordinator.register_initial(
            RuntimeLocator::new(AgentRuntimeKind::FreshAgent, provider, durable_id),
            &runtime_id,
        );
        self.live
            .lock()
            .unwrap()
            .insert((provider.to_string(), durable_id.to_string()), runtime_id);
        let created = ServerMessage::FreshAgentCreated(FreshAgentCreated {
            provider: provider.to_string(),
            request_id: create.request_id,
            runtime_provider: provider.to_string(),
            session_id: durable_id.to_string(),
            session_type: match provider {
                "claude" => "freshclaude",
                "codex" => "freshcodex",
                "opencode" => "freshopencode",
                _ => unreachable!(),
            }
            .to_string(),
            session_ref: create.session_ref,
            runtime: Some(runtime),
        });
        self.broadcast_tx
            .send(serde_json::to_string(&created).unwrap())
            .unwrap();
    }
}

#[tokio::test]
async fn production_fresh_adapters_preflight_exact_teardown_and_resume_all_providers() {
    for provider in ["claude", "codex", "opencode"] {
        let (_url, registry, mut state) = common::spawn_server_with_specs_and_state(vec![]).await;
        state.session_existence = Arc::new(PresentSessions);
        let durable_id = format!("durable-{provider}");
        let old_runtime_id = format!("fresh-{provider}-old");
        let locator =
            RuntimeLocator::new(AgentRuntimeKind::FreshAgent, provider, durable_id.clone());
        let old = state
            .restart
            .register_initial(locator.clone(), &old_runtime_id);
        let faithful = Arc::new(FaithfulFreshRuntime {
            coordinator: state.restart.clone(),
            broadcast_tx: Arc::clone(&state.broadcast_tx),
            live: Mutex::new(HashMap::from([(
                (provider.to_string(), durable_id.clone()),
                old_runtime_id.clone(),
            )])),
            events: Mutex::new(Vec::new()),
        });
        let production = freshell_ws::restart::ProductionRestartRuntime::with_fresh_runtime(
            state.clone(),
            faithful.clone(),
        );
        let request = AgentRestart {
            request_id: format!("production-{provider}-restart"),
            provider: provider.to_string(),
            session_id: durable_id.clone(),
            kind: AgentRuntimeKind::FreshAgent,
            live_id: old_runtime_id.clone(),
            expected_generation: old.generation,
        };

        let outcome = state.restart.execute(request, &production).await;

        let ServerMessage::AgentRestartReplaced(replaced) =
            outcome.messages.last().expect("terminal result")
        else {
            panic!("expected {provider} replacement: {:?}", outcome.messages)
        };
        assert_eq!(
            replaced.runtime,
            RuntimeDescriptor {
                runtime_id: format!("fresh-{provider}-replacement"),
                generation: old.generation + 1,
            }
        );
        assert_eq!(
            state.restart.runtime_for_locator(&locator),
            Some(replaced.runtime.clone())
        );
        assert_eq!(
            faithful.events.lock().unwrap().as_slice(),
            [
                format!("preflight:{provider}:{durable_id}"),
                format!("shutdown:{provider}:{durable_id}:{old_runtime_id}"),
                format!("resume:{provider}:{durable_id}"),
                format!("preflight:{provider}:{durable_id}"),
            ],
            "{provider} must use preflight, exact-runtime teardown, and durable resume"
        );
        registry.kill_all();
    }
}

#[tokio::test]
async fn boot_recovery_runs_through_the_registered_adapter_and_persists_result() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("restart-state.json");
    let request = restart("boot-pending-r1", "term-1", 1);
    let coordinator = RestartCoordinator::new_persistent(path.clone()).unwrap();
    coordinator.register_initial(locator(), "term-1");
    coordinator
        .execute(request.clone(), &ReplacementFails)
        .await;
    drop(coordinator);

    let reopened = RestartCoordinator::new_persistent(path).unwrap();
    let runtime = Arc::new(RecoveringReplacement {
        attempts: AtomicUsize::new(0),
    });
    let _runtime_registration = reopened.set_runtime(runtime.clone());
    let emitted = Mutex::new(Vec::new());
    reopened
        .recover_pending_registered(|message| emitted.lock().unwrap().push(message.clone()))
        .await;

    assert_eq!(runtime.attempts.load(Ordering::SeqCst), 1);
    assert!(reopened.pending_recoveries().is_empty());
    assert!(matches!(
        emitted.lock().unwrap().last(),
        Some(ServerMessage::AgentRestartReplaced(_))
    ));
    let replay = reopened.execute_registered(request, |_| {}).await;
    assert!(replay.replayed);
}

#[tokio::test]
async fn fresh_agent_runtime_is_registered_before_created_broadcast() {
    let coordinator = RestartCoordinator::new();
    let (broadcast_tx, mut broadcast_rx) = tokio::sync::broadcast::channel(16);
    let auth = Arc::new("test-token".to_string());
    let state = freshell_freshagent::FreshOpencodeState::new(
        freshell_freshagent::FreshAgentState::new(auth, Arc::new(broadcast_tx)),
    );
    state.set_runtime_registry(Arc::new(coordinator.clone()));
    let create: freshell_protocol::FreshAgentCreate = serde_json::from_value(serde_json::json!({
        "requestId": "fresh-create-1",
        "sessionType": "freshopencode",
        "provider": "opencode"
    }))
    .unwrap();

    state.handle_create(create).await;

    let raw = broadcast_rx.recv().await.unwrap();
    let created: ServerMessage = serde_json::from_str(&raw).unwrap();
    let ServerMessage::FreshAgentCreated(created) = created else {
        panic!("expected freshAgent.created")
    };
    let runtime = created.runtime.expect("runtime is stamped at creation");
    assert!(runtime.runtime_id.starts_with("fresh-runtime-"));
    assert_eq!(
        coordinator.runtime_for_locator(&RuntimeLocator::new(
            AgentRuntimeKind::FreshAgent,
            "opencode",
            "freshopencode-fresh-create-1",
        )),
        Some(runtime)
    );
}
