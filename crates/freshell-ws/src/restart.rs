//! Provider-agnostic runtime ownership and restart transaction coordinator.
//!
//! Provider teardown/resume adapters live outside this module. This module owns
//! the cross-provider invariants: immutable request fingerprints, live runtime
//! generations, preflight-before-shutdown ordering, and terminal-result replay.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use freshell_protocol::{
    AgentRestart, AgentRestartFailed, AgentRestartFailureCode, AgentRestartReplaced,
    AgentRestartStarted, AgentRuntimeKind, OldRuntimeDescriptor, RuntimeDescriptor, ServerMessage,
};
use serde::{Deserialize, Serialize};

/// Canonical durable identity of one restartable runtime.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredResult {
    fingerprint: AgentRestart,
    terminal: ServerMessage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingRestartRecovery {
    pub request_id: String,
    pub request: AgentRestart,
    pub old_runtime: RuntimeDescriptor,
    pub last_failure: Option<AgentRestartFailed>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedGeneration {
    locator: RuntimeLocator,
    generation: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedResult {
    request_id: String,
    result: StoredResult,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedOwnership {
    #[serde(default)]
    generations: Vec<PersistedGeneration>,
    #[serde(default)]
    results: Vec<PersistedResult>,
    #[serde(default)]
    pending_recoveries: Vec<PendingRestartRecovery>,
}

#[derive(Debug, Default)]
struct RuntimeOwnership {
    by_locator: HashMap<RuntimeLocator, RuntimeDescriptor>,
    /// Exact descriptor for every observed live runtime id, including retired
    /// generations whose already-queued output/exit frames still need fencing.
    descriptor_by_live: HashMap<(AgentRuntimeKind, String), RuntimeDescriptor>,
    locator_by_live: HashMap<(AgentRuntimeKind, String), RuntimeLocator>,
    fresh_event_aliases: HashMap<(String, String), RuntimeDescriptor>,
    last_generation: HashMap<RuntimeLocator, u64>,
    results: HashMap<String, StoredResult>,
    pending_recoveries: HashMap<String, PendingRestartRecovery>,
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
#[async_trait::async_trait]
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

struct UnavailableRestartRuntime;

#[async_trait::async_trait]
impl RestartRuntime for UnavailableRestartRuntime {
    type ResumePlan = ();

    async fn preflight(&self, _request: &AgentRestart) -> Result<(), RestartFailure> {
        Err(RestartFailure::new(
            AgentRestartFailureCode::Unresumable,
            "no restart adapter is registered for this runtime",
            false,
        ))
    }

    async fn shutdown_for_restart(
        &self,
        _request: &AgentRestart,
        _plan: &(),
    ) -> Result<(), RestartFailure> {
        unreachable!("unavailable restart adapter never passes preflight")
    }

    async fn create_replacement(
        &self,
        _request: &AgentRestart,
        _plan: (),
    ) -> Result<String, RestartFailure> {
        unreachable!("unavailable restart adapter never passes preflight")
    }
}

/// One server-owned registry for runtime descriptors and restart results.
#[derive(Clone)]
pub struct RestartCoordinator {
    ownership: Arc<Mutex<RuntimeOwnership>>,
    persistence_path: Option<Arc<PathBuf>>,
    locator_locks: Arc<Mutex<HashMap<RuntimeLocator, Arc<tokio::sync::Mutex<()>>>>>,
    request_locks: Arc<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>,
    registered_runtime: Arc<std::sync::RwLock<Option<Arc<dyn RestartRuntime<ResumePlan = ()>>>>>,
}

impl Default for RestartCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

impl RestartCoordinator {
    pub fn new() -> Self {
        Self {
            ownership: Arc::new(Mutex::new(RuntimeOwnership::default())),
            persistence_path: None,
            locator_locks: Arc::new(Mutex::new(HashMap::new())),
            request_locks: Arc::new(Mutex::new(HashMap::new())),
            registered_runtime: Arc::new(std::sync::RwLock::new(None)),
        }
    }

    pub fn new_persistent(path: impl Into<PathBuf>) -> std::io::Result<Self> {
        let path = path.into();
        let ownership = Self::load_persisted(&path)?;
        Ok(Self {
            ownership: Arc::new(Mutex::new(ownership)),
            persistence_path: Some(Arc::new(path)),
            locator_locks: Arc::new(Mutex::new(HashMap::new())),
            request_locks: Arc::new(Mutex::new(HashMap::new())),
            registered_runtime: Arc::new(std::sync::RwLock::new(None)),
        })
    }

    fn load_persisted(path: &Path) -> std::io::Result<RuntimeOwnership> {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(RuntimeOwnership::default())
            }
            Err(error) => return Err(error),
        };
        let persisted: PersistedOwnership = serde_json::from_slice(&bytes).map_err(|error| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid restart transaction state: {error}"),
            )
        })?;
        Ok(RuntimeOwnership {
            last_generation: persisted
                .generations
                .into_iter()
                .map(|entry| (entry.locator, entry.generation))
                .collect(),
            results: persisted
                .results
                .into_iter()
                .map(|entry| (entry.request_id, entry.result))
                .collect(),
            pending_recoveries: persisted
                .pending_recoveries
                .into_iter()
                .map(|entry| (entry.request_id.clone(), entry))
                .collect(),
            ..RuntimeOwnership::default()
        })
    }

    fn persist_locked(&self, ownership: &RuntimeOwnership) -> std::io::Result<()> {
        let Some(path) = self.persistence_path.as_deref() else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let state = PersistedOwnership {
            generations: ownership
                .last_generation
                .iter()
                .map(|(locator, generation)| PersistedGeneration {
                    locator: locator.clone(),
                    generation: *generation,
                })
                .collect(),
            results: ownership
                .results
                .iter()
                .map(|(request_id, result)| PersistedResult {
                    request_id: request_id.clone(),
                    result: result.clone(),
                })
                .collect(),
            pending_recoveries: ownership.pending_recoveries.values().cloned().collect(),
        };
        let bytes = serde_json::to_vec_pretty(&state)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        let temp = path.with_extension(format!(
            "tmp-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temp)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
        }
        if let Err(error) = std::fs::rename(&temp, path.as_path()) {
            let _ = std::fs::remove_file(&temp);
            return Err(error);
        }
        if let Some(parent) = path.parent() {
            if let Ok(directory) = std::fs::File::open(parent) {
                let _ = directory.sync_all();
            }
        }
        Ok(())
    }

    fn persist_or_log(&self, ownership: &RuntimeOwnership, operation: &'static str) {
        if let Err(error) = self.persist_locked(ownership) {
            tracing::error!(
                error = %error,
                operation,
                "agent.restart.persistence.failed"
            );
        }
    }

    pub fn pending_recoveries(&self) -> Vec<PendingRestartRecovery> {
        self.ownership
            .lock()
            .expect("restart ownership lock")
            .pending_recoveries
            .values()
            .cloned()
            .collect()
    }

    pub fn set_runtime(&self, runtime: Arc<dyn RestartRuntime<ResumePlan = ()>>) {
        *self
            .registered_runtime
            .write()
            .expect("registered restart runtime lock") = Some(runtime);
    }

    pub async fn execute_registered<F>(&self, request: AgentRestart, emit: F) -> RestartOutcome
    where
        F: FnMut(&ServerMessage),
    {
        let runtime = self
            .registered_runtime
            .read()
            .expect("registered restart runtime lock")
            .clone();
        let runtime = runtime.unwrap_or_else(|| Arc::new(UnavailableRestartRuntime));
        self.execute_with_events(request, runtime.as_ref(), emit)
            .await
    }

    fn locator_lock(&self, locator: &RuntimeLocator) -> Arc<tokio::sync::Mutex<()>> {
        self.locator_locks
            .lock()
            .expect("restart locator locks")
            .entry(locator.clone())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    fn request_lock(&self, request_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.request_locks
            .lock()
            .expect("restart request locks")
            .entry(request_id.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
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
            ownership
                .last_generation
                .entry(locator.clone())
                .and_modify(|generation| *generation = (*generation).max(existing.generation))
                .or_insert(existing.generation);
            Self::bind_locator_locked(&mut ownership, locator, runtime_id, existing.clone());
            self.persist_or_log(&ownership, "bind_existing_runtime");
            return existing;
        }
        let generation = ownership
            .last_generation
            .get(&locator)
            .map_or(1, |generation| generation.saturating_add(1));
        let descriptor = RuntimeDescriptor {
            runtime_id: runtime_id.clone(),
            generation,
        };
        Self::bind_locator_locked(
            &mut ownership,
            locator.clone(),
            runtime_id,
            descriptor.clone(),
        );
        ownership.last_generation.insert(locator, generation);
        self.persist_or_log(&ownership, "register_runtime");
        descriptor
    }

    fn bind_locator_locked(
        ownership: &mut RuntimeOwnership,
        locator: RuntimeLocator,
        runtime_id: String,
        descriptor: RuntimeDescriptor,
    ) {
        let live_key = (locator.kind, runtime_id.clone());
        if let Some(previous_locator) = ownership
            .locator_by_live
            .insert(live_key.clone(), locator.clone())
        {
            if previous_locator != locator
                && ownership
                    .by_locator
                    .get(&previous_locator)
                    .is_some_and(|current| current.runtime_id == runtime_id)
            {
                ownership.by_locator.remove(&previous_locator);
            }
        }
        if let Some(previous_runtime) = ownership.by_locator.insert(locator, descriptor.clone()) {
            if previous_runtime.runtime_id != runtime_id {
                ownership
                    .locator_by_live
                    .remove(&(live_key.0, previous_runtime.runtime_id));
            }
        }
        ownership.descriptor_by_live.insert(live_key, descriptor);
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
        let descriptor = ownership
            .descriptor_by_live
            .entry((kind, runtime_id.clone()))
            .or_insert_with(|| RuntimeDescriptor {
                runtime_id,
                generation: 1,
            })
            .clone();
        self.persist_or_log(&ownership, "register_unbound_runtime");
        descriptor
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
            ServerMessage::TerminalSessionAssociated(frame) => {
                let locator = RuntimeLocator::new(
                    AgentRuntimeKind::Terminal,
                    &frame.session_ref.provider,
                    &frame.session_ref.session_id,
                );
                frame.runtime = Some(self.register_initial(locator, &frame.terminal_id));
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
                frame.runtime = Some(match frame.runtime.as_ref() {
                    Some(runtime) => self.register_supplied(locator, runtime.clone()),
                    None => self.register_initial(
                        locator,
                        format!("fresh-runtime-{}", uuid::Uuid::new_v4()),
                    ),
                });
                if let Some(runtime) = frame.runtime.clone() {
                    self.ownership
                        .lock()
                        .expect("restart ownership lock")
                        .fresh_event_aliases
                        .insert((frame.provider.clone(), frame.session_id.clone()), runtime);
                }
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
                frame.runtime = Some(match frame.runtime.as_ref() {
                    Some(runtime) => self.register_supplied(locator, runtime.clone()),
                    None => self.register_initial(
                        locator,
                        format!("fresh-runtime-{}", uuid::Uuid::new_v4()),
                    ),
                });
                if let Some(runtime) = frame.runtime.clone() {
                    let mut ownership = self.ownership.lock().expect("restart ownership lock");
                    ownership.fresh_event_aliases.insert(
                        (frame.provider.clone(), frame.previous_session_id.clone()),
                        runtime.clone(),
                    );
                    ownership
                        .fresh_event_aliases
                        .insert((frame.provider.clone(), frame.session_id.clone()), runtime);
                }
            }
            ServerMessage::FreshAgentEvent(frame) => {
                if frame.runtime.is_none() {
                    frame.runtime = self
                        .ownership
                        .lock()
                        .expect("restart ownership lock")
                        .fresh_event_aliases
                        .get(&(frame.provider.clone(), frame.session_id.clone()))
                        .cloned()
                        .or_else(|| {
                            self.runtime_for_locator(&RuntimeLocator::new(
                                AgentRuntimeKind::FreshAgent,
                                &frame.provider,
                                &frame.session_id,
                            ))
                        });
                }
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
            Some(
                "terminal.session.associated"
                    | "freshAgent.created"
                    | "freshAgent.event"
                    | "freshAgent.session.materialized"
            )
        ) {
            return json.to_string();
        }
        let Ok(mut message) = serde_json::from_str::<ServerMessage>(json) else {
            return json.to_string();
        };
        self.observe_server_message(&mut message);
        serde_json::to_string(&message).unwrap_or_else(|_| json.to_string())
    }

    pub fn register_supplied(
        &self,
        locator: RuntimeLocator,
        descriptor: RuntimeDescriptor,
    ) -> RuntimeDescriptor {
        let mut ownership = self.ownership.lock().expect("restart ownership lock");
        let last_generation = ownership
            .last_generation
            .get(&locator)
            .copied()
            .unwrap_or_default();
        if descriptor.generation < last_generation {
            return descriptor;
        }
        if descriptor.generation == last_generation
            && ownership
                .by_locator
                .get(&locator)
                .is_some_and(|current| current.runtime_id != descriptor.runtime_id)
        {
            return descriptor;
        }
        ownership
            .last_generation
            .insert(locator.clone(), descriptor.generation);
        Self::bind_locator_locked(
            &mut ownership,
            locator,
            descriptor.runtime_id.clone(),
            descriptor.clone(),
        );
        self.persist_or_log(&ownership, "register_supplied_runtime");
        descriptor
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

    /// Run or replay a transaction. Request-id and durable-locator locks close
    /// duplicate/overlap races without serializing unrelated providers or runtimes.
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
        R: RestartRuntime + ?Sized,
        F: FnMut(&ServerMessage),
    {
        let locator = RuntimeLocator::from_request(&request);
        let request_lock = self.request_lock(&request.request_id);
        let locator_lock = self.locator_lock(&locator);
        let _request_guard = request_lock.lock_owned().await;
        let _locator_guard = locator_lock.lock_owned().await;

        if let Some(outcome) = self.replay_or_conflict(&request) {
            for message in &outcome.messages {
                emit(message);
            }
            return outcome;
        }

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

        {
            let mut ownership = self.ownership.lock().expect("restart ownership lock");
            ownership.pending_recoveries.insert(
                request.request_id.clone(),
                PendingRestartRecovery {
                    request_id: request.request_id.clone(),
                    request: request.clone(),
                    old_runtime: expected.clone(),
                    last_failure: None,
                },
            );
            self.persist_or_log(&ownership, "record_shutdown_pending");
        }

        if let Err(failure) = runtime.shutdown_for_restart(&request, &plan).await {
            let mut ownership = self.ownership.lock().expect("restart ownership lock");
            ownership.pending_recoveries.remove(&request.request_id);
            self.persist_or_log(&ownership, "clear_failed_shutdown");
            drop(ownership);
            let mut outcome = self.store_failure(request, expected, failure, false);
            emit(&outcome.messages[0]);
            outcome.messages.insert(0, started);
            return outcome;
        }

        {
            let mut ownership = self.ownership.lock().expect("restart ownership lock");
            if ownership
                .by_locator
                .get(&locator)
                .is_some_and(|current| current == &expected)
            {
                ownership.by_locator.remove(&locator);
            }
            self.persist_or_log(&ownership, "record_shutdown");
        }

        let replacement_id = match runtime.create_replacement(&request, plan).await {
            Ok(runtime_id) => runtime_id,
            Err(failure) => {
                if failure.retryable {
                    let failed = AgentRestartFailed {
                        request_id: request.request_id.clone(),
                        provider: request.provider.clone(),
                        session_id: request.session_id.clone(),
                        kind: request.kind,
                        runtime: expected.clone(),
                        code: failure.code,
                        message: failure.message.clone(),
                        retryable: true,
                    };
                    let mut ownership = self.ownership.lock().expect("restart ownership lock");
                    if let Some(recovery) =
                        ownership.pending_recoveries.get_mut(&request.request_id)
                    {
                        recovery.last_failure = Some(failed);
                    }
                    self.persist_or_log(&ownership, "record_retryable_replacement_failure");
                } else {
                    let mut ownership = self.ownership.lock().expect("restart ownership lock");
                    ownership.pending_recoveries.remove(&request.request_id);
                    self.persist_or_log(&ownership, "clear_terminal_replacement_failure");
                }
                let mut outcome = self.store_failure(request, expected, failure, false);
                emit(&outcome.messages[0]);
                outcome.messages.insert(0, started);
                return outcome;
            }
        };

        let replacement = {
            let mut ownership = self.ownership.lock().expect("restart ownership lock");
            let current = ownership.by_locator.get(&locator).cloned();
            let observed_replacement = current.as_ref().is_some_and(|current| {
                current.runtime_id == replacement_id
                    && current.generation == expected.generation.saturating_add(1)
            });
            if current
                .as_ref()
                .is_some_and(|current| current != &expected && !observed_replacement)
            {
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
                current.expect("observed replacement has current descriptor")
            } else {
                RuntimeDescriptor {
                    runtime_id: replacement_id.clone(),
                    generation: expected.generation.saturating_add(1),
                }
            };
            ownership
                .descriptor_by_live
                .insert((request.kind, replacement_id), descriptor.clone());
            ownership
                .last_generation
                .insert(locator.clone(), descriptor.generation);
            Self::bind_locator_locked(
                &mut ownership,
                locator,
                descriptor.runtime_id.clone(),
                descriptor.clone(),
            );
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
            tracing::info!(
                request_id = %request.request_id,
                provider = %request.provider,
                session_id = %request.session_id,
                kind = ?request.kind,
                "agent.restart.replayed"
            );
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
        tracing::warn!(
            request_id = %request.request_id,
            provider = %request.provider,
            session_id = %request.session_id,
            kind = ?request.kind,
            "agent.restart.request_id_conflict"
        );
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
        let mut ownership = self.ownership.lock().expect("restart ownership lock");
        let is_replaced = matches!(&terminal, ServerMessage::AgentRestartReplaced(_));
        ownership.results.insert(
            request.request_id.clone(),
            StoredResult {
                fingerprint: request.clone(),
                terminal,
            },
        );
        if is_replaced {
            ownership.pending_recoveries.remove(&request.request_id);
        }
        self.persist_or_log(&ownership, "store_terminal_result");
    }
}

impl freshell_freshagent::FreshRuntimeRegistry for RestartCoordinator {
    fn register_runtime(
        &self,
        provider: &str,
        durable_session_id: &str,
        live_runtime_id: &str,
    ) -> RuntimeDescriptor {
        self.register_initial(
            RuntimeLocator::new(AgentRuntimeKind::FreshAgent, provider, durable_session_id),
            live_runtime_id,
        )
    }
}
