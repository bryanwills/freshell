//! Bounded auto-resume for crashed coding-agent terminals (Lane D1).
//!
//! Policy: a coding-agent terminal (mode ∈ AUTO_RESUME_MODES) that exits
//! NON-ZERO is auto-resumed up to `delays.len()` times with backoff, from its
//! server-side identity (identity registry / pane ledger). Clean exits
//! (code 0) and user kills (structurally excluded upstream — `kill_internal`
//! removes the registry row so `finish_pty_exit` returns `false` and no
//! CrashEvent is ever sent) NEVER auto-resume. The registry's
//! respawn-generation cap is the outer loop bound (campaign plan §7.5).
//! Schedule shape mirrors the repo exemplar `activity.rs::lane_retry_delay_ms`.
//!
//! Coverage boundary: only WS-created terminals feed CrashEvents — their exit
//! hook is built by `terminal::build_pty_exit_hook`. REST/freshagent-created
//! agent panes (`freshell-freshagent/src/terminal_tabs.rs`'s own exit hook)
//! are out of scope for auto-resume in this lane and keep today's behavior.
//! (Both hooks funnel through `finish_pty_exit`, so a future registry-layer
//! observation could cover all paths; recorded as future work.)

pub(crate) const AUTO_RESUME_MODES: [&str; 4] = ["claude", "codex", "opencode", "amplifier"];

/// Backoff before retry N (index = attempts already made). 2 retries max
/// per user ruling 2026-07-27. After the last entry: exhausted and LOUD.
pub(crate) const AUTO_RESUME_DEFAULT_DELAYS_MS: [u64; 2] = [2_000, 10_000];

/// A crashed generation that lived at least this long proves the previous
/// resume was healthy — the attempt counter resets (mirrors
/// `DEFAULT_RESPAWN_LIVENESS_WINDOW_MS` in freshell-terminal).
pub(crate) const AUTO_RESUME_HEALTHY_LIFETIME_MS: i64 = 30_000;

/// Crash notification from the PTY exit hook. Only sent for NATURAL exits
/// (`finish_pty_exit` returned `true`) — user kills never produce one.
/// `pub` (not `pub(crate)`): it rides the public `WsState.auto_resume_tx`
/// field, and integration tests drain it until the hub (Task 5) exists.
#[derive(Debug, Clone)]
pub struct CrashEvent {
    pub terminal_id: String,
    pub exit_code: i64,
    pub mode: String,
    pub create_request_id: Option<String>,
    /// `now - created_at` of the generation that just died.
    pub lifetime_ms: i64,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CrashContext<'a> {
    pub exit_code: i64,
    pub mode: &'a str,
    pub create_request_id: Option<&'a str>,
    pub has_resumable_identity: bool,
    pub lifetime_ms: i64,
    /// Consecutive auto-resume attempts already made for this createRequestId.
    pub prior_attempts: u32,
    /// `registry.respawn_exhausted(create_request_id)` — outer loop bound.
    pub cap_exhausted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutoResumeDecision {
    Resume { attempt: u32, delay_ms: u64 },
    SettleExited { reason: &'static str },
}

pub(crate) fn decide(ctx: &CrashContext<'_>, delays: &[u64]) -> AutoResumeDecision {
    use AutoResumeDecision::SettleExited;
    if ctx.exit_code == 0 {
        return SettleExited {
            reason: "clean_exit",
        };
    }
    if !AUTO_RESUME_MODES.contains(&ctx.mode) {
        return SettleExited {
            reason: "not_agent_mode",
        };
    }
    if ctx.create_request_id.is_none() {
        return SettleExited {
            reason: "no_create_request_id",
        };
    }
    if !ctx.has_resumable_identity {
        return SettleExited {
            reason: "no_resumable_identity",
        };
    }
    if ctx.cap_exhausted {
        return SettleExited {
            reason: "respawn_cap_exhausted",
        };
    }
    let effective_prior = if ctx.lifetime_ms >= AUTO_RESUME_HEALTHY_LIFETIME_MS {
        0
    } else {
        ctx.prior_attempts
    };
    match delays.get(effective_prior as usize).copied() {
        Some(delay_ms) => AutoResumeDecision::Resume {
            attempt: effective_prior + 1,
            delay_ms,
        },
        None => SettleExited {
            reason: "retries_exhausted",
        },
    }
}

/// `FRESHELL_AUTO_RESUME_DELAYS_MS="2000,10000"` — e2e tests set tiny values.
pub(crate) fn parse_delays_env(raw: &str) -> Option<Vec<u64>> {
    let parsed: Option<Vec<u64>> = raw
        .split(',')
        .map(|s| s.trim().parse::<u64>().ok().filter(|v| *v > 0))
        .collect();
    parsed.filter(|v| !v.is_empty())
}

pub(crate) fn auto_resume_delays() -> Vec<u64> {
    match std::env::var("FRESHELL_AUTO_RESUME_DELAYS_MS") {
        Ok(raw) => parse_delays_env(&raw).unwrap_or_else(|| {
            // Misconfiguration (e.g. trailing comma, non-numeric, zero) must
            // be observable — the override silently reverting to defaults is
            // otherwise indistinguishable from the env var not being set.
            tracing::warn!(
                raw,
                "FRESHELL_AUTO_RESUME_DELAYS_MS is set but unparseable — falling back to default delays"
            );
            AUTO_RESUME_DEFAULT_DELAYS_MS.to_vec()
        }),
        Err(_) => AUTO_RESUME_DEFAULT_DELAYS_MS.to_vec(),
    }
}

/// The auto-resume orchestrator: consumes [`CrashEvent`]s, applies
/// [`decide`], and drives the retry pipeline (recovering frame → backoff →
/// post-sleep guards → lease claim → respawn → lease completion → replaced
/// frame) through an [`AutoResumeDriver`].
/// Backoff (ms) between hub-body restarts after a driver panic — escalating so
/// a hot-panicking driver cannot spin a restart loop, capped at the last entry
/// so auto-resume is NEVER permanently lost (council 7w4h/xkhx, crusty: an
/// unsupervised panic silently ending auto-resume forever would reinstate the
/// exact overnight-grey-pane incident this feature prevents). The counter
/// resets after a body that ran healthy for [`AUTO_RESUME_HEALTHY_LIFETIME_MS`].
const HUB_SUPERVISOR_BACKOFF_MS: &[u64] = &[1_000, 5_000, 30_000, 60_000];

pub(crate) fn spawn_hub_with_driver<D: AutoResumeDriver + Sync>(
    driver: D,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<CrashEvent>,
    delays: Vec<u64>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        // SUPERVISOR: `rx` and the attempts map are owned HERE, outside the
        // catch_unwind boundary, so a driver panic mid-event drops only the
        // in-flight body future — the crash-event channel (whose senders live
        // in every PTY exit hook) and the retry bookkeeping both survive the
        // restart. (Respawning with a fresh channel would NOT work: exit
        // hooks clone the sender at hook-build time.)
        let mut attempts: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
        let mut consecutive_panics: u32 = 0;
        loop {
            let body_started = std::time::Instant::now();
            let body = std::panic::AssertUnwindSafe(run_hub_body(
                &driver,
                &mut rx,
                &delays,
                &mut attempts,
            ));
            match futures_util::FutureExt::catch_unwind(body).await {
                // Channel closed: every sender dropped (server shutdown).
                Ok(()) => return,
                Err(panic) => {
                    if body_started.elapsed().as_millis() as i64 >= AUTO_RESUME_HEALTHY_LIFETIME_MS
                    {
                        consecutive_panics = 0;
                    }
                    let idx =
                        (consecutive_panics as usize).min(HUB_SUPERVISOR_BACKOFF_MS.len() - 1);
                    let backoff_ms = HUB_SUPERVISOR_BACKOFF_MS[idx];
                    consecutive_panics = consecutive_panics.saturating_add(1);
                    let message: String = panic
                        .downcast_ref::<&'static str>()
                        .map(|s| (*s).to_string())
                        .or_else(|| panic.downcast_ref::<String>().cloned())
                        .unwrap_or_else(|| "<non-string panic payload>".to_string());
                    // The payload box is `dyn Any + Send` (not Sync): drop it
                    // BEFORE the backoff await so this future stays Send.
                    drop(panic);
                    tracing::error!(
                        panic = %message,
                        consecutive_panics,
                        restart_in_ms = backoff_ms,
                        "terminal.auto_resume.hub_panicked — restarting driver"
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
                }
            }
        }
    })
}

/// One incarnation of the hub loop. Returns only when the crash-event channel
/// closes; a driver panic unwinds out to the supervisor in
/// [`spawn_hub_with_driver`], which restarts this body with the same `rx` and
/// `attempts` after a bounded backoff.
async fn run_hub_body<D: AutoResumeDriver + Sync>(
    driver: &D,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<CrashEvent>,
    delays: &[u64],
    attempts: &mut std::collections::HashMap<String, u32>,
) {
    {
        // Retaining exhausted / pane-closed entries is DELIBERATE (not a
        // leak): evicting on exhaustion would refill the retry budget for an
        // immediate manual-Relaunch re-crash.
        let max_attempts = delays.len() as u32;
        // Design note (serialization): handling events sequentially in ONE
        // task means a backoff sleep delays other panes' resumes by up to
        // 10s worst-case. Acceptable at v1 — crashes are rare, the budget is
        // tiny, and full serialization is the strongest anti-storm property
        // (one respawn in flight, ever).
        while let Some(ev) = rx.recv().await {
            let sref = driver.resumable_session_ref(&ev.terminal_id);
            let ctx = CrashContext {
                exit_code: ev.exit_code,
                mode: &ev.mode,
                create_request_id: ev.create_request_id.as_deref(),
                has_resumable_identity: sref.is_some(),
                lifetime_ms: ev.lifetime_ms,
                prior_attempts: ev
                    .create_request_id
                    .as_deref()
                    .and_then(|k| attempts.get(k).copied())
                    .unwrap_or(0),
                cap_exhausted: ev
                    .create_request_id
                    .as_deref()
                    .map(|k| driver.cap_exhausted(k))
                    .unwrap_or(true),
            };
            match decide(&ctx, delays) {
                AutoResumeDecision::SettleExited { reason } => {
                    if ev.mode != "shell" {
                        driver.log_settled(&ev.terminal_id, reason);
                    }
                    if reason == "clean_exit" || ev.lifetime_ms >= AUTO_RESUME_HEALTHY_LIFETIME_MS {
                        if let Some(k) = &ev.create_request_id {
                            attempts.remove(k);
                        }
                    }
                }
                AutoResumeDecision::Resume { attempt, delay_ms } => {
                    let (provider, session_id, cwd) = sref.expect("checked by decide");
                    let key = ev.create_request_id.clone().expect("checked by decide");
                    attempts.insert(key.clone(), attempt);
                    driver.emit_recovering(
                        &ev.terminal_id,
                        &ev.mode,
                        ev.exit_code,
                        attempt,
                        max_attempts,
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    // Guards AFTER the sleep — the world may have moved on.
                    if let Some(reason) =
                        driver.pre_respawn_guard(&provider, &session_id, &ev.terminal_id)
                    {
                        driver.log_settled(&ev.terminal_id, reason);
                        continue;
                    }
                    if !driver.claim_session(&provider, &session_id, &key).await {
                        driver.log_settled(&ev.terminal_id, "session_lease_held");
                        continue;
                    }
                    let spec = RespawnSpec {
                        mode: ev.mode.clone(),
                        provider: provider.clone(),
                        session_id: session_id.clone(),
                        create_request_id: key.clone(),
                        cwd,
                    };
                    match driver.respawn(&spec).await {
                        Ok(new_tid) => {
                            if driver
                                .complete_claim(&provider, &session_id, &key, &new_tid)
                                .await
                            {
                                driver.emit_replaced(
                                    &ev.terminal_id,
                                    &new_tid,
                                    ev.exit_code,
                                    attempt,
                                    max_attempts,
                                );
                            } else {
                                // Binding raced away between claim and completion; the
                                // driver already killed its own orphan child. No
                                // terminal.replaced — the pane stays settled exited.
                                driver.log_settled(&ev.terminal_id, "lease_completion_lost");
                            }
                        }
                        Err(err) => {
                            driver.fail_claim(&provider, &session_id, &key);
                            tracing::warn!(terminal_id = %ev.terminal_id, error = %err, "terminal.auto_resume.respawn_failed");
                            driver.log_settled(&ev.terminal_id, "respawn_failed");
                        }
                    }
                }
            }
        }
    }
}

/// Orchestrator-facing effects, faked in unit tests.
///
/// LEASE-SHAPE NOTE (fresh-eyes fix): the trait mirrors the REAL registry
/// lease API, which is asymmetric — success binds the lease to the NEW
/// terminal via `complete_session_ref_claim(locator, holder_create_request_id,
/// terminal_id) -> bool` (registry.rs:1964), failure releases it via
/// `fail_session_ref_claim(locator, holder_create_request_id)` (registry.rs:2007).
/// A single symmetric `release_claim` cannot implement that discipline, so the
/// trait exposes `complete_claim` / `fail_claim` distinctly, and the claim call
/// carries the holder create-request-id the registry keys the lease by.
///
/// ASYNC-SHAPE NOTE: `claim_session` and `complete_claim` return futures (the
/// same RPITIT shape as `respawn`) because the production impl must AWAIT the
/// kill→confirm discipline on its lease paths — `ExpiredNeedsKill` in the
/// claim rounds, and the kill-own-child mirror on a lost completion — exactly
/// like the create ingress does (`terminal.rs` claim rounds / complete==false
/// path). A sync signature would force blocking a runtime worker.
pub(crate) trait AutoResumeDriver: Send + 'static {
    fn cap_exhausted(&self, create_request_id: &str) -> bool;
    /// (provider, session_id, cwd)
    fn resumable_session_ref(&self, terminal_id: &str) -> Option<(String, String, Option<String>)>;
    /// Post-backoff guard. Some(reason) aborts the resume and settles with that
    /// reason ("session_owned_live" when a live terminal already owns the
    /// session-ref; "pane_closed" when the pane's ledger binding was retired
    /// during the backoff). None = clear to claim.
    fn pre_respawn_guard(
        &self,
        provider: &str,
        session_id: &str,
        old_terminal_id: &str,
    ) -> Option<&'static str>;
    /// Acquire the session-ref lease for this holder; false = not acquirable → abort.
    /// The PRODUCTION impl runs the create ingress's full bounded claim
    /// discipline internally — the hub only sees the outcome.
    fn claim_session(
        &self,
        provider: &str,
        session_id: &str,
        create_request_id: &str,
    ) -> impl std::future::Future<Output = bool> + Send;
    /// Bind the acquired lease to the freshly spawned terminal
    /// (complete_session_ref_claim). false = the binding raced away; the
    /// PRODUCTION impl has already killed its own orphan child before
    /// returning (mirror of the ingress complete==false path).
    fn complete_claim(
        &self,
        provider: &str,
        session_id: &str,
        create_request_id: &str,
        new_terminal_id: &str,
    ) -> impl std::future::Future<Output = bool> + Send;
    /// Release a claim whose respawn failed (fail_session_ref_claim).
    fn fail_claim(&self, provider: &str, session_id: &str, create_request_id: &str);
    fn respawn(
        &self,
        req: &RespawnSpec,
    ) -> impl std::future::Future<Output = Result<String, String>> + Send;
    fn emit_recovering(
        &self,
        terminal_id: &str,
        mode: &str,
        exit_code: i64,
        attempt: u32,
        max_attempts: u32,
    );
    fn emit_replaced(&self, old: &str, new: &str, exit_code: i64, attempt: u32, max_attempts: u32);
    fn log_settled(&self, terminal_id: &str, reason: &str);
}

/// Everything a respawn needs, resolved by the hub before the driver call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RespawnSpec {
    pub mode: String,
    pub provider: String,
    pub session_id: String,
    pub create_request_id: String,
    pub cwd: Option<String>,
}

/// The production [`AutoResumeDriver`]: delegates to the real registry /
/// identity / ledger / respawn seam / broadcast bus.
pub(crate) struct WsAutoResumeDriver {
    pub(crate) state: crate::WsState,
}

fn session_locator(provider: &str, session_id: &str) -> freshell_protocol::SessionLocator {
    freshell_protocol::SessionLocator {
        provider: provider.to_string(),
        session_id: session_id.to_string(),
    }
}

impl AutoResumeDriver for WsAutoResumeDriver {
    fn cap_exhausted(&self, create_request_id: &str) -> bool {
        self.state.registry.respawn_exhausted(create_request_id)
    }

    /// Identity registry first (retired-inclusive — the exit hook retires
    /// the entry before the CrashEvent is handled), pane-ledger binding as
    /// the fallback home.
    fn resumable_session_ref(&self, terminal_id: &str) -> Option<(String, String, Option<String>)> {
        if self.state.registry.is_restart_retiring(terminal_id) {
            return None;
        }
        if let Some(entry) = self.state.identity.get(terminal_id) {
            if let (Some(provider), Some(session_id)) = (entry.provider, entry.session_id) {
                return Some((provider, session_id, entry.cwd));
            }
        }
        let locator = self
            .state
            .pane_ledger
            .bound_session_ref_for_terminal(terminal_id)?;
        let cwd = self
            .state
            .pane_ledger
            .list_bindings()
            .into_iter()
            .find(|r| r.provider == locator.provider && r.session_id == locator.session_id)
            .and_then(|r| r.cwd);
        Some((locator.provider, locator.session_id, cwd))
    }

    fn pre_respawn_guard(
        &self,
        provider: &str,
        session_id: &str,
        old_terminal_id: &str,
    ) -> Option<&'static str> {
        // A restart may begin after the CrashEvent was dequeued but before
        // this post-backoff check. Its boot-scoped tombstone survives removal
        // of the predecessor registry row.
        if self.state.registry.is_restart_retiring(old_terminal_id) {
            return Some("restart_retiring");
        }
        // The user already relaunched this session during the backoff.
        if self
            .state
            .registry
            .live_terminal_for_session_ref(&session_locator(provider, session_id))
            .is_some()
        {
            return Some("session_owned_live");
        }
        // The pane was closed during the backoff: `terminal.kill` retires the
        // ledger binding (`retire_closed`), so a still-Bound row is the
        // "pane still wants this session" signal. Ledger-disabled caveat:
        // `bound_session_ref_for_terminal` returns `None` both when retired
        // and when the ledger is disabled — only a RETIRED binding means
        // pane_closed, so skip the sub-check when the ledger is disabled
        // (the live-owner check and the lease still guard).
        if self.state.pane_ledger.is_enabled()
            && self
                .state
                .pane_ledger
                .bound_session_ref_for_terminal(old_terminal_id)
                .is_none()
        {
            return Some("pane_closed");
        }
        None
    }

    /// The create ingress's FULL bounded claim discipline, headless
    /// (mirror of `terminal.rs::handle_create`'s claim rounds): at most one
    /// ExpiredNeedsKill kill→confirm→force-release round, then re-claim;
    /// `Held`/`BoundElsewhere` (and rounds exhausted) are `false`.
    ///
    /// `holder_conn` is MINTED via `registry.new_connection_id()` — never a
    /// literal that could collide with a real WS connection id, or a client
    /// disconnect sweep could release the orchestrator's lease mid-respawn.
    /// A minted id is never swept, so the orchestrator OWNS the full release
    /// discipline on every path: success (`complete_claim`), respawn failure
    /// (`fail_claim`), completion failure (kill own child, below). No
    /// connection-death safety net exists for this holder.
    fn claim_session(
        &self,
        provider: &str,
        session_id: &str,
        create_request_id: &str,
    ) -> impl std::future::Future<Output = bool> + Send {
        let state = self.state.clone();
        let locator = session_locator(provider, session_id);
        let create_request_id = create_request_id.to_string();
        async move {
            use freshell_terminal::registry::SessionRefClaim;
            let holder_conn = state.registry.new_connection_id();
            for round in 0..2u8 {
                match state.registry.claim_session_ref(
                    &locator,
                    &create_request_id,
                    holder_conn,
                    crate::terminal::now_ms().max(0) as u64,
                ) {
                    SessionRefClaim::Acquired => return true,
                    SessionRefClaim::BoundElsewhere { .. } | SessionRefClaim::Held { .. } => {
                        return false;
                    }
                    SessionRefClaim::ExpiredNeedsKill { pid } => {
                        if round == 0
                            && crate::terminal::kill_session_ref_holder_and_confirm(
                                &state.registry,
                                pid,
                            )
                            .await
                        {
                            state.registry.force_release_after_confirmed_kill(&locator);
                            continue; // the slot is now free — re-claim
                        }
                        // Unconfirmed kill (or a second expiry): hold the
                        // lease closed and abort, mirroring the ingress.
                        tracing::error!(target: "invariant",
                            provider = %locator.provider,
                            session_id = %locator.session_id,
                            pid,
                            "session_ref_lease_expired_kill_unconfirmed: holding lease closed");
                        return false;
                    }
                }
            }
            false
        }
    }

    /// Mirror of the ingress complete==false path (`terminal.rs`): a lease
    /// revoked while spawning means killing OUR OWN just-spawned child via
    /// the registry handle, confirming death, then force-releasing — only
    /// then does `false` go back to the hub.
    fn complete_claim(
        &self,
        provider: &str,
        session_id: &str,
        create_request_id: &str,
        new_terminal_id: &str,
    ) -> impl std::future::Future<Output = bool> + Send {
        let state = self.state.clone();
        let locator = session_locator(provider, session_id);
        let create_request_id = create_request_id.to_string();
        let new_terminal_id = new_terminal_id.to_string();
        async move {
            if state.registry.complete_session_ref_claim(
                &locator,
                &create_request_id,
                &new_terminal_id,
            ) {
                return true;
            }
            let pid = state.registry.pid_of(&new_terminal_id);
            state.registry.kill(&new_terminal_id);
            let confirmed = match pid {
                Some(pid) => crate::terminal::confirm_pid_dead_within_500ms(pid).await,
                // No pid handle to probe: the registry kill removed the row;
                // nothing is left to signal, so treat as confirmed.
                None => true,
            };
            if confirmed {
                state.registry.force_release_after_confirmed_kill(&locator);
            } else {
                tracing::error!(target: "invariant",
                    terminal_id = %new_terminal_id,
                    provider = %locator.provider,
                    session_id = %locator.session_id,
                    "session_ref_lease_revoked_child_kill_unconfirmed: holding lease closed");
            }
            false
        }
    }

    /// The headless driver holds no RAII `SessionRefLeaseGuard` (the WS
    /// ingress's failure-path release) — this explicit call IS its
    /// failure-path release.
    fn fail_claim(&self, provider: &str, session_id: &str, create_request_id: &str) {
        self.state
            .registry
            .fail_session_ref_claim(&session_locator(provider, session_id), create_request_id);
    }

    fn respawn(
        &self,
        req: &RespawnSpec,
    ) -> impl std::future::Future<Output = Result<String, String>> + Send {
        let state = self.state.clone();
        let req = crate::terminal::AgentRespawnRequest {
            mode: req.mode.clone(),
            provider: req.provider.clone(),
            session_id: req.session_id.clone(),
            create_request_id: req.create_request_id.clone(),
            cwd: req.cwd.clone(),
        };
        async move {
            crate::terminal::respawn_agent_terminal(&state, &req)
                .await
                .map_err(|err| match err {
                    crate::terminal::RespawnError::LaunchUnresolvable(msg) => msg,
                    crate::terminal::RespawnError::Spawn(io) => io.to_string(),
                })
        }
    }

    fn emit_recovering(
        &self,
        terminal_id: &str,
        mode: &str,
        exit_code: i64,
        attempt: u32,
        max_attempts: u32,
    ) {
        let msg = freshell_protocol::ServerMessage::TerminalStatus(freshell_protocol::TerminalStatus {
            status: freshell_protocol::RuntimeStatus::Recovering,
            terminal_id: terminal_id.to_string(),
            attempt: Some(attempt as i64),
            // The client renders attempt/max/exit from these typed FIELDS;
            // `reason` below is purely presentational and safe to reword
            // (council 7w4h/xkhx: prose must never be protocol).
            max_attempts: Some(max_attempts as i64),
            exit_code: Some(exit_code),
            reason: Some(format!(
                "{mode} crashed (exit {exit_code}) — auto-resuming, attempt {attempt}/{max_attempts}"
            )),
        });
        match serde_json::to_string(&msg) {
            Ok(json) => {
                let _ = self.state.broadcast_tx.send(json);
            }
            Err(err) => {
                tracing::error!(terminal_id, error = %err, "terminal.auto_resume.recovering_frame_serialize_failed");
            }
        }
    }

    fn emit_replaced(&self, old: &str, new: &str, exit_code: i64, attempt: u32, max_attempts: u32) {
        let msg = freshell_protocol::ServerMessage::TerminalReplaced(
            freshell_protocol::TerminalReplaced {
                old_terminal_id: old.to_string(),
                new_terminal_id: new.to_string(),
                exit_code,
                attempt,
                max_attempts,
            },
        );
        match serde_json::to_string(&msg) {
            Ok(json) => {
                let _ = self.state.broadcast_tx.send(json);
            }
            Err(err) => {
                tracing::error!(old_terminal_id = old, new_terminal_id = new, error = %err, "terminal.auto_resume.replaced_frame_serialize_failed");
            }
        }
    }

    fn log_settled(&self, terminal_id: &str, reason: &str) {
        tracing::info!(terminal_id, reason, "terminal.auto_resume.settled");
    }
}

/// Spawn the production auto-resume hub (delays from
/// [`auto_resume_delays`] — env-overridable). Wired in
/// `freshell-server/src/main.rs` next to the `spawn_idle_monitor` precedent.
pub fn spawn_auto_resume_hub(
    state: crate::WsState,
    rx: tokio::sync::mpsc::UnboundedReceiver<CrashEvent>,
) -> tokio::task::JoinHandle<()> {
    spawn_auto_resume_hub_with_delays(state, rx, auto_resume_delays())
}

/// [`spawn_auto_resume_hub`] with an explicit backoff schedule. The
/// integration-test harness uses this to inject tiny delays: the harness is
/// in-process, so a `FRESHELL_AUTO_RESUME_DELAYS_MS` env write would leak
/// across parallel tests in the same binary.
pub fn spawn_auto_resume_hub_with_delays(
    state: crate::WsState,
    rx: tokio::sync::mpsc::UnboundedReceiver<CrashEvent>,
    delays: Vec<u64>,
) -> tokio::task::JoinHandle<()> {
    spawn_hub_with_driver(WsAutoResumeDriver { state }, rx, delays)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx<'a>() -> CrashContext<'a> {
        CrashContext {
            exit_code: 1,
            mode: "claude",
            create_request_id: Some("cr-1"),
            has_resumable_identity: true,
            lifetime_ms: 5_000,
            prior_attempts: 0,
            cap_exhausted: false,
        }
    }
    const DELAYS: [u64; 2] = [2_000, 10_000];

    #[test]
    fn nonzero_agent_exit_resumes_with_schedule() {
        assert_eq!(
            decide(&ctx(), &DELAYS),
            AutoResumeDecision::Resume {
                attempt: 1,
                delay_ms: 2_000
            }
        );
        let c = CrashContext {
            prior_attempts: 1,
            ..ctx()
        };
        assert_eq!(
            decide(&c, &DELAYS),
            AutoResumeDecision::Resume {
                attempt: 2,
                delay_ms: 10_000
            }
        );
    }

    #[test]
    fn clean_exit_never_resumes() {
        let c = CrashContext {
            exit_code: 0,
            ..ctx()
        };
        assert_eq!(
            decide(&c, &DELAYS),
            AutoResumeDecision::SettleExited {
                reason: "clean_exit"
            }
        );
    }

    #[test]
    fn shell_mode_never_resumes() {
        let c = CrashContext {
            mode: "shell",
            ..ctx()
        };
        assert_eq!(
            decide(&c, &DELAYS),
            AutoResumeDecision::SettleExited {
                reason: "not_agent_mode"
            }
        );
        // Unknown future modes are fail-safe too:
        let c = CrashContext {
            mode: "mystery",
            ..ctx()
        };
        assert_eq!(
            decide(&c, &DELAYS),
            AutoResumeDecision::SettleExited {
                reason: "not_agent_mode"
            }
        );
    }

    #[test]
    fn all_four_agent_modes_are_eligible() {
        for mode in AUTO_RESUME_MODES {
            let c = CrashContext { mode, ..ctx() };
            assert!(
                matches!(decide(&c, &DELAYS), AutoResumeDecision::Resume { .. }),
                "mode {mode}"
            );
        }
    }

    #[test]
    fn missing_identity_settles_exited_immediately() {
        let c = CrashContext {
            has_resumable_identity: false,
            ..ctx()
        };
        assert_eq!(
            decide(&c, &DELAYS),
            AutoResumeDecision::SettleExited {
                reason: "no_resumable_identity"
            }
        );
        let c = CrashContext {
            create_request_id: None,
            ..ctx()
        };
        assert_eq!(
            decide(&c, &DELAYS),
            AutoResumeDecision::SettleExited {
                reason: "no_create_request_id"
            }
        );
    }

    #[test]
    fn respawn_cap_exhaustion_settles_exited() {
        let c = CrashContext {
            cap_exhausted: true,
            ..ctx()
        };
        assert_eq!(
            decide(&c, &DELAYS),
            AutoResumeDecision::SettleExited {
                reason: "respawn_cap_exhausted"
            }
        );
    }

    #[test]
    fn retries_are_bounded_and_exhaust_loudly() {
        let c = CrashContext {
            prior_attempts: 2,
            ..ctx()
        };
        assert_eq!(
            decide(&c, &DELAYS),
            AutoResumeDecision::SettleExited {
                reason: "retries_exhausted"
            }
        );
    }

    #[test]
    fn healthy_lifetime_resets_the_attempt_counter() {
        // A generation that lived >= 30s means the previous resume was healthy:
        // this crash starts a fresh budget even with prior attempts recorded.
        let c = CrashContext {
            prior_attempts: 2,
            lifetime_ms: AUTO_RESUME_HEALTHY_LIFETIME_MS,
            ..ctx()
        };
        assert_eq!(
            decide(&c, &DELAYS),
            AutoResumeDecision::Resume {
                attempt: 1,
                delay_ms: 2_000
            }
        );
    }

    #[test]
    fn delays_env_override_is_parsed_and_bad_values_fall_back() {
        assert_eq!(parse_delays_env("50,100"), Some(vec![50, 100]));
        assert_eq!(parse_delays_env("2000"), Some(vec![2000]));
        assert_eq!(parse_delays_env(""), None);
        assert_eq!(parse_delays_env("fast,slow"), None);
        assert_eq!(parse_delays_env("0"), None); // zero-delay loops are forbidden
    }

    // ---- Task 5: hub orchestration (fake driver, paused tokio time) ----

    use std::sync::{Arc, Mutex};

    fn crash(
        terminal_id: &str,
        exit_code: i64,
        mode: &str,
        create_request_id: Option<&str>,
        lifetime_ms: i64,
    ) -> CrashEvent {
        CrashEvent {
            terminal_id: terminal_id.to_string(),
            exit_code,
            mode: mode.to_string(),
            create_request_id: create_request_id.map(str::to_string),
            lifetime_ms,
        }
    }

    #[derive(Debug)]
    struct FakeState {
        cap_exhausted: bool,
        session: Option<(String, String, Option<String>)>,
        guard: Option<&'static str>,
        claim_ok: bool,
        complete_ok: bool,
        panic_next_recovering: bool,
        respawn_result: Result<String, String>,
        recovering: Vec<(String, u32, u32)>,
        replaced: Vec<(String, String, u32)>,
        respawns: Vec<RespawnSpec>,
        claims: Vec<String>,
        completes: Vec<String>,
        fails: Vec<String>,
        settled: Vec<(String, String)>,
    }

    /// Records every orchestrator effect; each knob is mutable mid-test so
    /// one hub can be driven through per-event configurations.
    #[derive(Clone)]
    struct FakeDriver {
        inner: Arc<Mutex<FakeState>>,
    }

    impl FakeDriver {
        /// Identity present, cap ok, guard clear, claim ok, respawn -> Ok("t-new").
        fn healthy() -> Self {
            Self {
                inner: Arc::new(Mutex::new(FakeState {
                    cap_exhausted: false,
                    session: Some(("claude".into(), "sess-1".into(), None)),
                    guard: None,
                    claim_ok: true,
                    complete_ok: true,
                    panic_next_recovering: false,
                    respawn_result: Ok("t-new".into()),
                    recovering: Vec::new(),
                    replaced: Vec::new(),
                    respawns: Vec::new(),
                    claims: Vec::new(),
                    completes: Vec::new(),
                    fails: Vec::new(),
                    settled: Vec::new(),
                })),
            }
        }

        fn lock(&self) -> std::sync::MutexGuard<'_, FakeState> {
            self.inner.lock().expect("fake driver lock")
        }

        fn set_cap_exhausted(&self, v: bool) {
            self.lock().cap_exhausted = v;
        }
        fn set_session(&self, v: Option<(String, String, Option<String>)>) {
            self.lock().session = v;
        }
        fn set_guard(&self, v: Option<&'static str>) {
            self.lock().guard = v;
        }
        fn set_claim_ok(&self, v: bool) {
            self.lock().claim_ok = v;
        }
        fn set_complete_ok(&self, v: bool) {
            self.lock().complete_ok = v;
        }
        fn set_respawn_result(&self, v: Result<String, String>) {
            self.lock().respawn_result = v;
        }
        fn set_panic_next_recovering(&self, v: bool) {
            self.lock().panic_next_recovering = v;
        }

        /// (old_terminal_id, attempt, max_attempts)
        fn recovering_calls(&self) -> Vec<(String, u32, u32)> {
            self.lock().recovering.clone()
        }
        /// (old_terminal_id, new_terminal_id, attempt)
        fn replaced_calls(&self) -> Vec<(String, String, u32)> {
            self.lock().replaced.clone()
        }
        fn respawn_calls(&self) -> Vec<RespawnSpec> {
            self.lock().respawns.clone()
        }
        fn claim_calls(&self) -> Vec<String> {
            self.lock().claims.clone()
        }
        fn complete_calls(&self) -> Vec<String> {
            self.lock().completes.clone()
        }
        fn fail_calls(&self) -> Vec<String> {
            self.lock().fails.clone()
        }
        fn settled_reasons(&self) -> Vec<String> {
            self.lock().settled.iter().map(|(_, r)| r.clone()).collect()
        }
    }

    impl AutoResumeDriver for FakeDriver {
        fn cap_exhausted(&self, _create_request_id: &str) -> bool {
            self.lock().cap_exhausted
        }
        fn resumable_session_ref(
            &self,
            _terminal_id: &str,
        ) -> Option<(String, String, Option<String>)> {
            self.lock().session.clone()
        }
        fn pre_respawn_guard(
            &self,
            _provider: &str,
            _session_id: &str,
            _old_terminal_id: &str,
        ) -> Option<&'static str> {
            self.lock().guard
        }
        fn claim_session(
            &self,
            _provider: &str,
            _session_id: &str,
            create_request_id: &str,
        ) -> impl std::future::Future<Output = bool> + Send {
            let ok = {
                let mut s = self.lock();
                s.claims.push(create_request_id.to_string());
                s.claim_ok
            };
            std::future::ready(ok)
        }
        fn complete_claim(
            &self,
            _provider: &str,
            _session_id: &str,
            create_request_id: &str,
            _new_terminal_id: &str,
        ) -> impl std::future::Future<Output = bool> + Send {
            let ok = {
                let mut s = self.lock();
                s.completes.push(create_request_id.to_string());
                s.complete_ok
            };
            std::future::ready(ok)
        }
        fn fail_claim(&self, _provider: &str, _session_id: &str, create_request_id: &str) {
            self.lock().fails.push(create_request_id.to_string());
        }
        fn respawn(
            &self,
            req: &RespawnSpec,
        ) -> impl std::future::Future<Output = Result<String, String>> + Send {
            let result = {
                let mut s = self.lock();
                s.respawns.push(req.clone());
                s.respawn_result.clone()
            };
            std::future::ready(result)
        }
        fn emit_recovering(
            &self,
            terminal_id: &str,
            _mode: &str,
            _exit_code: i64,
            attempt: u32,
            max_attempts: u32,
        ) {
            // One-shot injected panic for the supervision test. The flag is
            // consumed and the guard DROPPED before panicking so the mutex is
            // never poisoned for subsequent events.
            let should_panic = {
                let mut s = self.lock();
                if s.panic_next_recovering {
                    s.panic_next_recovering = false;
                    true
                } else {
                    s.recovering
                        .push((terminal_id.to_string(), attempt, max_attempts));
                    false
                }
            };
            if should_panic {
                panic!("test-injected driver panic");
            }
        }
        fn emit_replaced(
            &self,
            old: &str,
            new: &str,
            _exit_code: i64,
            attempt: u32,
            _max_attempts: u32,
        ) {
            self.lock()
                .replaced
                .push((old.to_string(), new.to_string(), attempt));
        }
        fn log_settled(&self, terminal_id: &str, reason: &str) {
            self.lock()
                .settled
                .push((terminal_id.to_string(), reason.to_string()));
        }
    }

    /// Let the hub task run to its next await point (a few yields — the hub's
    /// non-timer awaits are all ready futures, so it runs whole iterations).
    async fn drain() {
        for _ in 0..5u8 {
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test(start_paused = true)]
    async fn crash_resumes_after_first_backoff_and_emits_frames() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let fake = FakeDriver::healthy(); // identity present, cap ok, claim ok, respawn -> Ok("t-new")
        let _hub = spawn_hub_with_driver(fake.clone(), rx, vec![2_000, 10_000]);
        tx.send(crash("t1", 1, "claude", Some("cr-1"), 5_000))
            .unwrap();
        tokio::task::yield_now().await;
        assert_eq!(fake.recovering_calls(), vec![("t1".into(), 1u32, 2u32)]);
        assert!(fake.respawn_calls().is_empty(), "must wait out the backoff");
        tokio::time::advance(std::time::Duration::from_millis(2_000)).await;
        tokio::task::yield_now().await;
        assert_eq!(fake.respawn_calls().len(), 1);
        assert_eq!(
            fake.replaced_calls(),
            vec![("t1".into(), "t-new".into(), 1u32)]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn second_crash_uses_second_delay_then_exhausts() {
        // crash cr-1 (lifetime 1s) -> attempt 1 @2s; crash again -> attempt 2 @10s;
        // crash again -> settled("retries_exhausted"), NO third respawn.
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let fake = FakeDriver::healthy();
        let _hub = spawn_hub_with_driver(fake.clone(), rx, vec![2_000, 10_000]);

        tx.send(crash("t1", 1, "claude", Some("cr-1"), 1_000))
            .unwrap();
        drain().await;
        tokio::time::advance(std::time::Duration::from_millis(2_000)).await;
        drain().await;
        assert_eq!(fake.respawn_calls().len(), 1);

        tx.send(crash("t-new", 1, "claude", Some("cr-1"), 1_000))
            .unwrap();
        drain().await;
        assert_eq!(
            fake.recovering_calls(),
            vec![("t1".into(), 1u32, 2u32), ("t-new".into(), 2u32, 2u32)]
        );
        // The first delay is NOT enough for attempt 2.
        tokio::time::advance(std::time::Duration::from_millis(2_000)).await;
        drain().await;
        assert_eq!(
            fake.respawn_calls().len(),
            1,
            "attempt 2 waits the full 10s"
        );
        tokio::time::advance(std::time::Duration::from_millis(8_000)).await;
        drain().await;
        assert_eq!(fake.respawn_calls().len(), 2);

        tx.send(crash("t-new2", 1, "claude", Some("cr-1"), 1_000))
            .unwrap();
        drain().await;
        assert_eq!(
            fake.respawn_calls().len(),
            2,
            "budget exhausted: no third respawn"
        );
        assert_eq!(
            fake.settled_reasons(),
            vec!["retries_exhausted".to_string()]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn healthy_generation_resets_attempts() {
        // two crashes (attempts 1,2), then a crash with lifetime_ms = 60_000:
        // attempt resets to 1 with the first delay again.
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let fake = FakeDriver::healthy();
        let _hub = spawn_hub_with_driver(fake.clone(), rx, vec![2_000, 10_000]);

        tx.send(crash("t1", 1, "claude", Some("cr-1"), 1_000))
            .unwrap();
        drain().await;
        tokio::time::advance(std::time::Duration::from_millis(2_000)).await;
        drain().await;
        tx.send(crash("t2", 1, "claude", Some("cr-1"), 1_000))
            .unwrap();
        drain().await;
        tokio::time::advance(std::time::Duration::from_millis(10_000)).await;
        drain().await;
        assert_eq!(fake.respawn_calls().len(), 2);

        // Healthy generation (>= 30s): fresh budget, attempt 1, first delay.
        tx.send(crash("t3", 1, "claude", Some("cr-1"), 60_000))
            .unwrap();
        drain().await;
        assert_eq!(
            fake.recovering_calls(),
            vec![
                ("t1".into(), 1u32, 2u32),
                ("t2".into(), 2u32, 2u32),
                ("t3".into(), 1u32, 2u32)
            ]
        );
        tokio::time::advance(std::time::Duration::from_millis(2_000)).await;
        drain().await;
        assert_eq!(fake.respawn_calls().len(), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn live_session_owner_aborts_resume_silently() {
        // pre_respawn_guard -> Some("session_owned_live") (user already relaunched):
        // no respawn, no claim, settled("session_owned_live").
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let fake = FakeDriver::healthy();
        fake.set_guard(Some("session_owned_live"));
        let _hub = spawn_hub_with_driver(fake.clone(), rx, vec![2_000, 10_000]);

        tx.send(crash("t1", 1, "claude", Some("cr-1"), 1_000))
            .unwrap();
        drain().await;
        tokio::time::advance(std::time::Duration::from_millis(2_000)).await;
        drain().await;
        assert!(fake.respawn_calls().is_empty());
        assert!(fake.claim_calls().is_empty());
        assert_eq!(
            fake.settled_reasons(),
            vec!["session_owned_live".to_string()]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn pane_closed_during_backoff_settles_pane_closed() {
        // pre_respawn_guard -> Some("pane_closed") (ledger binding retired during
        // the backoff): no respawn, no claim, settled("pane_closed").
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let fake = FakeDriver::healthy();
        let _hub = spawn_hub_with_driver(fake.clone(), rx, vec![2_000, 10_000]);

        tx.send(crash("t1", 1, "claude", Some("cr-1"), 1_000))
            .unwrap();
        drain().await;
        // The pane is closed DURING the backoff — the guard runs after it.
        fake.set_guard(Some("pane_closed"));
        tokio::time::advance(std::time::Duration::from_millis(2_000)).await;
        drain().await;
        assert!(fake.respawn_calls().is_empty());
        assert!(fake.claim_calls().is_empty());
        assert_eq!(fake.settled_reasons(), vec!["pane_closed".to_string()]);
    }

    #[tokio::test(start_paused = true)]
    async fn restart_retiring_terminal_is_rejected_after_backoff_without_affecting_unrelated_crash()
    {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let fake = FakeDriver::healthy();
        let _hub = spawn_hub_with_driver(fake.clone(), rx, vec![2_000, 10_000]);

        tx.send(crash(
            "t-restart-retiring",
            137,
            "codex",
            Some("cr-restart"),
            1_000,
        ))
        .unwrap();
        drain().await;
        // The proxy-triggered non-zero exit entered the queue before the
        // restart transaction marked its predecessor. Teardown remains slow
        // beyond the auto-resume delay, so the post-backoff guard is the
        // load-bearing fence.
        fake.set_guard(Some("restart_retiring"));
        tokio::time::advance(std::time::Duration::from_millis(2_000)).await;
        drain().await;
        assert!(fake.respawn_calls().is_empty());
        assert!(fake.claim_calls().is_empty());
        assert_eq!(fake.settled_reasons(), vec!["restart_retiring".to_string()]);

        // The fence is locator/terminal scoped: an unrelated crash still
        // follows the ordinary auto-resume path.
        fake.set_guard(None);
        tx.send(crash(
            "t-unrelated",
            1,
            "claude",
            Some("cr-unrelated"),
            1_000,
        ))
        .unwrap();
        drain().await;
        tokio::time::advance(std::time::Duration::from_millis(2_000)).await;
        drain().await;
        assert_eq!(fake.respawn_calls().len(), 1);
        assert_eq!(fake.respawn_calls()[0].create_request_id, "cr-unrelated");
    }

    #[tokio::test(start_paused = true)]
    async fn lost_lease_claim_aborts_resume() {
        // claim_session -> false: no respawn, settled("session_lease_held").
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let fake = FakeDriver::healthy();
        fake.set_claim_ok(false);
        let _hub = spawn_hub_with_driver(fake.clone(), rx, vec![2_000, 10_000]);

        tx.send(crash("t1", 1, "claude", Some("cr-1"), 1_000))
            .unwrap();
        drain().await;
        tokio::time::advance(std::time::Duration::from_millis(2_000)).await;
        drain().await;
        assert_eq!(fake.claim_calls(), vec!["cr-1".to_string()]);
        assert!(fake.respawn_calls().is_empty());
        assert_eq!(
            fake.settled_reasons(),
            vec!["session_lease_held".to_string()]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn failed_respawn_settles_loudly() {
        // respawn -> Err("spawn failed"): fail_claim called (NOT complete_claim),
        // settled("respawn_failed").
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let fake = FakeDriver::healthy();
        fake.set_respawn_result(Err("spawn failed".into()));
        let _hub = spawn_hub_with_driver(fake.clone(), rx, vec![2_000, 10_000]);

        tx.send(crash("t1", 1, "claude", Some("cr-1"), 1_000))
            .unwrap();
        drain().await;
        tokio::time::advance(std::time::Duration::from_millis(2_000)).await;
        drain().await;
        assert_eq!(fake.respawn_calls().len(), 1);
        assert_eq!(fake.fail_calls(), vec!["cr-1".to_string()]);
        assert!(fake.complete_calls().is_empty());
        assert_eq!(fake.settled_reasons(), vec!["respawn_failed".to_string()]);
    }

    #[tokio::test(start_paused = true)]
    async fn lost_lease_completion_settles_without_replaced_frame() {
        // respawn -> Ok("t-new") but complete_claim -> false (binding raced away;
        // production driver kills its own child before returning false):
        // NO terminal.replaced emitted, settled("lease_completion_lost").
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let fake = FakeDriver::healthy();
        fake.set_complete_ok(false);
        let _hub = spawn_hub_with_driver(fake.clone(), rx, vec![2_000, 10_000]);

        tx.send(crash("t1", 1, "claude", Some("cr-1"), 1_000))
            .unwrap();
        drain().await;
        tokio::time::advance(std::time::Duration::from_millis(2_000)).await;
        drain().await;
        assert_eq!(fake.respawn_calls().len(), 1);
        assert_eq!(fake.complete_calls(), vec!["cr-1".to_string()]);
        assert!(fake.replaced_calls().is_empty());
        assert!(
            fake.fail_calls().is_empty(),
            "completion loss is NOT fail_claim"
        );
        assert_eq!(
            fake.settled_reasons(),
            vec!["lease_completion_lost".to_string()]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn cap_exhausted_and_no_identity_and_clean_and_shell_settle_without_respawn() {
        // four events: cap_exhausted=true / resumable_session_ref=None /
        // exit_code=0 / mode="shell" — zero respawn calls, zero recovering frames.
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let fake = FakeDriver::healthy();
        let _hub = spawn_hub_with_driver(fake.clone(), rx, vec![2_000, 10_000]);

        fake.set_cap_exhausted(true);
        tx.send(crash("t1", 1, "claude", Some("cr-1"), 1_000))
            .unwrap();
        drain().await;

        fake.set_cap_exhausted(false);
        fake.set_session(None);
        tx.send(crash("t2", 1, "claude", Some("cr-2"), 1_000))
            .unwrap();
        drain().await;

        fake.set_session(Some(("claude".into(), "sess-1".into(), None)));
        tx.send(crash("t3", 0, "claude", Some("cr-3"), 1_000))
            .unwrap();
        tx.send(crash("t4", 1, "shell", Some("cr-4"), 1_000))
            .unwrap();
        drain().await;

        assert!(fake.respawn_calls().is_empty());
        assert!(fake.recovering_calls().is_empty());
        assert!(fake.claim_calls().is_empty());
    }

    /// Council MEDIUM fix (crusty, 7w4h/xkhx review): a driver panic must not
    /// silently end auto-resume forever — that would reinstate the exact
    /// incident this feature exists to prevent (a crashed pane sitting grey
    /// for hours). The hub is supervised: the panic is caught, logged ERROR,
    /// and the loop restarted after a bounded backoff, with the crash-event
    /// receiver surviving the restart.
    ///
    /// Real time (not start_paused): the supervision backoff is a real sleep.
    #[tokio::test(flavor = "multi_thread")]
    async fn hub_survives_driver_panic_and_processes_subsequent_crashes() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let fake = FakeDriver::healthy();
        fake.set_panic_next_recovering(true);
        let _hub = spawn_hub_with_driver(fake.clone(), rx, vec![10]);

        // Event 1: the driver panics mid-processing (inside emit_recovering).
        tx.send(crash("t1", 1, "claude", Some("cr-1"), 5_000))
            .unwrap();
        // Event 2: must still be processed by the restarted hub body.
        tx.send(crash("t2", 1, "claude", Some("cr-2"), 5_000))
            .unwrap();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let respawned: Vec<String> = fake
                .respawn_calls()
                .iter()
                .map(|r| r.create_request_id.clone())
                .collect();
            if respawned.contains(&"cr-2".to_string()) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "hub never recovered from the driver panic: respawns={respawned:?}"
            );
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    }
}
