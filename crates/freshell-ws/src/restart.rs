//! Provider-agnostic runtime ownership and restart transaction coordinator.
//!
//! Provider teardown/resume adapters live outside this module. This module owns
//! the cross-provider invariants: immutable request fingerprints, live runtime
//! generations, preflight-before-shutdown ordering, and terminal-result replay.

use std::collections::{HashMap, HashSet, VecDeque};
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
    /// Distinct client request ids correlated to the same retired runtime.
    /// The owner remains authoritative for storage/recovery, while every
    /// follower receives and can replay a request-specific terminal result.
    #[serde(default)]
    pub followers: Vec<AgentRestart>,
    pub old_runtime: RuntimeDescriptor,
    /// True until provider teardown has crossed every process/consumer/lease
    /// barrier. A retry or boot recovery must finish this phase before it may
    /// create a replacement.
    #[serde(default)]
    pub retirement_pending: bool,
    #[serde(default)]
    pub resume_context: Option<RestartResumeContext>,
    pub last_failure: Option<AgentRestartFailed>,
}

/// Minimal provider resume data that must survive a server restart after the
/// predecessor has already been retired.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestartResumeContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_shell: Option<freshell_protocol::Shell>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_permission_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_sandbox: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_codex_managed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fresh_session_type: Option<freshell_protocol::SessionType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fresh_cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fresh_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fresh_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fresh_permission_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fresh_sandbox: Option<freshell_protocol::Sandbox>,
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

#[derive(Debug, Clone, Default)]
struct RuntimeOwnership {
    by_locator: HashMap<RuntimeLocator, RuntimeDescriptor>,
    locator_order: VecDeque<RuntimeLocator>,
    /// Exact descriptor for every observed live runtime id, including retired
    /// generations whose already-queued output/exit frames still need fencing.
    descriptor_by_live: HashMap<(AgentRuntimeKind, String), RuntimeDescriptor>,
    descriptor_order: VecDeque<(AgentRuntimeKind, String)>,
    locator_by_live: HashMap<(AgentRuntimeKind, String), RuntimeLocator>,
    /// Distinguishes a genuinely never-bound terminal from a predecessor whose
    /// locator was deliberately removed during replacement. Retired entries
    /// remain for the same bounded late-frame window as their descriptors.
    retired_live: HashSet<(AgentRuntimeKind, String)>,
    fresh_event_aliases: HashMap<(String, String), RuntimeDescriptor>,
    fresh_alias_order: VecDeque<(String, String)>,
    last_generation: HashMap<RuntimeLocator, u64>,
    generation_order: VecDeque<RuntimeLocator>,
    results: HashMap<String, StoredResult>,
    result_order: VecDeque<String>,
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

    /// Capture the small provider context required to resume after process
    /// restart. Called after preflight and persisted before teardown.
    fn persisted_resume_context(
        &self,
        _request: &AgentRestart,
        _plan: &Self::ResumePlan,
    ) -> Option<RestartResumeContext> {
        None
    }

    /// Resume a replacement whose selected predecessor was already shut down
    /// before a retryable failure or server restart. Implementations may
    /// override this when recovery needs a path distinct from ordinary
    /// preflight; the default rebuilds a resume plan and creates directly.
    async fn recover_replacement(
        &self,
        request: &AgentRestart,
        _context: Option<&RestartResumeContext>,
    ) -> Result<String, RestartFailure> {
        let plan = self.preflight(request).await?;
        self.create_replacement(request, plan).await
    }

    /// Continue a teardown that was durably recorded before provider
    /// quiescence completed. The default is suitable for adapters whose live
    /// runtime remains preflightable; production adapters may resume an
    /// internal quarantined retirement directly.
    async fn recover_retirement(
        &self,
        request: &AgentRestart,
        _context: Option<&RestartResumeContext>,
    ) -> Result<(), RestartFailure> {
        let plan = self.preflight(request).await?;
        self.shutdown_for_restart(request, &plan).await
    }
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
    persistence_fault: Option<Arc<String>>,
    locator_locks: Arc<Mutex<HashMap<RuntimeLocator, Arc<tokio::sync::Mutex<()>>>>>,
    locator_lock_order: Arc<Mutex<VecDeque<RuntimeLocator>>>,
    request_locks: Arc<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>,
    request_lock_order: Arc<Mutex<VecDeque<String>>>,
    lock_limit: usize,
    ownership_limit: usize,
    registered_runtime:
        Arc<std::sync::RwLock<Option<std::sync::Weak<dyn RestartRuntime<ResumePlan = ()>>>>>,
}

const DEFAULT_RESTART_PERSISTED_GENERATION_LIMIT: usize = 1_024;
const DEFAULT_RESTART_LOCK_LIMIT: usize = 1_024;
const DEFAULT_RESTART_OWNERSHIP_LIMIT: usize = 4_096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetainedOwnershipCounts {
    pub descriptors: usize,
    pub live_locators: usize,
    pub current_locators: usize,
    pub fresh_aliases: usize,
    pub generation_high_waters: usize,
}

/// Keeps a dynamically-installed restart adapter alive without introducing a
/// coordinator → adapter → [`crate::WsState`] reference cycle.
pub struct RestartRuntimeRegistration {
    _runtime: Arc<dyn RestartRuntime<ResumePlan = ()>>,
}

struct LockRetentionGuard<'a>(&'a RestartCoordinator);

impl Drop for LockRetentionGuard<'_> {
    fn drop(&mut self) {
        self.0.prune_inactive_locks();
    }
}

impl Default for RestartCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

impl RestartCoordinator {
    pub fn new() -> Self {
        let mut coordinator = Self::new_with_limits(
            DEFAULT_RESTART_PERSISTED_GENERATION_LIMIT,
            DEFAULT_RESTART_LOCK_LIMIT,
        );
        coordinator.ownership_limit = DEFAULT_RESTART_OWNERSHIP_LIMIT;
        coordinator
    }

    pub fn new_with_limits(ownership_limit: usize, lock_limit: usize) -> Self {
        Self {
            ownership: Arc::new(Mutex::new(RuntimeOwnership::default())),
            persistence_path: None,
            persistence_fault: None,
            locator_locks: Arc::new(Mutex::new(HashMap::new())),
            locator_lock_order: Arc::new(Mutex::new(VecDeque::new())),
            request_locks: Arc::new(Mutex::new(HashMap::new())),
            request_lock_order: Arc::new(Mutex::new(VecDeque::new())),
            lock_limit: lock_limit.max(1),
            ownership_limit: ownership_limit.max(1),
            registered_runtime: Arc::new(std::sync::RwLock::new(None)),
        }
    }

    pub fn new_persistent(path: impl Into<PathBuf>) -> std::io::Result<Self> {
        Self::new_persistent_with_limits(
            path,
            DEFAULT_RESTART_PERSISTED_GENERATION_LIMIT,
            DEFAULT_RESTART_LOCK_LIMIT,
        )
    }

    pub fn new_persistent_with_limits(
        path: impl Into<PathBuf>,
        ownership_limit: usize,
        lock_limit: usize,
    ) -> std::io::Result<Self> {
        let path = path.into();
        let ownership_limit = ownership_limit.max(1);
        let lock_limit = lock_limit.max(1);
        let ownership = Self::load_persisted(&path, ownership_limit)?;
        Ok(Self {
            ownership: Arc::new(Mutex::new(ownership)),
            persistence_path: Some(Arc::new(path)),
            persistence_fault: None,
            locator_locks: Arc::new(Mutex::new(HashMap::new())),
            locator_lock_order: Arc::new(Mutex::new(VecDeque::new())),
            request_locks: Arc::new(Mutex::new(HashMap::new())),
            request_lock_order: Arc::new(Mutex::new(VecDeque::new())),
            lock_limit,
            ownership_limit,
            registered_runtime: Arc::new(std::sync::RwLock::new(None)),
        })
    }

    pub fn disabled_for_persistence(error: impl Into<String>) -> Self {
        let mut coordinator = Self::new();
        coordinator.persistence_fault = Some(Arc::new(error.into()));
        coordinator
    }

    fn load_persisted(path: &Path, ownership_limit: usize) -> std::io::Result<RuntimeOwnership> {
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
        let mut persisted_results = persisted.results;
        let pending_recoveries: HashMap<_, _> = persisted
            .pending_recoveries
            .into_iter()
            .map(|entry| (entry.request_id.clone(), entry))
            .collect();
        // Migration from the first durable format: retryable replacement
        // failures were incorrectly stored as terminal results. A pending
        // recovery is authoritative and must remain executable after reopen.
        persisted_results.retain(|entry| {
            !pending_recoveries.values().any(|pending| {
                pending.request_id == entry.request_id
                    || pending
                        .followers
                        .iter()
                        .any(|follower| follower.request_id == entry.request_id)
            })
        });
        let result_order = persisted_results
            .iter()
            .map(|entry| entry.request_id.clone())
            .collect();
        let mut generations = persisted.generations;
        if generations.len() > ownership_limit {
            generations.drain(..generations.len().saturating_sub(ownership_limit));
        }
        let generation_order = generations
            .iter()
            .map(|entry| entry.locator.clone())
            .collect();
        Ok(RuntimeOwnership {
            last_generation: generations
                .into_iter()
                .map(|entry| (entry.locator, entry.generation))
                .collect(),
            generation_order,
            results: persisted_results
                .into_iter()
                .map(|entry| (entry.request_id, entry.result))
                .collect(),
            result_order,
            pending_recoveries,
            ..RuntimeOwnership::default()
        })
    }

    fn persist_locked(&self, ownership: &RuntimeOwnership) -> std::io::Result<()> {
        if let Some(error) = &self.persistence_fault {
            return Err(std::io::Error::other(error.as_str()));
        }
        let Some(path) = self.persistence_path.as_deref() else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let state = PersistedOwnership {
            generations: ownership
                .generation_order
                .iter()
                .filter_map(|locator| {
                    ownership
                        .last_generation
                        .get(locator)
                        .map(|generation| PersistedGeneration {
                            locator: locator.clone(),
                            generation: *generation,
                        })
                })
                .collect(),
            results: ownership
                .result_order
                .iter()
                .filter_map(|request_id| {
                    ownership
                        .results
                        .get(request_id)
                        .map(|result| PersistedResult {
                            request_id: request_id.clone(),
                            result: result.clone(),
                        })
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

    fn try_update_ownership<T>(
        &self,
        operation: &'static str,
        update: impl FnOnce(&mut RuntimeOwnership) -> T,
    ) -> std::io::Result<T> {
        let mut ownership = self.ownership.lock().expect("restart ownership lock");
        let mut candidate = ownership.clone();
        let output = update(&mut candidate);
        self.persist_locked(&candidate).map_err(|error| {
            tracing::error!(
                error = %error,
                operation,
                "agent.restart.persistence.failed"
            );
            error
        })?;
        *ownership = candidate;
        Ok(output)
    }

    fn insert_result_locked(
        &self,
        ownership: &mut RuntimeOwnership,
        request: &AgentRestart,
        terminal: ServerMessage,
    ) {
        ownership
            .result_order
            .retain(|id| id != &request.request_id);
        ownership.result_order.push_back(request.request_id.clone());
        ownership.results.insert(
            request.request_id.clone(),
            StoredResult {
                fingerprint: request.clone(),
                terminal,
            },
        );
        // A terminal restart result is the server's durable reply to a request
        // that may have lost its socket before delivery. Do not evict it based
        // on local cache pressure: there is no client acknowledgement or
        // negotiated expiry protocol yet, so eviction would make an identical
        // reconnect retry indistinguishable from a brand-new transaction.
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

    pub fn set_runtime(
        &self,
        runtime: Arc<dyn RestartRuntime<ResumePlan = ()>>,
    ) -> RestartRuntimeRegistration {
        *self
            .registered_runtime
            .write()
            .expect("registered restart runtime lock") = Some(Arc::downgrade(&runtime));
        RestartRuntimeRegistration { _runtime: runtime }
    }

    pub async fn execute_registered<F>(&self, request: AgentRestart, emit: F) -> RestartOutcome
    where
        F: FnMut(&ServerMessage),
    {
        let runtime = self
            .registered_runtime
            .read()
            .expect("registered restart runtime lock")
            .as_ref()
            .and_then(std::sync::Weak::upgrade);
        let runtime = runtime.unwrap_or_else(|| Arc::new(UnavailableRestartRuntime));
        self.execute_with_events(request, runtime.as_ref(), emit)
            .await
    }

    /// Retry every durable post-shutdown replacement after the production
    /// adapter has been installed. Terminal results are stored before this
    /// method forwards them to the shared broadcast bus.
    pub async fn recover_pending_registered<F>(&self, mut emit: F)
    where
        F: FnMut(&ServerMessage),
    {
        for recovery in self.pending_recoveries() {
            self.execute_registered(recovery.request, &mut emit).await;
        }
    }

    fn locator_lock(&self, locator: &RuntimeLocator) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.locator_locks.lock().expect("restart locator locks");
        if let Some(lock) = locks.get(locator) {
            return Arc::clone(lock);
        }
        let lock = Arc::new(tokio::sync::Mutex::new(()));
        locks.insert(locator.clone(), Arc::clone(&lock));
        let mut order = self
            .locator_lock_order
            .lock()
            .expect("restart locator lock order");
        order.push_back(locator.clone());
        let mut inspected = 0;
        while locks.len() > self.lock_limit && inspected < order.len() {
            let Some(oldest) = order.pop_front() else {
                break;
            };
            if locks
                .get(&oldest)
                .is_some_and(|lock| Arc::strong_count(lock) == 1)
            {
                locks.remove(&oldest);
            } else {
                order.push_back(oldest);
            }
            inspected += 1;
        }
        lock
    }

    fn request_lock(&self, request_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.request_locks.lock().expect("restart request locks");
        if let Some(lock) = locks.get(request_id) {
            return Arc::clone(lock);
        }
        let request_id = request_id.to_string();
        let lock = Arc::new(tokio::sync::Mutex::new(()));
        locks.insert(request_id.clone(), Arc::clone(&lock));
        let mut order = self
            .request_lock_order
            .lock()
            .expect("restart request lock order");
        order.push_back(request_id);
        let mut inspected = 0;
        while locks.len() > self.lock_limit && inspected < order.len() {
            let Some(oldest) = order.pop_front() else {
                break;
            };
            if locks
                .get(&oldest)
                .is_some_and(|lock| Arc::strong_count(lock) == 1)
            {
                locks.remove(&oldest);
            } else {
                order.push_back(oldest);
            }
            inspected += 1;
        }
        lock
    }

    fn prune_inactive_locks(&self) {
        {
            let mut locks = self.request_locks.lock().expect("restart request locks");
            let mut order = self
                .request_lock_order
                .lock()
                .expect("restart request lock order");
            let mut inspected = 0;
            while locks.len() > self.lock_limit && inspected < order.len() {
                let Some(oldest) = order.pop_front() else {
                    break;
                };
                if locks
                    .get(&oldest)
                    .is_some_and(|lock| Arc::strong_count(lock) == 1)
                {
                    locks.remove(&oldest);
                } else {
                    order.push_back(oldest);
                }
                inspected += 1;
            }
        }
        {
            let mut locks = self.locator_locks.lock().expect("restart locator locks");
            let mut order = self
                .locator_lock_order
                .lock()
                .expect("restart locator lock order");
            let mut inspected = 0;
            while locks.len() > self.lock_limit && inspected < order.len() {
                let Some(oldest) = order.pop_front() else {
                    break;
                };
                if locks
                    .get(&oldest)
                    .is_some_and(|lock| Arc::strong_count(lock) == 1)
                {
                    locks.remove(&oldest);
                } else {
                    order.push_back(oldest);
                }
                inspected += 1;
            }
        }
    }

    fn touch_key<T: PartialEq + Clone>(order: &mut VecDeque<T>, key: &T) {
        order.retain(|candidate| candidate != key);
        order.push_back(key.clone());
    }

    fn set_generation_locked(
        ownership: &mut RuntimeOwnership,
        locator: RuntimeLocator,
        generation: u64,
    ) {
        ownership
            .last_generation
            .insert(locator.clone(), generation);
        Self::touch_key(&mut ownership.generation_order, &locator);
    }

    fn set_fresh_alias_locked(
        ownership: &mut RuntimeOwnership,
        alias: (String, String),
        descriptor: RuntimeDescriptor,
    ) {
        ownership
            .fresh_event_aliases
            .insert(alias.clone(), descriptor);
        Self::touch_key(&mut ownership.fresh_alias_order, &alias);
    }

    fn touch_descriptor_locked(ownership: &mut RuntimeOwnership, key: (AgentRuntimeKind, String)) {
        Self::touch_key(&mut ownership.descriptor_order, &key);
    }

    fn prune_ownership_locked(ownership: &mut RuntimeOwnership, limit: usize) {
        while ownership.descriptor_order.len() > limit {
            let Some(key) = ownership.descriptor_order.pop_front() else {
                break;
            };
            let removed = ownership.descriptor_by_live.remove(&key);
            ownership.retired_live.remove(&key);
            if let Some(locator) = ownership.locator_by_live.remove(&key) {
                if ownership.by_locator.get(&locator) == removed.as_ref() {
                    ownership.by_locator.remove(&locator);
                    ownership.locator_order.retain(|entry| entry != &locator);
                }
            }
            if let Some(removed) = removed {
                ownership
                    .fresh_event_aliases
                    .retain(|_, descriptor| descriptor != &removed);
                ownership
                    .fresh_alias_order
                    .retain(|alias| ownership.fresh_event_aliases.contains_key(alias));
            }
        }
        while ownership.locator_order.len() > limit {
            let Some(locator) = ownership.locator_order.pop_front() else {
                break;
            };
            if let Some(descriptor) = ownership.by_locator.remove(&locator) {
                ownership
                    .locator_by_live
                    .remove(&(locator.kind, descriptor.runtime_id.clone()));
            }
        }
        while ownership.fresh_alias_order.len() > limit {
            let Some(alias) = ownership.fresh_alias_order.pop_front() else {
                break;
            };
            ownership.fresh_event_aliases.remove(&alias);
        }
        while ownership.generation_order.len() > limit {
            let Some(locator) = ownership.generation_order.pop_front() else {
                break;
            };
            ownership.last_generation.remove(&locator);
        }
    }

    pub fn retained_result_count(&self) -> usize {
        self.ownership
            .lock()
            .expect("restart ownership lock")
            .results
            .len()
    }

    pub fn retained_lock_counts(&self) -> (usize, usize) {
        (
            self.request_locks
                .lock()
                .expect("restart request locks")
                .len(),
            self.locator_locks
                .lock()
                .expect("restart locator locks")
                .len(),
        )
    }

    pub fn retained_ownership_counts(&self) -> RetainedOwnershipCounts {
        let ownership = self.ownership.lock().expect("restart ownership lock");
        RetainedOwnershipCounts {
            descriptors: ownership.descriptor_by_live.len(),
            live_locators: ownership.locator_by_live.len(),
            current_locators: ownership.by_locator.len(),
            fresh_aliases: ownership.fresh_event_aliases.len(),
            generation_high_waters: ownership.last_generation.len(),
        }
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
            let live_key = (locator.kind, runtime_id.clone());
            if ownership.retired_live.contains(&live_key) {
                return existing;
            }
            let was_unbound = !ownership.locator_by_live.contains_key(&live_key);
            let existing = if was_unbound {
                let generation = ownership
                    .last_generation
                    .get(&locator)
                    .map_or(existing.generation, |generation| {
                        generation.saturating_add(1).max(existing.generation)
                    });
                let promoted = RuntimeDescriptor {
                    runtime_id: runtime_id.clone(),
                    generation,
                };
                ownership
                    .descriptor_by_live
                    .insert((locator.kind, runtime_id.clone()), promoted.clone());
                promoted
            } else {
                existing
            };
            let generation = ownership
                .last_generation
                .get(&locator)
                .copied()
                .unwrap_or_default()
                .max(existing.generation);
            Self::set_generation_locked(&mut ownership, locator.clone(), generation);
            Self::bind_locator_locked(&mut ownership, locator, runtime_id, existing.clone());
            Self::prune_ownership_locked(&mut ownership, self.ownership_limit);
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
        Self::set_generation_locked(&mut ownership, locator, generation);
        Self::prune_ownership_locked(&mut ownership, self.ownership_limit);
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
                let previous_key = (live_key.0, previous_runtime.runtime_id);
                ownership.locator_by_live.remove(&previous_key);
                ownership.retired_live.insert(previous_key);
            }
        }
        let bound_locator = ownership
            .locator_by_live
            .get(&live_key)
            .cloned()
            .expect("locator inserted above");
        Self::touch_key(&mut ownership.locator_order, &bound_locator);
        ownership.retired_live.remove(&live_key);
        ownership
            .descriptor_by_live
            .insert(live_key.clone(), descriptor);
        Self::touch_descriptor_locked(ownership, live_key);
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
                runtime_id: runtime_id.clone(),
                generation: 1,
            })
            .clone();
        Self::touch_descriptor_locked(&mut ownership, (kind, runtime_id));
        Self::prune_ownership_locked(&mut ownership, self.ownership_limit);
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
                    terminal.runtime = self.observe_terminal(
                        &terminal.terminal_id,
                        terminal.session_ref.as_ref(),
                        terminal.runtime.as_ref(),
                    );
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
                    let mut ownership = self.ownership.lock().expect("restart ownership lock");
                    Self::set_fresh_alias_locked(
                        &mut ownership,
                        (frame.provider.clone(), frame.session_id.clone()),
                        runtime,
                    );
                    Self::prune_ownership_locked(&mut ownership, self.ownership_limit);
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
                    Self::set_fresh_alias_locked(
                        &mut ownership,
                        (frame.provider.clone(), frame.previous_session_id.clone()),
                        runtime.clone(),
                    );
                    Self::set_fresh_alias_locked(
                        &mut ownership,
                        (frame.provider.clone(), frame.session_id.clone()),
                        runtime,
                    );
                    Self::prune_ownership_locked(&mut ownership, self.ownership_limit);
                }
            }
            ServerMessage::FreshAgentEvent(frame) => {
                if frame.runtime.is_none() {
                    let alias = self
                        .ownership
                        .lock()
                        .expect("restart ownership lock")
                        .fresh_event_aliases
                        .get(&(frame.provider.clone(), frame.session_id.clone()))
                        .cloned();
                    frame.runtime = alias.or_else(|| {
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

    /// Verify the descriptor portion of the `agentRestartV1` capability
    /// contract after [`Self::observe_server_message`] enrichment.
    ///
    /// Fields remain optional in the protocol types so a new v7 client can
    /// still parse an older v7 server. Once `ready.capabilities.agentRestartV1`
    /// is present, these runtime-addressed surfaces are required.
    pub fn restart_runtime_contract_satisfied(message: &ServerMessage) -> bool {
        match message {
            ServerMessage::TerminalCreated(frame) => frame.runtime.is_some(),
            ServerMessage::TerminalAttachReady(frame) => frame.runtime.is_some(),
            ServerMessage::TerminalInventory(frame) => frame
                .terminals
                .iter()
                .all(|terminal| terminal.runtime.is_some()),
            ServerMessage::TerminalOutput(frame) => frame.runtime.is_some(),
            ServerMessage::TerminalOutputBatch(frame) => frame.runtime.is_some(),
            ServerMessage::TerminalOutputGap(frame) => frame.runtime.is_some(),
            ServerMessage::TerminalExit(frame) => frame.runtime.is_some(),
            ServerMessage::TerminalSessionAssociated(frame) => frame.runtime.is_some(),
            ServerMessage::FreshAgentCreated(frame) => frame.runtime.is_some(),
            ServerMessage::FreshAgentEvent(frame) => {
                frame.runtime.is_some()
                    || frame.event.get("type").and_then(serde_json::Value::as_str)
                        == Some("freshAgent.error")
            }
            ServerMessage::FreshAgentSessionMaterialized(frame) => frame.runtime.is_some(),
            ServerMessage::PaneReconcileResult(frame) => frame.verdicts.iter().all(|verdict| {
                verdict.verdict != freshell_protocol::ReconcileVerdict::Attach
                    || verdict.runtime.is_some()
            }),
            _ => true,
        }
    }

    /// Enrich a pre-serialized broadcast frame when it belongs to the typed
    /// protocol. Unknown extension frames pass through byte-for-byte.
    pub fn observe_serialized(&self, json: &str) -> Option<String> {
        let Ok(envelope) = serde_json::from_str::<serde_json::Value>(json) else {
            return Some(json.to_string());
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
            return Some(json.to_string());
        }
        let Ok(mut message) = serde_json::from_str::<ServerMessage>(json) else {
            return Some(json.to_string());
        };
        self.observe_server_message(&mut message);
        if !Self::restart_runtime_contract_satisfied(&message) {
            tracing::error!(
                message = ?message,
                "agent.restart.runtime_descriptor_contract.failed"
            );
            return None;
        }
        Some(serde_json::to_string(&message).unwrap_or_else(|_| json.to_string()))
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
        Self::set_generation_locked(&mut ownership, locator.clone(), descriptor.generation);
        Self::bind_locator_locked(
            &mut ownership,
            locator,
            descriptor.runtime_id.clone(),
            descriptor.clone(),
        );
        Self::prune_ownership_locked(&mut ownership, self.ownership_limit);
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
        let _retention_guard = LockRetentionGuard(self);
        let _request_guard = request_lock.lock_owned().await;
        let _locator_guard = locator_lock.lock_owned().await;

        let pending = self
            .ownership
            .lock()
            .expect("restart ownership lock")
            .pending_recoveries
            .get(&request.request_id)
            .cloned();
        if let Some(pending) = pending {
            if pending.request != request {
                let outcome = self.request_conflict(&request);
                emit(&outcome.messages[0]);
                return outcome;
            }
            if let Some(replacement) = self.runtime_for_locator(&locator).filter(|current| {
                current.runtime_id != pending.old_runtime.runtime_id
                    && current.generation > pending.old_runtime.generation
            }) {
                return self
                    .adopt_pending_replacement_with_events(request, pending, replacement, emit)
                    .await;
            }
            return self
                .recover_pending_with_events(request, pending, runtime, emit)
                .await;
        }

        if let Some(outcome) = self.replay_or_conflict(&request) {
            for message in &outcome.messages {
                emit(message);
            }
            return outcome;
        }

        // A request id previously correlated as a follower has the same replay
        // and conflict semantics as the transaction owner's request id.
        let follower_pending = {
            let ownership = self.ownership.lock().expect("restart ownership lock");
            ownership.pending_recoveries.values().find_map(|pending| {
                pending
                    .followers
                    .iter()
                    .find(|follower| follower.request_id == request.request_id)
                    .map(|follower| (pending.clone(), follower.clone()))
            })
        };
        if let Some((pending, fingerprint)) = follower_pending {
            if fingerprint != request {
                let outcome = self.request_conflict(&request);
                emit(&outcome.messages[0]);
                return outcome;
            }
            if let Some(replacement) = self.runtime_for_locator(&locator).filter(|current| {
                current.runtime_id != pending.old_runtime.runtime_id
                    && current.generation > pending.old_runtime.generation
            }) {
                return self
                    .adopt_pending_replacement_with_events(request, pending, replacement, emit)
                    .await;
            }
            return self
                .recover_pending_with_events(request, pending, runtime, emit)
                .await;
        }

        // Once the owner has retired the runtime, the canonical live locator
        // is intentionally absent. Correlate a distinct requester by its exact
        // old descriptor before ordinary RUNTIME_NOT_FOUND validation, persist
        // that correlation, and let either client drive recovery.
        let correlated_pending = {
            let ownership = self.ownership.lock().expect("restart ownership lock");
            ownership
                .pending_recoveries
                .values()
                .find(|pending| Self::same_restart_target(&pending.request, &request))
                .cloned()
        };
        if let Some(mut pending) = correlated_pending {
            let owner_request_id = pending.request_id.clone();
            if let Err(error) = self.try_update_ownership("join_pending_recovery", |ownership| {
                if let Some(canonical) = ownership.pending_recoveries.get_mut(&owner_request_id) {
                    if !canonical
                        .followers
                        .iter()
                        .any(|follower| follower.request_id == request.request_id)
                    {
                        canonical.followers.push(request.clone());
                    }
                    pending = canonical.clone();
                }
            }) {
                let outcome = self.persistence_failure(&request, pending.old_runtime, error);
                emit(&outcome.messages[0]);
                return outcome;
            }
            return self
                .recover_pending_with_events(request, pending, runtime, emit)
                .await;
        }

        // A distinct requester can race the transaction owner with the exact
        // same locator + old runtime generation. The locator lock intentionally
        // makes the follower wait for that one teardown/replacement. Once it
        // enters, adopt the owner's durable successful result under the
        // follower's own request id instead of misclassifying the now-replaced
        // generation as stale.
        if let Some(outcome) = self.join_completed_replacement(&request) {
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
        let resume_context = runtime.persisted_resume_context(&request, &plan);

        let started = ServerMessage::AgentRestartStarted(AgentRestartStarted {
            request_id: request.request_id.clone(),
            provider: request.provider.clone(),
            session_id: request.session_id.clone(),
            kind: request.kind,
            runtime: expected.clone(),
        });
        if let Err(error) = self.try_update_ownership("record_shutdown_pending", |ownership| {
            ownership.pending_recoveries.insert(
                request.request_id.clone(),
                PendingRestartRecovery {
                    request_id: request.request_id.clone(),
                    request: request.clone(),
                    followers: Vec::new(),
                    old_runtime: expected.clone(),
                    retirement_pending: true,
                    resume_context,
                    last_failure: None,
                },
            );
        }) {
            let outcome = self.persistence_failure(&request, expected, error);
            emit(&outcome.messages[0]);
            return outcome;
        }
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
            tracing::warn!(
                request_id = %request.request_id,
                provider = %request.provider,
                session_id = %request.session_id,
                runtime_id = %expected.runtime_id,
                generation = expected.generation,
                code = ?failure.code,
                retryable = failure.retryable,
                error = %failure.message,
                "agent.restart.shutdown.failed"
            );
            let terminal = ServerMessage::AgentRestartFailed(self.failure_frame(
                &request,
                expected.clone(),
                &failure,
                failure.retryable,
            ));
            let persisted = if failure.retryable {
                self.try_update_ownership("record_retryable_shutdown_failure", |ownership| {
                    if let Some(recovery) =
                        ownership.pending_recoveries.get_mut(&request.request_id)
                    {
                        let ServerMessage::AgentRestartFailed(failed) = &terminal else {
                            unreachable!("shutdown failure frame")
                        };
                        recovery.last_failure = Some(failed.clone());
                    }
                })
            } else {
                self.try_update_ownership("store_failed_shutdown", |ownership| {
                    ownership.pending_recoveries.remove(&request.request_id);
                    self.insert_result_locked(ownership, &request, terminal.clone());
                })
            };
            let mut outcome = match persisted {
                Ok(()) => RestartOutcome {
                    messages: vec![terminal],
                    replayed: false,
                },
                Err(error) => self.persistence_failure_with_phase(
                    &request,
                    expected,
                    error,
                    failure.retryable,
                ),
            };
            emit(&outcome.messages[0]);
            outcome.messages.insert(0, started);
            return outcome;
        }

        if let Err(error) = self.try_update_ownership("record_retirement_complete", |ownership| {
            if let Some(recovery) = ownership.pending_recoveries.get_mut(&request.request_id) {
                recovery.retirement_pending = false;
                recovery.last_failure = None;
            }
        }) {
            let mut outcome = self.persistence_failure_with_phase(&request, expected, error, true);
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
                ownership
                    .retired_live
                    .insert((request.kind, expected.runtime_id.clone()));
                ownership.locator_order.retain(|entry| entry != &locator);
            }
        }

        let replacement_id = match runtime.create_replacement(&request, plan).await {
            Ok(runtime_id) => runtime_id,
            Err(failure) => {
                if failure.retryable {
                    tracing::warn!(
                        request_id = %request.request_id,
                        provider = %request.provider,
                        session_id = %request.session_id,
                        runtime_id = %expected.runtime_id,
                        generation = expected.generation,
                        code = ?failure.code,
                        retryable = true,
                        error = %failure.message,
                        "agent.restart.replacement.retryable_failure"
                    );
                    let failed = self.failure_frame(&request, expected.clone(), &failure, true);
                    let persisted = self.try_update_ownership(
                        "record_retryable_replacement_failure",
                        |ownership| {
                            if let Some(recovery) =
                                ownership.pending_recoveries.get_mut(&request.request_id)
                            {
                                recovery.last_failure = Some(failed.clone());
                            }
                        },
                    );
                    let mut outcome = match persisted {
                        Ok(()) => RestartOutcome {
                            messages: vec![ServerMessage::AgentRestartFailed(failed)],
                            replayed: false,
                        },
                        Err(error) => self.persistence_failure(&request, expected, error),
                    };
                    emit(&outcome.messages[0]);
                    outcome.messages.insert(0, started);
                    return outcome;
                } else {
                    tracing::warn!(
                        request_id = %request.request_id,
                        provider = %request.provider,
                        session_id = %request.session_id,
                        runtime_id = %expected.runtime_id,
                        generation = expected.generation,
                        code = ?failure.code,
                        retryable = failure.retryable,
                        error = %failure.message,
                        "agent.restart.replacement.failed"
                    );
                    let terminal = self.failure_message(&request, expected.clone(), &failure);
                    let persisted = self.try_update_ownership(
                        "store_terminal_replacement_failure",
                        |ownership| {
                            ownership.pending_recoveries.remove(&request.request_id);
                            self.insert_result_locked(ownership, &request, terminal.clone());
                        },
                    );
                    let mut outcome = match persisted {
                        Ok(()) => RestartOutcome {
                            messages: vec![terminal],
                            replayed: false,
                        },
                        Err(error) => self.persistence_failure(&request, expected, error),
                    };
                    emit(&outcome.messages[0]);
                    outcome.messages.insert(0, started);
                    return outcome;
                }
            }
        };
        if replacement_id == expected.runtime_id {
            let failure = RestartFailure::new(
                AgentRestartFailureCode::ReplacementFailed,
                "replacement did not produce a distinct live runtime",
                true,
            );
            tracing::warn!(
                request_id = %request.request_id,
                provider = %request.provider,
                session_id = %request.session_id,
                runtime_id = %expected.runtime_id,
                generation = expected.generation,
                code = ?failure.code,
                retryable = true,
                error = %failure.message,
                "agent.restart.replacement.retryable_failure"
            );
            let failed = self.failure_frame(&request, expected.clone(), &failure, true);
            let persisted =
                self.try_update_ownership("reject_reused_replacement_runtime", |ownership| {
                    if let Some(recovery) =
                        ownership.pending_recoveries.get_mut(&request.request_id)
                    {
                        recovery.last_failure = Some(failed.clone());
                    }
                });
            let mut outcome = match persisted {
                Ok(()) => RestartOutcome {
                    messages: vec![ServerMessage::AgentRestartFailed(failed)],
                    replayed: false,
                },
                Err(error) => self.persistence_failure(&request, expected, error),
            };
            emit(&outcome.messages[0]);
            outcome.messages.insert(0, started);
            return outcome;
        }

        let (_replacement, terminal) =
            match self.commit_replacement(&request, &expected, replacement_id) {
                Ok(committed) => committed,
                Err(error) => {
                    let mut outcome = self.persistence_failure(&request, expected, error);
                    emit(&outcome.messages[0]);
                    outcome.messages.insert(0, started);
                    return outcome;
                }
            };
        emit(&terminal);
        RestartOutcome {
            messages: vec![started, terminal],
            replayed: false,
        }
    }

    fn same_restart_target(left: &AgentRestart, right: &AgentRestart) -> bool {
        left.provider == right.provider
            && left.session_id == right.session_id
            && left.kind == right.kind
            && left.live_id == right.live_id
            && left.expected_generation == right.expected_generation
    }

    fn join_completed_replacement(&self, request: &AgentRestart) -> Option<RestartOutcome> {
        let joined = {
            let ownership = self.ownership.lock().expect("restart ownership lock");
            ownership.results.values().find_map(|stored| {
                if !Self::same_restart_target(&stored.fingerprint, request) {
                    return None;
                }
                match &stored.terminal {
                    ServerMessage::AgentRestartReplaced(replaced) => {
                        let mut correlated = replaced.clone();
                        correlated.request_id = request.request_id.clone();
                        Some(ServerMessage::AgentRestartReplaced(correlated))
                    }
                    _ => None,
                }
            })
        }?;

        if let Err(error) = self.try_update_ownership("join_completed_replacement", |ownership| {
            self.insert_result_locked(ownership, request, joined.clone());
        }) {
            return Some(self.persistence_failure(
                request,
                RuntimeDescriptor {
                    runtime_id: request.live_id.clone(),
                    generation: request.expected_generation,
                },
                error,
            ));
        }
        tracing::info!(
            request_id = %request.request_id,
            provider = %request.provider,
            session_id = %request.session_id,
            old_runtime_id = %request.live_id,
            old_generation = request.expected_generation,
            "agent.restart.joined"
        );
        Some(RestartOutcome {
            messages: vec![joined],
            replayed: true,
        })
    }

    async fn recover_pending_with_events<R, F>(
        &self,
        request: AgentRestart,
        pending: PendingRestartRecovery,
        runtime: &R,
        mut emit: F,
    ) -> RestartOutcome
    where
        R: RestartRuntime + ?Sized,
        F: FnMut(&ServerMessage),
    {
        let started = ServerMessage::AgentRestartStarted(AgentRestartStarted {
            request_id: request.request_id.clone(),
            provider: request.provider.clone(),
            session_id: request.session_id.clone(),
            kind: request.kind,
            runtime: pending.old_runtime.clone(),
        });
        emit(&started);
        if pending.retirement_pending {
            if let Err(failure) = runtime
                .recover_retirement(&request, pending.resume_context.as_ref())
                .await
            {
                tracing::warn!(
                    request_id = %request.request_id,
                    provider = %request.provider,
                    session_id = %request.session_id,
                    old_runtime_id = %pending.old_runtime.runtime_id,
                    old_generation = pending.old_runtime.generation,
                    code = ?failure.code,
                    retryable = failure.retryable,
                    error = %failure.message,
                    "agent.restart.retirement_recovery.failed"
                );
                let failed = self.failure_frame(
                    &request,
                    pending.old_runtime.clone(),
                    &failure,
                    failure.retryable,
                );
                let owner_request_id = pending.request_id.clone();
                let persisted = if failure.retryable {
                    self.try_update_ownership("record_retryable_retirement_failure", |ownership| {
                        if let Some(recovery) =
                            ownership.pending_recoveries.get_mut(&owner_request_id)
                        {
                            recovery.last_failure = Some(failed.clone());
                        }
                    })
                } else {
                    self.try_update_ownership("store_terminal_retirement_failure", |ownership| {
                        let correlated = ownership
                            .pending_recoveries
                            .remove(&owner_request_id)
                            .map(|recovery| {
                                std::iter::once(recovery.request)
                                    .chain(recovery.followers)
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_else(|| vec![request.clone()]);
                        for correlated_request in correlated {
                            let mut correlated_failed = failed.clone();
                            correlated_failed.request_id = correlated_request.request_id.clone();
                            self.insert_result_locked(
                                ownership,
                                &correlated_request,
                                ServerMessage::AgentRestartFailed(correlated_failed),
                            );
                        }
                    })
                };
                let terminal = match persisted {
                    Ok(()) => ServerMessage::AgentRestartFailed(failed),
                    Err(error) => self
                        .persistence_failure_with_phase(
                            &request,
                            pending.old_runtime,
                            error,
                            failure.retryable,
                        )
                        .messages
                        .remove(0),
                };
                emit(&terminal);
                return RestartOutcome {
                    messages: vec![started, terminal],
                    replayed: false,
                };
            }

            let owner_request_id = pending.request_id.clone();
            if let Err(error) =
                self.try_update_ownership("record_recovered_retirement_complete", |ownership| {
                    if let Some(recovery) = ownership.pending_recoveries.get_mut(&owner_request_id)
                    {
                        recovery.retirement_pending = false;
                        recovery.last_failure = None;
                    }
                })
            {
                let terminal = self
                    .persistence_failure_with_phase(&request, pending.old_runtime, error, true)
                    .messages
                    .remove(0);
                emit(&terminal);
                return RestartOutcome {
                    messages: vec![started, terminal],
                    replayed: false,
                };
            }
        }
        {
            let locator = RuntimeLocator::from_request(&request);
            let mut ownership = self.ownership.lock().expect("restart ownership lock");
            if ownership
                .by_locator
                .get(&locator)
                .is_some_and(|current| current == &pending.old_runtime)
            {
                ownership.by_locator.remove(&locator);
                ownership
                    .retired_live
                    .insert((request.kind, pending.old_runtime.runtime_id.clone()));
                ownership.locator_order.retain(|entry| entry != &locator);
            }
        }
        let replacement_id = match runtime
            .recover_replacement(&request, pending.resume_context.as_ref())
            .await
        {
            Ok(runtime_id) => runtime_id,
            Err(failure) => {
                tracing::warn!(
                    request_id = %request.request_id,
                    provider = %request.provider,
                    session_id = %request.session_id,
                    old_runtime_id = %pending.old_runtime.runtime_id,
                    old_generation = pending.old_runtime.generation,
                    code = ?failure.code,
                    retryable = failure.retryable,
                    error = %failure.message,
                    "agent.restart.recovery.failed"
                );
                let failed = self.failure_frame(
                    &request,
                    pending.old_runtime.clone(),
                    &failure,
                    failure.retryable,
                );
                let persisted = if failure.retryable {
                    let owner_request_id = pending.request_id.clone();
                    self.try_update_ownership("record_retryable_recovery_failure", |ownership| {
                        if let Some(recovery) =
                            ownership.pending_recoveries.get_mut(&owner_request_id)
                        {
                            recovery.last_failure = Some(failed.clone());
                        }
                    })
                } else {
                    let owner_request_id = pending.request_id.clone();
                    self.try_update_ownership("store_terminal_recovery_failure", |ownership| {
                        let correlated = ownership
                            .pending_recoveries
                            .remove(&owner_request_id)
                            .map(|recovery| {
                                std::iter::once(recovery.request)
                                    .chain(recovery.followers)
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_else(|| vec![request.clone()]);
                        for correlated_request in correlated {
                            let mut correlated_failed = failed.clone();
                            correlated_failed.request_id = correlated_request.request_id.clone();
                            self.insert_result_locked(
                                ownership,
                                &correlated_request,
                                ServerMessage::AgentRestartFailed(correlated_failed),
                            );
                        }
                    })
                };
                let terminal = match persisted {
                    Ok(()) => ServerMessage::AgentRestartFailed(failed),
                    Err(error) => self
                        .persistence_failure(&request, pending.old_runtime, error)
                        .messages
                        .remove(0),
                };
                emit(&terminal);
                return RestartOutcome {
                    messages: vec![started, terminal],
                    replayed: false,
                };
            }
        };
        if replacement_id == pending.old_runtime.runtime_id {
            let failure = RestartFailure::new(
                AgentRestartFailureCode::ReplacementFailed,
                "replacement did not produce a distinct live runtime",
                true,
            );
            tracing::warn!(
                request_id = %request.request_id,
                provider = %request.provider,
                session_id = %request.session_id,
                old_runtime_id = %pending.old_runtime.runtime_id,
                old_generation = pending.old_runtime.generation,
                code = ?failure.code,
                retryable = true,
                error = %failure.message,
                "agent.restart.recovery.failed"
            );
            let failed = self.failure_frame(&request, pending.old_runtime.clone(), &failure, true);
            let owner_request_id = pending.request_id.clone();
            let persisted =
                self.try_update_ownership("reject_reused_recovery_runtime", |ownership| {
                    if let Some(recovery) = ownership.pending_recoveries.get_mut(&owner_request_id)
                    {
                        recovery.last_failure = Some(failed.clone());
                    }
                });
            let terminal = match persisted {
                Ok(()) => ServerMessage::AgentRestartFailed(failed),
                Err(error) => self
                    .persistence_failure(&request, pending.old_runtime, error)
                    .messages
                    .remove(0),
            };
            emit(&terminal);
            return RestartOutcome {
                messages: vec![started, terminal],
                replayed: false,
            };
        }
        let terminal = match self.commit_replacement(&request, &pending.old_runtime, replacement_id)
        {
            Ok((_replacement, terminal)) => terminal,
            Err(error) => {
                let terminal = self
                    .persistence_failure(&request, pending.old_runtime, error)
                    .messages
                    .remove(0);
                emit(&terminal);
                return RestartOutcome {
                    messages: vec![started, terminal],
                    replayed: false,
                };
            }
        };
        emit(&terminal);
        RestartOutcome {
            messages: vec![started, terminal],
            replayed: false,
        }
    }

    async fn adopt_pending_replacement_with_events<F>(
        &self,
        request: AgentRestart,
        pending: PendingRestartRecovery,
        replacement: RuntimeDescriptor,
        mut emit: F,
    ) -> RestartOutcome
    where
        F: FnMut(&ServerMessage),
    {
        let started = ServerMessage::AgentRestartStarted(AgentRestartStarted {
            request_id: request.request_id.clone(),
            provider: request.provider.clone(),
            session_id: request.session_id.clone(),
            kind: request.kind,
            runtime: pending.old_runtime.clone(),
        });
        tracing::info!(
            request_id = %request.request_id,
            provider = %request.provider,
            session_id = %request.session_id,
            old_runtime_id = %pending.old_runtime.runtime_id,
            old_generation = pending.old_runtime.generation,
            runtime_id = %replacement.runtime_id,
            generation = replacement.generation,
            "agent.restart.recovery.adopting_registered_replacement"
        );
        emit(&started);
        let terminal =
            match self.commit_replacement(&request, &pending.old_runtime, replacement.runtime_id) {
                Ok((_replacement, terminal)) => terminal,
                Err(error) => {
                    let terminal = self
                        .persistence_failure(&request, pending.old_runtime, error)
                        .messages
                        .remove(0);
                    emit(&terminal);
                    return RestartOutcome {
                        messages: vec![started, terminal],
                        replayed: false,
                    };
                }
            };
        emit(&terminal);
        RestartOutcome {
            messages: vec![started, terminal],
            replayed: false,
        }
    }

    fn commit_replacement(
        &self,
        request: &AgentRestart,
        old_runtime: &RuntimeDescriptor,
        replacement_id: String,
    ) -> std::io::Result<(RuntimeDescriptor, ServerMessage)> {
        if replacement_id == old_runtime.runtime_id {
            return Err(std::io::Error::other(
                "replacement reported the retired runtime id",
            ));
        }
        let locator = RuntimeLocator::from_request(request);
        let replacement = {
            let ownership = self.ownership.lock().expect("restart ownership lock");
            let observed = ownership.by_locator.get(&locator).filter(|current| {
                current.runtime_id == replacement_id && current.generation > old_runtime.generation
            });
            observed.cloned().unwrap_or_else(|| RuntimeDescriptor {
                runtime_id: replacement_id.clone(),
                generation: ownership
                    .last_generation
                    .get(&locator)
                    .copied()
                    .unwrap_or(old_runtime.generation)
                    .max(old_runtime.generation)
                    .saturating_add(1),
            })
        };
        let terminal = ServerMessage::AgentRestartReplaced(AgentRestartReplaced {
            request_id: request.request_id.clone(),
            provider: request.provider.clone(),
            session_id: request.session_id.clone(),
            kind: request.kind,
            old_runtime: OldRuntimeDescriptor::from(old_runtime.clone()),
            runtime: replacement.clone(),
        });
        self.try_update_ownership("commit_replacement", |ownership| {
            let pending_keys = ownership
                .pending_recoveries
                .iter()
                .filter(|(_, pending)| Self::same_restart_target(&pending.request, request))
                .map(|(key, _)| key.clone())
                .collect::<Vec<_>>();
            let mut correlated_requests = vec![request.clone()];
            for key in pending_keys {
                if let Some(pending) = ownership.pending_recoveries.remove(&key) {
                    correlated_requests.push(pending.request);
                    correlated_requests.extend(pending.followers);
                }
            }
            correlated_requests.sort_by(|left, right| left.request_id.cmp(&right.request_id));
            correlated_requests.dedup_by(|left, right| left.request_id == right.request_id);

            let replacement_key = (request.kind, replacement.runtime_id.clone());
            ownership
                .descriptor_by_live
                .insert(replacement_key.clone(), replacement.clone());
            Self::touch_descriptor_locked(ownership, replacement_key);
            Self::set_generation_locked(ownership, locator.clone(), replacement.generation);
            Self::bind_locator_locked(
                ownership,
                locator,
                replacement.runtime_id.clone(),
                replacement.clone(),
            );
            for correlated_request in correlated_requests {
                let mut correlated_terminal = match &terminal {
                    ServerMessage::AgentRestartReplaced(message) => message.clone(),
                    _ => unreachable!("restart replacement commit stores a replaced frame"),
                };
                correlated_terminal.request_id = correlated_request.request_id.clone();
                self.insert_result_locked(
                    ownership,
                    &correlated_request,
                    ServerMessage::AgentRestartReplaced(correlated_terminal),
                );
            }
            Self::prune_ownership_locked(ownership, self.ownership_limit);
        })?;
        tracing::info!(
            request_id = %request.request_id,
            provider = %request.provider,
            session_id = %request.session_id,
            runtime_id = %replacement.runtime_id,
            generation = replacement.generation,
            "agent.restart.replaced"
        );
        Ok((replacement, terminal))
    }

    fn failure_frame(
        &self,
        request: &AgentRestart,
        descriptor: RuntimeDescriptor,
        failure: &RestartFailure,
        recovery_pending: bool,
    ) -> AgentRestartFailed {
        AgentRestartFailed {
            request_id: request.request_id.clone(),
            provider: request.provider.clone(),
            session_id: request.session_id.clone(),
            kind: request.kind,
            runtime: descriptor,
            code: failure.code,
            message: failure.message.clone(),
            retryable: failure.retryable,
            recovery_pending,
        }
    }

    fn failure_message(
        &self,
        request: &AgentRestart,
        descriptor: RuntimeDescriptor,
        failure: &RestartFailure,
    ) -> ServerMessage {
        ServerMessage::AgentRestartFailed(self.failure_frame(request, descriptor, failure, false))
    }

    fn persistence_failure(
        &self,
        request: &AgentRestart,
        descriptor: RuntimeDescriptor,
        error: std::io::Error,
    ) -> RestartOutcome {
        self.persistence_failure_with_phase(request, descriptor, error, false)
    }

    fn persistence_failure_with_phase(
        &self,
        request: &AgentRestart,
        descriptor: RuntimeDescriptor,
        error: std::io::Error,
        recovery_pending: bool,
    ) -> RestartOutcome {
        RestartOutcome {
            messages: vec![ServerMessage::AgentRestartFailed(AgentRestartFailed {
                request_id: request.request_id.clone(),
                provider: request.provider.clone(),
                session_id: request.session_id.clone(),
                kind: request.kind,
                runtime: descriptor,
                code: AgentRestartFailureCode::PreflightFailed,
                message: format!("restart transaction could not be durably recorded: {error}"),
                retryable: true,
                recovery_pending,
            })],
            replayed: false,
        }
    }

    fn request_conflict(&self, request: &AgentRestart) -> RestartOutcome {
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
        RestartOutcome {
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
                recovery_pending: false,
            })],
            replayed: true,
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
        Some(self.request_conflict(request))
    }

    fn store_failure(
        &self,
        request: AgentRestart,
        descriptor: RuntimeDescriptor,
        failure: RestartFailure,
        replayed: bool,
    ) -> RestartOutcome {
        let terminal = self.failure_message(&request, descriptor.clone(), &failure);
        tracing::warn!(
            request_id = %request.request_id,
            provider = %request.provider,
            session_id = %request.session_id,
            code = ?failure.code,
            retryable = failure.retryable,
            "agent.restart.failed"
        );
        match self.try_update_ownership("store_terminal_failure", |ownership| {
            self.insert_result_locked(ownership, &request, terminal.clone());
        }) {
            Ok(()) => RestartOutcome {
                messages: vec![terminal],
                replayed,
            },
            Err(error) => self.persistence_failure(&request, descriptor, error),
        }
    }
}

#[derive(Clone)]
struct ProductionResumePlan {
    context: RestartResumeContext,
}

/// Exact fresh-agent creation inputs captured while the selected runtime is
/// still live. The production implementation joins the live manager's flavour
/// with the durable pane-ledger settings; test adapters provide the same
/// contract without launching external providers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductionFreshResumePlan {
    pub session_type: freshell_protocol::SessionType,
    pub cwd: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub permission_mode: Option<String>,
    pub sandbox: Option<freshell_protocol::Sandbox>,
}

/// Narrow production seam over the three fresh-agent runtime managers.
///
/// Keeping this seam at the adapter boundary lets integration tests exercise
/// the real [`ProductionRestartRuntime`] ordering and broadcast correlation
/// without launching external Claude, Codex, or OpenCode binaries.
#[async_trait::async_trait]
pub trait ProductionFreshRuntime: Send + Sync {
    async fn has_live_session(
        &self,
        provider: freshell_protocol::AgentProvider,
        session_id: &str,
    ) -> bool;

    async fn shutdown_for_restart(
        &self,
        provider: freshell_protocol::AgentProvider,
        session_id: &str,
        expected_runtime_id: &str,
    ) -> bool;

    async fn shutdown_for_restart_detailed(
        &self,
        provider: freshell_protocol::AgentProvider,
        session_id: &str,
        expected_runtime_id: &str,
    ) -> freshell_freshagent::RestartShutdownOutcome {
        if self
            .shutdown_for_restart(provider, session_id, expected_runtime_id)
            .await
        {
            freshell_freshagent::RestartShutdownOutcome::Stopped
        } else {
            freshell_freshagent::RestartShutdownOutcome::Stale
        }
    }

    async fn capture_resume_plan(
        &self,
        provider: freshell_protocol::AgentProvider,
        session_id: &str,
    ) -> Option<ProductionFreshResumePlan>;

    async fn handle_create(
        &self,
        provider: freshell_protocol::AgentProvider,
        create: freshell_protocol::FreshAgentCreate,
    );
}

struct WsStateFreshRuntime {
    state: crate::WsState,
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
impl ProductionFreshRuntime for WsStateFreshRuntime {
    async fn has_live_session(
        &self,
        provider: freshell_protocol::AgentProvider,
        session_id: &str,
    ) -> bool {
        match provider {
            freshell_protocol::AgentProvider::Claude => {
                self.state.fresh_claude.has_live_session(session_id).await
            }
            freshell_protocol::AgentProvider::Codex => {
                self.state.fresh_codex.has_live_session(session_id).await
            }
            freshell_protocol::AgentProvider::Opencode => {
                self.state.fresh_opencode.has_live_session(session_id).await
            }
            freshell_protocol::AgentProvider::Amplifier => false,
        }
    }

    async fn shutdown_for_restart(
        &self,
        provider: freshell_protocol::AgentProvider,
        session_id: &str,
        expected_runtime_id: &str,
    ) -> bool {
        self.shutdown_for_restart_detailed(provider, session_id, expected_runtime_id)
            .await
            == freshell_freshagent::RestartShutdownOutcome::Stopped
    }

    async fn shutdown_for_restart_detailed(
        &self,
        provider: freshell_protocol::AgentProvider,
        session_id: &str,
        expected_runtime_id: &str,
    ) -> freshell_freshagent::RestartShutdownOutcome {
        match provider {
            freshell_protocol::AgentProvider::Claude => {
                self.state
                    .fresh_claude
                    .shutdown_for_restart_detailed(session_id, expected_runtime_id)
                    .await
            }
            freshell_protocol::AgentProvider::Codex => {
                self.state
                    .fresh_codex
                    .shutdown_for_restart_detailed(session_id, expected_runtime_id)
                    .await
            }
            freshell_protocol::AgentProvider::Opencode => {
                if self
                    .state
                    .fresh_opencode
                    .shutdown_for_restart(session_id, expected_runtime_id)
                    .await
                {
                    freshell_freshagent::RestartShutdownOutcome::Stopped
                } else {
                    freshell_freshagent::RestartShutdownOutcome::Stale
                }
            }
            freshell_protocol::AgentProvider::Amplifier => {
                freshell_freshagent::RestartShutdownOutcome::Stale
            }
        }
    }

    async fn capture_resume_plan(
        &self,
        provider: freshell_protocol::AgentProvider,
        session_id: &str,
    ) -> Option<ProductionFreshResumePlan> {
        let session_type = match provider {
            freshell_protocol::AgentProvider::Claude => {
                self.state
                    .fresh_claude
                    .live_session_type(session_id)
                    .await?
            }
            freshell_protocol::AgentProvider::Codex => self
                .state
                .fresh_codex
                .has_live_session(session_id)
                .await
                .then_some(freshell_protocol::SessionType::Freshcodex)?,
            freshell_protocol::AgentProvider::Opencode => self
                .state
                .fresh_opencode
                .has_live_session(session_id)
                .await
                .then_some(freshell_protocol::SessionType::Freshopencode)?,
            freshell_protocol::AgentProvider::Amplifier => return None,
        };
        let binding = self
            .state
            .pane_ledger
            .load_binding(provider_name(provider), session_id);
        let sandbox = match binding.as_ref().and_then(|row| row.sandbox.as_deref()) {
            None => None,
            Some("read-only") => Some(freshell_protocol::Sandbox::ReadOnly),
            Some("workspace-write") => Some(freshell_protocol::Sandbox::WorkspaceWrite),
            Some("danger-full-access") => Some(freshell_protocol::Sandbox::DangerFullAccess),
            Some(value) => {
                tracing::error!(
                    provider = provider_name(provider),
                    session_id,
                    sandbox = value,
                    "agent.restart.preflight.invalid_fresh_sandbox"
                );
                return None;
            }
        };
        Some(ProductionFreshResumePlan {
            session_type,
            cwd: binding.as_ref().and_then(|row| row.cwd.clone()),
            model: binding.as_ref().and_then(|row| row.model.clone()),
            effort: binding.as_ref().and_then(|row| row.effort.clone()),
            permission_mode: binding.as_ref().and_then(|row| row.permission_mode.clone()),
            sandbox,
        })
    }

    async fn handle_create(
        &self,
        provider: freshell_protocol::AgentProvider,
        create: freshell_protocol::FreshAgentCreate,
    ) {
        match provider {
            freshell_protocol::AgentProvider::Claude => {
                self.state.fresh_claude.handle_create(create).await
            }
            freshell_protocol::AgentProvider::Codex => {
                self.state.fresh_codex.handle_create(create).await
            }
            freshell_protocol::AgentProvider::Opencode => {
                self.state.fresh_opencode.handle_create(create).await
            }
            freshell_protocol::AgentProvider::Amplifier => {}
        }
    }
}

/// Production adapter that deliberately delegates to the same built-in
/// terminal restore and fresh-agent resume paths used by ordinary clients.
pub struct ProductionRestartRuntime {
    state: crate::WsState,
    plans: Mutex<HashMap<String, ProductionResumePlan>>,
    fresh_runtime: Arc<dyn ProductionFreshRuntime>,
}

impl ProductionRestartRuntime {
    pub fn new(state: crate::WsState) -> Self {
        let fresh_runtime = Arc::new(WsStateFreshRuntime {
            state: state.clone(),
        });
        Self::with_fresh_runtime(state, fresh_runtime)
    }

    /// Construct the production adapter around a faithful fresh-runtime seam.
    /// Production uses [`Self::new`]; integration tests inject deterministic
    /// managers while preserving the adapter's real preflight, teardown,
    /// resume request, broadcast correlation, and generation commit paths.
    pub fn with_fresh_runtime(
        state: crate::WsState,
        fresh_runtime: Arc<dyn ProductionFreshRuntime>,
    ) -> Self {
        Self {
            state,
            plans: Mutex::new(HashMap::new()),
            fresh_runtime,
        }
    }

    fn provider(
        request: &AgentRestart,
    ) -> Result<
        (
            freshell_protocol::AgentProvider,
            freshell_protocol::SessionType,
        ),
        RestartFailure,
    > {
        match request.provider.as_str() {
            "claude" => Ok((
                freshell_protocol::AgentProvider::Claude,
                freshell_protocol::SessionType::Freshclaude,
            )),
            "codex" => Ok((
                freshell_protocol::AgentProvider::Codex,
                freshell_protocol::SessionType::Freshcodex,
            )),
            "opencode" => Ok((
                freshell_protocol::AgentProvider::Opencode,
                freshell_protocol::SessionType::Freshopencode,
            )),
            "amplifier" if request.kind == AgentRuntimeKind::Terminal => Ok((
                freshell_protocol::AgentProvider::Amplifier,
                freshell_protocol::SessionType::Freshclaude,
            )),
            _ => Err(RestartFailure::new(
                AgentRestartFailureCode::Unresumable,
                format!(
                    "{} is not an approved built-in provider for {:?} restart",
                    request.provider, request.kind
                ),
                false,
            )),
        }
    }

    fn validate_durable(&self, request: &AgentRestart) -> Result<(), RestartFailure> {
        use crate::existence::SessionExistence;
        match self
            .state
            .session_existence
            .exists(&request.provider, &request.session_id)
        {
            SessionExistence::Present => Ok(()),
            SessionExistence::Unknown => Err(RestartFailure::new(
                AgentRestartFailureCode::PreflightFailed,
                "session index is still warming; retry restart shortly",
                true,
            )),
            SessionExistence::Absent | SessionExistence::ProviderUnavailable => {
                Err(RestartFailure::new(
                    AgentRestartFailureCode::Unresumable,
                    "durable session is unavailable",
                    false,
                ))
            }
        }
    }

    async fn fresh_is_live(
        &self,
        provider: freshell_protocol::AgentProvider,
        session_id: &str,
    ) -> bool {
        self.fresh_runtime
            .has_live_session(provider, session_id)
            .await
    }

    async fn create_fresh_replacement(
        &self,
        request: &AgentRestart,
        provider: freshell_protocol::AgentProvider,
        plan: &RestartResumeContext,
    ) -> Result<String, RestartFailure> {
        let create_request_id = format!(
            "agent-restart:{}:{}",
            request.request_id,
            uuid::Uuid::new_v4()
        );
        let mut receiver = self.state.broadcast_tx.subscribe();
        let create = freshell_protocol::FreshAgentCreate {
            request_id: create_request_id.clone(),
            session_type: plan.fresh_session_type.ok_or_else(|| {
                RestartFailure::new(
                    AgentRestartFailureCode::ReplacementFailed,
                    "fresh-agent restart plan omitted its runtime flavour",
                    false,
                )
            })?,
            cwd: plan.fresh_cwd.clone(),
            effort: plan.fresh_effort.clone(),
            legacy_restore_context: None,
            model: plan.fresh_model.clone(),
            model_selection: None,
            permission_mode: plan.fresh_permission_mode.clone(),
            plugins: None,
            provider: Some(provider),
            resume_session_id: Some(request.session_id.clone()),
            sandbox: plan.fresh_sandbox,
            session_ref: Some(freshell_protocol::SessionLocator {
                provider: request.provider.clone(),
                session_id: request.session_id.clone(),
            }),
        };
        self.fresh_runtime.handle_create(provider, create).await;
        let result_deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let frame = tokio::time::timeout_at(result_deadline, receiver.recv())
                .await
                .map_err(|_| {
                    RestartFailure::new(
                        AgentRestartFailureCode::ReplacementFailed,
                        "fresh-agent replacement did not report a result",
                        true,
                    )
                })?
                .map_err(|error| {
                    RestartFailure::new(
                        AgentRestartFailureCode::ReplacementFailed,
                        format!("fresh-agent replacement result was lost: {error}"),
                        true,
                    )
                })?;
            let Ok(message) = serde_json::from_str::<ServerMessage>(&frame) else {
                continue;
            };
            match message {
                ServerMessage::FreshAgentCreated(created)
                    if created.request_id == create_request_id =>
                {
                    let runtime_id = created
                        .runtime
                        .map(|runtime| runtime.runtime_id)
                        .ok_or_else(|| {
                            RestartFailure::new(
                                AgentRestartFailureCode::ReplacementFailed,
                                "fresh-agent replacement omitted its runtime descriptor",
                                true,
                            )
                        })?;
                    if runtime_id == request.live_id
                        || !self.fresh_is_live(provider, &request.session_id).await
                    {
                        return Err(RestartFailure::new(
                            AgentRestartFailureCode::ReplacementFailed,
                            "fresh-agent replacement did not become a distinct live runtime",
                            true,
                        ));
                    }
                    return Ok(runtime_id);
                }
                ServerMessage::FreshAgentCreateFailed(failed)
                    if failed.request_id == create_request_id =>
                {
                    return Err(RestartFailure::new(
                        AgentRestartFailureCode::ReplacementFailed,
                        failed.message,
                        failed.retryable.unwrap_or(false),
                    ));
                }
                _ => {}
            }
        }
    }

    async fn create_from_builtin_path(
        &self,
        request: &AgentRestart,
        plan: ProductionResumePlan,
    ) -> Result<String, RestartFailure> {
        let (provider, _) = Self::provider(request)?;
        match request.kind {
            AgentRuntimeKind::Terminal => crate::terminal::create_terminal_replacement(
                &self.state,
                request,
                plan.context.terminal_cwd.clone(),
                freshell_terminal::TerminalRestartLaunch {
                    shell: plan
                        .context
                        .terminal_shell
                        .unwrap_or(freshell_protocol::Shell::System),
                    permission_mode: plan.context.terminal_permission_mode.clone(),
                    model: plan.context.terminal_model.clone(),
                    sandbox: plan.context.terminal_sandbox.clone(),
                    codex_managed: plan.context.terminal_codex_managed,
                },
            )
            .await
            .map_err(|message| {
                RestartFailure::new(AgentRestartFailureCode::ReplacementFailed, message, true)
            }),
            AgentRuntimeKind::FreshAgent => {
                self.create_fresh_replacement(request, provider, &plan.context)
                    .await
            }
        }
    }
}

#[async_trait::async_trait]
impl RestartRuntime for ProductionRestartRuntime {
    type ResumePlan = ();

    async fn preflight(&self, request: &AgentRestart) -> Result<(), RestartFailure> {
        let (provider, _) = Self::provider(request)?;
        self.validate_durable(request)?;
        let context = match request.kind {
            AgentRuntimeKind::Terminal => {
                let probe = self.state.registry.probe(&request.live_id).ok_or_else(|| {
                    RestartFailure::new(
                        AgentRestartFailureCode::RuntimeNotFound,
                        "selected terminal is no longer live",
                        false,
                    )
                })?;
                if probe.mode != request.provider {
                    return Err(RestartFailure::new(
                        AgentRestartFailureCode::StaleGeneration,
                        "selected terminal provider does not match restart request",
                        false,
                    ));
                }
                if !self
                    .state
                    .cli_commands
                    .iter()
                    .any(|spec| spec.name == request.provider)
                {
                    return Err(RestartFailure::new(
                        AgentRestartFailureCode::Unresumable,
                        "built-in terminal provider is not installed",
                        false,
                    ));
                }
                let launch = probe.restart_launch.ok_or_else(|| {
                    RestartFailure::new(
                        AgentRestartFailureCode::PreflightFailed,
                        "selected terminal is missing its authoritative restart settings",
                        false,
                    )
                })?;
                RestartResumeContext {
                    terminal_cwd: probe.cwd,
                    terminal_shell: Some(launch.shell),
                    terminal_permission_mode: launch.permission_mode,
                    terminal_model: launch.model,
                    terminal_sandbox: launch.sandbox,
                    terminal_codex_managed: launch.codex_managed,
                    ..RestartResumeContext::default()
                }
            }
            AgentRuntimeKind::FreshAgent => {
                if !self.fresh_is_live(provider, &request.session_id).await {
                    return Err(RestartFailure::new(
                        AgentRestartFailureCode::RuntimeNotFound,
                        "selected fresh-agent runtime is no longer live",
                        false,
                    ));
                }
                let plan = self
                    .fresh_runtime
                    .capture_resume_plan(provider, &request.session_id)
                    .await
                    .ok_or_else(|| {
                        RestartFailure::new(
                            AgentRestartFailureCode::PreflightFailed,
                            "selected fresh-agent runtime settings are unavailable",
                            false,
                        )
                    })?;
                RestartResumeContext {
                    fresh_session_type: Some(plan.session_type),
                    fresh_cwd: plan.cwd,
                    fresh_model: plan.model,
                    fresh_effort: plan.effort,
                    fresh_permission_mode: plan.permission_mode,
                    fresh_sandbox: plan.sandbox,
                    ..RestartResumeContext::default()
                }
            }
        };
        let mut plans = self.plans.lock().expect("production restart plans");
        if plans.len() >= DEFAULT_RESTART_LOCK_LIMIT {
            if let Some(oldest) = plans.keys().next().cloned() {
                plans.remove(&oldest);
            }
        }
        plans.insert(request.request_id.clone(), ProductionResumePlan { context });
        Ok(())
    }

    async fn shutdown_for_restart(
        &self,
        request: &AgentRestart,
        _plan: &(),
    ) -> Result<(), RestartFailure> {
        let (provider, _) = Self::provider(request)?;
        match request.kind {
            AgentRuntimeKind::Terminal => {
                match crate::terminal::shutdown_terminal_for_restart(&self.state, &request.live_id)
                    .await
                {
                    Ok(true) => {}
                    Ok(false) => {
                        return Err(RestartFailure::new(
                            AgentRestartFailureCode::StaleGeneration,
                            "selected terminal disappeared before shutdown",
                            false,
                        ));
                    }
                    Err(message) => {
                        return Err(RestartFailure::new(
                            AgentRestartFailureCode::ShutdownFailed,
                            message,
                            true,
                        ));
                    }
                }
            }
            AgentRuntimeKind::FreshAgent => {
                let outcome = self
                    .fresh_runtime
                    .shutdown_for_restart_detailed(provider, &request.session_id, &request.live_id)
                    .await;
                match outcome {
                    freshell_freshagent::RestartShutdownOutcome::Stopped => {}
                    freshell_freshagent::RestartShutdownOutcome::Stale => {
                        return Err(RestartFailure::new(
                            AgentRestartFailureCode::StaleGeneration,
                            "selected fresh-agent runtime changed before shutdown",
                            false,
                        ));
                    }
                    freshell_freshagent::RestartShutdownOutcome::RetirementIncomplete => {
                        return Err(RestartFailure::new(
                            AgentRestartFailureCode::ShutdownFailed,
                            "selected fresh-agent runtime retirement is not yet quiescent",
                            true,
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    async fn create_replacement(
        &self,
        request: &AgentRestart,
        _plan: (),
    ) -> Result<String, RestartFailure> {
        let plan = self
            .plans
            .lock()
            .expect("production restart plans")
            .remove(&request.request_id)
            .ok_or_else(|| {
                RestartFailure::new(
                    AgentRestartFailureCode::ReplacementFailed,
                    "restart resume plan disappeared before replacement",
                    true,
                )
            })?;
        self.create_from_builtin_path(request, plan).await
    }

    fn persisted_resume_context(
        &self,
        request: &AgentRestart,
        _plan: &(),
    ) -> Option<RestartResumeContext> {
        self.plans
            .lock()
            .expect("production restart plans")
            .get(&request.request_id)
            .map(|plan| plan.context.clone())
    }

    async fn recover_replacement(
        &self,
        request: &AgentRestart,
        context: Option<&RestartResumeContext>,
    ) -> Result<String, RestartFailure> {
        Self::provider(request)?;
        self.validate_durable(request)?;
        self.create_from_builtin_path(
            request,
            ProductionResumePlan {
                context: context.cloned().ok_or_else(|| {
                    RestartFailure::new(
                        AgentRestartFailureCode::ReplacementFailed,
                        "persisted restart resume plan is unavailable",
                        false,
                    )
                })?,
            },
        )
        .await
    }

    async fn recover_retirement(
        &self,
        request: &AgentRestart,
        _context: Option<&RestartResumeContext>,
    ) -> Result<(), RestartFailure> {
        // Production teardown owns a quarantined, runtime-fenced retirement;
        // it does not need to re-run live preflight after destructive removal.
        self.shutdown_for_restart(request, &()).await
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
