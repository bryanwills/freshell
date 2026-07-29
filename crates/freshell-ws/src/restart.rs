//! Provider-agnostic runtime ownership and restart transaction coordinator.
//!
//! Provider teardown/resume adapters live outside this module. This module owns
//! the cross-provider invariants: immutable request fingerprints, live runtime
//! generations, preflight-before-shutdown ordering, and terminal-result replay.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use freshell_protocol::{
    AgentRestart, AgentRestartFailed, AgentRestartFailureCode, AgentRestartReplaced,
    AgentRestartStarted, AgentRuntimeKind, OldRuntimeDescriptor, RuntimeDescriptor, ServerMessage,
};

/// Canonical durable identity of one restartable runtime.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RuntimeLocator {
    pub kind: AgentRuntimeKind,
    pub provider: String,
    pub session_id: String,
}

impl RuntimeLocator {
    pub fn new(
        kind: AgentRuntimeKind,
        provider: impl Into<String>,
        session_id: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            provider: provider.into(),
            session_id: session_id.into(),
        }
    }

    fn from_request(request: &AgentRestart) -> Self {
        Self::new(
            request.kind,
            request.provider.clone(),
            request.session_id.clone(),
        )
    }
}

#[derive(Debug, Clone)]
struct StoredResult {
    fingerprint: AgentRestart,
    terminal: ServerMessage,
}

#[derive(Debug, Default)]
struct RuntimeOwnership {
    by_locator: HashMap<RuntimeLocator, RuntimeDescriptor>,
    /// Exact descriptor for every observed live runtime id, including retired
    /// generations whose already-queued output/exit frames still need fencing.
    descriptor_by_live: HashMap<(AgentRuntimeKind, String), RuntimeDescriptor>,
    results: HashMap<String, StoredResult>,
}

/// Error returned by a provider-specific restart adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartFailure {
    pub code: AgentRestartFailureCode,
    pub message: String,
    pub retryable: bool,
}

impl RestartFailure {
    pub fn new(code: AgentRestartFailureCode, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code,
            message: message.into(),
            retryable,
        }
    }
}

/// Existing provider-specific resume paths implement this seam in Task 2.
#[allow(async_fn_in_trait)]
pub trait RestartRuntime: Send + Sync {
    type ResumePlan: Send;

    /// Validate durable resume eligibility and capture every setting needed by
    /// the eventual replacement. This always runs while the old runtime lives.
    async fn preflight(&self, request: &AgentRestart) -> Result<Self::ResumePlan, RestartFailure>;

    /// Quiesce only the selected live runtime without final-close semantics.
    async fn shutdown_for_restart(
        &self,
        request: &AgentRestart,
        plan: &Self::ResumePlan,
    ) -> Result<(), RestartFailure>;

    /// Create the one server-owned replacement through the normal resume path.
    async fn create_replacement(
        &self,
        request: &AgentRestart,
        plan: Self::ResumePlan,
    ) -> Result<String, RestartFailure>;
}

/// Messages to broadcast for one request. A replay contains only the stored
/// terminal result (`replaced` or `failed`), never a second `started`.
#[derive(Debug, Clone)]
pub struct RestartOutcome {
    pub messages: Vec<ServerMessage>,
    pub replayed: bool,
}

/// One server-owned registry for runtime descriptors and restart results.
#[derive(Clone, Default)]
pub struct RestartCoordinator {
    ownership: Arc<Mutex<RuntimeOwnership>>,
    execution: Arc<tokio::sync::Mutex<()>>,
}

impl RestartCoordinator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a runtime discovered by create/attach/inventory/reconciliation.
    /// Re-observing the same live id is stable; observing a new live id for the
    /// same durable locator advances the generation exactly once.
    pub fn register_initial(
        &self,
        locator: RuntimeLocator,
        runtime_id: impl Into<String>,
    ) -> RuntimeDescriptor {
        let runtime_id = runtime_id.into();
        let mut ownership = self.ownership.lock().expect("restart ownership lock");
        if let Some(existing) = ownership.by_locator.get(&locator) {
            if existing.runtime_id == runtime_id {
                return existing.clone();
            }
        }
        if let Some(existing) = ownership
            .descriptor_by_live
            .get(&(locator.kind, runtime_id.clone()))
            .cloned()
        {
            ownership.by_locator.insert(locator, existing.clone());
            return existing;
        }
        let generation = ownership
            .by_locator
            .get(&locator)
            .map_or(1, |current| current.generation.saturating_add(1));
        let descriptor = RuntimeDescriptor {
            runtime_id: runtime_id.clone(),
            generation,
        };
        ownership
            .descriptor_by_live
            .insert((locator.kind, runtime_id), descriptor.clone());
        ownership.by_locator.insert(locator, descriptor.clone());
        descriptor
    }

    /// Register a runtime that does not have a durable locator. This keeps the
    /// lifecycle contract fenced even for ordinary shell terminals, while a
    /// later durable observation can bind the same stable descriptor.
    pub fn register_live(
        &self,
        kind: AgentRuntimeKind,
        runtime_id: impl Into<String>,
    ) -> RuntimeDescriptor {
        let runtime_id = runtime_id.into();
        let mut ownership = self.ownership.lock().expect("restart ownership lock");
        ownership
            .descriptor_by_live
            .entry((kind, runtime_id.clone()))
            .or_insert_with(|| RuntimeDescriptor {
                runtime_id,
                generation: 1,
            })
            .clone()
    }

    /// Descriptor lookup used to stamp runtime-addressed lifecycle surfaces.
    pub fn runtime_for_live(
        &self,
        kind: AgentRuntimeKind,
        runtime_id: &str,
    ) -> Option<RuntimeDescriptor> {
        let ownership = self.ownership.lock().expect("restart ownership lock");
        ownership
            .descriptor_by_live
            .get(&(kind, runtime_id.to_string()))
            .cloned()
    }

    pub fn runtime_for_locator(&self, locator: &RuntimeLocator) -> Option<RuntimeDescriptor> {
        self.ownership
            .lock()
            .expect("restart ownership lock")
            .by_locator
            .get(locator)
            .cloned()
    }

    /// Stamp one typed lifecycle frame with this registry's descriptor. This
    /// is the single enrichment path used by handshake, direct terminal
    /// frames, registry fan-out, reconciliation, and fresh-agent broadcasts.
    pub fn observe_server_message(&self, message: &mut ServerMessage) {
        match message {
            ServerMessage::TerminalCreated(frame) => {
                frame.runtime = self.observe_terminal(
                    &frame.terminal_id,
                    frame.session_ref.as_ref(),
                    frame.runtime.as_ref(),
                );
            }
            ServerMessage::TerminalAttachReady(frame) => {
                frame.runtime = self.observe_terminal(
                    &frame.terminal_id,
                    frame.session_ref.as_ref(),
                    frame.runtime.as_ref(),
                );
            }
            ServerMessage::TerminalInventory(frame) => {
                for terminal in &mut frame.terminals {
                    terminal.runtime = if terminal.status
                        == freshell_protocol::TerminalRunStatus::Running
                    {
                        self.observe_terminal(
                            &terminal.terminal_id,
                            terminal.session_ref.as_ref(),
                            terminal.runtime.as_ref(),
                        )
                    } else {
                        terminal.runtime.clone().or_else(|| {
                            self.runtime_for_live(AgentRuntimeKind::Terminal, &terminal.terminal_id)
                        })
                    };
                }
            }
            ServerMessage::TerminalOutput(frame) => {
                frame.runtime = frame.runtime.clone().or_else(|| {
                    self.runtime_for_live(AgentRuntimeKind::Terminal, &frame.terminal_id)
                });
            }
            ServerMessage::TerminalOutputBatch(frame) => {
                frame.runtime = frame.runtime.clone().or_else(|| {
                    self.runtime_for_live(AgentRuntimeKind::Terminal, &frame.terminal_id)
                });
            }
            ServerMessage::TerminalOutputGap(frame) => {
                frame.runtime = frame.runtime.clone().or_else(|| {
                    self.runtime_for_live(AgentRuntimeKind::Terminal, &frame.terminal_id)
                });
            }
            ServerMessage::TerminalExit(frame) => {
                frame.runtime = frame.runtime.clone().or_else(|| {
                    self.runtime_for_live(AgentRuntimeKind::Terminal, &frame.terminal_id)
                });
            }
            ServerMessage::FreshAgentCreated(frame) => {
                let locator = frame
                    .session_ref
                    .as_ref()
                    .map(|session| {
                        RuntimeLocator::new(
                            AgentRuntimeKind::FreshAgent,
                            &session.provider,
                            &session.session_id,
                        )
                    })
                    .unwrap_or_else(|| {
                        RuntimeLocator::new(
                            AgentRuntimeKind::FreshAgent,
                            &frame.provider,
                            &frame.session_id,
                        )
                    });
                frame.runtime = Some(self.register_initial(locator, &frame.session_id));
            }
            ServerMessage::FreshAgentSessionMaterialized(frame) => {
                let locator = frame
                    .session_ref
                    .as_ref()
                    .map(|session| {
                        RuntimeLocator::new(
                            AgentRuntimeKind::FreshAgent,
                            &session.provider,
                            &session.session_id,
                        )
                    })
                    .unwrap_or_else(|| {
                        RuntimeLocator::new(
                            AgentRuntimeKind::FreshAgent,
                            &frame.provider,
                            &frame.session_id,
                        )
                    });
                frame.runtime = Some(self.register_initial(locator, &frame.session_id));
            }
            ServerMessage::FreshAgentEvent(frame) => {
                frame.runtime = frame.runtime.clone().or_else(|| {
                    self.runtime_for_live(AgentRuntimeKind::FreshAgent, &frame.session_id)
                        .or_else(|| {
                            Some(self.register_initial(
                                RuntimeLocator::new(
                                    AgentRuntimeKind::FreshAgent,
                                    &frame.provider,
                                    &frame.session_id,
                                ),
                                &frame.session_id,
                            ))
                        })
                });
            }
            ServerMessage::PaneReconcileResult(frame) => {
                for verdict in &mut frame.verdicts {
                    verdict.runtime = verdict.runtime.clone().or_else(|| {
                        if let Some(terminal_id) = verdict.terminal_id.as_deref() {
                            return self.observe_terminal(
                                terminal_id,
                                verdict.session_ref.as_ref(),
                                None,
                            );
                        }
                        verdict.session_ref.as_ref().and_then(|session| {
                            self.runtime_for_locator(&RuntimeLocator::new(
                                AgentRuntimeKind::FreshAgent,
                                &session.provider,
                                &session.session_id,
                            ))
                        })
                    });
                }
            }
            _ => {}
        }
    }

    /// Enrich a pre-serialized broadcast frame when it belongs to the typed
    /// protocol. Unknown extension frames pass through byte-for-byte.
    pub fn observe_serialized(&self, json: &str) -> String {
        let Ok(envelope) = serde_json::from_str::<serde_json::Value>(json) else {
            return json.to_string();
        };
        if !matches!(
            envelope.get("type").and_then(serde_json::Value::as_str),
            Some("freshAgent.created" | "freshAgent.event" | "freshAgent.session.materialized")
        ) {
            return json.to_string();
        }
        let Ok(mut message) = serde_json::from_str::<ServerMessage>(json) else {
            return json.to_string();
        };
        self.observe_server_message(&mut message);
        serde_json::to_string(&message).unwrap_or_else(|_| json.to_string())
    }

    fn observe_terminal(
        &self,
        terminal_id: &str,
        session_ref: Option<&freshell_protocol::SessionLocator>,
        supplied: Option<&RuntimeDescriptor>,
    ) -> Option<RuntimeDescriptor> {
        if let Some(runtime) = supplied {
            return Some(runtime.clone());
        }
        if let Some(runtime) = self.runtime_for_live(AgentRuntimeKind::Terminal, terminal_id) {
            return Some(runtime);
        }
        Some(if let Some(session_ref) = session_ref {
            self.register_initial(
                RuntimeLocator::new(
                    AgentRuntimeKind::Terminal,
                    &session_ref.provider,
                    &session_ref.session_id,
                ),
                terminal_id,
            )
        } else {
            self.register_live(AgentRuntimeKind::Terminal, terminal_id)
        })
    }

    /// Run or replay a transaction. A process-wide async mutex closes the
    /// same-request race: a concurrent resend waits, then observes the stored
    /// terminal result instead of running provider teardown a second time.
    pub async fn execute<R: RestartRuntime>(
        &self,
        request: AgentRestart,
        runtime: &R,
    ) -> RestartOutcome {
        self.execute_with_events(request, runtime, |_| {}).await
    }

    /// [`Self::execute`] with an event sink. `started` is delivered immediately
    /// after successful preflight and before shutdown; the terminal result is
    /// delivered only after it has been stored for reconnect replay.
    pub async fn execute_with_events<R, F>(
        &self,
        request: AgentRestart,
        runtime: &R,
        mut emit: F,
    ) -> RestartOutcome
    where
        R: RestartRuntime,
        F: FnMut(&ServerMessage),
    {
        let _execution = self.execution.lock().await;

        if let Some(outcome) = self.replay_or_conflict(&request) {
            for message in &outcome.messages {
                emit(message);
            }
            return outcome;
        }

        let locator = RuntimeLocator::from_request(&request);
        let expected = RuntimeDescriptor {
            runtime_id: request.live_id.clone(),
            generation: request.expected_generation,
        };
        let current = self.runtime_for_locator(&locator);
        if current.as_ref() != Some(&expected) {
            let failure = if current.is_none() {
                RestartFailure::new(
                    AgentRestartFailureCode::RuntimeNotFound,
                    "live runtime was not found for the durable session",
                    false,
                )
            } else {
                RestartFailure::new(
                    AgentRestartFailureCode::StaleGeneration,
                    "live runtime generation no longer matches the restart request",
                    false,
                )
            };
            let outcome = self.store_failure(request, current.unwrap_or(expected), failure, false);
            emit(&outcome.messages[0]);
            return outcome;
        }

        // The critical safety ordering: no started event and no teardown until
        // the provider has proved that the durable conversation can resume.
        let plan = match runtime.preflight(&request).await {
            Ok(plan) => plan,
            Err(failure) => {
                let outcome = self.store_failure(request, expected, failure, false);
                emit(&outcome.messages[0]);
                return outcome;
            }
        };

        let started = ServerMessage::AgentRestartStarted(AgentRestartStarted {
            request_id: request.request_id.clone(),
            provider: request.provider.clone(),
            session_id: request.session_id.clone(),
            kind: request.kind,
            runtime: expected.clone(),
        });
        tracing::info!(
            request_id = %request.request_id,
            provider = %request.provider,
            session_id = %request.session_id,
            runtime_id = %request.live_id,
            generation = request.expected_generation,
            "agent.restart.started"
        );
        emit(&started);

        if let Err(failure) = runtime.shutdown_for_restart(&request, &plan).await {
            let mut outcome = self.store_failure(request, expected, failure, false);
            emit(&outcome.messages[0]);
            outcome.messages.insert(0, started);
            return outcome;
        }

        let replacement_id = match runtime.create_replacement(&request, plan).await {
            Ok(runtime_id) => runtime_id,
            Err(failure) => {
                let mut outcome = self.store_failure(request, expected, failure, false);
                emit(&outcome.messages[0]);
                outcome.messages.insert(0, started);
                return outcome;
            }
        };

        let replacement = {
            let mut ownership = self.ownership.lock().expect("restart ownership lock");
            let Some(current) = ownership.by_locator.get(&locator).cloned() else {
                drop(ownership);
                let failure = RestartFailure::new(
                    AgentRestartFailureCode::RuntimeNotFound,
                    "runtime ownership disappeared during restart",
                    true,
                );
                let mut outcome = self.store_failure(request, expected, failure, false);
                emit(&outcome.messages[0]);
                outcome.messages.insert(0, started);
                return outcome;
            };
            let observed_replacement = current.runtime_id == replacement_id
                && current.generation == expected.generation.saturating_add(1);
            if current != expected && !observed_replacement {
                drop(ownership);
                let failure = RestartFailure::new(
                    AgentRestartFailureCode::StaleGeneration,
                    "runtime ownership changed during restart",
                    false,
                );
                let mut outcome = self.store_failure(request, expected, failure, false);
                emit(&outcome.messages[0]);
                outcome.messages.insert(0, started);
                return outcome;
            }
            let descriptor = if observed_replacement {
                current
            } else {
                RuntimeDescriptor {
                    runtime_id: replacement_id.clone(),
                    generation: expected.generation.saturating_add(1),
                }
            };
            ownership
                .descriptor_by_live
                .insert((request.kind, replacement_id), descriptor.clone());
            ownership.by_locator.insert(locator, descriptor.clone());
            descriptor
        };

        let terminal = ServerMessage::AgentRestartReplaced(AgentRestartReplaced {
            request_id: request.request_id.clone(),
            provider: request.provider.clone(),
            session_id: request.session_id.clone(),
            kind: request.kind,
            old_runtime: OldRuntimeDescriptor::from(expected),
            runtime: replacement.clone(),
        });
        tracing::info!(
            request_id = %request.request_id,
            provider = %request.provider,
            session_id = %request.session_id,
            runtime_id = %replacement.runtime_id,
            generation = replacement.generation,
            "agent.restart.replaced"
        );
        self.store_terminal(&request, terminal.clone());
        emit(&terminal);
        RestartOutcome {
            messages: vec![started, terminal],
            replayed: false,
        }
    }

    fn replay_or_conflict(&self, request: &AgentRestart) -> Option<RestartOutcome> {
        let ownership = self.ownership.lock().expect("restart ownership lock");
        let stored = ownership.results.get(&request.request_id)?;
        if stored.fingerprint == *request {
            return Some(RestartOutcome {
                messages: vec![stored.terminal.clone()],
                replayed: true,
            });
        }
        drop(ownership);
        let runtime = self
            .runtime_for_locator(&RuntimeLocator::from_request(request))
            .unwrap_or(RuntimeDescriptor {
                runtime_id: request.live_id.clone(),
                generation: request.expected_generation,
            });
        Some(RestartOutcome {
            messages: vec![ServerMessage::AgentRestartFailed(AgentRestartFailed {
                request_id: request.request_id.clone(),
                provider: request.provider.clone(),
                session_id: request.session_id.clone(),
                kind: request.kind,
                runtime,
                code: AgentRestartFailureCode::RequestIdConflict,
                message: "requestId was already used with a different restart fingerprint"
                    .to_string(),
                retryable: false,
            })],
            replayed: true,
        })
    }

    fn store_failure(
        &self,
        request: AgentRestart,
        descriptor: RuntimeDescriptor,
        failure: RestartFailure,
        replayed: bool,
    ) -> RestartOutcome {
        let terminal = ServerMessage::AgentRestartFailed(AgentRestartFailed {
            request_id: request.request_id.clone(),
            provider: request.provider.clone(),
            session_id: request.session_id.clone(),
            kind: request.kind,
            runtime: descriptor,
            code: failure.code,
            message: failure.message,
            retryable: failure.retryable,
        });
        tracing::warn!(
            request_id = %request.request_id,
            provider = %request.provider,
            session_id = %request.session_id,
            code = ?failure.code,
            retryable = failure.retryable,
            "agent.restart.failed"
        );
        self.store_terminal(&request, terminal.clone());
        RestartOutcome {
            messages: vec![terminal],
            replayed,
        }
    }

    fn store_terminal(&self, request: &AgentRestart, terminal: ServerMessage) {
        self.ownership
            .lock()
            .expect("restart ownership lock")
            .results
            .insert(
                request.request_id.clone(),
                StoredResult {
                    fingerprint: request.clone(),
                    terminal,
                },
            );
    }
}
